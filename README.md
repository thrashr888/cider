# cider

Manage your Mac from the command line. Reminders, Calendar, Contacts, Notes, Mail, Music, Keychain, Safari, and 30+ more Apple apps.

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

# Check your calendar
cider calendar

# Search contacts
cider contacts list --search Smith

# Control music
cider music play
cider music status
cider music pause

# Send a message
cider messages send --to "+15551234567" --text "On my way"

# Search your Mac
cider spotlight --query "quarterly report"
```

## What You Can Do

### Full CRUD

| App | Actions |
|-----|---------|
| Reminders | `list`, `create`, `complete`, `delete`, `lists` |
| Calendar | `list`, `create`, `delete`, `calendars` |
| Contacts | `list`, `get`, `create`, `update`, `delete`, `groups` |
| Notes | `list`, `get`, `create`, `update`, `delete`, `folders` |
| Mail | `list`, `get`, `read`, `unread`, `trash`, `mailboxes`, `send` |
| Keychain | `list`, `search`, `get-password`, `add`, `delete`, `keychains` |

### Actions & Controls

| App | Actions |
|-----|---------|
| Music | `list`, `play`, `pause`, `next`, `previous`, `status`, `playlists` |
| Messages | `list`, `send` |
| Shortcuts | `list`, `run`, `view`, `sign` |
| Screenshots | `list`, `capture` |
| Time Machine | `status`, `list`, `start`, `stop` |
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

Activity Monitor, Apps, Automator, Bluetooth, Books, Clock, Console, Disks, Find My, Fonts, Home, Maps, News, Photo Booth, Photos, Spotlight, Stickies, Stocks, Voice Memos, Weather

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

## License

MIT
