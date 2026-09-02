use std::io::{self, Read, Write};

use clap::{CommandFactory, Parser, Subcommand};

use cider::sources::bridge_cli::{self, WriteBackend};
use cider::{pretty, sources};

#[derive(Parser)]
#[command(
    name = "cider",
    version,
    about = "Read Apple app data from the command line. Outputs JSON to stdout, errors to stderr.",
    long_about = "cider reads data from macOS Apple apps and outputs structured JSON.\n\n\
                  Designed for both human use and AI agent consumption.\n\
                  All output is JSON on stdout. Progress/errors go to stderr.\n\
                  Use --pretty for human-readable formatting, omit for compact agent-friendly output."
)]
struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pretty: bool,

    /// Wrap responses in a stable top-level envelope
    #[arg(long, global = true)]
    envelope: bool,

    /// Show what mutating commands would do without executing them
    #[arg(long = "dry-run", global = true)]
    no_op: bool,

    /// Lowercase alias for --version
    #[arg(short = 'v', action = clap::ArgAction::Version, hide = true)]
    version_alias: (),

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show CPU, memory, and top processes (Activity Monitor)
    #[command(name = "activity-monitor")]
    ActivityMonitor,
    /// List installed applications (App Store)
    Apps,
    /// Show prompt-free read and Automation authorization status
    #[command(name = "auth-status")]
    AuthStatus,
    /// List Automator workflows
    Automator,
    /// List paired Bluetooth devices
    Bluetooth,
    /// Fetch books from Apple Books
    Books,
    /// The Cider Bridge helper app that gives `home` live HomeKit access
    Bridge {
        #[command(subcommand)]
        action: Option<BridgeAction>,
    },
    /// Interact with Calendar events
    Calendar {
        #[command(subcommand)]
        action: Option<CalendarAction>,
    },
    /// Show local time, world clocks, and alarms (Clock)
    Clock,
    /// Show recent system log entries (Console)
    Console {
        /// Minutes of logs to show
        #[arg(long, default_value = "30")]
        minutes: u32,
    },
    /// Interact with Apple Contacts
    Contacts {
        #[command(subcommand)]
        action: Option<ContactsAction>,
    },
    /// List mounted disks and volumes (Disk Utility)
    Disks,
    /// Check local databases, tools, and permissions without opening prompts
    Doctor,
    /// Recent call history (FaceTime + Phone)
    #[command(name = "facetime")]
    FaceTime {
        #[command(subcommand)]
        action: Option<FaceTimeAction>,
    },
    /// List installed fonts (Font Book)
    Fonts,
    /// Homes, rooms, accessories, and scenes (Home app cache, or live via Cider Bridge)
    Home {
        /// Require the Cider Bridge (launching it if needed) instead of the
        /// Home app cache; fail rather than fall back
        #[arg(long, global = true)]
        live: bool,
        #[command(subcommand)]
        action: Option<HomeAction>,
    },
    /// iCloud account, Drive quota, sync status and log, and Drive files (via brctl)
    #[command(name = "icloud")]
    Icloud {
        #[command(subcommand)]
        action: Option<IcloudAction>,
    },
    /// Manage Keychain passwords
    Keychain {
        #[command(subcommand)]
        action: Option<KeychainAction>,
    },
    /// Interact with Apple Mail
    Mail {
        #[command(subcommand)]
        action: Option<MailAction>,
    },
    /// Interact with Messages (iMessage/SMS)
    Messages {
        #[command(subcommand)]
        action: Option<MessagesAction>,
    },
    /// Interact with Music library
    Music {
        #[command(subcommand)]
        action: Option<MusicAction>,
    },
    /// List saved passwords (metadata only, no secrets)
    Passwords {
        #[command(subcommand)]
        action: Option<PasswordsAction>,
    },
    /// Interact with Apple Notes
    Notes {
        #[command(subcommand)]
        action: Option<NotesAction>,
    },
    /// List photos/videos from Photo Booth
    #[command(name = "photo-booth")]
    PhotoBooth,
    /// Fetch recent photos metadata from Photos
    Photos,
    /// Safari bookmarks, history, tabs, and reading list
    Safari {
        #[command(subcommand)]
        action: Option<SafariAction>,
    },
    /// Interact with Apple Reminders
    Reminders {
        #[command(subcommand)]
        action: Option<RemindersAction>,
    },
    /// Manage screen sharing
    #[command(name = "screen-sharing")]
    ScreenSharing {
        #[command(subcommand)]
        action: Option<ScreenSharingAction>,
    },
    /// Manage screenshots
    Screenshots {
        #[command(subcommand)]
        action: Option<ScreenshotsAction>,
    },
    /// Interact with Siri Shortcuts
    Shortcuts {
        #[command(subcommand)]
        action: Option<ShortcutsAction>,
    },
    /// Search files with Spotlight
    Spotlight {
        /// Search query
        #[arg(long)]
        query: String,
        /// Limit search to directory
        #[arg(long)]
        directory: Option<String>,
    },
    /// Fetch watchlists and cached quotes from Stocks
    Stocks {
        #[command(subcommand)]
        action: Option<StocksAction>,
    },
    /// Show and manage system information
    #[command(name = "system-info")]
    SystemInfo {
        #[command(subcommand)]
        action: Option<SystemInfoAction>,
    },
    /// Manage Time Machine backups
    #[command(name = "time-machine")]
    TimeMachine {
        #[command(subcommand)]
        action: Option<TimeMachineAction>,
    },
    /// Fetch voice memos
    #[command(name = "voice-memos")]
    VoiceMemos,
    /// Stream changes to local Apple data stores as JSON lines until Ctrl-C
    #[command(
        long_about = "Watch the stores behind Reminders, Calendar, Contacts, Notes, Home, and \
                      Shortcuts and print one compact JSON object per line as they change. \
                      Runs in the foreground until interrupted; there is no daemon. Lines are \
                      always compact (--pretty is ignored) and --envelope wraps each one.\n\n\
                      Each line is {source, at, kind, paths?}. With the cider-bridge CLI \
                      installed, reminders, calendar, and contacts stream EventKit/Contacts \
                      change notifications: kind is \"store_changed\" and there are no paths. \
                      Everything else (and everything, without the CLI or with --via \
                      fsevents) is FSEvents: kind is \"files_changed\" and paths lists the \
                      files that changed."
    )]
    Watch {
        /// Store to watch (repeatable); omit to watch all six
        #[arg(long, value_name = "SOURCE", value_parser = sources::watch::WatchSource::NAMES)]
        source: Vec<String>,
        /// Window, in milliseconds, for merging a burst of file events into one line
        #[arg(long = "debounce-ms", value_name = "MS", default_value_t = 2000)]
        debounce_ms: u64,
        /// Backend for reminders, calendar, and contacts: auto (cider-bridge when
        /// installed, else FSEvents), cli (require cider-bridge), or fsevents
        #[arg(long, value_name = "BACKEND", default_value = "auto", value_parser = sources::watch::Via::NAMES)]
        via: String,
    },
    /// Current conditions or forecast from Apple Weather (needs Cider Bridge)
    #[command(
        long_about = "WeatherKit through the Cider Bridge app (launched on demand): current \
                      conditions by default, --forecast for daily plus the next 24 hours. \
                      Location is --lat/--lon, else --home <name>, else the primary home's \
                      address from the Home app. Output carries Apple's required attribution \
                      block; show it alongside the data."
    )]
    Weather {
        /// Home whose address to use, by name or UUID (default: the primary home)
        #[arg(long, conflicts_with_all = ["lat", "lon"])]
        home: Option<String>,
        /// Latitude in degrees (with --lon)
        #[arg(long, requires = "lon", allow_hyphen_values = true)]
        lat: Option<f64>,
        /// Longitude in degrees (with --lat)
        #[arg(long, requires = "lat", allow_hyphen_values = true)]
        lon: Option<f64>,
        /// Daily forecast and the next 24 hours instead of current conditions
        #[arg(long)]
        forecast: bool,
        /// Days of daily forecast to return (implies --forecast)
        #[arg(long, value_name = "N")]
        days: Option<u32>,
    },
    /// Wi-Fi status and known networks
    #[command(name = "wifi")]
    Wifi {
        #[command(subcommand)]
        action: Option<WifiAction>,
    },
    /// Show machine-readable command schemas and capabilities
    Schema {
        /// Optional source/command name to inspect
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Subcommand)]
enum CalendarAction {
    /// List calendar events (default: past 7 days + next 30 days)
    List {
        /// Number of days to look back
        #[arg(long)]
        days_back: Option<u32>,
        /// Number of days to look ahead
        #[arg(long)]
        days_ahead: Option<u32>,
        /// Filter by calendar name
        #[arg(long)]
        calendar: Option<String>,
        /// Only records modified at or after this RFC 3339 timestamp
        /// (e.g. 2026-09-01T00:00:00Z)
        #[arg(long)]
        since: Option<String>,
    },
    /// Create a new calendar event
    Create {
        /// Event title
        #[arg(long)]
        title: String,
        /// Start date/time (ISO 8601)
        #[arg(long)]
        start: String,
        /// End date/time (ISO 8601)
        #[arg(long)]
        end: String,
        /// Calendar name (default: "Calendar")
        #[arg(long)]
        calendar: Option<String>,
        /// Event location
        #[arg(long)]
        location: Option<String>,
        /// Event notes
        #[arg(long)]
        notes: Option<String>,
        /// All-day event
        #[arg(long)]
        all_day: bool,
    },
    /// Create several events from a JSON array ("-" reads stdin)
    #[command(name = "batch-create")]
    BatchCreate {
        /// JSON array of event objects
        #[arg(long)]
        json: String,
    },
    /// Get a calendar event by stable ID
    Get {
        /// Event ID from `calendar list`
        #[arg(long)]
        id: String,
    },
    /// Update a calendar event in place by stable ID
    #[command(group(
        clap::ArgGroup::new("calendar_changes")
            .required(true)
            .args(["title", "start", "end", "location", "notes", "all_day"])
    ))]
    Update {
        /// Event ID from `calendar list`
        #[arg(long)]
        id: String,
        /// New event title
        #[arg(long)]
        title: Option<String>,
        /// New start date/time (ISO 8601)
        #[arg(long)]
        start: Option<String>,
        /// New end date/time (ISO 8601)
        #[arg(long)]
        end: Option<String>,
        /// New event location
        #[arg(long)]
        location: Option<String>,
        /// New event notes
        #[arg(long)]
        notes: Option<String>,
        /// Set all-day state to true or false
        #[arg(long)]
        all_day: Option<bool>,
    },
    /// Delete a calendar event by stable ID (legacy title/date also accepted)
    Delete {
        /// Event ID from `calendar list`
        #[arg(long, conflicts_with = "title", required_unless_present = "title")]
        id: Option<String>,
        /// Legacy event title; requires --date and refuses ambiguous matches
        #[arg(long, requires = "date")]
        title: Option<String>,
        /// Legacy event date (ISO 8601 date); requires --title
        #[arg(long, requires = "title")]
        date: Option<String>,
        /// Optional calendar name to narrow the search
        #[arg(long)]
        calendar: Option<String>,
    },
    /// List all calendar names
    Calendars,
}

#[derive(Subcommand)]
enum ContactsAction {
    /// List all contacts (default)
    List {
        /// Search contacts by name
        #[arg(long)]
        search: Option<String>,
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Get a single contact by ID
    Get {
        /// Contact ID
        #[arg(long)]
        id: String,
    },
    /// Create a new contact
    Create {
        /// First name
        #[arg(long, required_unless_present_any = ["last", "org"])]
        first: Option<String>,
        /// Last name
        #[arg(long)]
        last: Option<String>,
        /// Email address
        #[arg(long)]
        email: Vec<String>,
        /// Phone number
        #[arg(long)]
        phone: Vec<String>,
        /// Organization
        #[arg(long)]
        org: Option<String>,
        /// Middle name
        #[arg(long)]
        middle: Option<String>,
        /// Nickname
        #[arg(long)]
        nickname: Option<String>,
        /// Job title
        #[arg(long)]
        job_title: Option<String>,
        /// Department
        #[arg(long)]
        department: Option<String>,
        /// Birthday (ISO 8601 date)
        #[arg(long)]
        birthday: Option<String>,
        /// Contact note
        #[arg(long)]
        note: Option<String>,
    },
    /// Update an existing contact
    #[command(group(
        clap::ArgGroup::new("contact_changes")
            .required(true)
            .args([
                "first", "last", "email", "phone", "middle", "nickname", "org",
                "job_title", "department", "birthday", "note"
            ])
    ))]
    Update {
        /// Contact ID
        #[arg(long)]
        id: String,
        /// First name
        #[arg(long)]
        first: Option<String>,
        /// Last name
        #[arg(long)]
        last: Option<String>,
        /// Email address
        #[arg(long)]
        email: Option<String>,
        /// Phone number
        #[arg(long)]
        phone: Option<String>,
        /// Middle name
        #[arg(long)]
        middle: Option<String>,
        /// Nickname
        #[arg(long)]
        nickname: Option<String>,
        /// Organization
        #[arg(long)]
        org: Option<String>,
        /// Job title
        #[arg(long)]
        job_title: Option<String>,
        /// Department
        #[arg(long)]
        department: Option<String>,
        /// Birthday (ISO 8601 date)
        #[arg(long)]
        birthday: Option<String>,
        /// Contact note
        #[arg(long)]
        note: Option<String>,
    },
    /// Delete a contact
    Delete {
        /// Contact ID
        #[arg(long)]
        id: String,
    },
    /// List all contact groups
    Groups,
}

