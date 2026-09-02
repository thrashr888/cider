//! Live smoke suite: runs every read-only command against the Mac it is on.
//!
//! Ignored by default because CI runners have no user data and a JXA call can
//! open a permission dialog. Run it on a real machine before a release:
//!
//! ```sh
//! cargo test --test live -- --ignored --nocapture
//! ```
//!
//! The suite exists because `find-my` shipped with a reader that swallowed
//! every failure and printed `[]`, which no unit test could see. Three rules
//! catch that class of bug:
//!
//! 1. Every command finishes within the timeout and prints JSON.
//! 2. A non-zero exit still prints a typed error envelope.
//! 3. If `doctor` reports a backing store as `ok`, the command that reads it
//!    must succeed and return at least one row. A present store never yields
//!    an empty list silently.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(120);

/// The only subcommands this suite is allowed to invoke. Everything else a
/// source offers (create, update, delete, send, complete, ...) mutates the
/// machine and must never run from a test. `guard_read_only` enforces it.
const READ_VERBS: &[&str] = &["list", "status", "show", "lists", "networks", "watchlists"];

/// Commands the generic walk must not run as-is, and why.
const SKIP: &[(&str, &str)] = &[
    ("watch", "runs until interrupted"),
    ("schema", "not a data source"),
    (
        "spotlight",
        "requires --query; covered by an explicit case below",
    ),
];

/// Commands that need arguments to be a fair read-only smoke test.
fn explicit_cases() -> Vec<Vec<&'static str>> {
    vec![
        vec!["notes", "list", "--brief"],
        vec!["messages", "list", "--days", "7"],
        vec!["console", "--minutes", "5"],
        vec!["spotlight", "--query", "cider"],
        vec!["reminders", "list", "--since", "2000-01-01T00:00:00Z"],
        vec!["calendar", "list", "--since", "2000-01-01T00:00:00Z"],
    ]
}

/// doctor check name → command whose default read depends on that store.
const STORE_BACKED: &[(&str, &[&str])] = &[
    ("calendar_database", &["calendar"]),
    ("contacts_database", &["contacts"]),
    ("reminders_stores", &["reminders"]),
    ("mail_database", &["mail"]),
    ("home_cache", &["home"]),
    ("shortcuts_database", &["shortcuts"]),
];

/// Refuse to run anything that is not a bare command or a read verb.
fn guard_read_only(args: &[&str]) {
    if let Some(verb) = args.get(1) {
        if !verb.starts_with("--") {
            assert!(
                READ_VERBS.contains(verb),
                "refusing to run `cider {}`: `{verb}` is not a read-only verb",
                args.join(" ")
            );
        }
    }
    for flag in args {
        assert!(
            !matches!(*flag, "--force" | "--yes" | "--delete"),
            "refusing to run `cider {}`: destructive flag",
            args.join(" ")
        );
    }
}

struct Run {
    args: Vec<String>,
    code: Option<i32>,
    stdout: String,
    stderr: String,
    elapsed: Duration,
    timed_out: bool,
}

fn run(args: &[&str]) -> Run {
    guard_read_only(args);
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cider"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cider");
    let mut out = child.stdout.take().expect("stdout");
    let mut err = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut o = String::new();
        let mut e = String::new();
        let _ = out.read_to_string(&mut o);
        let _ = err.read_to_string(&mut e);
        let _ = tx.send((o, e));
    });
    let mut timed_out = false;
    let code = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break status.code(),
            None if started.elapsed() > TIMEOUT => {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    let (stdout, stderr) = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    Run {
        args: args.iter().map(|s| s.to_string()).collect(),
        code,
        stdout,
        stderr,
        elapsed: started.elapsed(),
        timed_out,
    }
}

fn json(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim()).ok()
}

