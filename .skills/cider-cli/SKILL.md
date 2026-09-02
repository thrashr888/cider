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
# What changed since a sync point: RFC 3339, or a bare date for local
# midnight; calendar, notes, and messages list take --since too, and
# calendar events carry modified_at.
cider reminders list --since 2026-09-01T00:00:00Z
cider reminders list --since 2026-09-01
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
cider icloud quota                              # remaining iCloud storage via brctl
cider icloud list --folder Documents --state cloud   # cloud-only placeholders; nothing is downloaded
cider --dry-run icloud evict --path Documents/big.mov
cider weather                                   # Apple Weather at the primary home's address; needs Cider Bridge
cider weather --forecast --days 5 --home "Casa"
cider weather --lat 37.75 --lon -122.49         # output includes the required `attribution` block
cider watch --source reminders --source calendar   # JSON line per change, runs until Ctrl-C
cider watch --source contacts --via fsevents       # force FSEvents instead of cider-bridge store_changed events
cider system-info show
cider permissions                               # every macOS permission cider can need, its state, the pane, who to grant it to
cider permissions --source calendar             # only what one command needs
cider permissions --pretty                      # table, headed by the app the grants belong to
cider doctor                                    # tools, stores, bridge, and a one-line `permissions` check
cider auth-status
```

### The Bridge (HomeKit, WeatherKit, EventKit Writes, Watch)

`cider` alone covers everything above except `home state|run|set|triggers`
and `weather`; those, faster Reminders/Calendar writes, and item-level
`watch` events come from the optional Swift helper (README, "Bridge").

```bash
# First: what is installed, is the app answering, protocol versions,
# per-store TCC state with the fix for each. Never launches anything.
cider bridge status
cider doctor | jq '.checks[] | select(.name | startswith("bridge"))'
# Cache-backed home reads always work; --envelope says which backend
# answered and how stale a cache read is.
cider --envelope home homes | jq '{source, cache_age_s}'
cider home homes --live                          # insist on the bridge
# Stop a running bridge (it also exits itself after ten idle minutes).
cider bridge quit
```

Facts to plan around:

- `brew install cider` includes the bridge app and `cider-bridge` CLI for
  everything **except HomeKit**. HomeKit needs a personal build
  (`cider bridge build --install`: Xcode, XcodeGen, a paid Apple team, and
  HomeKit enabled on the App ID). A packaged bridge fails HomeKit commands
  with code `homekit_unavailable`; do not retry, report it.
- `bridge_incompatible` means a stale app or CLI answered; the message names
  the fix (`cider bridge build --install`, or `brew upgrade cider`).
- Home ids differ between cache (`homeUUID`) and bridge
  (`HMHome.uniqueIdentifier`); room, accessory, and scene ids match. Select
  homes by name with `--home`; either id is accepted and mapped when possible.
- TCC grants belong to the app that launched `cider` (Terminal, the agent
  runner), not to `cider`. Calendar needs **Full Access** or EventKit hides
  every event; Reminders prompts on first use, Calendar and Contacts do not
  prompt a command-line requester and are set by hand. `cider permissions`
  (or `cider bridge status` → `cli_authorization.fixes`) lists what to
  grant where.

## Safety And Permissions

1. Always confirm with the user before any mutating command, especially `create`, `update`, `delete`, `send`, `add`, `set-name`, `defaults-write`, `screen-sharing enable`, `time-machine start/stop`, and `icloud download/evict` (evict removes the local copy; paths must be inside iCloud Drive).

2. Prefer `--dry-run` before real mutations whenever `cider schema --source <name>` reports `supports_dry_run: true`.

3. Some commands need macOS permissions or prompts. Run `cider permissions` before diagnosing a permission failure: it lists every permission, its state, the System Settings pane, and who to grant it to. The rule: macOS attributes cider's access to the app that *launched* it (Terminal, iTerm, the agent runner, a host app), so grants go to that app, never to `cider`, and a host app must declare the Info.plist usage strings (`host_app_requirements` in the report) or it is silently denied.
   - Full Disk Access gates every store read from disk: Messages, Mail, Safari, Photos, Books, Voice Memos, FaceTime, iCloud account, Stocks, Shortcuts, the Home cache, and the SQLite reads behind Calendar, Reminders, Contacts. Added by hand under Privacy & Security › Full Disk Access; there is no prompt.
   - Calendar needs **Full Access**, not Add Only (Add Only hides every event); current macOS shows no Calendar or Contacts prompt to a command-line requester, so those are set by hand after the first call registers the app. Reminders prompts on first use.
   - Automation (Notes, Music, Mail, Messages, Safari tabs, Shortcuts, and the JXA fallbacks) is granted per (launching app → target app) pair on the first AppleEvent and is always `not_probed`.
   - HomeKit belongs to Cider Bridge.app itself (personal build only); WeatherKit needs nothing.
   - Keychain password reads can trigger macOS security dialogs
   - `screen-sharing enable` and `screen-sharing disable` require `sudo`

4. `mail send` and `messages send` are real side effects, not previews.

5. Prefer `id` values advertised by `cider schema`. Calendar, Contacts, Mail, Messages, Notes, and Reminders expose stable targets. Calendar's legacy title/date delete must be treated as a compatibility fallback; it refuses ambiguous matches.

6. `cider permissions`, `cider doctor`, and `cider auth-status` are prompt-free. An Automation status of `not_probed` is intentional, because probing can itself open a macOS permission dialog.

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