#[derive(Subcommand)]
enum StocksAction {
    /// List watchlist symbols with cached quotes (default)
    List,
    /// List watchlists and their symbols
    Watchlists,
    /// Get the cached quote for one symbol
    Quote {
        /// Ticker symbol, e.g. AAPL
        #[arg(long)]
        symbol: String,
    },
}

#[derive(Subcommand)]
enum NotesAction {
    /// List notes (default)
    List {
        /// Filter by folder name
        #[arg(long)]
        folder: Option<String>,
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned (default 50 unless --brief)
        #[arg(long)]
        limit: Option<usize>,
        /// Skip note bodies — fast bulk listing of the whole library
        #[arg(long)]
        brief: bool,
        /// Only records modified at or after this RFC 3339 timestamp
        /// (e.g. 2026-09-01T00:00:00Z)
        #[arg(long)]
        since: Option<String>,
    },
    /// Get a single note by ID
    Get {
        /// Note ID
        #[arg(long)]
        id: String,
    },
    /// Create a new note
    Create {
        /// Note title
        #[arg(long)]
        title: String,
        /// Note body
        #[arg(long)]
        body: Option<String>,
        /// Folder name (default: "Notes")
        #[arg(long)]
        folder: Option<String>,
    },
    /// Update a note's body
    Update {
        /// Note ID
        #[arg(long)]
        id: String,
        /// New body content
        #[arg(long)]
        body: String,
    },
    /// Delete a note
    Delete {
        /// Note ID
        #[arg(long)]
        id: String,
    },
    /// List all note folders
    Folders,
}

#[derive(Subcommand)]
enum RemindersAction {
    /// List incomplete reminders
    List {
        /// Filter by list name
        #[arg(long)]
        list: Option<String>,
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
        /// Search titles and notes
        #[arg(long)]
        search: Option<String>,
        /// Include completed reminders
        #[arg(long)]
        include_completed: bool,
        /// Only records modified at or after this RFC 3339 timestamp
        /// (e.g. 2026-09-01T00:00:00Z)
        #[arg(long)]
        since: Option<String>,
    },
    /// Create a new reminder
    Create {
        /// Reminder title
        #[arg(long)]
        title: String,
        /// List name (default: "Reminders")
        #[arg(long)]
        list: Option<String>,
        /// Due date (ISO 8601)
        #[arg(long)]
        due: Option<String>,
        /// Priority (0=none, 1-9)
        #[arg(long)]
        priority: Option<i32>,
        /// Notes ("-" reads them from stdin, for long or multiline content)
        #[arg(long)]
        notes: Option<String>,
    },
    /// Get one reminder in full — complete title and notes, no truncation
    Get {
        /// Reminder id, from `reminders list` (unambiguous when several
        /// reminders share a title)
        #[arg(long, conflicts_with = "title", required_unless_present = "title")]
        id: Option<String>,
        /// Reminder title to fetch
        #[arg(long)]
        title: Option<String>,
        /// List to search in (default: all lists)
        #[arg(long)]
        list: Option<String>,
    },
    /// Update a reminder in place (preserves its id and creation date)
    #[command(group(
        clap::ArgGroup::new("reminder_changes")
            .required(true)
            .args(["new_title", "notes", "append_notes", "priority", "due"])
    ))]
    Update {
        /// Reminder id to update, from `reminders list` (unambiguous when
        /// several reminders share a title)
        #[arg(long, conflicts_with = "title", required_unless_present = "title")]
        id: Option<String>,
        /// Reminder title to update
        #[arg(long)]
        title: Option<String>,
        /// List to search in (default: all lists)
        #[arg(long)]
        list: Option<String>,
        /// New title
        #[arg(long)]
        new_title: Option<String>,
        /// Replace the notes ("-" reads them from stdin, for long or
        /// multiline content)
        #[arg(long)]
        notes: Option<String>,
        /// Append to the notes after a newline ("-" reads from stdin)
        #[arg(long, conflicts_with = "notes")]
        append_notes: Option<String>,
        /// New priority (0=none, 1=high, 5=medium, 9=low)
        #[arg(long)]
        priority: Option<i32>,
        /// New due date
        #[arg(long)]
        due: Option<String>,
    },
    /// Mark a reminder as complete
    Complete {
        /// Reminder id to complete, from `reminders list` (unambiguous when
        /// several reminders share a title)
        #[arg(long, conflicts_with = "title", required_unless_present = "title")]
        id: Option<String>,
        /// Reminder title to complete
        #[arg(long)]
        title: Option<String>,
        /// List to search in (default: all lists)
        #[arg(long)]
        list: Option<String>,
    },
    /// Mark a completed reminder as incomplete by stable ID
    Reopen {
        /// Reminder id from `reminders list --include-completed`
        #[arg(long)]
        id: String,
        /// List to search in (default: all lists)
        #[arg(long)]
        list: Option<String>,
    },
    /// Delete a reminder
    Delete {
        /// Reminder id to delete, from `reminders list` (unambiguous when
        /// several reminders share a title)
        #[arg(long, conflicts_with = "title", required_unless_present = "title")]
        id: Option<String>,
        /// Reminder title to delete
        #[arg(long)]
        title: Option<String>,
        /// List to search in (default: all lists)
        #[arg(long)]
        list: Option<String>,
    },
    /// Complete several reminders in one operation
    #[command(name = "batch-complete")]
    BatchComplete {
        /// Reminder IDs; repeat --id for each item
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
    },
    /// Reopen several reminders in one operation
    #[command(name = "batch-reopen")]
    BatchReopen {
        /// Reminder IDs; repeat --id for each item
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
    },
    /// Delete several reminders in one operation
    #[command(name = "batch-delete")]
    BatchDelete {
        /// Reminder IDs; repeat --id for each item
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
    },
    /// List all reminder lists
    Lists,
}

#[derive(Subcommand)]
enum MailAction {
    /// List recent inbox messages (default)
    List {
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
        /// Search subject, sender, and body preview
        #[arg(long)]
        search: Option<String>,
        /// Mailbox name or URL (default: INBOX)
        #[arg(long)]
        mailbox: Option<String>,
        /// Only unread messages
        #[arg(long)]
        unread: bool,
        /// Only flagged messages
        #[arg(long)]
        flagged: bool,
    },
    /// Get a single message by stable ID (legacy index also accepted)
    Get {
        /// Stable RFC Message-ID from list output
        #[arg(long, conflicts_with = "index", required_unless_present = "index")]
        id: Option<String>,
        /// Legacy one-based inbox index
        #[arg(long)]
        index: Option<usize>,
    },
    /// Mark a message as read
    Read {
        /// Stable RFC Message-ID from list output
        #[arg(long, conflicts_with = "index", required_unless_present = "index")]
        id: Option<String>,
        /// Legacy one-based inbox index
        #[arg(long)]
        index: Option<usize>,
    },
    /// Mark a message as unread
    Unread {
        /// Stable RFC Message-ID from list output
        #[arg(long, conflicts_with = "index", required_unless_present = "index")]
        id: Option<String>,
        /// Legacy one-based inbox index
        #[arg(long)]
        index: Option<usize>,
    },
    /// Move a message to trash
    Trash {
        /// Stable RFC Message-ID from list output
        #[arg(long, conflicts_with = "index", required_unless_present = "index")]
        id: Option<String>,
        /// Legacy one-based inbox index
        #[arg(long)]
        index: Option<usize>,
    },
    /// Mark several messages read in one operation
    #[command(name = "batch-read")]
    BatchRead {
        /// Stable message IDs; repeat --id for each message
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
    },
    /// Mark several messages unread in one operation
    #[command(name = "batch-unread")]
    BatchUnread {
        /// Stable message IDs; repeat --id for each message
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
    },
    /// Trash several messages in one operation
    #[command(name = "batch-trash")]
    BatchTrash {
        /// Stable message IDs; repeat --id for each message
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
    },
    /// List all mailbox names
    Mailboxes,
    /// Send an email
    Send {
        /// Recipient email address
        #[arg(long)]
        to: String,
        /// Email subject
        #[arg(long)]
        subject: String,
        /// Email body
        #[arg(long)]
        body: String,
    },
}

#[derive(Subcommand)]
enum MessagesAction {
    /// List recent messages (default)
    List {
        /// Number of days to look back
        #[arg(long, default_value = "30")]
        days: u32,
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
        /// Only messages sent at or after this RFC 3339 timestamp
        /// (e.g. 2026-09-01T00:00:00Z); replaces the --days window
        #[arg(long)]
        since: Option<String>,
    },
    /// Send an iMessage/SMS
    Send {
        /// Recipient phone number or email
        #[arg(long)]
        to: String,
        /// Message text
        #[arg(long)]
        text: String,
    },
}

#[derive(Subcommand)]
enum MusicAction {
    /// List tracks from library (default)
    List,
    /// Play a track, playlist, or resume playback
    Play {
        /// Track name to play
        #[arg(long)]
        track: Option<String>,
        /// Playlist name to play from
        #[arg(long)]
        playlist: Option<String>,
    },
    /// Pause playback
    Pause,
    /// Skip to next track
    Next,
    /// Go to previous track
    Previous,
    /// Show currently playing track info
    Status,
    /// List all playlists
    Playlists,
}

