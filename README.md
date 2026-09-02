# cider

Manage your Mac from the command line. Reminders, Calendar, Contacts, Notes, Mail, Music, Keychain, Safari, and 30+ more Apple apps.

## API Highlights

- **Fast reads, supported writes.** Bulk reads use local macOS indexes where
  they are reliable; Calendar falls back to app automation when its fast path
  is unavailable. Writes use the apps' supported JXA/AppleScript interfaces.
- **Stable identity.** Calendar, Mail, Contacts, and Reminders expose durable
  ids, and destructive operations prefer exact ids over titles or list
  positions. Ambiguous legacy matches fail safely.
- **Complete discovery.** `cider schema` is generated from the real command
  parser, so agents can discover all commands, arguments, defaults, dry-run
  support, and identifier contracts without a second hand-maintained API list.
- **Efficient batches.** Calendar, Mail, and Reminders batch commands reuse one
  app automation session and return a result for every item, including partial
  failures.
- **Full-fidelity PIM data.** Contacts return labeled multi-value fields and
  richer profile data; Calendar, Mail, and Reminders expose deeper read and
  mutation APIs without truncating content.
- **Prompt-free diagnostics.** `cider doctor` and `cider auth-status` inspect
  tools, data stores, and access state without triggering macOS permission
  dialogs.

## Install

```sh
brew tap thrashr888/tap && brew install cider
```

Or via Cargo:

```sh
cargo install cider-cli
```

## Quick Start

```sh
# See your reminders
cider reminders

# Pretty tables for humans
cider reminders --pretty

# Create a reminder
cider reminders create --title "Buy milk" --list Shopping

# Complete one by id — titles repeat, ids don't
cider reminders complete --id 4b7c5902-46a7-4f7a-a385-91b562ca8eb6

# Check your calendar
cider calendar

# Fetch/update/delete the exact event by its stable id
cider calendar get --id <event-id>
cider --dry-run calendar update --id <event-id> --location "Zoom"

# Search contacts
cider contacts list --search Smith

# Check local data access without triggering permission dialogs
cider doctor
cider auth-status

# Control music
cider music play
cider music status
cider music pause

# Send a message
cider messages send --to "+15551234567" --text "On my way"

# Watchlist prices from Apple Stocks
cider stocks
cider stocks quote --symbol AAPL

# Apple Weather at your home's address, or anywhere (needs Cider Bridge)
cider weather
cider weather --forecast --days 5
cider weather --lat 37.75 --lon -122.49

# Fast bulk listing of every Apple Note (no bodies)
cider notes list --brief

# Search your Mac
cider spotlight --query "quarterly report"
```

## What You Can Do

### Full CRUD

| App | Actions |
|-----|---------|
| Reminders | `list`, `get`, `create`, `update`, `complete`, `reopen`, `delete`, batch actions, `lists` |
| Calendar | `list`, `get`, `create`, `batch-create`, `update`, `delete`, `calendars` |
| Contacts | `list`, `get`, `create`, `update`, `delete`, `groups` |
| Notes | `list`, `get`, `create`, `update`, `delete`, `folders` |
| Mail | `list`/search, `get`, `read`, `unread`, `trash`, batch actions, `mailboxes`, `send` |
| Keychain | `list`, `search`, `get-password`, `add`, `delete`, `keychains` |

### Actions & Controls

| App | Actions |
|-----|---------|
| Music | `list`, `play`, `pause`, `next`, `previous`, `status`, `playlists` |
| Messages | `list`, `send` |
| Shortcuts | `list`, `run`, `view`, `export`, `gen`, `install`, `sign` |
| Screenshots | `list`, `capture` |
| Time Machine | `status`, `list`, `start`, `stop` |
| iCloud Drive | `list`, `download`, `evict` (evict removes the local copy; the file stays in iCloud) |
| Screen Sharing | `status`, `enable`, `disable` |
| System Info | `show`, `set-name`, `defaults-read`, `defaults-write` |
| Safari | `bookmarks`, `history`, `tabs`, `reading-list` |
| Wi-Fi | `status`, `networks` |

### Read + CRUD

