---
name: cider-cli
description: Manage macOS Apple apps using the cider command-line tool. Use when reading or changing Reminders, Calendar, Contacts, Notes, Mail, Messages, Music, Safari, Keychain, screenshots, system settings, or other Apple app data from the command line.
---

# Cider CLI

Use `cider` to query and control Apple apps on macOS. Prefer explicit subcommands and machine-readable JSON output.

## When to Use This Skill

Use this skill when:

- Reading data from Apple apps such as Reminders, Calendar, Contacts, Notes, Mail, Safari, Photos, or Music
- Performing local Mac automation through Apple apps and system utilities
- Taking screenshots, running shortcuts, checking Wi-Fi, or inspecting system info
- The user mentions `cider` directly or asks for Apple app data from the terminal

## Running The CLI

If `cider` is installed, use it directly:

```bash
cider reminders list --list Shopping --limit 20
```

When working inside this repository, prefer `cargo run --`:

```bash
cargo run -- reminders list --list Shopping --limit 20
cargo run -- contacts list --search Smith --limit 10
```

Prefer explicit subcommands like `list`, `create`, or `delete` instead of relying on default actions.

## Discovery

Use the CLI itself to discover available commands and flags:

```bash
# Top-level command groups
cider --help

# Source-specific help
cider reminders --help
cider notes create --help

# Machine-readable capabilities
cider schema
cider schema --source reminders
```

Use `cider schema` to inspect the complete parser-derived command tree: actions, arguments, required/default values, read/write classification, `--dry-run`, and identifier contracts. Use `--help` for concise interactive help.

## Agent-Friendly Features

### JSON Output By Default

Default stdout is compact JSON and is the best choice for automation:

```bash
cider contacts list --search Smith | jq '.[].name'
cider calendar list | jq '.[0]'
```

Use `--pretty` only when a human needs tabular output.

### Stable Envelope

Use `--envelope` when you want a consistent top-level wrapper:

```bash
cider --envelope notes get --id abc123
```

This returns `{"ok": true, "data": ...}`.

### Dry Run Before Mutations

For sources that support it, always run `--dry-run` before write operations:

```bash
cider --dry-run reminders create --title "Buy milk"
cider --dry-run messages send --to "+15551234567" --text "On my way"
cider --dry-run notes delete --id note-123
```

`--dry-run` validates intent without performing the side effect.

## Recommended Agent Workflow

```bash
# 1. Discover the source and flags
cider reminders --help
cider reminders create --help
cider schema --source reminders

# 2. Inspect current state in JSON
cider reminders list --list Shopping

# 3. Dry-run the mutation
cider --dry-run reminders create --title "Buy milk" --list Shopping

# 4. Ask for confirmation before executing the real mutation
cider reminders create --title "Buy milk" --list Shopping
```

## Common Workflows

### Reminders And Calendar

```bash
cider reminders list --list Shopping --limit 20
cider reminders list --search invoice --include-completed
# What changed since a sync point (RFC 3339); calendar, notes, and messages
# list take --since too, and calendar events now carry modified_at.
cider reminders list --since 2026-09-01T00:00:00Z
cider reminders create --title "Buy milk" --list Shopping --due "2026-03-14T18:00:00Z"
# Complete/delete by --id from `list` — titles repeat, ids don't. A --title
# call acts on the first open match and reports when there were others.
cider reminders complete --id 4b7c5902-46a7-4f7a-a385-91b562ca8eb6
cider reminders batch-complete --id <id-1> --id <id-2>
# Writes go through the native cider-bridge CLI (EventKit) when it is
# installed, else AppleScript/JXA; --dry-run says which. CIDER_BRIDGE_CLI=off
# forces the native path. Reads (list, --since) always use SQLite.
CIDER_BRIDGE_CLI=off cider reminders complete --id <id>
cider calendar list --days-ahead 14
cider calendar get --id <event-id>
cider calendar update --id <event-id> --location "Zoom"
cider calendar create --title "1:1" --start "2026-03-15T17:00:00Z" --end "2026-03-15T17:30:00Z"
```

### Contacts And Notes

```bash
cider contacts list --search Smith --limit 10
cider contacts get --id contact-123
cider contacts create --first Jane --last Doe --email jane@work.example --email jane@home.example --job-title Engineer
cider notes list --folder Work --limit 20
cider notes create --title "Meeting Notes" --body "Agenda..."
```

### Messages, Mail, And Music

```bash
cider messages list --days 7 --limit 20
cider messages send --to "+15551234567" --text "On my way"
cider mail list --limit 10
cider mail list --search invoice --unread --limit 25
cider mail get --id '<message-id@example.com>'
cider mail batch-read --id '<message-1@example.com>' --id '<message-2@example.com>'
cider music status
cider music play --playlist Favorites
```

### System And Utility Commands

```bash
cider screenshots list --limit 20
cider screenshots capture --path ~/Desktop/capture.png
cider shortcuts run --name "Daily Briefing"
cider home state --room Office                 # live HomeKit values; needs Cider Bridge (`cider bridge status`)
cider home run --scene "Good Night"
cider home triggers create-timer --name "Porch on" --at 2026-09-01T19:30:00-07:00 --repeat daily --scene "Porch On"
cider wifi status
cider weather                                   # Apple Weather at the primary home's address; needs Cider Bridge
cider weather --forecast --days 5 --home "Casa"
cider weather --lat 37.75 --lon -122.49         # output includes the required `attribution` block
cider watch --source reminders --source calendar   # JSON line per change, runs until Ctrl-C
cider watch --source contacts --via fsevents       # force FSEvents instead of cider-bridge store_changed events
cider system-info show
cider doctor
cider auth-status
```

## Safety And Permissions

1. Always confirm with the user before any mutating command, especially `create`, `update`, `delete`, `send`, `add`, `set-name`, `defaults-write`, `screen-sharing enable`, and `time-machine start/stop`.

2. Prefer `--dry-run` before real mutations whenever `cider schema --source <name>` reports `supports_dry_run: true`.

3. Some commands need macOS permissions or prompts:
   - Messages, Photos, and Safari history may require Full Disk Access
   - Keychain password reads can trigger macOS security dialogs
   - `screen-sharing enable` and `screen-sharing disable` require `sudo`

4. `mail send` and `messages send` are real side effects, not previews.

5. Prefer `id` values advertised by `cider schema`. Calendar, Contacts, Mail, Messages, Notes, and Reminders expose stable targets. Calendar's legacy title/date delete must be treated as a compatibility fallback; it refuses ambiguous matches.

6. `cider doctor` and `cider auth-status` are prompt-free. An Automation status of `not_probed` is intentional, because probing can itself open a macOS permission dialog.

## Best Practices

1. Use JSON output for automation and `jq` filtering.

2. Use `--pretty` only for human review.

3. Use explicit subcommands instead of implicit defaults.

4. Check `cider schema --source <name>` before assuming dry-run or stable ID support.

5. Keep diagnostics on stderr and parse only stdout.

6. For repository work, use `cargo run -- ...` so you exercise the local build.

## Reference

- CLI overview: `README.md`
- Command surface: `cider --help`
- Machine-readable capabilities: `cider schema`
