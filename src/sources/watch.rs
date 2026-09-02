//! Foreground change stream over the stores behind Apple apps.
//!
//! `cider watch` is not a daemon. It registers FSEvents watches on the
//! directories Reminders, Calendar, Contacts, Notes, Home, and Shortcuts
//! write to, folds the burst of file events one edit produces into a single
//! [`WatchEvent`] per source, and hands each to the caller until the process
//! is killed. It is the polling-free alternative to re-running `cider
//! reminders` on a timer: an event says *that* a store changed, and the
//! caller re-reads it with the matching `sources` function to learn *what*.
//!
//! When the native `cider-bridge` CLI is installed ([`super::bridge_cli`]),
//! Reminders, Calendar, and Contacts come from EventKit's and Contacts'
//! own change notifications instead (`kind: store_changed`, no paths):
//! item-level, already debounced by the framework, and unaffected by the
//! apps rewriting their WAL files for reasons that are not edits. The other
//! sources stay on FSEvents; [`Via`] chooses.

use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::bridge_cli;

/// A local data store [`watch`] can observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchSource {
    Reminders,
    Calendar,
    Contacts,
    Notes,
    Home,
    Shortcuts,
}

impl WatchSource {
    /// Every source, in the order `cider watch` defaults to.
    pub const ALL: [WatchSource; 6] = [
        Self::Reminders,
        Self::Calendar,
        Self::Contacts,
        Self::Notes,
        Self::Home,
        Self::Shortcuts,
    ];

    /// The CLI and JSON spelling of each source, parallel to [`Self::ALL`].
    pub const NAMES: [&'static str; 6] = [
        "reminders",
        "calendar",
        "contacts",
        "notes",
        "home",
        "shortcuts",
    ];

    /// The CLI and JSON spelling of this source.
    pub fn name(self) -> &'static str {
        match self {
            Self::Reminders => "reminders",
            Self::Calendar => "calendar",
            Self::Contacts => "contacts",
            Self::Notes => "notes",
            Self::Home => "home",
            Self::Shortcuts => "shortcuts",
        }
    }

    /// Whether `cider-bridge watch` can stream this source (EventKit and
    /// Contacts post change notifications; the others have no framework).
    pub fn has_cli_stream(self) -> bool {
        matches!(self, Self::Reminders | Self::Calendar | Self::Contacts)
    }
}

/// Which backend `cider watch` uses for the sources the CLI covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Via {
    /// The CLI when it is installed, FSEvents otherwise.
    #[default]
    Auto,
    /// The CLI, failing if it is not installed. Sources it does not cover
    /// still use FSEvents.
    Cli,
    /// FSEvents for everything, CLI or not.
    Fsevents,
}

impl Via {
    pub const NAMES: [&'static str; 3] = ["auto", "cli", "fsevents"];

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cli => "cli",
            Self::Fsevents => "fsevents",
        }
    }
}

impl std::str::FromStr for Via {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "auto" => Ok(Self::Auto),
            "cli" => Ok(Self::Cli),
            "fsevents" => Ok(Self::Fsevents),
            other => anyhow::bail!(
                "unknown --via {other:?}; expected one of {}",
                Self::NAMES.join(", ")
            ),
        }
    }
}

/// What a [`WatchEvent`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    /// FSEvents saw files change under the store; `paths` says which.
    FilesChanged,
    /// The framework behind the store (EventKit, Contacts) reported a
    /// change, via `cider-bridge watch`. No paths: it is not a file event.
    StoreChanged,
}

impl std::fmt::Display for WatchSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for WatchSource {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.name() == s)
            .with_context(|| {
                format!(
                    "unknown watch source {s:?}; expected one of {}",
                    Self::NAMES.join(", ")
                )
            })
    }
}

/// One change notification: everything that changed under one source's
/// store within a debounce window (FSEvents), or one framework
/// notification (`cider-bridge watch`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEvent {
    pub source: WatchSource,
    /// When the window closed and the event was emitted.
    pub at: DateTime<Utc>,
    pub kind: WatchKind,
    /// De-duplicated absolute paths of the files that changed, in first-seen
    /// order. Non-empty for `files_changed`, absent for `store_changed`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// A `cider-bridge watch` line's `data` (`{source, at, kind}`) as an event.
