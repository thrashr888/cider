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

#[cfg(test)]
mod no_stdio_in_library {
    use std::path::Path;

    /// Library code must never touch stdout or stderr. Alchemy links this
    /// crate into a Tauri app where stderr can be closed, and `eprintln!` on a
    /// closed stderr panics — it has aborted that app in the field. Diagnostics
    /// go through the `log` facade instead, which is a no-op until a consumer
    /// installs a logger; the `cider` binary installs one and keeps the same
    /// stderr output users have always seen. `src/main.rs` and anything behind
    /// the `cli` feature may still print.
    #[test]
    fn no_print_macros_outside_the_cli() {
        // Assembled rather than written out, so this test does not trip over
        // its own source when it scans `lib.rs`.
        let macros: Vec<String> = ["println", "eprintln", "print", "eprint"]
            .iter()
            .map(|name| format!("{name}!"))
            .collect();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![root.join("lib.rs")];
        let mut dirs = vec![root.join("sources")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("sources directory is readable") {
                let path = entry.expect("readable directory entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }
        files.sort();

        let mut offenders = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(file).expect("source file is readable");
            for (number, line) in text.lines().enumerate() {
                let code = line.trim_start();
                // Doc comments and ordinary comments show callers what to do
                // with the data; they are prose, not prints.
                if code.starts_with("//") {
                    continue;
                }
                if macros.iter().any(|macro_name| code.contains(macro_name)) {
                    offenders.push(format!(
                        "{}:{}: {}",
                        file.file_name().unwrap().to_string_lossy(),
                        number + 1,
                        code.trim()
                    ));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "library code must log, not print — use log::warn!/info!/debug! instead:\n{}",
            offenders.join("\n")
        );
    }
}