#[derive(Subcommand)]
enum ShortcutsAction {
    /// List all shortcuts (default)
    List {
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Run a shortcut by name
    Run {
        /// Shortcut name
        #[arg(long)]
        name: String,
        /// Input to pass to the shortcut (piped via stdin)
        #[arg(long)]
        input: Option<String>,
    },
    /// Open a shortcut in the Shortcuts app
    View {
        /// Shortcut name
        #[arg(long)]
        name: String,
    },
    /// Dump an installed shortcut's actions as JSON
    Export {
        /// Shortcut name
        #[arg(long)]
        name: String,
    },
    /// Build a .shortcut file from a JSON spec (scene, delay_seconds, speak, open_url, ssh steps)
    Gen {
        /// Spec file path, or - for stdin
        #[arg(long)]
        spec: String,
        /// Output .shortcut path (default: ./<name>.shortcut)
        #[arg(long)]
        output: Option<String>,
        /// Sign the file for anyone so Shortcuts will import it
        #[arg(long)]
        sign: bool,
    },
    /// Open a .shortcut file in Shortcuts, which prompts to add it
    Install {
        /// Path to a signed .shortcut file
        #[arg(long)]
        input: String,
    },
    /// Sign a shortcut file
    Sign {
        /// Input shortcut file path
        #[arg(long)]
        input: String,
        /// Output signed shortcut file path
        #[arg(long)]
        output: String,
        /// Signing mode: anyone or people-who-know-me
        #[arg(long)]
        mode: Option<String>,
    },
}

#[derive(Subcommand)]
enum ScreenSharingAction {
    /// Show screen sharing status (default)
    Status,
    /// Enable screen sharing (requires sudo)
    Enable,
    /// Disable screen sharing (requires sudo)
    Disable,
}

#[derive(Subcommand)]
enum ScreenshotsAction {
    /// List recent screenshots (default)
    List {
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Take a screenshot
    Capture {
        /// Interactive selection mode
        #[arg(long)]
        selection: bool,
        /// Capture a specific window
        #[arg(long)]
        window: bool,
        /// Output file path
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
enum SystemInfoAction {
    /// Show system information (default)
    Show,
    /// Set the computer name
    SetName {
        /// New computer name
        #[arg(long)]
        name: String,
    },
    /// Read defaults for a domain
    DefaultsRead {
        /// Defaults domain (e.g. com.apple.dock)
        #[arg(long)]
        domain: String,
        /// Specific key to read
        #[arg(long)]
        key: Option<String>,
    },
    /// Write a defaults value
    DefaultsWrite {
        /// Defaults domain (e.g. com.apple.dock)
        #[arg(long)]
        domain: String,
        /// Key to write
        #[arg(long)]
        key: String,
        /// Value to write
        #[arg(long)]
        value: String,
    },
}

#[derive(Subcommand)]
enum TimeMachineAction {
    /// Show Time Machine status
    Status,
    /// List backup paths
    List,
    /// Start a backup
    Start,
    /// Stop a running backup
    Stop,
}

#[derive(Subcommand)]
enum IcloudAction {
    /// Signed-in iCloud account and its services (default)
    Account,
    /// Remaining iCloud storage, from `brctl quota`
    Quota,
    /// Items not yet fully synced, from `brctl status`
    Status {
        /// Restrict to one container, e.g. com.apple.CloudDocs
        #[arg(long)]
        container: Option<String>,
    },
    /// Recent iCloud Drive (CloudDocs) log lines, from `brctl log`
    Log {
        /// How far back to read, in minutes
        #[arg(long, default_value_t = 30)]
        minutes: u64,
    },
    /// List iCloud Drive entries as local or cloud-only, without downloading anything
    List {
        /// Folder relative to the iCloud Drive root (default: the root)
        #[arg(long)]
        folder: Option<String>,
        /// Only entries in this state: local, cloud, or all
        #[arg(long, default_value = "all", value_parser = ["local", "cloud", "all"])]
        state: String,
        /// Walk subfolders too
        #[arg(long)]
        recursive: bool,
    },
    /// Download a cloud-only item to this Mac (`brctl download`)
    Download {
        /// Path, relative to the iCloud Drive root or absolute inside it
        #[arg(long)]
        path: String,
    },
    /// Remove the local copy of an item, keeping it in iCloud (`brctl evict`)
    Evict {
        /// Path, relative to the iCloud Drive root or absolute inside it
        #[arg(long)]
        path: String,
    },
}

#[derive(Subcommand)]
enum KeychainAction {
    /// List all keychain items including certs and keys (metadata only)
    List {
        /// Filter by kind: generic-password, internet-password, certificate, key
        #[arg(long)]
        kind: Option<String>,
    },
    /// Search keychain items by service, server, or account name
    Search {
        /// Search query
        #[arg(long)]
        query: String,
        /// Filter by kind
        #[arg(long)]
        kind: Option<String>,
    },
    /// Get a password for a generic (app) password. Triggers macOS security dialog.
    #[command(name = "get-password")]
    GetPassword {
        /// Service name
        #[arg(long)]
        service: String,
        /// Account name
        #[arg(long)]
        account: Option<String>,
    },
    /// Get a password for an internet password
    #[command(name = "get-internet-password")]
    GetInternetPassword {
        /// Server name
        #[arg(long)]
        server: String,
        /// Account name
        #[arg(long)]
        account: Option<String>,
    },
    /// Add a generic password to the keychain
    Add {
        /// Service name
        #[arg(long)]
        service: String,
        /// Account name
        #[arg(long)]
        account: String,
        /// Password value
        #[arg(long)]
        password: String,
        /// Label
        #[arg(long)]
        label: Option<String>,
    },
    /// Delete a generic password from the keychain
    Delete {
        /// Service name
        #[arg(long)]
        service: String,
        /// Account name
        #[arg(long)]
        account: Option<String>,
    },
    /// List all keychains
    Keychains,
}

#[derive(Subcommand)]
enum FaceTimeAction {
    /// List recent calls (default)
    List {
        /// Maximum number of calls to show
        #[arg(long, default_value = "50")]
        limit: u32,
    },
}

#[derive(Subcommand)]
enum HomeAction {
    /// Every home with its rooms, accessories, and scenes nested (default)
    List,
    /// One row per home with counts
    Homes,
    /// Rooms, one row each with the home they belong to
    Rooms {
        /// Restrict to one home, by name or UUID
        #[arg(long)]
        home: Option<String>,
    },
    /// Accessories with their room, category, and services
    Accessories {
        /// Restrict to one home, by name or UUID
        #[arg(long)]
        home: Option<String>,
        /// Restrict to one room, by name or UUID
        #[arg(long)]
        room: Option<String>,
    },
    /// Scenes (HomeKit action sets), user-made and built-in
    Scenes {
        /// Restrict to one home, by name or UUID
        #[arg(long)]
        home: Option<String>,
    },
    /// Live characteristic values, one row each (needs Cider Bridge)
    State {
        /// Restrict to one home, by name or UUID
        #[arg(long)]
        home: Option<String>,
        /// Restrict to one room, by name or UUID
        #[arg(long)]
        room: Option<String>,
        /// Restrict to one accessory, by name or UUID
        #[arg(long)]
        accessory: Option<String>,
    },
    /// Run a scene (needs Cider Bridge)
    Run {
        /// Scene name or UUID
        #[arg(long)]
        scene: String,
        /// Home the scene belongs to, by name or UUID
        #[arg(long)]
        home: Option<String>,
    },
    /// Set one characteristic of an accessory (needs Cider Bridge)
    Set {
        /// Accessory name or UUID
        #[arg(long)]
        accessory: String,
        /// Characteristic name, as shown by `home state`
        #[arg(long)]
        characteristic: String,
        /// New value: JSON if it parses (true, 50, "warm"), else a string
        #[arg(long)]
        value: String,
        /// Service name or UUID, when the accessory has several
        #[arg(long)]
        service: Option<String>,
        /// Home the accessory belongs to, by name or UUID
        #[arg(long)]
        home: Option<String>,
    },
    /// Automations: list them, or create/enable/disable/delete timer triggers (needs Cider Bridge)
    Triggers {
        #[command(subcommand)]
        action: Option<HomeTriggersAction>,
    },
}

#[derive(Subcommand)]
enum HomeTriggersAction {
    /// Every trigger with its schedule, scenes, and last fire (default)
    List {
        /// Restrict to one home, by name or UUID
        #[arg(long)]
        home: Option<String>,
    },
    /// Create a timer trigger that runs scenes on the home hub, Mac asleep or not
    #[command(name = "create-timer")]
    CreateTimer {
        /// Trigger name as it will appear in the Home app
        #[arg(long)]
        name: String,
        /// First fire time, RFC 3339 with offset (2026-09-01T19:30:00-07:00)
        #[arg(long)]
        at: String,
        /// Repeat: daily, weekly, or <minutes>m (e.g. 90m); omit for once
        #[arg(long)]
        repeat: Option<String>,
        /// Scene to run, by name or UUID; repeat for several
        #[arg(long, required = true)]
        scene: Vec<String>,
        /// Home to create it in, by name or UUID
        #[arg(long)]
        home: Option<String>,
    },
    /// Enable a trigger
    Enable {
        /// Trigger name or UUID
        #[arg(long)]
        trigger: String,
        #[arg(long)]
        home: Option<String>,
    },
    /// Disable a trigger without deleting it
    Disable {
        /// Trigger name or UUID
        #[arg(long)]
        trigger: String,
        #[arg(long)]
        home: Option<String>,
    },
    /// Delete a trigger
    Delete {
        /// Trigger name or UUID
        #[arg(long)]
        trigger: String,
        #[arg(long)]
        home: Option<String>,
    },
}

#[derive(Subcommand)]
enum BridgeAction {
    /// Is the app installed, and is it answering? (default; never launches it)
    Status,
    /// Build the app from the bridge/ sources with your Apple team
    Build {
        /// Apple Developer team id (falls back to $CIDER_TEAM_ID)
        #[arg(long)]
        team: Option<String>,
        /// Copy the result into ~/Applications when the build succeeds
        #[arg(long)]
        install: bool,
    },
    /// Copy a built app bundle into ~/Applications
    Install {
        /// App bundle to install (default: the newest under bridge/.build)
        #[arg(long)]
        from: Option<std::path::PathBuf>,
    },
    /// Ask a running bridge to exit
    Quit,
}

#[derive(Subcommand)]
enum PasswordsAction {
    /// List saved passwords (default, metadata only)
    List {
        /// Search by name, service, or account
        #[arg(long)]
        search: Option<String>,
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Get a password entry by service name
    Get {
        /// Service or server name
        #[arg(long)]
        service: String,
        /// Account name (narrows the match)
        #[arg(long)]
        account: Option<String>,
        /// Show the actual password value (triggers macOS auth)
        #[arg(long)]
        reveal: bool,
    },
    /// Create a new password
    Create {
        /// Service name
        #[arg(long)]
        service: String,
        /// Account name (username/email)
        #[arg(long)]
        account: String,
        /// Password value
        #[arg(long)]
        password: String,
        /// Display label
        #[arg(long)]
        label: Option<String>,
    },
    /// Update an existing password
    Update {
        /// Service name
        #[arg(long)]
        service: String,
        /// Account name
        #[arg(long)]
        account: String,
        /// New password value
        #[arg(long)]
        password: String,
    },
    /// Delete a password
    Delete {
        /// Service name
        #[arg(long)]
        service: String,
        /// Account name (narrows the match)
        #[arg(long)]
        account: Option<String>,
    },
}

#[derive(Subcommand)]
enum SafariAction {
    /// List Safari bookmarks (default)
    Bookmarks,
    /// List browsing history
    History {
        /// Max results
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// List currently open tabs
    Tabs,
    /// List Safari Reading List items
    #[command(name = "reading-list")]
    ReadingList,
}

#[derive(Subcommand)]
enum WifiAction {
    /// Show current Wi-Fi connection status (default)
    Status,
    /// List known/preferred Wi-Fi networks
    Networks,
}

fn print_output(value: &serde_json::Value, human: bool, envelope: bool) -> anyhow::Result<()> {
    let wrapped;
    let value = if envelope {
        wrapped = envelope_value(value);
        &wrapped
    } else {
        value
    };

    let mut out = io::stdout().lock();
    if human {
        pretty::render(&mut out, value)
    } else {
        serde_json::to_writer(&mut out, value)?;
        writeln!(out)?;
        Ok(())
    }
}

fn envelope_value(value: &serde_json::Value) -> serde_json::Value {
    let ok = value
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    serde_json::json!({"ok": ok, "data": value})
}

/// `print_output` for commands that have more than one backend: with
/// `--envelope` the wrapper carries `"source"` so a caller knows whether the
/// data is live or cached (`cider home`), or whether a write went through
/// `cider-bridge` or AppleScript/JXA. Without the envelope the output is
/// unchanged.
fn print_sourced_output(
    value: &serde_json::Value,
    human: bool,
    envelope: bool,
    source: &str,
) -> anyhow::Result<()> {
    if !envelope {
        return print_output(value, human, false);
    }
    let mut wrapped = envelope_value(value);
    wrapped["source"] = serde_json::json!(source);
    print_output(&wrapped, human, false)
}

/// A Reminders/Calendar write result from whichever backend ran it.
fn print_write_output(
    value: &serde_json::Value,
    human: bool,
    envelope: bool,
    via: sources::bridge_cli::WriteBackend,
) -> anyhow::Result<()> {
    print_sourced_output(value, human, envelope, via.as_str())
}

fn print_batch_output(
    result: &sources::BatchActionResult,
    human: bool,
    envelope: bool,
) -> anyhow::Result<()> {
    print_output(&serde_json::to_value(result)?, human, envelope)?;
    if !result.ok {
        anyhow::bail!(
            "{} failed for {} of {} items",
            result.action,
            result.failed,
            result.requested
        );
    }
    Ok(())
}

/// `--since` takes RFC 3339 only: the sources compare it against store
/// timestamps, so it has to name an unambiguous instant (with an offset).
fn parse_since(value: Option<&str>) -> anyhow::Result<Option<chrono::DateTime<chrono::Utc>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match chrono::DateTime::parse_from_rfc3339(value.trim()) {
        Ok(dt) => Ok(Some(dt.with_timezone(&chrono::Utc))),
        Err(_) => {
            anyhow::bail!("--since must be RFC 3339, e.g. 2026-09-01T00:00:00Z (got {value:?})")
        }
    }
}

fn paginate_vec<T>(items: Vec<T>, offset: Option<usize>, limit: Option<usize>) -> Vec<T> {
    let offset = offset.unwrap_or(0);
    let iter = items.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

#[derive(serde::Serialize)]
struct DryRunResult {
    ok: bool,
    dry_run: bool,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn print_dry_run(
    action: &str,
    message: impl Into<String>,
    human: bool,
    envelope: bool,
) -> anyhow::Result<()> {
    let result = DryRunResult {
        ok: true,
        dry_run: true,
        action: action.to_string(),
        message: Some(message.into()),
    };
    print_output(&serde_json::to_value(&result)?, human, envelope)
}

/// `--source` names to watch targets; no names means every store.
fn watch_sources(names: &[String]) -> anyhow::Result<Vec<sources::watch::WatchSource>> {
    if names.is_empty() {
        return Ok(sources::watch::WatchSource::ALL.to_vec());
    }
    names.iter().map(|name| name.parse()).collect()
}

/// One watch event as one compact JSON line, flushed so a reader sees it now.
fn emit_watch_event(event: &sources::watch::WatchEvent, envelope: bool) -> anyhow::Result<()> {
    print_output(&serde_json::to_value(event)?, false, envelope)?;
    io::stdout().flush()?;
    Ok(())
}

macro_rules! run_source {
    ($source:expr, $pretty:expr, $envelope:expr) => {{
        let records = $source.await?;
        print_output(&serde_json::to_value(&records)?, $pretty, $envelope)?;
    }};
}

fn build_schema(source: Option<&str>) -> serde_json::Value {
    let mut root = Cli::command();
    root.build();
    let commands = root
        .get_subcommands()
        .filter(|command| command.get_name() != "help")
        .map(|command| command_schema(command, true))
        .collect::<Vec<_>>();

    match source {
        Some(source) => commands
            .into_iter()
            .find(|item| item["name"].as_str() == Some(source))
            .unwrap_or_else(|| serde_json::json!({"source":source,"error":"unknown_source"})),
        None => {
            let global_arguments = root
                .get_arguments()
                .filter(|argument| argument.is_global_set() && !argument.is_hide_set())
                .map(|argument| argument_schema(&root, argument))
                .collect::<Vec<_>>();
            serde_json::json!({
                "schema_version": 1,
                "command": "cider",
                "global_arguments": global_arguments,
                "commands": commands,
            })
        }
    }
}

fn command_schema(command: &clap::Command, top_level: bool) -> serde_json::Value {
    let name = command.get_name();
    let actions = command
        .get_subcommands()
        .filter(|action| action.get_name() != "help")
        .map(|action| command_schema(action, false))
        .collect::<Vec<_>>();
    let arguments = command
        .get_arguments()
        .filter(|argument| !argument.is_hide_set())
        .map(|argument| argument_schema(command, argument))
        .collect::<Vec<_>>();
    let mut usage_command = command.clone();
    let usage = usage_command.render_usage().to_string();
    let mut schema = serde_json::json!({
        "name": name,
        "description": command.get_about().map(ToString::to_string),
        "usage": usage,
        "arguments": arguments,
        "actions": actions,
    });

    if !top_level {
        let mutating = is_mutating_action(name);
        schema["kind"] = serde_json::json!(if mutating { "write" } else { "read" });
        schema["supports_dry_run"] = serde_json::json!(mutating);
    } else {
        let has_mutations = schema["actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| action["kind"] == "write"));
        schema["supports_dry_run"] = serde_json::json!(has_mutations);
        schema["source"] = serde_json::json!(name);
        schema["verbs"] = serde_json::json!(schema["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|action| action["name"].as_str())
            .collect::<Vec<_>>());
        schema["list_args"] = serde_json::json!(schema["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|action| action["name"] == "list")
            .and_then(|action| action["arguments"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|argument| argument["name"].as_str())
            .collect::<Vec<_>>());
        if let Some(identifier) = identifier_contract(name) {
            schema["stable_ids"] = serde_json::json!(true);
            schema["identifiers"] = identifier;
        } else {
            schema["stable_ids"] = serde_json::json!(false);
        }
        if name == "mail" {
            schema["friendly_mailboxes"] = serde_json::json!(true);
        }
        if name == "schema" {
            schema["capabilities"] = serde_json::json!(["schema"]);
        }
    }
    schema
}

fn argument_schema(command: &clap::Command, argument: &clap::Arg) -> serde_json::Value {
    let defaults = argument
        .get_default_values()
        .iter()
        .map(|value| value.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let value_names = argument
        .get_value_names()
        .map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let conflicts_with = command
        .get_arg_conflicts_with(argument)
        .into_iter()
        .filter(|conflict| !conflict.is_hide_set())
        .map(|conflict| conflict.get_id().as_str().to_string())
        .collect::<Vec<_>>();
    serde_json::json!({
        "name": argument.get_id().as_str(),
        "long": argument.get_long().map(|long| format!("--{long}")),
        "short": argument.get_short().map(|short| format!("-{short}")),
        "description": argument.get_help().map(ToString::to_string),
        "required": argument.is_required_set(),
        "global": argument.is_global_set(),
        "value_names": value_names,
        "default_values": defaults,
        "conflicts_with": conflicts_with,
        "action": format!("{:?}", argument.get_action()).to_lowercase(),
    })
}

fn is_mutating_action(action: &str) -> bool {
    matches!(
        action,
        "create"
            | "batch-create"
            | "update"
            | "delete"
            | "batch-delete"
            | "complete"
            | "batch-complete"
            | "reopen"
            | "batch-reopen"
            | "read"
            | "batch-read"
            | "unread"
            | "batch-unread"
            | "trash"
            | "batch-trash"
            | "send"
            | "add"
            | "play"
            | "pause"
            | "next"
            | "previous"
            | "run"
            | "view"
            | "sign"
            | "capture"
            | "enable"
            | "disable"
            | "set"
            | "set-name"
            | "create-timer"
            | "defaults-write"
            | "start"
            | "stop"
            | "build"
            | "install"
            | "download"
            | "evict"
    )
}

fn identifier_contract(source: &str) -> Option<serde_json::Value> {
    match source {
        "calendar" => Some(serde_json::json!({
            "stable": true,
            "field": "id",
            "format": "calendar_uid",
        })),
        "contacts" => Some(serde_json::json!({
            "stable": true,
            "field": "id",
            "format": "contacts_unique_id",
        })),
        "mail" => Some(serde_json::json!({
            "stable": true,
            "field": "id",
            "format": "rfc_message_id",
            "fallback_format": "local:<rowid>",
            "legacy_target": "index",
        })),
        "messages" | "notes" | "reminders" => Some(serde_json::json!({
            "stable": true,
            "field": "id",
        })),
        _ => None,
    }
}

/// `cider home`: reads go to the bridge when it is required (`--live`) or
/// already answering, else to the Home app cache; everything HomeKit-only
/// (`state`, `run`, `set`, `triggers`) always goes through the bridge.
async fn run_home(
    live: bool,
    action: Option<HomeAction>,
    pretty: bool,
    envelope: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    use sources::bridge::Bridge;
    use sources::home_live::{self as hl, Source};

    let action = action.unwrap_or(HomeAction::List);
    match &action {
        HomeAction::List
        | HomeAction::Homes
        | HomeAction::Rooms { .. }
        | HomeAction::Accessories { .. }
        | HomeAction::Scenes { .. } => {
            let (value, source) = match hl::bridge_for(live).await?.as_mut() {
                Some(bridge) => {
                    let value = match &action {
                        HomeAction::List => hl::list(bridge).await?,
                        HomeAction::Homes => hl::homes(bridge).await?,
                        HomeAction::Rooms { home } => hl::rooms(bridge, home.as_deref()).await?,
                        HomeAction::Accessories { home, room } => {
                            hl::accessories(bridge, home.as_deref(), room.as_deref()).await?
                        }
                        HomeAction::Scenes { home } => hl::scenes(bridge, home.as_deref()).await?,
                        _ => unreachable!("read subcommands only"),
                    };
                    (value, Source::Bridge)
                }
                None => {
                    let value = match &action {
                        HomeAction::List => serde_json::to_value(sources::home::list().await?)?,
                        HomeAction::Homes => serde_json::to_value(sources::home::homes().await?)?,
                        HomeAction::Rooms { home } => {
                            serde_json::to_value(sources::home::rooms(home.as_deref()).await?)?
                        }
                        HomeAction::Accessories { home, room } => serde_json::to_value(
                            sources::home::accessories(home.as_deref(), room.as_deref()).await?,
                        )?,
                        HomeAction::Scenes { home } => {
                            serde_json::to_value(sources::home::scenes(home.as_deref()).await?)?
                        }
                        _ => unreachable!("read subcommands only"),
                    };
                    (value, Source::Cache)
                }
            };
            print_sourced_output(&value, pretty, envelope, source.as_str())
        }
        HomeAction::State {
            home,
            room,
            accessory,
        } => {
            let mut bridge = Bridge::connect().await?;
            let rows = hl::state(
                &mut bridge,
                home.as_deref(),
                room.as_deref(),
                accessory.as_deref(),
            )
            .await?;
            print_output(&serde_json::to_value(&rows)?, pretty, envelope)
        }
        HomeAction::Run { scene, home } => {
            if dry_run {
                return print_dry_run(
                    "home.run",
                    format!("Would run scene {scene:?} via Cider Bridge"),
                    pretty,
                    envelope,
                );
            }
            let mut bridge = Bridge::connect().await?;
            let result = hl::run_scene(&mut bridge, home.as_deref(), scene).await?;
            print_output(&serde_json::to_value(&result)?, pretty, envelope)
        }
        HomeAction::Set {
            accessory,
            characteristic,
            value,
            service,
            home,
        } => {
            let value = hl::parse_value(value);
            if dry_run {
                return print_dry_run(
                    "home.set",
                    format!(
                        "Would set {characteristic:?} of {accessory:?} to {value} via Cider Bridge"
                    ),
                    pretty,
                    envelope,
                );
            }
            let mut bridge = Bridge::connect().await?;
            let result = hl::set(
                &mut bridge,
                home.as_deref(),
                accessory,
                characteristic,
                value,
                service.as_deref(),
            )
            .await?;
            print_output(&result, pretty, envelope)
        }
        HomeAction::Triggers { action } => {
            let action = action
                .as_ref()
                .unwrap_or(&HomeTriggersAction::List { home: None });
            match action {
                HomeTriggersAction::List { home } => {
                    let mut bridge = Bridge::connect().await?;
                    let value = hl::triggers(&mut bridge, home.as_deref()).await?;
                    print_output(&value, pretty, envelope)
                }
                HomeTriggersAction::CreateTimer {
                    name,
                    at,
                    repeat,
                    scene,
                    home,
                } => {
                    hl::validate_fire_at(at)?;
                    let recurrence = repeat.as_deref().map(hl::parse_repeat).transpose()?;
                    if dry_run {
                        return print_dry_run(
                            "home.triggers.create-timer",
                            format!(
                                "Would create timer trigger {name:?} at {at} ({}) running {} via Cider Bridge",
                                recurrence
                                    .as_ref()
                                    .map(|r| format!("repeat {r}"))
                                    .unwrap_or_else(|| "once".to_string()),
                                scene.join(", ")
                            ),
                            pretty,
                            envelope,
                        );
                    }
                    let mut bridge = Bridge::connect().await?;
                    let row =
                        hl::create_timer(&mut bridge, home.as_deref(), name, at, recurrence, scene)
                            .await?;
                    print_output(&row, pretty, envelope)
                }
                HomeTriggersAction::Enable { trigger, home }
                | HomeTriggersAction::Disable { trigger, home } => {
                    let enabled = matches!(action, HomeTriggersAction::Enable { .. });
                    let verb = if enabled { "enable" } else { "disable" };
                    if dry_run {
                        return print_dry_run(
                            &format!("home.triggers.{verb}"),
                            format!("Would {verb} trigger {trigger:?} via Cider Bridge"),
                            pretty,
                            envelope,
                        );
                    }
                    let mut bridge = Bridge::connect().await?;
                    let row =
                        hl::set_trigger_enabled(&mut bridge, home.as_deref(), trigger, enabled)
                            .await?;
                    print_output(&row, pretty, envelope)
                }
                HomeTriggersAction::Delete { trigger, home } => {
                    if dry_run {
                        return print_dry_run(
                            "home.triggers.delete",
                            format!("Would delete trigger {trigger:?} via Cider Bridge"),
                            pretty,
                            envelope,
                        );
                    }
                    let mut bridge = Bridge::connect().await?;
                    let result = hl::delete_trigger(&mut bridge, home.as_deref(), trigger).await?;
                    print_output(&serde_json::to_value(&result)?, pretty, envelope)
                }
            }
        }
    }
}

async fn run_bridge(
    action: Option<BridgeAction>,
    pretty: bool,
    envelope: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    use sources::bridge;

    match action.unwrap_or(BridgeAction::Status) {
        BridgeAction::Status => {
            let status = bridge::status().await;
            print_output(&serde_json::to_value(&status)?, pretty, envelope)
        }
        BridgeAction::Build { team, install } => {
            if dry_run {
                let script = bridge::build_script_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "bridge/scripts/build.sh (not found)".to_string());
                return print_dry_run(
                    "bridge.build",
                    format!(
                        "Would run {script}{}{}",
                        team.as_deref()
                            .map(|t| format!(" with CIDER_TEAM_ID={t}"))
                            .unwrap_or_default(),
                        if install {
                            " and install into ~/Applications"
                        } else {
                            ""
                        }
                    ),
                    pretty,
                    envelope,
                );
            }
            let result = bridge::build(team.as_deref(), install).await?;
            print_output(&serde_json::to_value(&result)?, pretty, envelope)
        }
        BridgeAction::Install { from } => {
            if dry_run {
                let source = from
                    .clone()
                    .or_else(bridge::built_app_path)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(no built app found)".to_string());
                return print_dry_run(
                    "bridge.install",
                    format!("Would copy {source} into ~/Applications"),
                    pretty,
                    envelope,
                );
            }
            let result = bridge::install(from.as_deref()).await?;
            print_output(&serde_json::to_value(&result)?, pretty, envelope)
        }
        BridgeAction::Quit => {
            let result = bridge::quit().await?;
            print_output(&serde_json::to_value(&result)?, pretty, envelope)
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ActivityMonitor => {
            run_source!(sources::activity_monitor::fetch(), cli.pretty, cli.envelope)
        }
        Commands::Apps => run_source!(sources::apps::fetch(), cli.pretty, cli.envelope),
        Commands::AuthStatus => {
            let report = sources::doctor::auth_status().await;
            print_output(&serde_json::to_value(&report)?, cli.pretty, cli.envelope)?;
        }
        Commands::Automator => run_source!(sources::automator::fetch(), cli.pretty, cli.envelope),
        Commands::Bluetooth => run_source!(sources::bluetooth::list(), cli.pretty, cli.envelope),
        Commands::Books => run_source!(sources::books::fetch(), cli.pretty, cli.envelope),
        Commands::Calendar { action } => match action {
            None => {
                run_source!(
                    sources::calendar::list(None, None, None, None),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(CalendarAction::List {
                days_back,
                days_ahead,
                calendar,
                since,
            }) => {
                let since = parse_since(since.as_deref())?;
                run_source!(
                    sources::calendar::list(days_back, days_ahead, calendar.as_deref(), since),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(CalendarAction::Create {
                title,
                start,
                end,
                calendar,
                location,
                notes,
                all_day,
            }) => {
                sources::calendar::validate_event_range(&start, &end)?;
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "calendar.create",
                        format!(
                            "Would create event '{title}' starting {start} ending {end} {}",
                            via.label()
                        ),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let value = if via.is_cli() {
                        bridge_cli::calendar_create(
                            &title,
                            &start,
                            &end,
                            calendar.as_deref(),
                            location.as_deref(),
                            notes.as_deref(),
                            all_day,
                        )
                        .await?
                    } else {
                        serde_json::to_value(
                            sources::calendar::create(
                                &title,
                                &start,
                                &end,
                                calendar.as_deref(),
                                location.as_deref(),
                                notes.as_deref(),
                                all_day,
                            )
                            .await?,
                        )?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(CalendarAction::BatchCreate { json }) => {
                let json = stdin_if_dash(Some(json))?.unwrap_or_default();
                let events: Vec<sources::calendar::NewCalendarEvent> = serde_json::from_str(&json)
                    .map_err(|error| anyhow::anyhow!("Invalid calendar batch JSON: {error}"))?;
                sources::calendar::validate_batch(&events)?;
                if cli.no_op {
                    print_dry_run(
                        "calendar.batch-create",
                        format!("Would create {} calendar events", events.len()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::calendar::batch_create(&events).await?;
                    print_batch_output(&result, cli.pretty, cli.envelope)?;
                }
            }
            Some(CalendarAction::Get { id }) => {
                let result = sources::calendar::get(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(CalendarAction::Update {
                id,
                title,
                start,
                end,
                location,
                notes,
                all_day,
            }) => {
                if let Some(start) = start.as_deref() {
                    sources::calendar::validate_event_range(
                        start,
                        end.as_deref().unwrap_or(start),
                    )?;
                } else if let Some(end) = end.as_deref() {
                    sources::calendar::validate_event_range(end, end)?;
                }
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "calendar.update",
                        format!("Would update calendar event '{id}' {}", via.label()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let fields = sources::calendar::UpdateFields {
                        title: title.as_deref(),
                        start: start.as_deref(),
                        end: end.as_deref(),
                        location: location.as_deref(),
                        notes: notes.as_deref(),
                        all_day,
                    };
                    let value = if via.is_cli() {
                        bridge_cli::calendar_update(&id, &fields).await?
                    } else {
                        serde_json::to_value(sources::calendar::update(&id, &fields).await?)?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(CalendarAction::Delete {
                id,
                title,
                date,
                calendar,
            }) => {
                // The CLI deletes by id; the legacy title/date target stays
                // on JXA, which is where its ambiguity check lives.
                let via = if id.is_some() {
                    WriteBackend::detect()
                } else {
                    WriteBackend::Native
                };
                if cli.no_op {
                    let target =
                        id.as_deref()
                            .map(|id| format!("id '{id}'"))
                            .unwrap_or_else(|| {
                                format!(
                                    "'{}' on {}",
                                    title.as_deref().unwrap_or(""),
                                    date.as_deref().unwrap_or("")
                                )
                            });
                    print_dry_run(
                        "calendar.delete",
                        format!("Would delete event {target} {}", via.label()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let value = match id {
                        Some(id) if via.is_cli() => bridge_cli::calendar_delete(&id).await?,
                        Some(id) => {
                            serde_json::to_value(sources::calendar::delete_by_id(&id).await?)?
                        }
                        None => serde_json::to_value(
                            sources::calendar::delete(
                                title.as_deref().unwrap_or(""),
                                date.as_deref().unwrap_or(""),
                                calendar.as_deref(),
                            )
                            .await?,
                        )?,
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(CalendarAction::Calendars) => {
                run_source!(sources::calendar::calendars(), cli.pretty, cli.envelope)
            }
        },
        Commands::Clock => run_source!(sources::clock::fetch(), cli.pretty, cli.envelope),
        Commands::Console { minutes } => {
            run_source!(
                sources::console_logs::fetch(minutes),
                cli.pretty,
                cli.envelope
            )
        }
        Commands::Contacts { action } => match action {
            None => {
                run_source!(sources::contacts::list(None), cli.pretty, cli.envelope)
            }
            Some(ContactsAction::List {
                search,
                offset,
                limit,
            }) => {
                let records = paginate_vec(
                    sources::contacts::list(search.as_deref()).await?,
                    offset,
                    limit,
                );
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(ContactsAction::Get { id }) => {
                let result = sources::contacts::get(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(ContactsAction::Create {
                first,
                last,
                email,
                phone,
                org,
                middle,
                nickname,
                job_title,
                department,
                birthday,
                note,
            }) => {
                let first = first.as_deref().unwrap_or("");
                let last = last.as_deref().unwrap_or("");
                if cli.no_op {
                    print_dry_run(
                        "contacts.create",
                        format!("Would create contact '{} {}'", first, last),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let contact = sources::contacts::NewContact {
                        first,
                        last,
                        middle: middle.as_deref(),
                        nickname: nickname.as_deref(),
                        organization: org.as_deref(),
                        job_title: job_title.as_deref(),
                        department: department.as_deref(),
                        birthday: birthday.as_deref(),
                        note: note.as_deref(),
                        emails: &email,
                        phones: &phone,
                    };
                    let result = sources::contacts::create_detailed(&contact).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ContactsAction::Update {
                id,
                first,
                last,
                email,
                phone,
                middle,
                nickname,
                org,
                job_title,
                department,
                birthday,
                note,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "contacts.update",
                        format!("Would update contact '{id}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let fields = sources::contacts::ContactUpdates {
                        first: first.as_deref(),
                        last: last.as_deref(),
                        middle: middle.as_deref(),
                        nickname: nickname.as_deref(),
                        organization: org.as_deref(),
                        job_title: job_title.as_deref(),
                        department: department.as_deref(),
                        birthday: birthday.as_deref(),
                        note: note.as_deref(),
                        email: email.as_deref(),
                        phone: phone.as_deref(),
                    };
                    let result = sources::contacts::update_detailed(&id, &fields).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ContactsAction::Delete { id }) => {
                if cli.no_op {
                    print_dry_run(
                        "contacts.delete",
                        format!("Would delete contact '{id}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::contacts::delete(&id).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ContactsAction::Groups) => {
                run_source!(sources::contacts::groups(), cli.pretty, cli.envelope)
            }
        },
        Commands::Disks => run_source!(sources::disks::list(), cli.pretty, cli.envelope),
        Commands::Doctor => {
            let report = sources::doctor::inspect().await;
            print_output(&serde_json::to_value(&report)?, cli.pretty, cli.envelope)?;
        }
        Commands::FaceTime { action } => match action {
            None => {
                run_source!(sources::facetime::list(50), cli.pretty, cli.envelope)
            }
            Some(FaceTimeAction::List { limit }) => {
                run_source!(sources::facetime::list(limit), cli.pretty, cli.envelope)
            }
        },
        Commands::Fonts => run_source!(sources::fonts::fetch(), cli.pretty, cli.envelope),
        Commands::Home { live, action } => {
            run_home(live, action, cli.pretty, cli.envelope, cli.no_op).await?
        }
        Commands::Bridge { action } => {
            run_bridge(action, cli.pretty, cli.envelope, cli.no_op).await?
        }
        Commands::Keychain { action } => match action {
            None | Some(KeychainAction::List { kind: None }) => {
                run_source!(sources::keychain::list(None), cli.pretty, cli.envelope)
            }
            Some(KeychainAction::List { kind }) => {
                run_source!(
                    sources::keychain::list(kind.as_deref()),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(KeychainAction::Search { query, kind }) => {
                run_source!(
                    sources::keychain::search(&query, kind.as_deref()),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(KeychainAction::GetPassword { service, account }) => {
                let pw = sources::keychain::get_password(&service, account.as_deref()).await?;
                print_output(&serde_json::to_value(&pw)?, cli.pretty, cli.envelope)?;
            }
            Some(KeychainAction::GetInternetPassword { server, account }) => {
                let pw =
                    sources::keychain::get_internet_password(&server, account.as_deref()).await?;
                print_output(&serde_json::to_value(&pw)?, cli.pretty, cli.envelope)?;
            }
            Some(KeychainAction::Add {
                service,
                account,
                password,
                label,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "keychain.add",
                        format!("Would add keychain password for {service}/{account}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::keychain::add(&service, &account, &password, label.as_deref())
                            .await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(KeychainAction::Delete { service, account }) => {
                if cli.no_op {
                    print_dry_run(
                        "keychain.delete",
                        format!("Would delete keychain password for {service}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::keychain::delete(&service, account.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(KeychainAction::Keychains) => {
                run_source!(sources::keychain::keychains(), cli.pretty, cli.envelope)
            }
        },
        Commands::Mail { action } => match action {
            None => {
                let records = paginate_vec(sources::mail::list().await?, None, None);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(MailAction::List {
                offset,
                limit,
                search,
                mailbox,
                unread,
                flagged,
            }) => {
                let fetch_limit = offset.unwrap_or(0) + limit.unwrap_or(50);
                let query = sources::mail::MailQuery {
                    search: search.as_deref(),
                    mailbox: mailbox.as_deref().or(Some("INBOX")),
                    unread_only: unread,
                    flagged_only: flagged,
                    limit: fetch_limit,
                };
                let records = paginate_vec(sources::mail::search(&query).await?, offset, limit);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(MailAction::Get { id, index }) => {
                let result = match (id.as_deref(), index) {
                    (Some(id), _) => sources::mail::get_by_id(id).await?,
                    (_, Some(index)) => sources::mail::get(index).await?,
                    _ => unreachable!("clap requires id or index"),
                };
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(MailAction::Read { id, index }) => {
                let target = mail_target_description(id.as_deref(), index);
                if cli.no_op {
                    print_dry_run(
                        "mail.read",
                        format!("Would mark inbox message {target} as read"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = match (id.as_deref(), index) {
                        (Some(id), _) => sources::mail::read_by_id(id).await?,
                        (_, Some(index)) => sources::mail::read(index).await?,
                        _ => unreachable!("clap requires id or index"),
                    };
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::Unread { id, index }) => {
                let target = mail_target_description(id.as_deref(), index);
                if cli.no_op {
                    print_dry_run(
                        "mail.unread",
                        format!("Would mark inbox message {target} as unread"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = match (id.as_deref(), index) {
                        (Some(id), _) => sources::mail::unread_by_id(id).await?,
                        (_, Some(index)) => sources::mail::unread(index).await?,
                        _ => unreachable!("clap requires id or index"),
                    };
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::Trash { id, index }) => {
                let target = mail_target_description(id.as_deref(), index);
                if cli.no_op {
                    print_dry_run(
                        "mail.trash",
                        format!("Would trash inbox message {target}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = match (id.as_deref(), index) {
                        (Some(id), _) => sources::mail::trash_by_id(id).await?,
                        (_, Some(index)) => sources::mail::trash(index).await?,
                        _ => unreachable!("clap requires id or index"),
                    };
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::BatchRead { ids }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.batch-read",
                        format!("Would mark {} messages read", ids.len()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::batch_read(&ids).await?;
                    print_batch_output(&result, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::BatchUnread { ids }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.batch-unread",
                        format!("Would mark {} messages unread", ids.len()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::batch_unread(&ids).await?;
                    print_batch_output(&result, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::BatchTrash { ids }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.batch-trash",
                        format!("Would trash {} messages", ids.len()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::batch_trash(&ids).await?;
                    print_batch_output(&result, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::Mailboxes) => {
                run_source!(sources::mail::mailboxes(), cli.pretty, cli.envelope)
            }
            Some(MailAction::Send { to, subject, body }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.send",
                        format!("Would send mail to {to} with subject '{subject}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::send(&to, &subject, &body).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Messages { action } => match action {
            None => {
                run_source!(sources::messages::list(30, None), cli.pretty, cli.envelope)
            }
            Some(MessagesAction::List {
                days,
                offset,
                limit,
                since,
            }) => {
                let since = parse_since(since.as_deref())?;
                let records =
                    paginate_vec(sources::messages::list(days, since).await?, offset, limit);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(MessagesAction::Send { to, text }) => {
                if cli.no_op {
                    print_dry_run(
                        "messages.send",
                        format!("Would send message to {to}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::messages::send(&to, &text).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Music { action } => match action {
            None | Some(MusicAction::List) => {
                run_source!(sources::music::list(), cli.pretty, cli.envelope)
            }
            Some(MusicAction::Play { track, playlist }) => {
                if cli.no_op {
                    print_dry_run(
                        "music.play",
                        "Would start Music playback",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::music::play(track.as_deref(), playlist.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MusicAction::Pause) => {
                if cli.no_op {
                    print_dry_run(
                        "music.pause",
                        "Would pause Music playback",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::music::pause().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MusicAction::Next) => {
                if cli.no_op {
                    print_dry_run(
                        "music.next",
                        "Would skip to next track",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::music::next().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MusicAction::Previous) => {
                if cli.no_op {
                    print_dry_run(
                        "music.previous",
                        "Would go to previous track",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::music::previous().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MusicAction::Status) => {
                let result = sources::music::status().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(MusicAction::Playlists) => {
                run_source!(sources::music::playlists(), cli.pretty, cli.envelope)
            }
        },
        Commands::Passwords { action } => match action {
            None => {
                run_source!(sources::passwords::list(None), cli.pretty, cli.envelope)
            }
            Some(PasswordsAction::List {
                search,
                offset,
                limit,
            }) => {
                let records = paginate_vec(
                    sources::passwords::list(search.as_deref()).await?,
                    offset,
                    limit,
                );
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(PasswordsAction::Get {
                service,
                account,
                reveal,
            }) => {
                if reveal {
                    let pw = sources::passwords::get_password(&service, account.as_deref()).await?;
                    print_output(&serde_json::to_value(&pw)?, cli.pretty, cli.envelope)?;
                } else {
                    let entry = sources::passwords::get(&service, account.as_deref()).await?;
                    print_output(&serde_json::to_value(&entry)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(PasswordsAction::Create {
                service,
                account,
                password,
                label,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "passwords.create",
                        format!("Would create password for {service}/{account}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::passwords::create(&service, &account, &password, label.as_deref())
                            .await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(PasswordsAction::Update {
                service,
                account,
                password,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "passwords.update",
                        format!("Would update password for {service}/{account}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::passwords::update(&service, &account, &password).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(PasswordsAction::Delete { service, account }) => {
                if cli.no_op {
                    print_dry_run(
                        "passwords.delete",
                        format!("Would delete password for {service}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::passwords::delete(&service, account.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Notes { action } => match action {
            None => {
                run_source!(
                    sources::notes::list(None, Some(50), None),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(NotesAction::List {
                folder,
                offset,
                limit,
                brief,
                since,
            }) => {
                let since = parse_since(since.as_deref())?;
                // Bodies cost one Apple event per note, so the walk stops at
                // offset+limit (default 50); --brief skips bodies and lists
                // the whole library in bulk.
                let records = if brief {
                    sources::notes::list_brief(folder.as_deref(), since).await?
                } else {
                    let cap = limit.map(|l| l + offset.unwrap_or(0)).or(Some(50));
                    sources::notes::list(folder.as_deref(), cap, since).await?
                };
                let records = paginate_vec(records, offset, limit);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(NotesAction::Get { id }) => {
                let result = sources::notes::get(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(NotesAction::Create {
                title,
                body,
                folder,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "notes.create",
                        format!("Would create note '{title}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::notes::create(&title, body.as_deref(), folder.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(NotesAction::Update { id, body }) => {
                if cli.no_op {
                    print_dry_run(
                        "notes.update",
                        format!("Would update note '{id}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::notes::update(&id, &body).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(NotesAction::Delete { id }) => {
                if cli.no_op {
                    print_dry_run(
                        "notes.delete",
                        format!("Would delete note '{id}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::notes::delete(&id).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(NotesAction::Folders) => {
                run_source!(sources::notes::folders(), cli.pretty, cli.envelope)
            }
        },
        Commands::PhotoBooth => {
            run_source!(sources::photo_booth::fetch(), cli.pretty, cli.envelope)
        }
        Commands::Photos => run_source!(sources::photos::fetch(), cli.pretty, cli.envelope),
        Commands::Safari { action } => match action {
            None | Some(SafariAction::Bookmarks) => {
                run_source!(sources::safari::bookmarks(), cli.pretty, cli.envelope)
            }
            Some(SafariAction::History { limit }) => {
                run_source!(
                    sources::safari::history(Some(limit)),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(SafariAction::Tabs) => {
                run_source!(sources::safari::tabs(), cli.pretty, cli.envelope)
            }
            Some(SafariAction::ReadingList) => {
                run_source!(sources::reading_list::fetch(), cli.pretty, cli.envelope)
            }
        },
        Commands::Reminders { action } => match action {
            None => {
                run_source!(sources::reminders::list(None), cli.pretty, cli.envelope)
            }
            Some(RemindersAction::List {
                list,
                offset,
                limit,
                search,
                include_completed,
                since,
            }) => {
                let since = parse_since(since.as_deref())?;
                let records = paginate_vec(
                    sources::reminders::query(
                        list.as_deref(),
                        search.as_deref(),
                        include_completed,
                        since,
                    )
                    .await?,
                    offset,
                    limit,
                );
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(RemindersAction::Create {
                title,
                list,
                due,
                priority,
                notes,
            }) => {
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "reminders.create",
                        format!("Would create reminder '{title}' {}", via.label()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let notes = stdin_if_dash(notes)?;
                    let value = if via.is_cli() {
                        bridge_cli::reminders_create(
                            &title,
                            list.as_deref(),
                            due.as_deref(),
                            priority,
                            notes.as_deref(),
                        )
                        .await?
                    } else {
                        serde_json::to_value(
                            sources::reminders::create(
                                &title,
                                list.as_deref(),
                                due.as_deref(),
                                priority,
                                notes.as_deref(),
                            )
                            .await?,
                        )?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(RemindersAction::Get { id, title, list }) => {
                let target = reminder_target(id.as_deref(), title.as_deref());
                let result = sources::reminders::get(target, list.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(RemindersAction::Update {
                id,
                title,
                list,
                new_title,
                notes,
                append_notes,
                priority,
                due,
            }) => {
                let target = reminder_target(id.as_deref(), title.as_deref());
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "reminders.update",
                        format!(
                            "Would update reminder {} {}",
                            target.describe(),
                            via.label()
                        ),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let notes = stdin_if_dash(notes)?;
                    let append_notes = stdin_if_dash(append_notes)?;
                    let fields = sources::reminders::UpdateFields {
                        title: new_title.as_deref(),
                        notes: notes.as_deref(),
                        append_notes: append_notes.as_deref(),
                        priority,
                        due: due.as_deref(),
                    };
                    let value = if via.is_cli() {
                        let id = bridge_cli::reminder_id(target, list.as_deref()).await?;
                        bridge_cli::reminders_update(&id, &fields).await?
                    } else {
                        serde_json::to_value(
                            sources::reminders::update(target, list.as_deref(), &fields).await?,
                        )?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(RemindersAction::Complete { id, title, list }) => {
                // clap guarantees exactly one of the two is present.
                let target = reminder_target(id.as_deref(), title.as_deref());
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "reminders.complete",
                        format!(
                            "Would complete reminder {} {}",
                            target.describe(),
                            via.label()
                        ),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let value = if via.is_cli() {
                        let id = bridge_cli::reminder_id(target, list.as_deref()).await?;
                        bridge_cli::reminders_set_completed(&id, true).await?
                    } else {
                        serde_json::to_value(
                            sources::reminders::complete(target, list.as_deref()).await?,
                        )?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(RemindersAction::Reopen { id, list }) => {
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "reminders.reopen",
                        format!("Would reopen reminder id {id} {}", via.label()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let value = if via.is_cli() {
                        bridge_cli::reminders_set_completed(id.trim(), false).await?
                    } else {
                        serde_json::to_value(
                            sources::reminders::reopen(
                                sources::reminders::Target::Id(&id),
                                list.as_deref(),
                            )
                            .await?,
                        )?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(RemindersAction::Delete { id, title, list }) => {
                let target = reminder_target(id.as_deref(), title.as_deref());
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        "reminders.delete",
                        format!(
                            "Would delete reminder {} {}",
                            target.describe(),
                            via.label()
                        ),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let value = if via.is_cli() {
                        let id = bridge_cli::reminder_id(target, list.as_deref()).await?;
                        bridge_cli::reminders_delete(&id).await?
                    } else {
                        serde_json::to_value(
                            sources::reminders::delete(target, list.as_deref()).await?,
                        )?
                    };
                    print_write_output(&value, cli.pretty, cli.envelope, via)?;
                }
            }
            Some(RemindersAction::BatchComplete { ref ids })
            | Some(RemindersAction::BatchReopen { ref ids })
            | Some(RemindersAction::BatchDelete { ref ids }) => {
                let verb = match action {
                    Some(RemindersAction::BatchComplete { .. }) => "complete",
                    Some(RemindersAction::BatchReopen { .. }) => "reopen",
                    _ => "delete",
                };
                let via = WriteBackend::detect();
                if cli.no_op {
                    print_dry_run(
                        &format!("reminders.batch-{verb}"),
                        format!("Would {verb} {} reminders {}", ids.len(), via.label()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = if via.is_cli() {
                        bridge_cli::reminders_batch(verb, ids).await?
                    } else {
                        match verb {
                            "complete" => sources::reminders::batch_complete(ids).await?,
                            "reopen" => sources::reminders::batch_reopen(ids).await?,
                            _ => sources::reminders::batch_delete(ids).await?,
                        }
                    };
                    print_batch_output(&result, cli.pretty, cli.envelope)?;
                }
            }
            Some(RemindersAction::Lists) => {
                run_source!(sources::reminders::lists(), cli.pretty, cli.envelope)
            }
        },
        Commands::ScreenSharing { action } => match action {
            None | Some(ScreenSharingAction::Status) => {
                run_source!(sources::screen_sharing::status(), cli.pretty, cli.envelope)
            }
            Some(ScreenSharingAction::Enable) => {
                if cli.no_op {
                    print_dry_run(
                        "screen-sharing.enable",
                        "Would enable screen sharing",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::screen_sharing::enable().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ScreenSharingAction::Disable) => {
                if cli.no_op {
                    print_dry_run(
                        "screen-sharing.disable",
                        "Would disable screen sharing",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::screen_sharing::disable().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Screenshots { action } => match action {
            None => {
                let records = paginate_vec(sources::screenshots::list().await?, None, None);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(ScreenshotsAction::List { offset, limit }) => {
                let records = paginate_vec(sources::screenshots::list().await?, offset, limit);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(ScreenshotsAction::Capture {
                selection,
                window,
                path,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "screenshots.capture",
                        "Would capture a screenshot",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::screenshots::capture(selection, window, path.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Shortcuts { action } => match action {
            None => {
                let records = paginate_vec(sources::shortcuts::list().await?, None, None);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(ShortcutsAction::List { offset, limit }) => {
                let records = paginate_vec(sources::shortcuts::list().await?, offset, limit);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(ShortcutsAction::Run { name, input }) => {
                if cli.no_op {
                    print_dry_run(
                        "shortcuts.run",
                        format!("Would run shortcut '{name}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::shortcuts::run(&name, input.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ShortcutsAction::View { name }) => {
                if cli.no_op {
                    print_dry_run(
                        "shortcuts.view",
                        format!("Would open shortcut '{name}' in Shortcuts"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::shortcuts::view(&name).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ShortcutsAction::Export { name }) => {
                let result = sources::shortcuts::export(&name).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(ShortcutsAction::Gen { spec, output, sign }) => {
                let json = if spec == "-" {
                    let mut buf = String::new();
                    io::stdin().read_to_string(&mut buf)?;
                    buf
                } else {
                    std::fs::read_to_string(&spec)?
                };
                let spec = sources::shortcuts::parse_spec(&json)?;
                let output =
                    output.unwrap_or_else(|| sources::shortcuts::default_output(&spec.name));
                if cli.no_op {
                    print_dry_run(
                        "shortcuts.gen",
                        format!(
                            "Would generate {}shortcut '{}' ({} steps) at '{output}'",
                            if sign { "signed " } else { "" },
                            spec.name,
                            spec.steps.len()
                        ),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::shortcuts::gen(&spec, &output, sign).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ShortcutsAction::Install { input }) => {
                if cli.no_op {
                    print_dry_run(
                        "shortcuts.install",
                        format!("Would open '{input}' in Shortcuts to add it"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::shortcuts::install(&input).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ShortcutsAction::Sign {
                input,
                output,
                mode,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "shortcuts.sign",
                        format!("Would sign shortcut file '{input}' to '{output}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::shortcuts::sign(&input, &output, mode.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Spotlight { query, directory } => {
            run_source!(
                sources::spotlight::search(&query, directory.as_deref()),
                cli.pretty,
                cli.envelope
            )
        }
        Commands::Stocks { action } => match action {
            None | Some(StocksAction::List) => {
                run_source!(sources::stocks::fetch(), cli.pretty, cli.envelope)
            }
            Some(StocksAction::Watchlists) => {
                run_source!(sources::stocks::watchlists(), cli.pretty, cli.envelope)
            }
            Some(StocksAction::Quote { symbol }) => {
                let quotes = sources::stocks::quotes(std::slice::from_ref(&symbol)).await?;
                let quote = quotes
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No cached quote for {symbol}"))?;
                print_output(&serde_json::to_value(&quote)?, cli.pretty, cli.envelope)?;
            }
        },
        Commands::SystemInfo { action } => match action {
            None | Some(SystemInfoAction::Show) => {
                run_source!(sources::system_info::show(), cli.pretty, cli.envelope)
            }
            Some(SystemInfoAction::SetName { name }) => {
                if cli.no_op {
                    print_dry_run(
                        "system-info.set-name",
                        format!("Would set computer name to '{name}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::system_info::set_computer_name(&name).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(SystemInfoAction::DefaultsRead { domain, key }) => {
                let result = sources::system_info::defaults_read(&domain, key.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(SystemInfoAction::DefaultsWrite { domain, key, value }) => {
                if cli.no_op {
                    print_dry_run(
                        "system-info.defaults-write",
                        format!("Would write default {domain} {key}={value}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::system_info::defaults_write(&domain, &key, &value).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::Icloud { action } => match action {
            None | Some(IcloudAction::Account) => {
                run_source!(sources::icloud::account(), cli.pretty, cli.envelope)
            }
            Some(IcloudAction::Quota) => {
                run_source!(sources::icloud::quota(), cli.pretty, cli.envelope)
            }
            Some(IcloudAction::Status { container }) => {
                run_source!(
                    sources::icloud::status(container.as_deref()),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(IcloudAction::Log { minutes }) => {
                run_source!(sources::icloud::log(minutes), cli.pretty, cli.envelope)
            }
            Some(IcloudAction::List {
                folder,
                state,
                recursive,
            }) => {
                let state = match state.as_str() {
                    "local" => Some(sources::icloud::DriveState::Local),
                    "cloud" => Some(sources::icloud::DriveState::Cloud),
                    _ => None,
                };
                run_source!(
                    sources::icloud::list(folder.as_deref(), state, recursive),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(IcloudAction::Download { path }) => {
                if cli.no_op {
                    let target = sources::icloud::resolve_drive_path(
                        &sources::icloud::drive_root()?,
                        &path,
                    )?;
                    print_dry_run(
                        "icloud.download",
                        format!("Would download {} from iCloud", target.display()),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::icloud::download(&path).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(IcloudAction::Evict { path }) => {
                if cli.no_op {
                    let target = sources::icloud::resolve_drive_path(
                        &sources::icloud::drive_root()?,
                        &path,
                    )?;
                    print_dry_run(
                        "icloud.evict",
                        format!(
                            "Would remove the local copy of {} (it stays in iCloud)",
                            target.display()
                        ),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::icloud::evict(&path).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::TimeMachine { action } => match action {
            None | Some(TimeMachineAction::Status) => {
                run_source!(sources::time_machine::status(), cli.pretty, cli.envelope)
            }
            Some(TimeMachineAction::List) => {
                run_source!(sources::time_machine::list(), cli.pretty, cli.envelope)
            }
            Some(TimeMachineAction::Start) => {
                if cli.no_op {
                    print_dry_run(
                        "time-machine.start",
                        "Would start a Time Machine backup",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::time_machine::start().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(TimeMachineAction::Stop) => {
                if cli.no_op {
                    print_dry_run(
                        "time-machine.stop",
                        "Would stop the current Time Machine backup",
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::time_machine::stop().await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
        },
        Commands::VoiceMemos => {
            run_source!(sources::voice_memos::fetch(), cli.pretty, cli.envelope)
        }
        Commands::Weather {
            home,
            lat,
            lon,
            forecast,
            days,
        } => {
            let location = sources::weather::resolve_location(lat, lon, home.as_deref()).await?;
            let mut bridge = sources::bridge::Bridge::connect().await?;
            let value = if forecast || days.is_some() {
                sources::weather::forecast(&mut bridge, &location, days).await?
            } else {
                sources::weather::current(&mut bridge, &location).await?
            };
            print_output(&value, cli.pretty, cli.envelope)?;
        }
        Commands::Watch {
            source,
            debounce_ms,
            via,
        } => {
            let targets = watch_sources(&source)?;
            let via: sources::watch::Via = via.parse()?;
            let envelope = cli.envelope;
            let debounce = std::time::Duration::from_millis(debounce_ms);
            sources::watch::watch_via(&targets, debounce, via, |event| {
                // The callback cannot propagate an error; a closed stdout
                // (`cider watch | head`) is the normal way a stream ends.
                if let Err(err) = emit_watch_event(&event, envelope) {
                    if is_broken_pipe(&err) {
                        std::process::exit(0);
                    }
                    eprintln!("cider watch: {err}");
                    std::process::exit(1);
                }
            })
            .await?;
        }
        Commands::Wifi { action } => match action {
            None | Some(WifiAction::Status) => {
                run_source!(sources::wifi::status(), cli.pretty, cli.envelope)
            }
            Some(WifiAction::Networks) => {
                run_source!(sources::wifi::networks(), cli.pretty, cli.envelope)
            }
        },
        Commands::Schema { source } => {
            let schema = build_schema(source.as_deref());
            print_output(&schema, cli.pretty, cli.envelope)?;
        }
    }

    Ok(())
}

/// Resolve the `--id` / `--title` pair into a target. clap enforces exactly
/// one of them (`conflicts_with` + `required_unless_present`), so the fallback
/// is unreachable in practice.
fn reminder_target<'a>(
    id: Option<&'a str>,
    title: Option<&'a str>,
) -> sources::reminders::Target<'a> {
    match (id, title) {
        (Some(id), _) => sources::reminders::Target::Id(id),
        (None, Some(title)) => sources::reminders::Target::Title(title),
        (None, None) => sources::reminders::Target::Title(""),
    }
}

fn mail_target_description(id: Option<&str>, index: Option<usize>) -> String {
    match (id, index) {
        (Some(id), _) => format!("id '{id}'"),
        (_, Some(index)) => format!("#{index}"),
        _ => String::new(),
    }
}

/// Shell arguments are awkward carriers for long or multiline text, so a
/// literal "-" means "read the value from stdin" — the Unix convention.
fn stdin_if_dash(value: Option<String>) -> anyhow::Result<Option<String>> {
    match value {
        Some(v) if v == "-" => {
            let mut buf = String::new();
            io::Read::read_to_string(&mut io::stdin(), &mut buf)?;
            Ok(Some(buf.trim_end_matches('\n').to_string()))
        }
        other => Ok(other),
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|io_err| io_err.kind() == io::ErrorKind::BrokenPipe)
    })
}

fn classify_error_code(err: &anyhow::Error) -> String {
    if let Some(bridge) = err.downcast_ref::<sources::bridge::BridgeError>() {
        return bridge_error_code(bridge);
    }
    let msg = err.to_string().to_lowercase();
    if msg.contains("not configured") {
        "not_configured"
    } else if msg.contains("not found") {
        "not_found"
    } else if msg.contains("permission") || msg.contains("full disk access") {
        "permission_denied"
    } else if msg.contains("out of range") || msg.contains("invalid") || msg.contains("ambiguous") {
        "invalid_input"
    } else if msg.contains("timed out") {
        "timeout"
    } else {
        "operation_failed"
    }
    .to_string()
}

/// Bridge failures keep their own codes; the RFC's remote codes that have a
/// cider equivalent are normalized to it, the rest are prefixed.
fn bridge_error_code(error: &sources::bridge::BridgeError) -> String {
    use sources::bridge::BridgeError;
    match error {
        BridgeError::NotInstalled => "bridge_not_installed".to_string(),
        BridgeError::CliNotInstalled => "bridge_cli_not_installed".to_string(),
        BridgeError::Unreachable(_) => "bridge_unreachable".to_string(),
        BridgeError::Protocol(_) => "bridge_protocol_error".to_string(),
        BridgeError::Remote { code, .. } => match code.as_str() {
            "not_found" => "not_found".to_string(),
            "invalid_args" => "invalid_input".to_string(),
            "homekit_denied" | "permission_denied" => "permission_denied".to_string(),
            "timeout" => "timeout".to_string(),
            "weather_unavailable" => "weather_unavailable".to_string(),
            other => format!("bridge_{other}"),
        },
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match run().await {
        Ok(()) => Ok(()),
        Err(err) if is_broken_pipe(&err) => Ok(()),
        Err(err) => {
            let payload = serde_json::json!({
                "ok": false,
                "error": {
                    "code": classify_error_code(&err),
                    "message": err.to_string(),
                }
            });
            eprintln!("{}", payload);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_since_accepts_rfc3339_and_normalizes_to_utc() {
        assert!(parse_since(None).unwrap().is_none());
        let utc = parse_since(Some("2026-09-01T00:00:00Z")).unwrap().unwrap();
        assert_eq!(utc.to_rfc3339(), "2026-09-01T00:00:00+00:00");
        let offset = parse_since(Some(" 2026-09-01T02:30:00+02:30 "))
            .unwrap()
            .unwrap();
        assert_eq!(offset, utc, "offsets normalize to the same instant");
    }

    #[test]
    fn parse_since_rejects_non_rfc3339() {
        for bad in [
            "2026-09-01",
            "yesterday",
            "2026-09-01 00:00:00",
            "",
            "1756684800",
        ] {
            let error = parse_since(Some(bad)).expect_err(bad).to_string();
            assert!(
                error.starts_with("--since must be RFC 3339, e.g. 2026-09-01T00:00:00Z"),
                "{bad}: {error}"
            );
        }
    }

    #[test]
    fn schema_is_generated_for_every_top_level_command() {
        let schema = build_schema(None);
        let expected = Cli::command()
            .get_subcommands()
            .filter(|command| command.get_name() != "help")
            .count();
        assert_eq!(schema["schema_version"], 1);
        assert_eq!(schema["commands"].as_array().unwrap().len(), expected);
        // Floor is well below the real count so removing a dead source is
        // never blocked here, while a broken schema walk still fails loudly.
        assert!(expected >= 30, "commands must not disappear from schema");
    }

    #[test]
    fn schema_includes_deep_actions_arguments_and_id_contracts() {
        let calendar = build_schema(Some("calendar"));
        assert_eq!(calendar["identifiers"]["stable"], true);
        let actions = calendar["actions"].as_array().unwrap();
        let update = actions
            .iter()
            .find(|action| action["name"] == "update")
            .unwrap();
        assert_eq!(update["kind"], "write");
        assert!(update["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument["name"] == "id" && argument["required"] == true));

        let mail = build_schema(Some("mail"));
        assert_eq!(mail["identifiers"]["format"], "rfc_message_id");
        assert!(mail["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["name"] == "batch-trash"));
    }

    #[test]
    fn stable_targets_parse_and_legacy_targets_remain_compatible() {
        assert!(Cli::try_parse_from(["cider", "mail", "read", "--id", "<m@example.com>"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "mail", "read", "--index", "1"]).is_ok());
        assert!(Cli::try_parse_from([
            "cider",
            "calendar",
            "delete",
            "--title",
            "Meeting",
            "--date",
            "2026-09-01"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["cider", "calendar", "delete"]).is_err());
        assert!(Cli::try_parse_from(["cider", "contacts", "create", "--org", "Acme"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "contacts", "create"]).is_err());
        assert!(Cli::try_parse_from(["cider", "calendar", "update", "--id", "abc"]).is_err());
        assert!(Cli::try_parse_from(["cider", "contacts", "update", "--id", "abc"]).is_err());
        assert!(Cli::try_parse_from(["cider", "reminders", "update", "--id", "abc"]).is_err());
    }

    #[test]
    fn watch_accepts_known_sources_and_rejects_others() {
        use sources::watch::WatchSource;

        let cli = Cli::try_parse_from([
            "cider",
            "watch",
            "--source",
            "reminders",
            "--source",
            "notes",
            "--debounce-ms",
            "500",
        ])
        .unwrap();
        match cli.command {
            Commands::Watch {
                source,
                debounce_ms,
                via,
            } => {
                assert_eq!(
                    watch_sources(&source).unwrap(),
                    vec![WatchSource::Reminders, WatchSource::Notes]
                );
                assert_eq!(debounce_ms, 500);
                assert_eq!(via, "auto");
            }
            _ => panic!("expected watch"),
        }
        match Cli::try_parse_from(["cider", "watch", "--via", "fsevents"])
            .unwrap()
            .command
        {
            Commands::Watch {
                source,
                debounce_ms,
                via,
            } => {
                assert_eq!(watch_sources(&source).unwrap(), WatchSource::ALL.to_vec());
                assert_eq!(debounce_ms, 2000);
                assert_eq!(via, "fsevents");
            }
            _ => panic!("expected watch"),
        }
        assert!(Cli::try_parse_from(["cider", "watch", "--source", "contacts"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "watch", "--source", "mail"]).is_err());
        assert!(Cli::try_parse_from(["cider", "watch", "--via", "socket"]).is_err());
    }

    #[test]
    fn weather_takes_coordinates_together_or_a_home() {
        match Cli::try_parse_from(["cider", "weather", "--lat", "37.75", "--lon", "-122.49"])
            .unwrap()
            .command
        {
            Commands::Weather {
                lat,
                lon,
                forecast,
                days,
                home,
            } => {
                assert_eq!((lat, lon), (Some(37.75), Some(-122.49)));
                assert!(!forecast);
                assert!(days.is_none());
                assert!(home.is_none());
            }
            _ => panic!("expected weather"),
        }
        match Cli::try_parse_from(["cider", "weather", "--home", "Casa", "--days", "3"])
            .unwrap()
            .command
        {
            Commands::Weather { home, days, .. } => {
                assert_eq!(home.as_deref(), Some("Casa"));
                assert_eq!(days, Some(3));
            }
            _ => panic!("expected weather"),
        }
        assert!(Cli::try_parse_from(["cider", "weather"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "weather", "--forecast"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "weather", "--lat", "1"]).is_err());
        assert!(Cli::try_parse_from([
            "cider", "weather", "--home", "x", "--lat", "1", "--lon", "2"
        ])
        .is_err());
        let schema = build_schema(Some("weather"));
        assert_eq!(schema["name"], "weather");
    }

    #[test]
    fn envelope_preserves_inner_failure_status() {
        let wrapped = envelope_value(&serde_json::json!({
            "ok": false,
            "action": "batch-delete"
        }));
        assert_eq!(wrapped["ok"], false);
        assert_eq!(wrapped["data"]["action"], "batch-delete");
    }

    #[test]
    fn home_live_flag_is_accepted_before_or_after_the_subcommand() {
        for argv in [
            vec!["cider", "home", "--live", "homes"],
            vec!["cider", "home", "homes", "--live"],
        ] {
            match Cli::try_parse_from(argv).unwrap().command {
                Commands::Home { live, action } => {
                    assert!(live);
                    assert!(matches!(action, Some(HomeAction::Homes)));
                }
                _ => panic!("expected home"),
            }
        }
        match Cli::try_parse_from(["cider", "home"]).unwrap().command {
            Commands::Home { live, action } => {
                assert!(!live);
                assert!(action.is_none());
            }
            _ => panic!("expected home"),
        }
    }

    #[test]
    fn home_bridge_subcommands_parse() {
        let cli = Cli::try_parse_from([
            "cider",
            "home",
            "triggers",
            "create-timer",
            "--name",
            "Porch on",
            "--at",
            "2026-09-01T19:30:00-07:00",
            "--repeat",
            "daily",
            "--scene",
            "Porch On",
            "--scene",
            "Hall On",
        ])
        .unwrap();
        match cli.command {
            Commands::Home {
                action:
                    Some(HomeAction::Triggers {
                        action: Some(HomeTriggersAction::CreateTimer { scene, repeat, .. }),
                    }),
                ..
            } => {
                assert_eq!(scene, vec!["Porch On", "Hall On"]);
                assert_eq!(repeat.as_deref(), Some("daily"));
            }
            _ => panic!("expected triggers create-timer"),
        }
        // A timer needs at least one scene.
        assert!(Cli::try_parse_from([
            "cider",
            "home",
            "triggers",
            "create-timer",
            "--name",
            "x",
            "--at",
            "2026-09-01T19:30:00Z"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "cider",
            "home",
            "set",
            "--accessory",
            "Lamp",
            "--characteristic",
            "Power",
            "--value",
            "true"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["cider", "home", "run", "--scene", "Good Night"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "home", "state", "--room", "Office"]).is_ok());
        assert!(
            Cli::try_parse_from(["cider", "bridge", "build", "--team", "ABC", "--install"]).is_ok()
        );
        assert!(Cli::try_parse_from(["cider", "bridge"]).is_ok());
    }

    #[test]
    fn sourced_envelope_carries_the_backend() {
        use sources::home_live::Source;
        let value = serde_json::json!([{"id": "H1"}]);
        let mut out = Vec::new();
        let mut wrapped = envelope_value(&value);
        wrapped["source"] = serde_json::json!(Source::Bridge.as_str());
        serde_json::to_writer(&mut out, &wrapped).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["source"], "bridge");
        assert_eq!(parsed["data"], value);
        assert_eq!(Source::Cache.as_str(), "cache");
    }

    #[test]
    fn home_schema_classifies_bridge_actions() {
        let schema = build_schema(Some("home"));
        let kinds: std::collections::HashMap<&str, &str> = schema["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["name"].as_str().unwrap(), a["kind"].as_str().unwrap()))
            .collect();
        assert_eq!(kinds["state"], "read");
        assert_eq!(kinds["run"], "write");
        assert_eq!(kinds["set"], "write");
        assert!(schema["supports_dry_run"].as_bool().unwrap());
    }

    #[test]
    fn bridge_errors_get_their_own_codes() {
        use sources::bridge::BridgeError;
        let not_installed: anyhow::Error = BridgeError::NotInstalled.into();
        assert_eq!(classify_error_code(&not_installed), "bridge_not_installed");
        let denied: anyhow::Error = BridgeError::Remote {
            code: "homekit_denied".into(),
            message: "no".into(),
        }
        .into();
        assert_eq!(classify_error_code(&denied), "permission_denied");
        let unavailable: anyhow::Error = BridgeError::Remote {
            code: "homekit_unavailable".into(),
            message: "no".into(),
        }
        .into();
        assert_eq!(
            classify_error_code(&unavailable),
            "bridge_homekit_unavailable"
        );
        // Context added on the way up must not hide the typed error.
        let wrapped = anyhow::Error::from(BridgeError::Unreachable("x".into())).context("listing");
        assert_eq!(classify_error_code(&wrapped), "bridge_unreachable");
        let cli: anyhow::Error = BridgeError::CliNotInstalled.into();
        assert_eq!(classify_error_code(&cli), "bridge_cli_not_installed");
        let weather: anyhow::Error = BridgeError::Remote {
            code: "weather_unavailable".into(),
            message: "no".into(),
        }
        .into();
        assert_eq!(classify_error_code(&weather), "weather_unavailable");
        let tcc: anyhow::Error = BridgeError::Remote {
            code: "permission_denied".into(),
            message: "no".into(),
        }
        .into();
        assert_eq!(classify_error_code(&tcc), "permission_denied");
        assert_eq!(
            classify_error_code(&anyhow::anyhow!("thing not found")),
            "not_found"
        );
    }

    #[test]
    fn icloud_surface_parses_and_classifies_mutations() {
        assert!(Cli::try_parse_from(["cider", "icloud"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "icloud", "account"]).is_ok());
        assert!(Cli::try_parse_from(["cider", "icloud", "quota"]).is_ok());
        assert!(Cli::try_parse_from([
            "cider",
            "icloud",
            "status",
            "--container",
            "com.apple.CloudDocs"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["cider", "icloud", "log", "--minutes", "5"]).is_ok());
        assert!(Cli::try_parse_from([
            "cider",
            "icloud",
            "list",
            "--folder",
            "Documents",
            "--state",
            "cloud",
            "--recursive"
        ])
        .is_ok());
        assert!(
            Cli::try_parse_from(["cider", "icloud", "list", "--state", "remote"]).is_err(),
            "--state is local|cloud|all"
        );
        assert!(Cli::try_parse_from([
            "cider",
            "--dry-run",
            "icloud",
            "evict",
            "--path",
            "Documents/x.txt"
        ])
        .is_ok());
        assert!(
            Cli::try_parse_from(["cider", "icloud", "download"]).is_err(),
            "--path is required"
        );

        let schema = build_schema(Some("icloud"));
        let kinds: std::collections::HashMap<&str, &str> = schema["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["name"].as_str().unwrap(), a["kind"].as_str().unwrap()))
            .collect();
        assert_eq!(kinds["list"], "read");
        assert_eq!(kinds["quota"], "read");
        assert_eq!(kinds["download"], "write");
        assert_eq!(kinds["evict"], "write");
        assert!(schema["supports_dry_run"].as_bool().unwrap());

        assert_eq!(
            classify_error_code(&anyhow::anyhow!(
                "iCloud is not configured: nobody signed in"
            )),
            "not_configured"
        );
        assert_eq!(
            classify_error_code(&anyhow::anyhow!("invalid path: \"../x\" uses `..`")),
            "invalid_input"
        );
    }
}
