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
| `cider shortcuts` | `list`, `run`, `view`, `sign` |
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
cider shortcuts view --name "Morning Routine"
cider shortcuts sign --input ./MyShortcut.shortcut --output ./MyShortcut-signed.shortcut --mode anyone

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

## JXA Workarounds / Preferred Primitives

cider should avoid JXA wherever possible. Preferred order:

1. Native CLI tools (`shortcuts`, `tmutil`, `log`, `screencapture`, `scutil`, `defaults`)
2. SQLite / plist / filesystem reads
3. AppleScript for narrow writes on one object at a time
4. JXA only as a last resort for narrow app automation

Current examples:
- Reminders reads use SQLite; reminder mutations now prefer AppleScript over broad JXA scans
- Calendar reads prefer local databases when available
- Safari Reading List uses plist parsing
- Voice Memos uses SQLite
- Photo Booth uses filesystem inspection
- Shortcuts uses Apple's native `shortcuts` CLI instead of scripting

## Coverage Matrix

| Area | Read | Write/Automation | Status | Notes |
|------|------|------------------|--------|-------|
| Reminders | SQLite | AppleScript | strong | Reads avoid JXA; scoped create/complete/delete verified |
| Calendar | DB + fallback | AppleScript | strong | Reads prefer DB; scoped create/delete verified |
| Contacts | JXA | JXA | partial | Works, but still too JXA-heavy and a replacement target |
| Notes | AppleScript | AppleScript | strong | list/get/create/update/delete working; folder metadata can be inconsistent |
| Mail | JXA | JXA + Mail app send | partial | Works, but still a JXA reduction target |
| Messages | SQLite | AppleScript/JXA send | strong | Reads are local DB-backed |
| Music | JXA | JXA | partial | Works, but live-app control likely remains scripting-based |
| Shortcuts | native CLI | native CLI | strong | list/run/view/sign supported; no true CRUD in Apple CLI |
| Voice Memos | SQLite | none | strong | Read coverage good; no public write API |
| Photo Booth | filesystem | none | strong | Simple filesystem-backed read model |
| Reading List | plist | none | strong | Read-only plist parsing |
| Photos | SQLite | none | partial | Good metadata reads; writes likely need native Swift/Photos.framework |
| Home | blocked | blocked | blocked | Likely needs native Swift + entitlements |
| Journal | blocked | blocked | blocked | Encrypted/private |
| Weather | blocked | blocked | blocked | Encrypted/private cache |

## Dry-run plan for dangerous commands

Recommended future `--dry-run` support:

- mail send
  - return resolved recipient/subject/body without sending
- messages send
  - return recipient/text without sending
- reminders create/complete/delete
  - return target list/title/action without mutating
- calendar create/delete
  - return resolved calendar/title/date/times without mutating
- shortcuts run
  - hard to guarantee because shortcut side effects are arbitrary; document as not safely dry-runnable
- screen-sharing enable/disable
  - print the `launchctl` command that would run
- system-info set-name / defaults-write
  - print exact system mutation command
- time-machine start/stop
  - print exact `tmutil` invocation
- screenshots capture
  - print resolved output path and mode flags

Implementation suggestion:
- add global `--dry-run` flag
- thread it through mutating subcommands only
- return normal `ActionResult` JSON with `ok: true`, `action`, and a `message` describing the skipped mutation
- never fake success for commands that need validation from the live app; instead say `would run ...`

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
| `shortcuts` | Apple CLI supports invocation/discovery (`list`, `run`, `view`, `sign`) but not true create/update/delete CRUD |
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