/// Rules 1 and 2. Returns a failure description, or None when the run is
/// acceptable.
fn check_shape(run: &Run) -> Option<String> {
    if run.timed_out {
        return Some(format!("hung for more than {TIMEOUT:?}"));
    }
    // Data goes to stdout; a failure prints its envelope on stderr.
    let stream = if run.code == Some(0) {
        &run.stdout
    } else {
        &run.stderr
    };
    let Some(value) = json(stream) else {
        return Some(format!(
            "no JSON on {} (exit {:?}): {}",
            if run.code == Some(0) {
                "stdout"
            } else {
                "stderr"
            },
            run.code,
            stream
                .trim()
                .lines()
                .last()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>()
        ));
    };
    match run.code {
        Some(0) => None,
        _ => {
            let code = value
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if value.get("ok") == Some(&Value::Bool(false)) && !code.is_empty() {
                None
            } else {
                Some(format!(
                    "exit {:?} without a typed error envelope: {}",
                    run.code,
                    stream.chars().take(200).collect::<String>()
                ))
            }
        }
    }
}

/// Rule 3: a present store must yield rows.
fn check_store_backed(run: &Run, doctor: &BTreeMap<String, String>) -> Option<String> {
    let head = run.args.first().map(String::as_str).unwrap_or("");
    if run.args.len() != 1 {
        return None;
    }
    let store = STORE_BACKED
        .iter()
        .find(|(_, commands)| commands.contains(&head))
        .map(|(store, _)| *store)?;
    if doctor.get(store).map(String::as_str) != Some("ok") {
        return None;
    }
    let rows = json(&run.stdout)
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(1);
    if run.code != Some(0) {
        Some(format!(
            "doctor reports {store} ok but the command failed: {}",
            run.stderr.trim().chars().take(200).collect::<String>()
        ))
    } else if rows == 0 {
        Some(format!(
            "doctor reports {store} ok but the command returned an empty list (the find-my bug)"
        ))
    } else {
        None
    }
}

fn doctor_statuses() -> BTreeMap<String, String> {
    let run = run(&["doctor"]);
    let value = json(&run.stdout).expect("doctor prints JSON");
    value["checks"]
        .as_array()
        .expect("doctor checks")
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap_or("").to_string(),
                c["status"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn read_only_commands() -> Vec<String> {
    let run = run(&["schema"]);
    let value = json(&run.stdout).expect("schema prints JSON");
    value["commands"]
        .as_array()
        .expect("schema commands")
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .filter(|name| !SKIP.iter().any(|(skip, _)| skip == name))
        .collect()
}

#[test]
#[ignore = "needs a real Mac with user data; run with --ignored before a release"]
fn every_read_only_command_answers_honestly() {
    let doctor = doctor_statuses();
    let mut failures = Vec::new();
    let mut summary = Vec::new();

    let mut cases: Vec<Vec<String>> = read_only_commands()
        .into_iter()
        .map(|name| vec![name])
        .collect();
    cases.extend(
        explicit_cases()
            .into_iter()
            .map(|c| c.into_iter().map(str::to_string).collect()),
    );

    for case in &cases {
        let args: Vec<&str> = case.iter().map(String::as_str).collect();
        let run = run(&args);
        let rows = json(&run.stdout)
            .and_then(|v| v.as_array().map(Vec::len))
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string());
        summary.push(format!(
            "{:<45} exit={:<5} rows={:<5} {:.1}s",
            args.join(" "),
            run.code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "hung".into()),
            rows,
            run.elapsed.as_secs_f32()
        ));
        for problem in [check_shape(&run), check_store_backed(&run, &doctor)]
            .into_iter()
            .flatten()
        {
            failures.push(format!("{}: {problem}", args.join(" ")));
        }
    }

    println!("{}", summary.join("\n"));
    assert!(
        failures.is_empty(),
        "{} of {} runs failed:\n  {}",
        failures.len(),
        cases.len(),
        failures.join("\n  ")
    );
}

#[test]
#[should_panic(expected = "not a read-only verb")]
fn guard_refuses_mutating_verbs() {
    guard_read_only(&["reminders", "delete", "--id", "x"]);
}

#[test]
fn guard_allows_bare_commands_and_read_verbs() {
    guard_read_only(&["reminders"]);
    guard_read_only(&["notes", "list", "--brief"]);
    guard_read_only(&["console", "--minutes", "5"]);
}

#[test]
fn skip_reasons_and_store_map_are_well_formed() {
    for (name, reason) in SKIP {
        assert!(!name.is_empty() && !reason.is_empty());
    }
    for (store, commands) in STORE_BACKED {
        assert!(!store.is_empty() && !commands.is_empty());
    }
}
