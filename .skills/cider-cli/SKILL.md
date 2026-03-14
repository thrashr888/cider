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

Use `cider schema` to learn whether a source supports `--dry-run`, pagination args, and stable IDs. Use `--help` for exact subcommand flags.

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
cider reminders create --title "Buy milk" --list Shopping --due "2026-03-14T18:00:00Z"
cider calendar list --days-ahead 14
cider calendar create --title "1:1" --start "2026-03-15T17:00:00Z" --end "2026-03-15T17:30:00Z"
```

### Contacts And Notes

```bash
cider contacts list --search Smith --limit 10
cider contacts get --id contact-123
cider notes list --folder Work --limit 20
cider notes create --title "Meeting Notes" --body "Agenda..."
```

### Messages, Mail, And Music

```bash
cider messages list --days 7 --limit 20
cider messages send --to "+15551234567" --text "On my way"
cider mail list --limit 10
cider music status
cider music play --playlist Favorites
```

### System And Utility Commands

```bash
cider screenshots list --limit 20
cider screenshots capture --path ~/Desktop/capture.png
cider shortcuts run --name "Daily Briefing"
cider wifi status
cider system-info show
```

## Safety And Permissions

1. Always confirm with the user before any mutating command, especially `create`, `update`, `delete`, `send`, `add`, `set-name`, `defaults-write`, `screen-sharing enable`, and `time-machine start/stop`.

2. Prefer `--dry-run` before real mutations whenever `cider schema --source <name>` reports `supports_dry_run: true`.

3. Some commands need macOS permissions or prompts:
   - Messages, Photos, and Safari history may require Full Disk Access
   - Keychain password reads can trigger macOS security dialogs
   - `screen-sharing enable` and `screen-sharing disable` require `sudo`

4. `mail send` and `messages send` are real side effects, not previews.

5. Prefer explicit identifiers where available. If a source does not advertise stable IDs, be careful with title-based deletes and updates.

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
