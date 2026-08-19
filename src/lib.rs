//! cider as a library.
//!
//! Every module under [`sources`] talks to one macOS app, returning plain
//! serde types rather than the JSON the `cider` binary prints. The binary is a
//! thin Clap front-end over exactly these functions, so a Rust caller can skip
//! the subprocess, the JSON round-trip, and the "is cider installed, and is it
//! new enough?" question entirely.
//!
//! ```no_run
//! # async fn demo() -> anyhow::Result<()> {
//! for r in cider::sources::reminders::list(Some("Shopping")).await? {
//!     println!("{} {}", r.id, r.title);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Everything here shells out to macOS's own tools — `osascript`, `sqlite3`,
//! and friends — so it needs no third-party binary on PATH, but it does need
//! the same TCC permissions the app itself would: a caller inheriting a
//! sandbox or missing Full Disk Access sees the same denials the CLI reports.
//!
//! `pretty` is behind the `cli` feature, which the binary turns on; a library
//! consumer wants the data, not the tables.

pub mod sources;

#[cfg(feature = "cli")]
pub mod pretty;
