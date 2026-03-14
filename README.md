# cider

Read Apple app data from the command line. JSON on stdout, errors on stderr. Designed for both humans and AI agents.

## Commands

| Command | Source | Method |
|---------|--------|--------|
| `cider activity-monitor` | Activity Monitor | ps, vm_stat, sysctl |
| `cider apps` | App Store / Installed Apps | mdfind |
| `cider automator` | Automator | mdfind |
| `cider books` | Apple Books | SQLite |
| `cider calendar` | Calendar | SQLite / JXA fallback |
| `cider clock` | Clock | date, plist |
| `cider console` | Console | log show (NDJSON) |
| `cider contacts` | Contacts | JXA (chunked) |
| `cider find-my` | Find My | cache files |
| `cider fonts` | Font Book | filesystem |
| `cider home` | Home | JXA |
| `cider journal` | Journal | encrypted (limited) |
| `cider mail` | Mail | JXA |
| `cider maps` | Maps | plist |
| `cider messages` | Messages | SQLite |
| `cider music` | Music | JXA |
| `cider news` | News | SQLite |
| `cider notes` | Notes | AppleScript |
| `cider photo-booth` | Photo Booth | filesystem |
| `cider photos` | Photos | SQLite |
| `cider reading-list` | Safari Reading List | plist |
| `cider reminders` | Reminders | SQLite |
| `cider screen-sharing` | Screen Sharing | launchctl |
| `cider screenshots` | Screenshots | filesystem |
| `cider shortcuts` | Shortcuts | shortcuts CLI |
| `cider stickies` | Stickies | python/plist |
| `cider stocks` | Stocks | plist |
| `cider system-info` | System Settings | sysctl, sw_vers |
| `cider time-machine` | Time Machine | tmutil |
| `cider voice-memos` | Voice Memos | SQLite |
| `cider weather` | Weather | cache (limited) |

## Install

```sh
cargo install --path .
```

## Usage

```sh
# Compact JSON (agent-friendly)
cider reminders

# Pretty-print (human-friendly)
cider reminders --pretty

# Pipe to jq
cider contacts | jq '.[].name'
cider activity-monitor | jq '.[0].top_processes[:5]'

# With options
cider messages --days 7
cider console --minutes 5
```

## Agent Usage

cider outputs structured JSON on stdout and errors/progress on stderr, following [agent-friendly CLI principles](https://justin.poehnelt.com/posts/rewrite-your-cli-for-ai-agents/):

- All output is valid JSON arrays or objects
- Errors go to stderr with descriptive messages
- Omit `--pretty` for compact, token-efficient output
- Each command is independent — no shared state

## Requirements

- macOS (uses osascript, SQLite databases in ~/Library, etc.)
- Full Disk Access may be required for Messages, Photos, Safari Reading List
- Some commands require specific apps to have been used at least once

## Build

```sh
cargo build --release
```

The binary will be at `target/release/cider`.
