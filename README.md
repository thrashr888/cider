# cider

Manage macOS Apple apps from the command line. JSON on stdout, errors on stderr. Designed for both humans and AI agents.

## Commands with CRUD

These commands support subcommands (`list`, `create`, `delete`, etc.). Running the command bare defaults to `list`.

| Command | Subcommands |
|---------|-------------|
| `cider reminders` | `list`, `create`, `complete`, `delete`, `lists` |
| `cider calendar` | `list`, `create`, `delete`, `calendars` |
| `cider contacts` | `list`, `get`, `create`, `update`, `delete`, `groups` |
| `cider notes` | `list`, `get`, `create`, `update`, `delete`, `folders` |
| `cider mail` | `list`, `get`, `read`, `unread`, `trash`, `mailboxes`, `send` |
| `cider messages` | `list`, `send` |
| `cider music` | `list`, `play`, `pause`, `next`, `previous`, `status`, `playlists` |
| `cider shortcuts` | `list`, `run` |
| `cider screenshots` | `list`, `capture` |
| `cider time-machine` | `status`, `list`, `start`, `stop` |
| `cider screen-sharing` | `status`, `enable`, `disable` |
| `cider system-info` | `show`, `set-name`, `defaults-read`, `defaults-write` |

## Read-Only Commands

| Command | Source |
|---------|--------|
| `cider activity-monitor` | CPU, memory, top processes |
| `cider apps` | Installed applications |
| `cider automator` | Automator workflows |
| `cider books` | Apple Books library |
| `cider clock` | Local time, world clocks, alarms |
| `cider console` | System log entries |
| `cider find-my` | Find My devices |
| `cider fonts` | Installed fonts |
| `cider home` | HomeKit accessories |
| `cider maps` | Maps bookmarks |
| `cider photo-booth` | Photo Booth photos |
| `cider photos` | Photos metadata |
| `cider reading-list` | Safari Reading List |
| `cider stocks` | Stocks watchlist |
| `cider voice-memos` | Voice memos |
| `cider weather` | Weather data |

## Install

```sh
cargo install --path .
```

## Usage

```sh
# List reminders (default action)
cider reminders
cider reminders list --list Shopping

# Create
cider reminders create --title "Buy milk" --list Shopping --priority 1
cider calendar create --title "Standup" --start "2026-03-15T10:00:00" --end "2026-03-15T10:30:00"
cider contacts create --first Alice --last Smith --email alice@example.com
cider notes create --title "Meeting notes" --body "Discussed Q2 plans" --folder Work

# Manage
cider reminders complete --title "Buy milk"
cider contacts update --id ABC123 --phone "+15551234567"
cider mail read --index 1
cider mail send --to boss@work.com --subject "Update" --body "Done."
cider messages send --to "+15551234567" --text "On my way"

# Music controls
cider music play --playlist "Chill"
cider music pause
cider music status

# System
cider screenshots capture --selection
cider time-machine start
cider system-info defaults-read com.apple.dock autohide
cider shortcuts run --name "Morning Routine"

# Pretty-print any command
cider contacts list --search Smith --pretty
```

## Agent Usage

cider outputs structured JSON on stdout and errors/progress on stderr, following [agent-friendly CLI principles](https://justin.poehnelt.com/posts/rewrite-your-cli-for-ai-agents/):

- All output is valid JSON arrays or objects
- Write operations return `{"ok": true, "action": "...", "id": "...", "message": "..."}`
- Errors go to stderr with descriptive messages
- Omit `--pretty` for compact, token-efficient output
- Each command is independent — no shared state
- Broken pipe handled gracefully (safe to pipe to `head`)

## Write Limitations

The following cannot support write operations without compiled Swift binaries or private entitlements:

| Command | Reason |
|---------|--------|
| `activity-monitor` | Process table is read-only |
| `apps` | App install requires App Store / MDM |
| `books` | No scripting dictionary |
| `clock` | System clock requires root; Clock.app has no API |
| `console` | System logs are append-only |
| `find-my` | Locked behind private APIs |
| `fonts` | No scripting dictionary |
| `home` | Requires compiled Swift with entitlements. Use `cider shortcuts run` as workaround |
| `journal` | Encrypted, no API |
| `maps` | Minimal scripting dictionary |
| `news` | Read-only SQLite cache |
| `photo-booth` | Just filesystem photos |
| `photos` | Requires Photos.framework (compiled Swift) |
| `reading-list` | Safari overwrites plist changes |
| `stickies` | Binary archive format |
| `stocks` | iCloud-synced, local edits overwritten |
| `voice-memos` | No scripting dictionary |
| `weather` | Read-only cache |

## Requirements

- macOS (uses osascript, SQLite databases in ~/Library, etc.)
- Full Disk Access may be required for Messages, Photos, Safari Reading List
- `screen-sharing enable/disable` requires `sudo`
- `mail send` will actually send email via Mail.app
- Some commands require the app to have been used at least once

## Build

```sh
cargo build --release
```

The binary will be at `target/release/cider`.
