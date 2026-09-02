//! Foreground change stream over the on-disk stores behind Apple apps.
//!
//! `cider watch` is not a daemon. It registers FSEvents watches on the
//! directories Reminders, Calendar, Notes, Home, and Shortcuts write to,
//! folds the burst of file events one edit produces into a single
//! [`WatchEvent`] per source, and hands each to the caller until the process
//! is killed. It is the polling-free alternative to re-running `cider
//! reminders` on a timer: an event says *that* a store changed, and the
//! caller re-reads it with the matching `sources` function to learn *what*.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::Instant;

/// A local data store [`watch`] can observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchSource {
    Reminders,
    Calendar,
    Notes,
    Home,
    Shortcuts,
}

impl WatchSource {
    /// Every source, in the order `cider watch` defaults to.
    pub const ALL: [WatchSource; 5] = [
        Self::Reminders,
        Self::Calendar,
        Self::Notes,
        Self::Home,
        Self::Shortcuts,
    ];

    /// The CLI and JSON spelling of each source, parallel to [`Self::ALL`].
    pub const NAMES: [&'static str; 5] = ["reminders", "calendar", "notes", "home", "shortcuts"];

    /// The CLI and JSON spelling of this source.
    pub fn name(self) -> &'static str {
        match self {
            Self::Reminders => "reminders",
            Self::Calendar => "calendar",
            Self::Notes => "notes",
            Self::Home => "home",
            Self::Shortcuts => "shortcuts",
        }
    }
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

/// One coalesced change notification: everything that changed under one
/// source's store within a debounce window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEvent {
    pub source: WatchSource,
    /// When the window closed and the event was emitted.
    pub at: DateTime<Utc>,
    /// De-duplicated absolute paths of the files that changed, in first-seen
    /// order. Always non-empty.
    pub paths: Vec<String>,
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
                    paths: vec![
                        "/a/Stores/Data.sqlite-wal".to_string(),
                        "/a/Stores/Data.sqlite".to_string(),
                    ],
                },
                WatchEvent {
                    source: WatchSource::Notes,
                    at: at(),
                    paths: vec!["/n/NoteStore.sqlite".to_string()],
                },
            ]
        );
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
            paths: vec!["/x".to_string()],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["source"], "home");
        assert_eq!(json["at"], "2026-09-01T12:00:00Z");
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
