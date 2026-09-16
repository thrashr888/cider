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
- **Local MCP server.** `cider mcp` exposes selected read-only sources as typed
  tools, using the same library as the CLI. See [MCP setup](#mcp-server).
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
| Shortcuts | `list`, `run`, `view`, `export`, `gen`, `install`, `sign` — an `ssh` step to this Mac needs Remote Login on (System Settings › General › Sharing); `gen` refuses to build one while port 22 is closed unless you pass `--allow-unreachable-ssh` |
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

Activity Monitor, Apps, Automator, Bluetooth, Books, Clock, Console, Disks, Fonts, Home (`list`, `homes`, `rooms`, `accessories`, `scenes`), iCloud (`account`, `quota`, `status`, `log`, `list` — placeholder-aware, never downloads), Knowledge (`list`, `streams` — local activity history), Photo Booth, Photos, Spotlight, Stocks (`list`, `watchlists`, `quote`), Voice Memos, Weather (`current`, `--forecast`; needs Cider Bridge)

Notifications (`list`), Downloads (`list`), Interactions (`list`), and Biome
(`streams`, `list`) also expose retained local history.

### Knowledge activity history

Read the local Core Duet store at
`~/Library/Application Support/Knowledge/knowledgeC.db`:

```bash
cider knowledge                              # newest 100 events
cider knowledge streams --pretty             # available streams, counts, time ranges
cider knowledge list --stream /app/usage --limit 20 --pretty
cider knowledge list --stream /display/isBacklit --since 2026-09-01 --until 2026-09-02
cider knowledge list --limit 100 --offset 100 # next page
cider knowledge list --since 2026-09-01T12:00:00Z --envelope
```

`events` is an alias for `list`. Stream matching is exact. Time filters apply
to event **start times**, with an inclusive `--since` and exclusive `--until`;
they accept RFC 3339 timestamps or dates interpreted as local midnight.
Results sort newest first, with a row-ID tie-breaker. Pagination is applied
after filtering; new events can shift offsets between calls.

Events include `id`, `stream`, UTC `start_date`, `end_date`, `creation_date`,
`duration_seconds`, and the raw `value_string`, `value_integer`, `value_double`,
and `value_type_code` fields when present. IDs use the stored UUID, falling
back to `local:<rowid>` scoped to the current database. Durations are omitted
for missing or reversed intervals. Numeric value fields can contain internal
hashes rather than measurements; Cider preserves them without interpreting
Apple's private type codes. Binary and structured metadata are not decoded.

Available streams and retention depend on macOS and the device. This is the
history currently retained in Knowledge, not a complete Screen Time report.
Reads use SQLite read-only mode, including the live WAL. Missing, inaccessible,
or incompatible databases fail explicitly; an empty store returns `[]`.
Full Disk Access may be needed for the launching app; check
`cider permissions --source knowledge` and `cider doctor` (`knowledge_database`).
Library consumers can use `sources::knowledge::{list, streams, ListOptions}`
without the `cli` feature.

### Notifications, download origins, interactions, and Biome

```bash
cider notifications list --limit 20 --pretty
cider notifications list --app com.apple.MobileSMS --since 2026-09-01
cider downloads list --app com.apple.Safari --limit 20 --pretty
cider interactions list --since 2026-09-01 --until 2026-09-02 --envelope
cider biome streams --pretty
cider biome list --stream App.InFocus --limit 20 --pretty
cider biome list --stream ScreenTime.AppUsage --since 2026-09-01
cider biome list --stream Device.Wireless.WiFi --limit 5 --raw
```

All four sources are read-only. The first three default to `list`, returning
100 records newest first. `--app` matches an exact bundle identifier.
`--since` is inclusive and `--until` exclusive; both accept RFC 3339 or a
local calendar date. They filter notification delivery times, quarantine
event times, or interaction start times. `--limit` (0–10000) and `--offset`
apply after filtering, with deterministic tie-breaking. New records or
retention changes can shift pages between calls.

| Source | Store and output |
|--------|------------------|
| `notifications` | `~/Library/Group Containers/group.com.apple.usernoted/db2/db`, falling back to the older `DARWIN_USER_DIR/com.apple.notificationcenter/db2/db`. Returns app, delivery/request times, presented state, title, subtitle, body, and identifier. Binary plists are decoded in Rust. Localized non-string text is retained in `localized_content`; malformed payloads keep their metadata and carry `decode_error`. |
| `downloads` | `~/Library/Preferences/com.apple.LaunchServices.QuarantineEventsV2`. Returns the downloading app/agent, event time, data URL, origin URL/title, sender metadata, and raw quarantine type code. This is retained quarantine history, not a Downloads folder listing or a complete record of transfers. It never opens URLs or changes quarantine attributes. |
| `interactions` | `/private/var/db/CoreDuet/People/interactionC.db`. Returns app, start/end times, sender and recipient metadata, account, content URL, and raw direction/mechanism codes. Missing contact records retain their local IDs. These are donated interaction records, not message bodies or a complete communication history. |
| `biome` | `~/Library/Biome/streams/{restricted,public}/<stream>/local/`. Defaults to `streams`, listing segment counts and bytes, including empty streams. `list` (alias `events`) requires `--stream`; `--namespace public` selects the other namespace. Only local segments are read; remote-device directories and tombstone files are excluded. |

SQLite stores are opened with `-readonly` and include their live WAL.
Notifications, downloads, and interactions use stored UUIDs for event IDs,
falling back to `local:<rowid>` scoped to that database. Participant IDs
are always local row IDs. Missing/inaccessible stores and incompatible SQL
schemas fail explicitly; successfully queried empty stores return `[]`.

Biome supports SEGB v1 and v2 framing, skips deleted/unwritten entries,
checks CRC32, and orders matching events by record timestamp and ID across
segments before pagination. IDs are `<namespace>:<stream>:<segment>:<offset>`
and remain meaningful while the segment is retained. `timestamp` and optional
`end_timestamp` come from the segment record, not inferred payload fields.
Common protobuf fields are named for `App.InFocus`, `App.WebUsage`,
`ScreenTime.AppUsage`, `Device.Wireless.WiFi`, `Device.Wireless.Bluetooth`,
`Notification.Usage`, and `SystemSettings.SearchTerms`. App focus and Screen
Time status codes retain 0/1 (out of/in focus); Wi-Fi and Bluetooth retain
0/1 (disconnected/connected).

`payload.format` is `protobuf`, `plist`, or `opaque`. Protobuf fields retain
their field numbers, repeated occurrences, wire types, and values; unknown
fixed-width fields remain hex, and length-delimited fields expose readable
UTF-8 or hex without guessing embedded schemas. The `text` representation
means readable UTF-8, not a verified protobuf string type. Binary plists, including those embedded in protobuf fields, use
Cider's existing plist/archive decoder. Corrupt, oversized, or unsupported
payloads have an explicit `opaque` reason; CRC failures also set
`crc_valid: false`. `--raw` includes the original payload as `raw_hex`.
This does not promise semantic decoding of every private Apple stream.

Biome limits one segment to 64 MiB, a scan to 512 MiB and 20 seconds, retained
page payloads to 32 MiB, decoded payloads to 1 MiB, and `limit + offset` to
10000. Narrow time filters or reduce the page size if its retained-page
budget is exceeded. A live scan is not an atomic snapshot across segments;
framing errors fail with the segment path rather than silently dropping data.

Use `cider permissions --source <source>` for Full Disk Access guidance and
`cider doctor` for `notifications_database`, `downloads_database`,
`interactions_database`, and `biome_streams` checks. Availability and retention
vary by macOS version. Library consumers have the same API through
`sources::{notifications,downloads,interactions,biome}` without the `cli`
feature. The SQLite readers accept `HistoryOptions` (also exported as each
module's `ListOptions`); Biome exposes its own `ListOptions` and `Namespace`.

SEGB format references and protobuf field names are attributed in
`src/sources/biome/FORMAT_LICENSE`.

### What can you learn from local history?

These examples use `jq` with Cider's default JSON output. Each query examines
at most the requested number of retained records; counts describe that sample,
not all activity. Add `--since` and `--until` before the pipe to narrow a time
window. Availability depends on what macOS has retained.

**Notifications: which apps send the most notifications?**

Rank apps within the latest 1,000 retained notifications to find candidates
for quieter notification settings. A stored notification does not prove you
saw or read it.

```bash
cider notifications list --limit 1000 | jq '
  group_by(.app)
  | map({app: (.[0].app // "unknown"), notifications: length})
  | sort_by(.notifications) | reverse
'
```

**Downloads: which app recorded a download, and where did it come from?**

Inspect recent download events to identify the recording app and time, plus
the source URL and referring page when present. Some stores retain no URLs;
`null` means the origin is unavailable. The record can survive after the file
is moved or deleted, so it does not establish that the file is still on disk.

```bash
cider downloads list --limit 20 | jq '
  [.[] | {timestamp, app: (.app // .agent_name),
          url, origin_url, origin_title}]
'
```

**Interactions: who appears in recent communication records, and in which app?**

Show the participants and app for each recorded interaction. This can help
retrace a conversation across apps, but participants may include your own
identity, and records do not include message bodies. Names fall back to
identifiers or local contact IDs when unavailable.

```bash
cider interactions list --limit 20 | jq '
  def person: if . == null then null
              else (.display_name // .identifier // .id) end;
  [.[] | {start_date, app,
          sender: (.sender | person),
          recipients: [.recipients[] | person]}]
'
```

**Biome: which apps did you switch into, and when?**

Build a chronological view of focus-entry events from the latest 100
`App.InFocus` records. Only records with valid checksums and a decoded app
identifier are included. These are app switches, not measurements of attention
or time spent.

```bash
cider biome list --stream App.InFocus --limit 100 | jq '
  [.[] | select(.crc_valid and .status_code == 1 and .app != null)
   | {timestamp, app}]
  | sort_by(.timestamp)
'
```

**Knowledge: what app-usage intervals were recorded?**

Inspect recorded start/end times and durations to reconstruct an activity
window. Intervals can overlap, and filtering by start time does not trim an
interval at the window boundary, so summing durations is not a reliable total
of screen time.

```bash
cider knowledge list --stream /app/usage --limit 20 | jq '
  [.[] | {app: .value_string, start: .start_date,
          end: .end_date, seconds: .duration_seconds}]
'
```

### With the Bridge

These need the optional Swift helper described under [Bridge](#bridge); `cider bridge status` says what you have.

| Command | What it does |
|---------|--------------|
| `home state`, `home run`, `home set`, `home triggers …` | Live HomeKit values, scenes, characteristics, and timer automations (personal build only) |
| `weather [--forecast] [--days N] [--home <name> \| --lat --lon]` | WeatherKit current conditions or daily forecast, with Apple's required `attribution` |
| `reminders create\|update\|complete\|reopen\|delete\|batch-*`, `calendar create\|update\|delete` | Same commands, EventKit instead of AppleScript when `cider-bridge` is installed (`--envelope` says `"source": "cli"\|"native"`) |
| `watch [--source …] [--via auto\|cli\|fsevents]` | One JSON line per store change; EventKit/Contacts notifications with the CLI, FSEvents otherwise |

## MCP server

Run Cider as a local [Model Context Protocol](https://modelcontextprotocol.io/)
server so an assistant can discover and call its read-only tools:

```bash
cider mcp
# Expose only the sources this client needs:
cider mcp --sources knowledge,notifications,downloads,interactions,biome
```

The client launches this command and communicates over stdio. No port,
background service, or separate runtime is needed. In a terminal it waits for
MCP messages; normal tool output arrives in the client. The server exits when
the client closes stdin. `--pretty`, `--envelope`, and `--dry-run` are rejected
because stdout is reserved for MCP protocol messages.

For clients that use an `mcpServers` configuration, add:

```json
{
  "mcpServers": {
    "cider": {
      "command": "/opt/homebrew/bin/cider",
      "args": ["mcp", "--sources", "knowledge,notifications,downloads,interactions,biome"]
    }
  }
}
```

Replace `command` with the absolute path printed by `command -v cider`.
For a development build, use the absolute path to `target/release/cider`
after running `cargo build --release`. Restart or reconnect the client after
changing its configuration. This setup example exposes only the five history
sources; add `calendar`, `reminders`, or `doctor` to enable those tools.

### Available MCP tools

With no `--sources` option, all eight sources below are enabled. An explicit
list replaces that default. Unknown source names fail at startup, and disabled
tools are excluded from discovery and cannot be invoked by name. This first
version exposes the following subset of Cider; it has no write tools or generic
shell/CLI execution tool.

| Source | Tools | Useful question |
|--------|-------|-----------------|
| `knowledge` | `knowledge_list`, `knowledge_streams` | What app-usage intervals were recorded yesterday? |
| `notifications` | `notifications_list` | Which apps dominate my retained notifications? |
| `downloads` | `downloads_list` | Which app recorded these download events? |
| `interactions` | `interactions_list` | Who appears in recent communication metadata? |
| `biome` | `biome_list`, `biome_streams` | Which apps did I switch into? |
| `calendar` | `calendar_list`, `calendar_calendars` | What events are coming up? |
| `reminders` | `reminders_list`, `reminders_lists` | What incomplete reminders are on my Shopping list? |
| `doctor` | `doctor` | Which local stores are available, and what access is missing? |

Arguments are typed and discoverable through MCP `tools/list`. For example,
a client can call `knowledge_list` with:

```json
{
  "stream": "/app/usage",
  "since": "2026-09-14",
  "until": "2026-09-15",
  "limit": 20
}
```

Tool results contain `structuredContent` with `{"ok": true, "data": ...}`
and a text content block containing the same JSON for client compatibility.
Records preserve the CLI's field names and values. Source failures, invalid
arguments, and size/time limits return `isError: true` with
`{"ok": false, "error": {"code": "...", "message": "..."}}`; an unknown or
disabled tool produces a protocol error. Neither requires restarting the server.

All list/stream-discovery tools default to 100 records and accept `limit`
(0–1000) and `offset`; their sum must not exceed 10000. Increase `offset` by
the number returned to continue, and stop at an empty page. Results reflect
live stores, so intervening changes can shift pages. Calendar events sort by
start date then ID, reminders by ID, and calendar/reminder list names
alphabetically. Other sources keep their CLI ordering. Calendar and Reminders
paginate after their source query; limiting output does not limit the underlying
store scan. `doctor` returns a report and takes no arguments.

History `since`/`until` filters are inclusive/exclusive and accept RFC 3339 or
local-midnight dates. Calendar and Reminders use `since` for **modification
time**, matching the CLI. Calendar defaults to 7 days back and 30 days ahead;
each bound is capped at 365 days. Each successful JSON payload is limited to
1 MiB (it is duplicated in structured and text content), with a 60-second
per-call deadline and at most four active reads. Oversized results fail
explicitly; narrow filters, lower `limit`, or omit Biome `raw` to retry.

### Permissions and builds

The app launching Cider needs the same macOS permissions as CLI usage; a
terminal's Full Disk Access grant does not necessarily apply to an MCP client.
Use `cider permissions --source <source>` for guidance. Discovery does not
read personal stores or open prompts. Calendar reads may fall back to app
automation; calendar-name and reminder-list discovery use app automation.
Those calls may request authorization. Enabling `doctor`
exposes store/access diagnostics for all sources, even those not enabled as
MCP tools. Returned content is local user data, not instructions, and the
client/model receives the data its enabled tools return.

MCP uses the official Rust SDK and is included in default builds. Building
with MCP requires Rust 1.88 or newer. Library consumers using
`default-features = false` do not pull in MCP or Clap. To build the CLI without
MCP, use `cargo build --release --no-default-features --features cli`.

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

`cider permissions` lists every macOS permission cider can need with its
state for the app that launched it, the System Settings pane, and the
Info.plist keys a host app must declare (see [Permissions](#permissions)).
`cider doctor` checks required macOS tools, the Calendar, Contacts,
Reminders, and newest Mail data stores, the bridge, and summarizes the
permissions in one `permissions` check. `cider auth-status` is the older
per-store view of the same read/write state. None of them sends an
AppleEvent: even a permission probe can open a macOS dialog, so Automation
authorization is reported as `not_probed` and real writes surface any
denial.

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
sees the same Full Disk Access denials the CLI reports. Your app is the
responsible process, so it must carry the usage strings listed under
[Permissions](#embedding-cider-in-an-app-alchemy-tauri-any-host);
`cider::permissions::report().await` and `cider::doctor::inspect().await`
return what `cider permissions` and `cider doctor` print.

## Requirements

- macOS
- The permissions in [Permissions](#permissions), granted to the app that
  launches cider; `cider permissions` shows which are missing
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

## Permissions

macOS attributes a command-line tool's privacy access to its **responsible
process**: the app that launched it — Terminal, iTerm, an agent runner, or
the app that links the crate — so every grant below belongs to that app, not
to `cider`. A prompt appears only if that app's Info.plist declares the
matching usage string, and an app that never asked never appears in System
Settings › Privacy & Security, so nobody can pre-grant it.

Run `cider permissions` first (every permission, its state for your launcher,
the exact pane, and who to grant it to; `--source calendar` narrows it to one
command, `--pretty` tabulates), then `cider doctor` (tools, stores, the
bridge, and a one-line `permissions` summary). Both are prompt-free: they
open files for reading, ask an installed `cider-bridge` for its status, and
ping a bridge that is already running — never an AppleEvent, never a launch.

| Permission | Needed by | Granted to | How |
|------------|-----------|------------|-----|
| **Full Disk Access** | `messages`, `mail`, `safari`, `reading-list`, `photos`, `books`, `voice-memos`, `facetime`, `icloud account`, `stocks`, `shortcuts`, `knowledge`, `notifications`, `downloads`, `interactions`, `biome`, `home` (cache), `watch`, and the SQLite reads behind `calendar`, `reminders`, `contacts` | launching app | Privacy & Security › Full Disk Access: add the app by hand, then relaunch it. No prompt, no Info.plist key; `sudo` does not bypass it |
| **Calendars** | `calendar` through `cider-bridge` (EventKit) | launching app | Privacy & Security › Calendars → **Full Access**, not Add Only (Add Only hides every event). Current macOS shows no Calendar prompt to a command-line requester: the first call registers the app in the pane, and you set it by hand |
| **Reminders** | `reminders` through `cider-bridge` | launching app | The first call prompts; grant Full Access. Afterwards: Privacy & Security › Reminders |
| **Contacts** | `contacts` through `cider-bridge` | launching app | Privacy & Security › Contacts. Like Calendar, no prompt for a command-line requester: set it by hand after the first call |
| **Automation** (one pair per target app) | `notes`, `music`, `mail send/read/unread/trash/get`, `messages send`, `safari tabs`, `shortcuts run/view`, and the AppleScript/JXA fallbacks for `calendar`, `reminders`, `contacts` when the bridge is absent | launching app → target app | The first AppleEvent prompts, per pair; afterwards Privacy & Security › Automation. Always `not_probed`: the probe would itself be an AppleEvent |
| **HomeKit** | `home state/run/set/triggers`, `home --live` | Cider Bridge.app | The bridge app prompts on its first HomeKit call; Privacy & Security › HomeKit → Cider Bridge. Personal build only |
| **Location** | nothing yet (reserved for a `location` command) | launching app | Privacy & Security › Location Services |

WeatherKit (`weather`) needs no user permission.

Full Disk Access is the one to grant first. It covers every store cider
reads straight from disk: `~/Library/Messages/chat.db`,
`~/Library/Mail/V*/MailData/Envelope Index`, `~/Library/Safari/History.db`
and `Bookmarks.plist`, the Photos library database, Books, Voice Memos, the
call history, `~/Library/Accounts/Accounts4.sqlite`, the Stocks and Home
containers, `~/Library/Shortcuts`, `~/Library/Application Support/Knowledge/knowledgeC.db`, and the Calendar, Reminders, and Contacts
databases. `cider permissions` checks it by opening the Messages and Safari
stores for reading, which never prompts — and it has to open them: a
protected file's metadata reads fine even when opening it fails with EPERM.

### Embedding cider in an app (Alchemy, Tauri, any host)

When another app links the crate or runs the binary, *that app* is the
responsible process. It must ship the usage strings, or macOS refuses to
prompt and the access is silently denied forever — and, having never asked,
the app never appears in System Settings for the user to fix. Copy these
into the host's Info.plist (`cider::HOST_INFO_PLIST_KEYS` in the library):

```xml
<key>NSCalendarsFullAccessUsageDescription</key>
<string>Reads and updates your calendar events.</string>
<key>NSRemindersFullAccessUsageDescription</key>
<string>Reads and updates your reminders.</string>
<key>NSContactsUsageDescription</key>
<string>Looks up and updates your contacts.</string>
<key>NSAppleEventsUsageDescription</key>
<string>Controls Notes, Mail, Music, Messages, Safari, and Shortcuts on your behalf.</string>
<!-- optional today: reserved for a future `location` command -->
<key>NSLocationWhenInUseUsageDescription</key>
<string>Uses your location.</string>
```

Full Disk Access and HomeKit need no key: the user adds the host app under
Full Disk Access by hand, and HomeKit belongs to Cider Bridge.app, which
declares its own. From Rust, `cider::permissions::report().await` returns
the same report the CLI prints — show `how_to_grant` for anything `denied`,
`add_only`, or `not_determined` — and `cider::doctor::inspect().await` is
`cider doctor`.

## Bridge

Everything above works with the Rust binary alone. Some Apple data only
exists behind a framework that loads in a signed app, so `cider` can also
use an optional Swift helper: **Cider Bridge.app**, a Mac Catalyst app it
launches on demand and talks to over a Unix socket (it quits after ten idle
minutes; no daemon), and **`cider-bridge`**, a native CLI it runs per call.
Sources and protocol: [`bridge/`](bridge/), [docs/RFC-swift-bridge.md](docs/RFC-swift-bridge.md).

**Start here:** `cider bridge status` (what is installed, whether the app is
answering, protocol versions, per-store authorization) and `cider doctor`
(the `bridge_*` checks: app, socket, CLI, signing-profile expiry,
authorization, HomeKit). Neither launches the app or opens a dialog.

### What it adds

| Area | Without the bridge | With the bridge |
|------|--------------------|-----------------|
| **Home** | `cider home` reads the Home app's on-disk cache: homes, rooms, accessories, scenes, no live values, as fresh as the last time the Home app ran (`--envelope` reports `"source": "cache"` and `cache_age_s`; rows carry `cache_updated_at`) | The same reads go live (`"source": "bridge"`; `--live` insists on it), plus `home state [--room] [--accessory]`, `home run --scene`, `home set --accessory --characteristic --value`, and `home triggers create-timer\|enable\|disable\|delete` — timer automations that fire on the home hub with the Mac asleep. **Needs a personal build** (below). |
| **Weather** | Nothing: the Weather app's cache is encrypted | `cider weather [--forecast --days N] [--home <name> \| --lat --lon]`, WeatherKit with Apple's required `attribution` block — keep it next to the numbers you show a person. Location is `--lat/--lon`, else `--home`, else the primary home's address from the Home app. |
| **Reminders, Calendar writes** | AppleScript/JXA (works, seconds per write) | EventKit through `cider-bridge`: same commands and result shape, milliseconds, the saved row as `record`; `--dry-run` names the path and `--envelope` reports `"source": "cli"\|"native"`. Bulk reads stay on SQLite. `CIDER_BRIDGE_CLI=off` forces the old path. |
| **`cider watch`** | FSEvents on the on-disk stores (`"kind":"files_changed"`, coalesced `paths`) | Item-level EventKit/Contacts change notifications for reminders, calendar, contacts (`"kind":"store_changed"`); `--via cli` fails rather than falling back. An event says *that* a store changed; re-read it to learn what. |

Home ids differ between the two backends (the cache reports `homeUUID`, the
bridge `HMHome.uniqueIdentifier`; room, accessory, and scene ids match), so
select homes by name. `--home` takes a name first, then an id from either
space, mapping through the name when it can.

### What `brew install cider` includes

The Homebrew formula ships the bridge app and the CLI next to `cider`
(`libexec/Cider Bridge.app`, `bin/cider-bridge`), Developer ID signed and
notarized. That covers **everything except HomeKit**: WeatherKit, EventKit
and Contacts writes, and `watch`. Apple grants the HomeKit entitlement only
to App Store and development builds, so a packaged bridge reports
`homekit_entitled: false` and HomeKit commands fail with
`homekit_unavailable` (cache-backed `home` reads keep working). Cargo
installs get the Rust binary only.

### Getting HomeKit: a personal build

1. Xcode, [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`), and a paid Apple Developer team.
2. In the developer portal, enable **HomeKit** (and **WeatherKit**, if wanted) on the App ID `dev.thrasher.cider.bridge`. Xcode's automatic signing registers your Mac and makes the profile but cannot add HomeKit to a Catalyst App ID; that step is manual, once.
3. From a cider checkout: `cider bridge build --install --team <TEAM_ID>` (or `CIDER_TEAM_ID` in the environment or `bridge/.env.local`). This builds with XcodeGen + `xcodebuild`, installs `~/Applications/Cider Bridge.app` — which wins over the packaged copy — and puts `cider-bridge` inside it.

The development profile expires a year after it is made and the app then
silently stops launching; `cider doctor` reports `bridge_profile` as
`expiring` inside the last thirty days. Rebuild to renew.

### Permissions and configuration

The bridge's grants follow the rules in [Permissions](#permissions):
Calendars, Reminders, and Contacts belong to the app that launched `cider`
(`cider permissions`, or `cider bridge status` → `cli_authorization`, shows
each store's state with the fix), and HomeKit belongs to Cider Bridge.app
itself.

`cider bridge status|build|install|quit` manages the app;
`CIDER_BRIDGE_APP=/path/to/Cider Bridge.app` and
`CIDER_BRIDGE_CLI=/path/to/cider-bridge` point at builds elsewhere. Cider
checks the bridge's protocol version on every connection and fails with
`bridge_incompatible` when a stale app or CLI answers, naming the fix
(`cider bridge build --install`, or `brew upgrade cider` for the packaged
copy).

## License

MIT