/// `None` for a source cider does not know, or a line without one.
pub fn event_from_cli(data: &Json) -> Option<WatchEvent> {
    let source: WatchSource = data.get("source")?.as_str()?.parse().ok()?;
    let at = data
        .get("at")
        .and_then(Json::as_str)
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    Some(WatchEvent {
        source,
        at,
        kind: WatchKind::StoreChanged,
        paths: Vec::new(),
    })
}

/// Split `sources` into the ones `cider-bridge watch` will stream and the
/// ones FSEvents will, for `via`. Duplicates are dropped, order kept.
/// [`Via::Cli`] without the CLI is an error; [`Via::Auto`] falls back.
pub fn split_sources(
    sources: &[WatchSource],
    via: Via,
    cli_installed: bool,
) -> anyhow::Result<(Vec<WatchSource>, Vec<WatchSource>)> {
    let use_cli = match via {
        Via::Fsevents => false,
        Via::Auto => cli_installed,
        Via::Cli => {
            if !cli_installed {
                return Err(super::bridge::BridgeError::CliNotInstalled.into());
            }
            true
        }
    };
    let mut cli = Vec::new();
    let mut fs = Vec::new();
    for &source in sources {
        let bucket = if use_cli && source.has_cli_stream() {
            &mut cli
        } else {
            &mut fs
        };
        if !bucket.contains(&source) {
            bucket.push(source);
        }
    }
    Ok((cli, fs))
}

/// The directories a source's app writes its store into, relative to `home`.
///
/// Pure: nothing here checks the filesystem, so a caller can show what
/// *would* be watched. [`watch`] itself skips paths that do not exist.
pub fn store_paths(source: &WatchSource, home: &Path) -> Vec<PathBuf> {
    let relative: &[&str] = match source {
        WatchSource::Reminders => {
            &["Library/Group Containers/group.com.apple.reminders/Container_v1/Stores"]
        }
        // Calendar.sqlitedb and its -wal live directly in the group container.
        WatchSource::Calendar => &["Library/Group Containers/group.com.apple.calendar"],
        WatchSource::Contacts => &["Library/Application Support/AddressBook"],
        WatchSource::Notes => &["Library/Group Containers/group.com.apple.notes"],
        WatchSource::Home => &["Library/Containers/com.apple.Home/Data/Library/Caches/com.apple.HomeKit/com.apple.Home/com.apple.HomeKit.configurations"],
        WatchSource::Shortcuts => &["Library/Shortcuts"],
    };
    relative.iter().map(|rel| home.join(rel)).collect()
}

