//! Read retained local Biome streams. No daemon, stream registration, or writes.
mod protobuf;
mod segb;
use super::{
    keyed_archive::{decode_bytes, hex_encode},
    local_store::{home_path, read_error, HistoryOptions},
};
use anyhow::{ensure, Context};
use chrono::{DateTime, Utc};
pub use protobuf::{Field as ProtobufField, Value as ProtobufValue};
use serde::Serialize;
use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub const STREAMS_RELATIVE_PATH: &str = "Library/Biome/streams";
const MAX_SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SCAN_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Namespace {
    #[default]
    Restricted,
    Public,
}
impl Namespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restricted => "restricted",
            Self::Public => "public",
        }
    }
}
impl std::str::FromStr for Namespace {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "restricted" => Ok(Self::Restricted),
            "public" => Ok(Self::Public),
            _ => anyhow::bail!("invalid Biome namespace: {s}"),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct BiomeStream {
    pub name: String,
    pub namespace: Namespace,
    pub segment_count: usize,
    pub bytes: u64,
}
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub stream: String,
    pub namespace: Namespace,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: u32,
    pub offset: u32,
    pub raw: bool,
}
impl Default for ListOptions {
    fn default() -> Self {
        Self {
            stream: "App.InFocus".into(),
            namespace: Namespace::Restricted,
            since: None,
            until: None,
            limit: 100,
            offset: 0,
            raw: false,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum Payload {
    Protobuf { fields: Vec<ProtobufField> },
    Plist { value: serde_json::Value },
    Opaque { byte_length: usize, reason: String },
}
#[derive(Debug, Serialize)]
pub struct BiomeEvent {
    /// Namespace, stream, segment filename, and byte offset; stable while retained.
    pub id: String,
    pub stream: String,
    pub namespace: Namespace,
    pub segment: String,
    pub offset: usize,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_timestamp: Option<DateTime<Utc>>,
    pub crc_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u64>,
    pub payload: Payload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_hex: Option<String>,
}
pub async fn streams() -> anyhow::Result<Vec<BiomeStream>> {
    let root = home_path(STREAMS_RELATIVE_PATH)?;
    tokio::task::spawn_blocking(move || streams_at(&root)).await?
}
pub async fn list(options: &ListOptions) -> anyhow::Result<Vec<BiomeEvent>> {
    let root = home_path(STREAMS_RELATIVE_PATH)?;
    let options = options.clone();
    tokio::task::spawn_blocking(move || list_at(&root, &options)).await?
}
fn check_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
        "invalid Biome stream name"
    );
    Ok(())
}
fn real_directory(path: &Path) -> anyhow::Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|e| read_error(path, e))?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "Invalid Biome directory (symlinks are not followed): {}",
        path.display()
    );
    Ok(())
}
fn entries(path: &Path) -> anyhow::Result<Vec<fs::DirEntry>> {
    fs::read_dir(path)
        .map_err(|e| read_error(path, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| read_error(path, e))
}
fn segments(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    real_directory(path)?;
    let local = path.join("local");
    match fs::symlink_metadata(&local) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(read_error(&local, e)),
        Ok(_) => {}
    }
    real_directory(&local)?;
    let mut files = Vec::new();
    for entry in entries(&local)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file(),
            "Invalid Biome segment (expected regular file): {}",
            entry.path().display()
        );
        files.push(entry.path());
    }
    files.sort();
    Ok(files)
}
fn streams_at(root: &Path) -> anyhow::Result<Vec<BiomeStream>> {
    real_directory(root)?;
    let mut streams = Vec::new();
    for namespace in [Namespace::Restricted, Namespace::Public] {
        let path = root.join(namespace.as_str());
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(read_error(&path, e)),
            Ok(_) => {}
        }
        real_directory(&path)?;
        for entry in entries(&path)? {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Invalid Biome stream filename"))?;
            check_name(&name)?;
            let files = segments(&entry.path())?;
            let bytes = files
                .iter()
                .try_fold(0u64, |sum, p| -> anyhow::Result<u64> {
                    Ok(sum + fs::metadata(p)?.len())
                })?;
            streams.push(BiomeStream {
                name,
                namespace,
                segment_count: files.len(),
                bytes,
            });
        }
    }
    streams.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.namespace.as_str().cmp(b.namespace.as_str()))
    });
    Ok(streams)
}
struct Candidate {
    id: String,
    segment: String,
    offset: usize,
    timestamp: DateTime<Utc>,
    end_timestamp: Option<DateTime<Utc>>,
    crc_valid: bool,
    data: Vec<u8>,
}
impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp && self.id == other.id
    }
}
impl Eq for Candidate {}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp
            .cmp(&other.timestamp)
            .then(self.id.cmp(&other.id))
    }
}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
fn list_at(root: &Path, o: &ListOptions) -> anyhow::Result<Vec<BiomeEvent>> {
    check_name(&o.stream)?;
    let filter = HistoryOptions {
        since: o.since,
        until: o.until,
        limit: o.limit,
        offset: o.offset,
        ..Default::default()
    };
    filter.validate()?;
    let take = (o.limit as usize)
        .checked_add(o.offset as usize)
        .context("Invalid Biome page size")?;
    ensure!(
        take <= 10_000,
        "invalid Biome page: limit + offset must be at most 10000; narrow the time range"
    );
    real_directory(root)?;
    let namespace = root.join(o.namespace.as_str());
    real_directory(&namespace)?;
    let paths = segments(&namespace.join(&o.stream))?;
    if o.limit == 0 {
        return Ok(vec![]);
    }
    let mut heap: BinaryHeap<Reverse<Candidate>> = BinaryHeap::new();
    let mut retained_bytes = 0;
    let mut scanned = 0;
    let started = Instant::now();
    for path in paths {
        let file = fs::File::open(&path).map_err(|e| read_error(&path, e))?;
        ensure!(
            file.metadata()?.len() <= MAX_SEGMENT_BYTES,
            "Biome segment exceeds 64 MiB: {}",
            path.display()
        );
        let mut bytes = Vec::new();
        file.take(MAX_SEGMENT_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_SEGMENT_BYTES,
            "Biome segment grew beyond 64 MiB"
        );
        scanned += bytes.len() as u64;
        ensure!(
            scanned <= MAX_SCAN_BYTES,
            "Biome stream exceeds 512 MiB scan budget"
        );
        ensure!(
            started.elapsed() < Duration::from_secs(20),
            "Biome scan timed out after 20 seconds"
        );
        let segment = path
            .file_name()
            .context("Missing segment name")?
            .to_string_lossy()
            .into_owned();
        segb::parse(&bytes, |r| {
            ensure!(
                started.elapsed() < Duration::from_secs(20),
                "Biome scan timed out after 20 seconds"
            );
            if !filter.contains(r.timestamp) {
                return Ok(());
            }
            let id = format!(
                "{}:{}:{}:{}",
                o.namespace.as_str(),
                o.stream,
                segment,
                r.offset
            );
            if heap.len() == take
                && heap
                    .peek()
                    .is_some_and(|min| (r.timestamp, &id) <= (min.0.timestamp, &min.0.id))
            {
                return Ok(());
            }
            if heap.len() == take {
                retained_bytes -= heap.pop().unwrap().0.data.len();
            }
            retained_bytes += r.data.len();
            ensure!(
                retained_bytes <= MAX_PAGE_BYTES,
                "Biome page exceeds 32 MiB; reduce --limit or narrow the time range"
            );
            heap.push(Reverse(Candidate {
                id,
                segment: segment.clone(),
                offset: r.offset,
                timestamp: r.timestamp,
                end_timestamp: r.end_timestamp,
                crc_valid: r.crc_valid,
                data: r.data.to_vec(),
            }));
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!("Cannot parse Biome segment {}: {e:#}", path.display()))?;
    }
    let mut rows = heap.into_iter().map(|r| r.0).collect::<Vec<_>>();
    rows.sort_by(|a, b| b.cmp(a));
    Ok(rows
        .into_iter()
        .skip(o.offset as usize)
        .take(o.limit as usize)
        .map(|r| event(r, o))
        .collect())
}
fn event(r: Candidate, o: &ListOptions) -> BiomeEvent {
    let decoded: Result<Payload, anyhow::Error> = if !r.crc_valid {
        Err(anyhow::anyhow!(
            "CRC mismatch; payload may have been overwritten"
        ))
    } else if r.data.len() > 1024 * 1024 {
        Err(anyhow::anyhow!("Payload exceeds 1 MiB decode limit"))
    } else if r.data.starts_with(b"bplist00") {
        decode_bytes(&r.data).map(|value| Payload::Plist { value })
    } else {
        protobuf::parse(&o.stream, &r.data).map(|fields| Payload::Protobuf { fields })
    };
    let payload = decoded.unwrap_or_else(|e| Payload::Opaque {
        byte_length: r.data.len(),
        reason: e.to_string(),
    });
    let (app, status_code) = match &payload {
        Payload::Protobuf { fields } => (
            protobuf::app(fields),
            fields.iter().find_map(|f| match (&f.name, &f.value) {
                (Some("status"), ProtobufValue::Unsigned(v)) => Some(*v),
                _ => None,
            }),
        ),
        _ => (None, None),
    };
    BiomeEvent {
        id: r.id,
        stream: o.stream.clone(),
        namespace: o.namespace,
        segment: r.segment,
        offset: r.offset,
        timestamp: r.timestamp,
        end_timestamp: r.end_timestamp,
        crc_valid: r.crc_valid,
        app,
        status_code,
        payload,
        raw_hex: o.raw.then(|| hex_encode(&r.data)),
    }
}
#[cfg(test)]
mod tests {
    use super::segb::tests::v2;
    use super::*;
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!("cider-biome-{}", uuid::Uuid::new_v4()));
        let local = root.join("restricted/App.InFocus/local");
        fs::create_dir_all(&local).unwrap();
        fs::write(
            local.join("100"),
            v2(&[
                (1, 1.0, b"\x18\x01\x32\x03app"),
                (3, 3.0, b"gone"),
                (1, 2.0, b"\x18\x00\x32\x03app"),
            ]),
        )
        .unwrap();
        fs::write(
            local.join("200"),
            v2(&[(1, 2.0, b"\x18\x01\x32\x03two"), (1, 4.0, b"\x00")]),
        )
        .unwrap();
        fs::create_dir_all(root.join("public/Empty/local")).unwrap();
        Fixture(root)
    }
    #[test]
    fn discovers_streams_and_pages_after_global_order_and_time_filter() {
        let f = fixture();
        let streams = streams_at(&f.0).unwrap();
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].segment_count, 2);
        let mut o = ListOptions {
            limit: 2,
            offset: 1,
            ..Default::default()
        };
        let events = list_at(&f.0, &o).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].app.as_deref(), Some("two"));
        assert_eq!(events[1].app.as_deref(), Some("app"));
        assert!(events[0].raw_hex.is_none());
        o.offset = 0;
        o.raw = true;
        o.since = super::super::local_store::apple_time(Some(2.0)).unwrap();
        o.until = super::super::local_store::apple_time(Some(4.0)).unwrap();
        let events = list_at(&f.0, &o).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].raw_hex.is_some());
        o.since = None;
        o.until = None;
        o.limit = 1;
        let events = list_at(&f.0, &o).unwrap();
        assert!(matches!(events[0].payload, Payload::Opaque { .. }));
        o.stream = "Empty".into();
        o.namespace = Namespace::Public;
        assert!(list_at(&f.0, &o).unwrap().is_empty());
        o.stream = "Missing".into();
        assert!(list_at(&f.0, &o).is_err());
    }
    #[test]
    fn rejects_traversal_symlinks_and_oversize_pages() {
        let f = fixture();
        for bad in ["../x", "/tmp", ".", "..", "a/b", "a\0b"] {
            let o = ListOptions {
                stream: bad.into(),
                ..Default::default()
            };
            assert!(list_at(&f.0, &o).is_err());
        }
        let o = ListOptions {
            offset: 10_000,
            ..Default::default()
        };
        assert!(list_at(&f.0, &o).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                f.0.join("restricted/App.InFocus"),
                f.0.join("restricted/Alias"),
            )
            .unwrap();
            let o = ListOptions {
                stream: "Alias".into(),
                ..Default::default()
            };
            assert!(list_at(&f.0, &o).is_err());
        }
    }
}
