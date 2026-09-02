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
//!
//! A second section covers the Cider Bridge read paths (`bridge status`,
//! `home --live`, `home state`, `home triggers`, `weather`, ...) and runs
//! only when the app is installed. It is the one place the suite has a
//! visible side effect: `--live` launches the bridge app if it is not
//! running, so the suite prints a line before it does and leaves the app
//! running afterwards (quitting would change state the user may rely on).
//! Everything it runs is still a read.

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
const READ_VERBS: &[&str] = &[
    "list",
    "status",
    "show",
    "lists",
    "networks",
    "watchlists",
    "homes",
    "scenes",
    "state",
    "triggers",
    "quota",
];

/// Flags that consume the next token as a value. Any other flag is taken
/// as bare, so the token after it is still checked as a verb: an unknown
/// value-taking flag fails loudly instead of letting a verb slip past.
const VALUE_FLAGS: &[&str] = &["--days", "--minutes", "--query", "--since"];

/// Commands the generic walk must not run as-is, and why.
const SKIP: &[(&str, &str)] = &[
    ("watch", "runs until interrupted"),
    ("weather", "launches Cider Bridge and needs WeatherKit"),
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
        vec!["icloud", "list"],
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
    ("icloud_drive", &["icloud", "list"]),
];

/// Refuse to run anything that is not a bare command or a read verb.
///
/// Every positional token after the command is a verb and must be in
/// `READ_VERBS`, wherever it sits: `home --live homes` and `home triggers
/// list` are both checked, and so is the `create-timer` in `home triggers
/// create-timer`. Flags are skipped, along with the value of a flag in
/// `VALUE_FLAGS`.
fn guard_read_only(args: &[&str]) {
    let mut expect_value = false;
    for token in args.iter().skip(1) {
        if expect_value {
            expect_value = false;
            continue;
        }
        if token.starts_with("--") {
            expect_value = VALUE_FLAGS.contains(token);
            continue;
        }
        assert!(
            READ_VERBS.contains(token),
            "refusing to run `cider {}`: `{token}` is not a read-only verb",
            args.join(" ")
        );
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
    // The mapping names the exact invocation that reads the store, so a
    // command whose default subcommand is something else (`icloud` →
    // `account`) is judged on `icloud list`, not on the bare command.
    let store = STORE_BACKED
        .iter()
        .find(|(_, commands)| {
            commands.len() == run.args.len() && commands.iter().zip(&run.args).all(|(a, b)| a == b)
        })
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
        summary.push(summarize(&run));
        for problem in [check_shape(&run), check_store_backed(&run, &doctor)]
            .into_iter()
            .flatten()
        {
            failures.push(format!("{}: {problem}", args.join(" ")));
        }
    }

    let bridge_runs = bridge_section(&mut summary, &mut failures);

    println!("{}", summary.join("\n"));
    assert!(
        failures.is_empty(),
        "{} of {} runs failed:\n  {}",
        failures.len(),
        cases.len() + bridge_runs,
        failures.join("\n  ")
    );
}

/// One summary line per run, in the same shape for both sections.
fn summarize(run: &Run) -> String {
    let rows = row_count(run)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "{:<45} exit={:<5} rows={:<5} {:.1}s",
        run.args.join(" "),
        run.code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "hung".into()),
        rows,
        run.elapsed.as_secs_f32()
    )
}

/// The `error.code` of a failed run's envelope, if it printed one.
fn error_code(run: &Run) -> Option<String> {
    json(&run.stderr)?
        .get("error")?
        .get("code")?
        .as_str()
        .map(str::to_string)
}

fn row_count(run: &Run) -> Option<usize> {
    json(&run.stdout).and_then(|v| v.as_array().map(Vec::len))
}

/// The cases of the bridge section, in order. `bridge status` runs first
/// on its own because it decides whether the rest run at all.
const BRIDGE_CASES: &[&[&str]] = &[
    &["home", "--live", "homes"],
    &["home", "--live", "scenes"],
    &["home", "state"],
    &["home", "triggers"],
    &["weather"],
    &["weather", "--days", "2"],
    &["permissions"],
    &["icloud", "quota"],
];