| App | Actions |
|-----|---------|
| FaceTime | `list` |
| Passwords | `list`, `get`, `create`, `update`, `delete` |

### Read-Only

Activity Monitor, Apps, Automator, Bluetooth, Books, Clock, Console, Disks, Fonts, Home (`list`, `homes`, `rooms`, `accessories`, `scenes`), iCloud (`account`, `quota`, `status`, `log`, `list` — placeholder-aware, never downloads), Photo Booth, Photos, Spotlight, Stocks (`list`, `watchlists`, `quote`), Voice Memos, Weather (`current`, `--forecast`; needs Cider Bridge)

### Home

`cider home` reads the Home app's on-disk cache, which is as fresh as the last time the app ran and has no characteristic values. With the **Cider Bridge** — a small Catalyst helper app you build once on your own Mac with `cider bridge build --install` (HomeKit only loads in a signed app, so it cannot be distributed) — the same commands go live, and `home` gains what only HomeKit can do: `home state [--room] [--accessory]` (live values, one row per characteristic), `home run --scene`, `home set --accessory --characteristic --value`, and `home triggers` with `create-timer --name --at --repeat --scene`, `enable`, `disable`, `delete` — timer automations that fire on the home hub with the Mac asleep. Reads use the bridge when it is already answering and the cache otherwise (`--envelope` reports `"source": "bridge"|"cache"`); `--live` requires the bridge, launching it on demand. `cider bridge status|build|install|quit` manages the app, and `cider doctor` reports `bridge_app`, `bridge_socket`, and `bridge_cli`. Protocol and design: [docs/RFC-swift-bridge.md](docs/RFC-swift-bridge.md).

### Weather

