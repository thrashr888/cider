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
//! macOS records those grants against the **responsible process** — the app
//! that launched the tool, which for a library consumer is the host app
//! itself. The host therefore has to declare the usage strings in
//! [`HOST_INFO_PLIST_KEYS`] before macOS will show a prompt on its behalf;
//! [`permissions::report`] lists every permission cider can need, who it is
//! granted to, and its state, without opening a dialog:
//!
//! ```no_run
//! # async fn demo() {
//! let report = cider::permissions::report().await;
//! for r in &report.requirements {
//!     println!("{:<28} {:<15} {}", r.permission.label(), r.status.as_str(), r.how_to_grant);
//! }
//! # }
//! ```
//!
//! `pretty` is behind the `cli` feature, which the binary turns on; a library
//! consumer wants the data, not the tables.

pub mod sources;

pub use sources::doctor::{self, inspect, DoctorReport};
pub use sources::permissions::{
    self, report, GrantedTo, Permission, PermissionStatus, PermissionsReport, Requirement,
    HOST_INFO_PLIST_KEYS,
};

#[cfg(feature = "cli")]
pub mod pretty;