/// Fold raw `(source, path)` file events into one [`WatchEvent`] per source.
///
/// Sources keep first-seen order, and so do paths within a source; a path
/// reported several times (FSEvents happily does this for one write)
/// appears once. Every returned event carries the same `at`.
pub fn coalesce(events: Vec<(WatchSource, String)>, at: DateTime<Utc>) -> Vec<WatchEvent> {
    let mut out: Vec<WatchEvent> = Vec::new();
    for (source, path) in events {
        let index = match out.iter().position(|event| event.source == source) {
            Some(index) => index,
            None => {
                out.push(WatchEvent {
                    source,
                    at,
                    kind: WatchKind::FilesChanged,
                    paths: Vec::new(),
                });
                out.len() - 1
            }
        };
        let paths = &mut out[index].paths;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    out
}

/// Whether a notify event could reflect a change to a store's contents.
///
/// Reads (`Access`) and inode-metadata-only touches (`Modify(Metadata)`) are
/// dropped: neither means the data a reader would see has changed. Anything
/// else — create, data write, rename, remove, and the catch-all `Any` — is
/// kept, because a missed change costs more than a spurious re-read.
fn is_content_change(kind: &EventKind) -> bool {
    !matches!(
        kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
    )
}

/// Which watched root a reported path falls under, if any.
fn source_for(roots: &[(WatchSource, PathBuf)], path: &Path) -> Option<WatchSource> {
    roots
        .iter()
        .find(|(_, root)| path.starts_with(root))
        .map(|(source, _)| *source)
}

/// Watch `sources` through whichever backends `via` selects and call
/// `on_event` with every change until a backend ends.
///
/// The CLI-covered sources stream from `cider-bridge watch` and the rest
/// from FSEvents ([`watch`]), concurrently; the first backend to fail ends
/// both. Like [`watch`], this only returns in practice when something went
/// wrong or the CLI exited.
pub async fn watch_via(
    sources: &[WatchSource],
    debounce: Duration,
    via: Via,
    mut on_event: impl FnMut(WatchEvent),
) -> anyhow::Result<()> {
    let (cli_sources, fs_sources) = split_sources(sources, via, bridge_cli::is_installed())?;
    let (tx, mut rx) = mpsc::unbounded_channel::<WatchEvent>();

    let fs_tx = tx.clone();
    let fs = async move {
        if fs_sources.is_empty() {
            return Ok(());
        }
        // The closure (and with it this sender) drops with the future once
        // it completes, which is what lets `rx` drain to a close below.
        watch(&fs_sources, debounce, move |event| {
            let _ = fs_tx.send(event);
        })
        .await
    };
    let cli_tx = tx;
    let cli = async move {
        if cli_sources.is_empty() {
            return Ok(());
        }
        let names: Vec<&str> = cli_sources.iter().map(|s| s.name()).collect();
        eprintln!(
            "cider watch: streaming {} via {}",
            names.join(", "),
            bridge_cli::CLI_NAME
        );
        bridge_cli::stream_watch(&names, move |data| {
            if let Some(event) = event_from_cli(&data) {
                let _ = cli_tx.send(event);
            }
        })
        .await
        .map_err(anyhow::Error::from)
    };

    let mut backends = pin!(async { tokio::try_join!(fs, cli).map(|_| ()) });
    let result = loop {
        tokio::select! {
            Some(event) = rx.recv() => on_event(event),
            result = &mut backends => break result,
        }
    };
    while let Ok(event) = rx.try_recv() {
        on_event(event);
    }
    result
}

/// Watch the stores behind `sources` and call `on_event` with each coalesced
/// change until the process exits.
///
/// Paths that do not exist on this machine are skipped with a note on
/// stderr; it is an error only if none of the requested stores exist. The
/// debounce window opens at the first file event after a quiet period and
/// closes `debounce` later, so a continuously-busy store still produces an
/// event every `debounce` rather than starving the caller.
///
/// This never returns `Ok` in practice — the watcher lives as long as the
/// call — so a caller wanting a bounded run should `select!` against it or
/// drop the task. Ctrl-C ends the `cider` binary the ordinary way.
pub async fn watch(
    sources: &[WatchSource],
    debounce: Duration,
    mut on_event: impl FnMut(WatchEvent),
) -> anyhow::Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot locate Apple data stores")?;

    let mut roots: Vec<(WatchSource, PathBuf)> = Vec::new();
    let mut seen: Vec<WatchSource> = Vec::new();
    for &source in sources {
        if seen.contains(&source) {
            continue;
        }
        seen.push(source);
        for path in store_paths(&source, &home) {
            if path.is_dir() {
                roots.push((source, path));
            } else {
                eprintln!(
                    "cider watch: skipping {source}: {} does not exist",
                    path.display()
                );
            }
        }
    }
    if roots.is_empty() {
        anyhow::bail!("none of the requested stores exist on this machine");
    }

    // The FSEvents thread pushes raw file events here; the loop below owns
    // the debounce clock, so the callback stays trivial.
    let (tx, mut rx) = mpsc::unbounded_channel::<(WatchSource, String)>();
    let callback_roots = roots.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        match result {
            Ok(event) if is_content_change(&event.kind) => {
                for path in event.paths {
                    if let Some(source) = source_for(&callback_roots, &path) {
                        // A closed receiver means `watch` is unwinding; the
                        // watcher is dropped right behind it.
                        let _ = tx.send((source, path.to_string_lossy().into_owned()));
                    }
                }
            }
            Ok(_) => {}
            Err(err) => eprintln!("cider watch: {err}"),
        }
    })
    .context("failed to start the filesystem watcher")?;
    for (source, root) in &roots {
        watcher
            .watch(root, RecursiveMode::Recursive)
            .with_context(|| format!("failed to watch {source} store {}", root.display()))?;
    }
    eprintln!(
        "cider watch: watching {} (Ctrl-C to stop)",
        seen.iter()
            .filter(|source| roots.iter().any(|(s, _)| s == *source))
            .map(|source| source.name())
            .collect::<Vec<_>>()
            .join(", ")
    );

    while let Some(first) = rx.recv().await {
        let deadline = Instant::now() + debounce;
        let mut pending = vec![first];
        // Collect until the window closes (`Err`) or the sender is gone.
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
            pending.push(event);
        }
        for event in coalesce(pending, Utc::now()) {
            on_event(event);
        }
    }

    drop(watcher);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode};

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn store_paths_return_expected_suffixes_under_home() {
        let home = Path::new("/Users/fake");
        let expected = [
            (
                WatchSource::Reminders,
                "Library/Group Containers/group.com.apple.reminders/Container_v1/Stores",
            ),
            (
                WatchSource::Calendar,
                "Library/Group Containers/group.com.apple.calendar",
            ),
            (
                WatchSource::Contacts,
                "Library/Application Support/AddressBook",
            ),
            (
                WatchSource::Notes,
                "Library/Group Containers/group.com.apple.notes",
            ),
            (
                WatchSource::Home,
                "Library/Containers/com.apple.Home/Data/Library/Caches/com.apple.HomeKit/com.apple.Home/com.apple.HomeKit.configurations",
            ),
            (WatchSource::Shortcuts, "Library/Shortcuts"),
        ];
        for (source, suffix) in expected {
            let paths = store_paths(&source, home);
            assert_eq!(
                paths.len(),
                1,
                "{source} should map to exactly one directory"
            );
            assert_eq!(paths[0], home.join(suffix), "{source}");
        }
    }

    #[test]
    fn coalesce_merges_duplicates_and_groups_by_source() {
        let events = vec![
            (
                WatchSource::Reminders,
                "/a/Stores/Data.sqlite-wal".to_string(),
            ),
            (WatchSource::Notes, "/n/NoteStore.sqlite".to_string()),
            (
                WatchSource::Reminders,
                "/a/Stores/Data.sqlite-wal".to_string(),
            ),
            (WatchSource::Reminders, "/a/Stores/Data.sqlite".to_string()),
            (WatchSource::Notes, "/n/NoteStore.sqlite".to_string()),
        ];
        let out = coalesce(events, at());
        assert_eq!(
            out,
            vec![
                WatchEvent {
                    source: WatchSource::Reminders,
                    at: at(),
                    kind: WatchKind::FilesChanged,
                    paths: vec![
                        "/a/Stores/Data.sqlite-wal".to_string(),
                        "/a/Stores/Data.sqlite".to_string(),
                    ],
                },
                WatchEvent {
                    source: WatchSource::Notes,
                    at: at(),
                    kind: WatchKind::FilesChanged,
                    paths: vec!["/n/NoteStore.sqlite".to_string()],
                },
            ]
        );
    }

    #[test]
    fn cli_lines_become_store_changed_events_without_paths() {
        let event = event_from_cli(&serde_json::json!({
            "source": "calendar", "at": "2026-09-01T12:00:00Z", "kind": "store_changed"
        }))
        .unwrap();
        assert_eq!(
            event,
            WatchEvent {
                source: WatchSource::Calendar,
                at: at(),
                kind: WatchKind::StoreChanged,
                paths: vec![],
            }
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "store_changed");
        assert!(json.get("paths").is_none(), "no paths key: {json}");
        // Round-trips, since a consumer may feed lines back in.
        assert_eq!(serde_json::from_value::<WatchEvent>(json).unwrap(), event);

        // Unknown source or missing source: not an event.
        assert!(event_from_cli(&serde_json::json!({"source": "mail"})).is_none());
        assert!(event_from_cli(&serde_json::json!({"kind": "store_changed"})).is_none());
        // A bad timestamp still yields an event, stamped now.
        assert!(event_from_cli(&serde_json::json!({"source": "contacts", "at": "soon"})).is_some());
    }

    #[test]
    fn via_splits_sources_between_cli_and_fsevents() {
        use WatchSource::*;
        let all = WatchSource::ALL;
        let (cli, fs) = split_sources(&all, Via::Auto, true).unwrap();
        assert_eq!(cli, vec![Reminders, Calendar, Contacts]);
        assert_eq!(fs, vec![Notes, Home, Shortcuts]);

        let (cli, fs) = split_sources(&all, Via::Auto, false).unwrap();
        assert!(cli.is_empty());
        assert_eq!(fs, all.to_vec());

        let (cli, fs) = split_sources(&all, Via::Fsevents, true).unwrap();
        assert!(cli.is_empty());
        assert_eq!(fs, all.to_vec());

        let (cli, fs) = split_sources(&[Reminders, Reminders, Notes], Via::Cli, true).unwrap();
        assert_eq!(cli, vec![Reminders], "duplicates collapse");
        assert_eq!(
            fs,
            vec![Notes],
            "--via cli keeps FSEvents for uncovered sources"
        );

        let error = split_sources(&all, Via::Cli, false).unwrap_err();
        assert!(
            error
                .downcast_ref::<crate::sources::bridge::BridgeError>()
                .is_some_and(|e| *e == crate::sources::bridge::BridgeError::CliNotInstalled),
            "{error}"
        );

        for (via, name) in Via::NAMES.iter().map(|n| (n.parse::<Via>().unwrap(), *n)) {
            assert_eq!(via.name(), name);
        }
        assert_eq!(Via::default(), Via::Auto);
        assert!("socket".parse::<Via>().is_err());
    }

    #[test]
    fn coalesce_of_nothing_is_empty() {
        assert!(coalesce(Vec::new(), at()).is_empty());
    }

    #[test]
    fn source_names_round_trip_through_serde_and_from_str() {
        for (source, name) in WatchSource::ALL.into_iter().zip(WatchSource::NAMES) {
            assert_eq!(source.name(), name);
            assert_eq!(name.parse::<WatchSource>().unwrap(), source);
            assert_eq!(
                serde_json::to_string(&source).unwrap(),
                format!("\"{name}\"")
            );
            assert_eq!(
                serde_json::from_str::<WatchSource>(&format!("\"{name}\"")).unwrap(),
                source
            );
        }
        let err = "mail".parse::<WatchSource>().unwrap_err().to_string();
        assert!(err.contains("unknown watch source"), "{err}");
        assert!(err.contains("reminders"), "{err}");
    }

    #[test]
    fn watch_event_serializes_with_snake_case_source() {
        let event = WatchEvent {
            source: WatchSource::Home,
            at: at(),
            kind: WatchKind::FilesChanged,
            paths: vec!["/x".to_string()],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["source"], "home");
        assert_eq!(json["at"], "2026-09-01T12:00:00Z");
        assert_eq!(json["kind"], "files_changed");
        assert_eq!(json["paths"], serde_json::json!(["/x"]));
    }

    #[test]
    fn content_change_filter_drops_access_and_metadata_only() {
        assert!(!is_content_change(&EventKind::Access(AccessKind::Read)));
        assert!(!is_content_change(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::AccessTime)
        )));
        assert!(is_content_change(&EventKind::Create(CreateKind::File)));
        assert!(is_content_change(&EventKind::Modify(ModifyKind::Data(
            DataChange::Content
        ))));
        assert!(is_content_change(&EventKind::Modify(ModifyKind::Name(
            RenameMode::Any
        ))));
        assert!(is_content_change(&EventKind::Remove(RemoveKind::File)));
        assert!(is_content_change(&EventKind::Any));
    }

    #[test]
    fn source_for_matches_the_root_a_path_lives_under() {
        let roots = vec![
            (WatchSource::Notes, PathBuf::from("/h/notes")),
            (
                WatchSource::Shortcuts,
                PathBuf::from("/h/Library/Shortcuts"),
            ),
        ];
        assert_eq!(
            source_for(&roots, Path::new("/h/notes/NoteStore.sqlite-wal")),
            Some(WatchSource::Notes)
        );
        assert_eq!(
            source_for(&roots, Path::new("/h/Library/Shortcuts/Shortcuts.sqlite")),
            Some(WatchSource::Shortcuts)
        );
        // Prefix match is by path component, not by string.
        assert_eq!(source_for(&roots, Path::new("/h/notesX/f")), None);
        assert_eq!(source_for(&roots, Path::new("/elsewhere")), None);
    }
}