`cider weather [--forecast] [--days N] [--home <name> | --lat <f> --lon <f>]` is WeatherKit through the same bridge app (launched on demand; the Weather app's own cache is encrypted, so there is nothing on disk to read). Current conditions by default — temperature, apparent temperature, condition, humidity, wind, pressure, UV, visibility — or `--forecast` for daily highs/lows/precipitation plus the next 24 hours. Location is `--lat/--lon`, else `--home <name>`, else the primary home's address from the Home app; the output says which under `location`. Every result carries Apple's required `attribution` block (service name, legal URL, logos): keep it next to the numbers when you show them to a person. WeatherKit needs the capability enabled on the bridge's App ID; without it the error is `weather_unavailable`.

### Faster Writes: the `cider-bridge` CLI

`cider bridge build --install` also installs a small native CLI, `cider-bridge`, that wraps EventKit and Contacts (ad-hoc signed, no paid team needed). When it is present, `reminders create|update|complete|reopen|delete|batch-*` and `calendar create|update|delete` go through it instead of AppleScript/JXA — the same commands, the same `{ok, action, id, message}` result, plus the saved row as `record`; `--envelope` reports `"source": "cli"|"native"` and `--dry-run` says which path would run. Bulk reads (`reminders list`, `calendar list`, `--since`) stay on SQLite, and `contacts` stays native. `CIDER_BRIDGE_CLI=off` forces the native path; `CIDER_BRIDGE_CLI=/path/to/cider-bridge` names a build elsewhere (the default search is inside `~/Applications/Cider Bridge.app`, then `~/.local/bin`, then `$PATH`). `cider bridge status` shows `cli_installed`/`cli_path`.

### Change Stream

`cider watch [--source reminders|calendar|contacts|notes|home|shortcuts]... [--debounce-ms 2000] [--via auto|cli|fsevents]` prints one compact JSON line per change until Ctrl-C (foreground, no daemon). Each line is `{"source":…,"at":…,"kind":…}`. With `cider-bridge` installed, reminders, calendar, and contacts stream EventKit/Contacts change notifications — `"kind":"store_changed"`, item-level, no file noise; everything else (and everything, with `--via fsevents` or without the CLI) is FSEvents on the on-disk stores — `"kind":"files_changed"` with the coalesced `paths`. `--via cli` fails rather than falling back. An event says *that* a store changed; re-read it with the matching command to learn what.

## Output

Default output is compact JSON — pipe to `jq`, feed to scripts, or use with AI agents:

```sh
cider contacts | jq '.[].name'
cider calendar | jq '[.[] | select(.is_all_day == false)]'
cider activity-monitor | jq '.[0].top_processes[:5]'
```

Add `--pretty` anywhere for human-readable tables:

```
$ cider --pretty reminders
ID                                    LIST       PRIORITY  TITLE
──────────────────────────────────────────────────────────────────
4b7c5902-46a7-4f7a-a385-91b562ca8eb6  Shopping   1         Buy milk
f4c021a1-2ed3-4f14-ab65-b8ce3b315a27  Work       0         Review PR
217 items
```

Write operations return a status object:

```
$ cider --pretty reminders create --title "Buy milk" --list Shopping
✓ created (buy_milk) — Reminder added
```

Batch writes use one app automation session and report every item, including
partial failures:

```json
{"ok":false,"action":"batch-delete","requested":2,"succeeded":1,"failed":1,"results":[{"id":"a","ok":true},{"id":"b","ok":false,"error":"not found"}]}
```

A partial batch exits non-zero after writing this result, and `--envelope`
keeps the outer `ok` value false as well.

Repeat `--id` for Reminders and Mail batches. Calendar batch creation accepts
a JSON array, or `--json -` to read it from stdin:

```sh
cider --dry-run reminders batch-complete --id <id-1> --id <id-2>
cider mail batch-read --id '<message-1@example.com>' --id '<message-2@example.com>'
printf '%s' '[{"title":"1:1","start":"2026-09-02T17:00:00Z","end":"2026-09-02T17:30:00Z"}]' \
  | cider calendar batch-create --json -
```

`reminders complete` and `reminders delete` take either `--title` or `--id`.
Titles are not unique, so a `--title` call acts on the first open match and
says so when there were others:

```
$ cider reminders complete --title "Review PR"
{"action":"completed","message":"Marked 'Review PR' (1 of 2 matching — pass --id to choose)","ok":true}
```

Pass the `id` from `reminders list` to name one exactly. A `--title` match only
ever considers reminders that are still open — the same set `reminders list`
shows — so a finished reminder of the same name can never absorb the action.

Reminder content round-trips in full: `list` and `get` return complete titles
and notes (newlines intact, no length cap), and `update` edits a reminder in
place — preserving its id and creation date:

```
$ cider reminders get --id 4b7c5902-46a7-4f7a-a385-91b562ca8eb6
$ cider reminders update --id 4b7c5902-... --priority 1 --new-title "Buy oat milk"
$ cider reminders update --id 4b7c5902-... --append-notes "also: check the sale"
$ long-notes-command | cider reminders update --id 4b7c5902-... --notes -
```

`--notes -` (and `--append-notes -`) read from stdin, for long or multiline
content that shell arguments handle badly.

Calendar mutations likewise prefer the `id` printed by `calendar list`.
Legacy `--title` plus `--date` deletion remains accepted, but it now refuses
to act if several events match instead of deleting an arbitrary one.

Mail list/get output uses the RFC Message-ID as `id` when Mail has one and
also includes `local_id`. Stable `--id` targeting is preferred; the old
one-based `--index` form remains available for compatibility. Mail listing can
search subject/sender/preview, select a mailbox, and filter unread or flagged
messages:

```sh
cider mail list --search invoice --mailbox INBOX --unread --limit 25
cider mail get --id '<message-id@example.com>'
```

Contacts include all labeled emails, phones, URLs, and postal addresses plus
middle name, nickname, job title, department, birthday, and notes when present.
Create and update accept the same richer name and work fields; repeat `--email`
or `--phone` during creation to add several values.

## Schema And Diagnostics

`cider schema` is generated from the real command parser. It describes every
top-level command, action, argument, required/default value, read/write kind,
dry-run support, and stable identifier contract. This avoids a second,
hand-maintained command list drifting out of date:

```sh
cider schema
cider schema --source calendar
```

`cider auth-status` reports prompt-free read access and write Automation state
for Calendar, Contacts, Reminders, and Mail. `cider doctor` checks required
macOS tools and the Calendar, Contacts, Reminders, and newest Mail data stores.
It deliberately does not send an AppleEvent: even a permission probe can open
a macOS dialog, so Automation authorization is reported as `not_probed` and
real writes surface any denial.

## How It Works

Cider exposes one API, not user-selectable backends. Internally it uses the
fastest reliable macOS path for each operation: local SQLite indexes for bulk
reads, and the apps' supported JXA/AppleScript interfaces for writes. That
keeps reads fast and writes supported without shipping Swift or Node sidecars.
If a Calendar database read fails, Cider falls through to its slower app
automation path and reports the failed fast path on stderr.

## Use as a Library

cider is also a Rust crate, so another Rust program can skip the subprocess,
the JSON round-trip, and the question of whether the binary is installed and
new enough:

```toml
[dependencies]
cider-cli = { version = "0.5", default-features = false }
```

```rust
for r in cider::sources::reminders::list(Some("Shopping")).await? {
    println!("{} {}", r.id, r.title);
}

cider::sources::reminders::complete(
    cider::sources::reminders::Target::Id(&id),
    Some("Shopping"),
).await?;
```

Every `sources::*` module returns plain serde types — the CLI is a thin Clap
front-end over exactly these functions. `default-features = false` drops the
Clap front-end and the `--pretty` table renderer, which a library caller never
uses.

The library shells out to macOS's own tools (`osascript`, `sqlite3`), so it
needs nothing on PATH — but it inherits your process's TCC permissions, and
sees the same Full Disk Access denials the CLI reports.

## Requirements

- macOS
- Some commands need **Full Disk Access** (Messages, Photos, Safari History)
- `screen-sharing enable/disable` requires `sudo`
- `mail send` and `messages send` will actually send — not a drill

## For AI Agents

cider follows [agent-friendly CLI principles](https://justin.poehnelt.com/posts/rewrite-your-cli-for-ai-agents/):

- JSON arrays/objects on stdout, errors on stderr
- Compact output by default (no `--pretty`) for token efficiency
- Write results: `{"ok": true, "action": "...", "id": "...", "message": "..."}`
- Each command is stateless and independent
- Broken pipe safe (`cider contacts | head` won't error)

## Agent Skills

This repo includes [Agent Skills](https://agentskills.io/) so compatible agents can learn how to use `cider` effectively.

### Installing Skills

```sh
# Install the repo's skills
npx skills add thrashr888/cider

# Install just the cider CLI usage skill
npx skills add thrashr888/cider@cider-cli

# Install to a specific agent
npx skills add thrashr888/cider -a claude-code
npx skills add thrashr888/cider -a cursor
```

Or copy the skills into another project manually:

```sh
git clone https://github.com/thrashr888/cider.git
cp -r cider/.skills /path/to/your/project/.skills
```

Compatible agents automatically discover skills in the `.skills/` directory.

### Available Skills

- `cider-cli` — guide for using `cider` to read and change Apple app data from the terminal

The `cider-cli` skill helps agents:

- discover commands with `cider --help` and `cider schema --source <name>`
- prefer compact JSON for automation and `--pretty` only for human review
- use `--dry-run` before supported mutations
- account for macOS permissions, dialogs, and real side effects like `mail send` and `messages send`

This repo also contains repo-maintenance skills in `.agents/skills/` for agents working on `cider` itself.

## Build from Source

```sh
git clone https://github.com/thrashr888/cider
cd cider
cargo build --release
# Binary at target/release/cider
```

## Bridge

HomeKit has no file or script interface and WeatherKit only loads in a
signed app, so `cider` can talk to an optional Swift helper, **Cider
Bridge**, over a Unix socket — and to its sibling, the native `cider-bridge`
CLI, over JSON stdio for EventKit/Contacts writes and change streams. The
Swift half lives in [`bridge/`](bridge/) (`swift test` there runs its unit
tests; `bridge/scripts/build.sh --team <id> --install` builds the Mac
Catalyst app with your own Apple Developer team and installs the CLI beside
it). The design, protocol, and command table are in
[docs/RFC-swift-bridge.md](docs/RFC-swift-bridge.md).

## License

MIT
