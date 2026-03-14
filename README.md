# cider

Read Apple app data from the command line. Outputs JSON for easy piping to `jq`, scripts, or other tools.

## Sources

| Command | Source | Method |
|---------|--------|--------|
| `cider reminders` | Apple Reminders | SQLite (direct) |
| `cider notes` | Apple Notes | AppleScript |
| `cider contacts` | Apple Contacts | JXA (chunked) |
| `cider messages` | iMessage / SMS | SQLite (direct) |
| `cider books` | Apple Books | SQLite (direct) |
| `cider reading-list` | Safari Reading List | plist |
| `cider all` | All of the above | Combined JSON |

## Install

```sh
cargo install --path .
```

## Usage

```sh
# Fetch reminders as JSON
cider reminders

# Pretty-print
cider reminders --pretty

# Pipe to jq
cider contacts | jq '.[].name'

# Messages from the last 7 days
cider messages --days 7

# Everything at once
cider all --pretty
```

## Requirements

- macOS (uses osascript, SQLite databases in ~/Library, etc.)
- Full Disk Access may be required for Messages and Safari Reading List

## Build

```sh
cargo build --release
```

The binary will be at `target/release/cider`.