/// The bridge read paths. Skipped, with a line saying so, when no
/// `Cider Bridge.app` is installed. Returns how many runs it made.
fn bridge_section(summary: &mut Vec<String>, failures: &mut Vec<String>) -> usize {
    let status = run(&["bridge", "status"]);
    summary.push(summarize(&status));
    failures.extend(check_shape(&status).map(|p| format!("bridge status: {p}")));
    let Some(value) = json(&status.stdout) else {
        return 1;
    };
    if value["installed"] != Value::Bool(true) {
        println!("live: Cider Bridge is not installed; skipping the bridge section");
        return 1;
    }
    if value["running"] != Value::Bool(true) {
        println!("live: launching Cider Bridge for the bridge section");
    }

    for case in BRIDGE_CASES {
        let run = run(case);
        summary.push(summarize(&run));
        let name = case.join(" ");
        failures.extend(check_shape(&run).map(|p| format!("{name}: {p}")));
        failures.extend(check_bridge_case(&run).map(|p| format!("{name}: {p}")));
    }
    println!("live: Cider Bridge left running (the suite never quits it)");
    BRIDGE_CASES.len() + 1
}

/// Rules specific to the bridge section, on top of `check_shape`: an
/// installed bridge asked for its homes must answer with at least one (a
/// present bridge that says `[]` is the find-my bug again), and `weather`
/// may fail only with `weather_unavailable`.
fn check_bridge_case(run: &Run) -> Option<String> {
    let args: Vec<&str> = run.args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["home", "--live", "homes"] => match (run.code, error_code(run)) {
            (Some(0), _) if row_count(run) == Some(0) => Some(
                "bridge is installed but the command returned an empty list (the find-my bug)"
                    .into(),
            ),
            (Some(0), _) => None,
            // A packaged bridge without the HomeKit entitlement says so.
            (_, Some(code)) if code == "homekit_unavailable" => None,
            (code, error) => Some(format!(
                "bridge is installed but the command failed (exit {code:?}, code {error:?})"
            )),
        },
        ["weather", ..] => match (run.code, error_code(run)) {
            (Some(0), _) => None,
            (_, Some(code)) if code == "weather_unavailable" => None,
            (code, error) => Some(format!(
                "may only fail with weather_unavailable, got exit {code:?} code {error:?}"
            )),
        },
        _ => None,
    }
}

#[test]
#[should_panic(expected = "not a read-only verb")]
fn guard_refuses_mutating_verbs() {
    guard_read_only(&["reminders", "delete", "--id", "x"]);
}

/// Every mutating verb the bridge, iCloud, and Shortcuts surfaces offer is
/// refused wherever it sits in the argument list, including after `--live`
/// and under `triggers`.
#[test]
fn guard_refuses_every_mutating_verb() {
    let mutating: &[&[&str]] = &[
        &[
            "home",
            "set",
            "--accessory",
            "a",
            "--characteristic",
            "c",
            "--value",
            "1",
        ],
        &["home", "run", "--scene", "s"],
        &["home", "--live", "run", "--scene", "s"],
        &["home", "triggers", "create-timer", "--name", "n"],
        &["home", "triggers", "delete", "--trigger", "t"],
        &["home", "triggers", "enable", "--trigger", "t"],
        &["home", "triggers", "disable", "--trigger", "t"],
        &["bridge", "install"],
        &["bridge", "build"],
        &["icloud", "download", "--path", "p"],
        &["icloud", "evict", "--path", "p"],
        &["shortcuts", "run", "--name", "n"],
        &["shortcuts", "install", "--input", "f"],
    ];
    for args in mutating {
        let message = std::panic::catch_unwind(|| guard_read_only(args))
            .expect_err(&format!("guard let `cider {}` through", args.join(" ")))
            .downcast::<String>()
            .map(|s| *s)
            .unwrap_or_default();
        assert!(
            message.contains("not a read-only verb"),
            "`cider {}` was refused for the wrong reason: {message}",
            args.join(" ")
        );
    }
}

#[test]
fn guard_allows_bare_commands_and_read_verbs() {
    guard_read_only(&["reminders"]);
    guard_read_only(&["notes", "list", "--brief"]);
    guard_read_only(&["console", "--minutes", "5"]);
    guard_read_only(&["spotlight", "--query", "cider"]);
    guard_read_only(&["reminders", "list", "--since", "2000-01-01T00:00:00Z"]);
    guard_read_only(&["home", "triggers", "list"]);
    guard_read_only(&["bridge", "status"]);
    for case in BRIDGE_CASES {
        guard_read_only(case);
    }
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
