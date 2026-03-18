use std::io::{self, Write};

use clap::{Parser, Subcommand};

mod pretty;
mod sources;

#[derive(Parser)]
#[command(
    name = "cider",
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
    /// List Automator workflows
    Automator,
    /// List paired Bluetooth devices
    Bluetooth,
    /// Fetch books from Apple Books
    Books,
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
    /// Recent call history (FaceTime + Phone)
    #[command(name = "facetime")]
    FaceTime {
        #[command(subcommand)]
        action: Option<FaceTimeAction>,
    },
    /// Fetch devices from Find My
    #[command(name = "find-my")]
    FindMy,
    /// List installed fonts (Font Book)
    Fonts,
    /// List HomeKit accessories (Home)
    Home,
    /// Fetch journal entries
    Journal,
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
    /// Fetch bookmarked places from Maps
    Maps,
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
    /// Fetch saved articles from Apple News
    News,
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
    /// Fetch sticky notes from Stickies
    Stickies,
    /// Search files with Spotlight
    Spotlight {
        /// Search query
        #[arg(long)]
        query: String,
        /// Limit search to directory
        #[arg(long)]
        directory: Option<String>,
    },
    /// Fetch stock watchlist from Stocks
    Stocks,
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
    /// Fetch weather data
    Weather,
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
    /// Delete a calendar event by title and date
    Delete {
        /// Event title to delete
        #[arg(long)]
        title: String,
        /// Date of the event (ISO 8601 date)
        #[arg(long)]
        date: String,
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
        #[arg(long)]
        first: String,
        /// Last name
        #[arg(long)]
        last: String,
        /// Email address
        #[arg(long)]
        email: Option<String>,
        /// Phone number
        #[arg(long)]
        phone: Option<String>,
        /// Organization
        #[arg(long)]
        org: Option<String>,
    },
    /// Update an existing contact
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
enum NotesAction {
    /// List notes (default)
    List {
        /// Filter by folder name
        #[arg(long)]
        folder: Option<String>,
        /// Skip the first N results
        #[arg(long)]
        offset: Option<usize>,
        /// Limit the number of results returned
        #[arg(long)]
        limit: Option<usize>,
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
        /// Notes
        #[arg(long)]
        notes: Option<String>,
    },
    /// Mark a reminder as complete
    Complete {
        /// Reminder title to complete
        #[arg(long)]
        title: String,
        /// List to search in
        #[arg(long)]
        list: Option<String>,
    },
    /// Delete a reminder
    Delete {
        /// Reminder title to delete
        #[arg(long)]
        title: String,
        /// List to search in
        #[arg(long)]
        list: Option<String>,
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
    },
    /// Get a single message by index (1-based)
    Get {
        /// Message index (1-based from list output)
        #[arg(long)]
        index: usize,
    },
    /// Mark a message as read
    Read {
        /// Message index (1-based)
        #[arg(long)]
        index: usize,
    },
    /// Mark a message as unread
    Unread {
        /// Message index (1-based)
        #[arg(long)]
        index: usize,
    },
    /// Move a message to trash
    Trash {
        /// Message index (1-based)
        #[arg(long)]
        index: usize,
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
        wrapped = serde_json::json!({"ok": true, "data": value});
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

macro_rules! run_source {
    ($source:expr, $pretty:expr, $envelope:expr) => {{
        let records = $source.await?;
        print_output(&serde_json::to_value(&records)?, $pretty, $envelope)?;
    }};
}

fn build_schema(source: Option<&str>) -> serde_json::Value {
    let commands = vec![
        serde_json::json!({"source":"schema","supports_dry_run":false,"capabilities":["schema"]}),
        serde_json::json!({"source":"contacts","supports_dry_run":true,"list_args":["search","offset","limit"],"stable_ids":true}),
        serde_json::json!({"source":"mail","supports_dry_run":true,"list_args":["offset","limit"],"stable_ids":true,"friendly_mailboxes":true}),
        serde_json::json!({"source":"messages","supports_dry_run":true,"list_args":["days","offset","limit"],"stable_ids":true}),
        serde_json::json!({"source":"notes","supports_dry_run":true,"list_args":["folder","offset","limit"],"stable_ids":true}),
        serde_json::json!({"source":"reminders","supports_dry_run":true,"list_args":["list","offset","limit"],"stable_ids":false}),
        serde_json::json!({"source":"shortcuts","supports_dry_run":true,"list_args":["offset","limit"],"stable_ids":false}),
        serde_json::json!({"source":"screenshots","supports_dry_run":true,"list_args":["offset","limit"],"stable_ids":false}),
    ];

    match source {
        Some(source) => commands
            .into_iter()
            .find(|item| item.get("source").and_then(|s| s.as_str()) == Some(source))
            .unwrap_or_else(|| serde_json::json!({"source":source,"error":"unknown_source"})),
        None => serde_json::json!({"commands":commands}),
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ActivityMonitor => {
            run_source!(sources::activity_monitor::fetch(), cli.pretty, cli.envelope)
        }
        Commands::Apps => run_source!(sources::apps::fetch(), cli.pretty, cli.envelope),
        Commands::Automator => run_source!(sources::automator::fetch(), cli.pretty, cli.envelope),
        Commands::Bluetooth => run_source!(sources::bluetooth::list(), cli.pretty, cli.envelope),
        Commands::Books => run_source!(sources::books::fetch(), cli.pretty, cli.envelope),
        Commands::Calendar { action } => match action {
            None => {
                run_source!(
                    sources::calendar::list(None, None, None),
                    cli.pretty,
                    cli.envelope
                )
            }
            Some(CalendarAction::List {
                days_back,
                days_ahead,
                calendar,
            }) => {
                run_source!(
                    sources::calendar::list(days_back, days_ahead, calendar.as_deref()),
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
                if cli.no_op {
                    print_dry_run(
                        "calendar.create",
                        format!("Would create event '{title}' starting {start} ending {end}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::calendar::create(
                        &title,
                        &start,
                        &end,
                        calendar.as_deref(),
                        location.as_deref(),
                        notes.as_deref(),
                        all_day,
                    )
                    .await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(CalendarAction::Delete {
                title,
                date,
                calendar,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "calendar.delete",
                        format!("Would delete event '{title}' on {date}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result =
                        sources::calendar::delete(&title, &date, calendar.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
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
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "contacts.create",
                        format!("Would create contact '{} {}'", first, last),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::contacts::create(
                        &first,
                        &last,
                        email.as_deref(),
                        phone.as_deref(),
                        org.as_deref(),
                    )
                    .await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(ContactsAction::Update {
                id,
                first,
                last,
                email,
                phone,
            }) => {
                if cli.no_op {
                    print_dry_run(
                        "contacts.update",
                        format!("Would update contact '{id}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::contacts::update(
                        &id,
                        first.as_deref(),
                        last.as_deref(),
                        email.as_deref(),
                        phone.as_deref(),
                    )
                    .await?;
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
        Commands::FaceTime { action } => match action {
            None => {
                run_source!(sources::facetime::list(50), cli.pretty, cli.envelope)
            }
            Some(FaceTimeAction::List { limit }) => {
                run_source!(sources::facetime::list(limit), cli.pretty, cli.envelope)
            }
        },
        Commands::FindMy => run_source!(sources::find_my::fetch(), cli.pretty, cli.envelope),
        Commands::Fonts => run_source!(sources::fonts::fetch(), cli.pretty, cli.envelope),
        Commands::Home => run_source!(sources::home::fetch(), cli.pretty, cli.envelope),
        Commands::Journal => run_source!(sources::journal::fetch(), cli.pretty, cli.envelope),
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
            Some(MailAction::List { offset, limit }) => {
                let records = paginate_vec(sources::mail::list().await?, offset, limit);
                print_output(&serde_json::to_value(&records)?, cli.pretty, cli.envelope)?;
            }
            Some(MailAction::Get { index }) => {
                let result = sources::mail::get(index).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
            }
            Some(MailAction::Read { index }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.read",
                        format!("Would mark inbox message #{index} as read"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::read(index).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::Unread { index }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.unread",
                        format!("Would mark inbox message #{index} as unread"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::unread(index).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(MailAction::Trash { index }) => {
                if cli.no_op {
                    print_dry_run(
                        "mail.trash",
                        format!("Would trash inbox message #{index}"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::mail::trash(index).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
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
        Commands::Maps => run_source!(sources::maps::fetch(), cli.pretty, cli.envelope),
        Commands::Messages { action } => match action {
            None => {
                run_source!(sources::messages::list(30), cli.pretty, cli.envelope)
            }
            Some(MessagesAction::List {
                days,
                offset,
                limit,
            }) => {
                let records = paginate_vec(sources::messages::list(days).await?, offset, limit);
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
        Commands::News => run_source!(sources::news::fetch(), cli.pretty, cli.envelope),
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
                run_source!(sources::notes::list(None), cli.pretty, cli.envelope)
            }
            Some(NotesAction::List {
                folder,
                offset,
                limit,
            }) => {
                let records = paginate_vec(
                    sources::notes::list(folder.as_deref()).await?,
                    offset,
                    limit,
                );
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
            }) => {
                let records = paginate_vec(
                    sources::reminders::list(list.as_deref()).await?,
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
                if cli.no_op {
                    print_dry_run(
                        "reminders.create",
                        format!("Would create reminder '{title}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::reminders::create(
                        &title,
                        list.as_deref(),
                        due.as_deref(),
                        priority,
                        notes.as_deref(),
                    )
                    .await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(RemindersAction::Complete { title, list }) => {
                if cli.no_op {
                    print_dry_run(
                        "reminders.complete",
                        format!("Would complete reminder '{title}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::reminders::complete(&title, list.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
                }
            }
            Some(RemindersAction::Delete { title, list }) => {
                if cli.no_op {
                    print_dry_run(
                        "reminders.delete",
                        format!("Would delete reminder '{title}'"),
                        cli.pretty,
                        cli.envelope,
                    )?;
                } else {
                    let result = sources::reminders::delete(&title, list.as_deref()).await?;
                    print_output(&serde_json::to_value(&result)?, cli.pretty, cli.envelope)?;
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
        Commands::Stickies => run_source!(sources::stickies::fetch(), cli.pretty, cli.envelope),
        Commands::Stocks => run_source!(sources::stocks::fetch(), cli.pretty, cli.envelope),
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
        Commands::Weather => run_source!(sources::weather::fetch(), cli.pretty, cli.envelope),
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

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|io_err| io_err.kind() == io::ErrorKind::BrokenPipe)
    })
}

fn classify_error_code(err: &anyhow::Error) -> &'static str {
    let msg = err.to_string().to_lowercase();
    if msg.contains("not found") {
        "not_found"
    } else if msg.contains("permission") || msg.contains("full disk access") {
        "permission_denied"
    } else if msg.contains("out of range") || msg.contains("invalid") {
        "invalid_input"
    } else if msg.contains("timed out") {
        "timeout"
    } else {
        "operation_failed"
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
