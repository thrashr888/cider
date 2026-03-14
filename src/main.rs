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
    #[command(subcommand)]
    command: Commands,

    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pretty: bool,
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
    List,
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
    List,
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
    List,
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
    /// List keychain items (metadata only, no passwords)
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

fn print_output(value: &serde_json::Value, human: bool) -> anyhow::Result<()> {
    let mut out = io::stdout().lock();
    if human {
        pretty::render(&mut out, value)
    } else {
        serde_json::to_writer(&mut out, value)?;
        out.write_all(b"\n")?;
        Ok(())
    }
}

macro_rules! run_source {
    ($source:expr, $pretty:expr) => {{
        let records = $source.await?;
        print_output(&serde_json::to_value(&records)?, $pretty)?;
    }};
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ActivityMonitor => run_source!(sources::activity_monitor::fetch(), cli.pretty),
        Commands::Apps => run_source!(sources::apps::fetch(), cli.pretty),
        Commands::Automator => run_source!(sources::automator::fetch(), cli.pretty),
        Commands::Bluetooth => run_source!(sources::bluetooth::list(), cli.pretty),
        Commands::Books => run_source!(sources::books::fetch(), cli.pretty),
        Commands::Calendar { action } => match action {
            None => {
                run_source!(sources::calendar::list(None, None, None), cli.pretty)
            }
            Some(CalendarAction::List {
                days_back,
                days_ahead,
                calendar,
            }) => {
                run_source!(
                    sources::calendar::list(days_back, days_ahead, calendar.as_deref()),
                    cli.pretty
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
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(CalendarAction::Delete {
                title,
                date,
                calendar,
            }) => {
                let result = sources::calendar::delete(&title, &date, calendar.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(CalendarAction::Calendars) => {
                run_source!(sources::calendar::calendars(), cli.pretty)
            }
        },
        Commands::Clock => run_source!(sources::clock::fetch(), cli.pretty),
        Commands::Console { minutes } => {
            run_source!(sources::console_logs::fetch(minutes), cli.pretty)
        }
        Commands::Contacts { action } => match action {
            None => {
                run_source!(sources::contacts::list(None), cli.pretty)
            }
            Some(ContactsAction::List { search }) => {
                run_source!(sources::contacts::list(search.as_deref()), cli.pretty)
            }
            Some(ContactsAction::Get { id }) => {
                let result = sources::contacts::get(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ContactsAction::Create {
                first,
                last,
                email,
                phone,
                org,
            }) => {
                let result = sources::contacts::create(
                    &first,
                    &last,
                    email.as_deref(),
                    phone.as_deref(),
                    org.as_deref(),
                )
                .await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ContactsAction::Update {
                id,
                first,
                last,
                email,
                phone,
            }) => {
                let result = sources::contacts::update(
                    &id,
                    first.as_deref(),
                    last.as_deref(),
                    email.as_deref(),
                    phone.as_deref(),
                )
                .await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ContactsAction::Delete { id }) => {
                let result = sources::contacts::delete(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ContactsAction::Groups) => {
                run_source!(sources::contacts::groups(), cli.pretty)
            }
        },
        Commands::Disks => run_source!(sources::disks::list(), cli.pretty),
        Commands::FindMy => run_source!(sources::find_my::fetch(), cli.pretty),
        Commands::Fonts => run_source!(sources::fonts::fetch(), cli.pretty),
        Commands::Home => run_source!(sources::home::fetch(), cli.pretty),
        Commands::Journal => run_source!(sources::journal::fetch(), cli.pretty),
        Commands::Keychain { action } => match action {
            None | Some(KeychainAction::List { kind: None }) => {
                run_source!(sources::keychain::list(None), cli.pretty)
            }
            Some(KeychainAction::List { kind }) => {
                run_source!(sources::keychain::list(kind.as_deref()), cli.pretty)
            }
            Some(KeychainAction::Search { query, kind }) => {
                run_source!(
                    sources::keychain::search(&query, kind.as_deref()),
                    cli.pretty
                )
            }
            Some(KeychainAction::GetPassword { service, account }) => {
                let pw = sources::keychain::get_password(&service, account.as_deref()).await?;
                print_output(&serde_json::to_value(&pw)?, cli.pretty)?;
            }
            Some(KeychainAction::GetInternetPassword { server, account }) => {
                let pw =
                    sources::keychain::get_internet_password(&server, account.as_deref()).await?;
                print_output(&serde_json::to_value(&pw)?, cli.pretty)?;
            }
            Some(KeychainAction::Add {
                service,
                account,
                password,
                label,
            }) => {
                let result =
                    sources::keychain::add(&service, &account, &password, label.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(KeychainAction::Delete { service, account }) => {
                let result = sources::keychain::delete(&service, account.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(KeychainAction::Keychains) => {
                run_source!(sources::keychain::keychains(), cli.pretty)
            }
        },
        Commands::Mail { action } => match action {
            None | Some(MailAction::List) => {
                run_source!(sources::mail::list(), cli.pretty)
            }
            Some(MailAction::Get { index }) => {
                let result = sources::mail::get(index).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MailAction::Read { index }) => {
                let result = sources::mail::read(index).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MailAction::Unread { index }) => {
                let result = sources::mail::unread(index).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MailAction::Trash { index }) => {
                let result = sources::mail::trash(index).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MailAction::Mailboxes) => {
                run_source!(sources::mail::mailboxes(), cli.pretty)
            }
            Some(MailAction::Send { to, subject, body }) => {
                let result = sources::mail::send(&to, &subject, &body).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::Maps => run_source!(sources::maps::fetch(), cli.pretty),
        Commands::Messages { action } => match action {
            None => {
                run_source!(sources::messages::list(30), cli.pretty)
            }
            Some(MessagesAction::List { days }) => {
                run_source!(sources::messages::list(days), cli.pretty)
            }
            Some(MessagesAction::Send { to, text }) => {
                let result = sources::messages::send(&to, &text).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::Music { action } => match action {
            None | Some(MusicAction::List) => {
                run_source!(sources::music::list(), cli.pretty)
            }
            Some(MusicAction::Play { track, playlist }) => {
                let result = sources::music::play(track.as_deref(), playlist.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MusicAction::Pause) => {
                let result = sources::music::pause().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MusicAction::Next) => {
                let result = sources::music::next().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MusicAction::Previous) => {
                let result = sources::music::previous().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MusicAction::Status) => {
                let result = sources::music::status().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(MusicAction::Playlists) => {
                run_source!(sources::music::playlists(), cli.pretty)
            }
        },
        Commands::News => run_source!(sources::news::fetch(), cli.pretty),
        Commands::Notes { action } => match action {
            None => {
                run_source!(sources::notes::list(None), cli.pretty)
            }
            Some(NotesAction::List { folder }) => {
                run_source!(sources::notes::list(folder.as_deref()), cli.pretty)
            }
            Some(NotesAction::Get { id }) => {
                let result = sources::notes::get(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(NotesAction::Create {
                title,
                body,
                folder,
            }) => {
                let result =
                    sources::notes::create(&title, body.as_deref(), folder.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(NotesAction::Update { id, body }) => {
                let result = sources::notes::update(&id, &body).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(NotesAction::Delete { id }) => {
                let result = sources::notes::delete(&id).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(NotesAction::Folders) => {
                run_source!(sources::notes::folders(), cli.pretty)
            }
        },
        Commands::PhotoBooth => run_source!(sources::photo_booth::fetch(), cli.pretty),
        Commands::Photos => run_source!(sources::photos::fetch(), cli.pretty),
        Commands::Safari { action } => match action {
            None | Some(SafariAction::Bookmarks) => {
                run_source!(sources::safari::bookmarks(), cli.pretty)
            }
            Some(SafariAction::History { limit }) => {
                run_source!(sources::safari::history(Some(limit)), cli.pretty)
            }
            Some(SafariAction::Tabs) => {
                run_source!(sources::safari::tabs(), cli.pretty)
            }
            Some(SafariAction::ReadingList) => {
                run_source!(sources::reading_list::fetch(), cli.pretty)
            }
        },
        Commands::Reminders { action } => match action {
            None => {
                run_source!(sources::reminders::list(None), cli.pretty)
            }
            Some(RemindersAction::List { list }) => {
                run_source!(sources::reminders::list(list.as_deref()), cli.pretty)
            }
            Some(RemindersAction::Create {
                title,
                list,
                due,
                priority,
                notes,
            }) => {
                let result = sources::reminders::create(
                    &title,
                    list.as_deref(),
                    due.as_deref(),
                    priority,
                    notes.as_deref(),
                )
                .await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(RemindersAction::Complete { title, list }) => {
                let result = sources::reminders::complete(&title, list.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(RemindersAction::Delete { title, list }) => {
                let result = sources::reminders::delete(&title, list.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(RemindersAction::Lists) => {
                run_source!(sources::reminders::lists(), cli.pretty)
            }
        },
        Commands::ScreenSharing { action } => match action {
            None | Some(ScreenSharingAction::Status) => {
                run_source!(sources::screen_sharing::status(), cli.pretty)
            }
            Some(ScreenSharingAction::Enable) => {
                let result = sources::screen_sharing::enable().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ScreenSharingAction::Disable) => {
                let result = sources::screen_sharing::disable().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::Screenshots { action } => match action {
            None | Some(ScreenshotsAction::List) => {
                run_source!(sources::screenshots::list(), cli.pretty)
            }
            Some(ScreenshotsAction::Capture {
                selection,
                window,
                path,
            }) => {
                let result =
                    sources::screenshots::capture(selection, window, path.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::Shortcuts { action } => match action {
            None | Some(ShortcutsAction::List) => {
                run_source!(sources::shortcuts::list(), cli.pretty)
            }
            Some(ShortcutsAction::Run { name, input }) => {
                let result = sources::shortcuts::run(&name, input.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ShortcutsAction::View { name }) => {
                let result = sources::shortcuts::view(&name).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(ShortcutsAction::Sign {
                input,
                output,
                mode,
            }) => {
                let result = sources::shortcuts::sign(&input, &output, mode.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::Spotlight { query, directory } => {
            run_source!(
                sources::spotlight::search(&query, directory.as_deref()),
                cli.pretty
            )
        }
        Commands::Stickies => run_source!(sources::stickies::fetch(), cli.pretty),
        Commands::Stocks => run_source!(sources::stocks::fetch(), cli.pretty),
        Commands::SystemInfo { action } => match action {
            None | Some(SystemInfoAction::Show) => {
                run_source!(sources::system_info::show(), cli.pretty)
            }
            Some(SystemInfoAction::SetName { name }) => {
                let result = sources::system_info::set_computer_name(&name).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(SystemInfoAction::DefaultsRead { domain, key }) => {
                let result = sources::system_info::defaults_read(&domain, key.as_deref()).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(SystemInfoAction::DefaultsWrite { domain, key, value }) => {
                let result = sources::system_info::defaults_write(&domain, &key, &value).await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::TimeMachine { action } => match action {
            None | Some(TimeMachineAction::Status) => {
                run_source!(sources::time_machine::status(), cli.pretty)
            }
            Some(TimeMachineAction::List) => {
                run_source!(sources::time_machine::list(), cli.pretty)
            }
            Some(TimeMachineAction::Start) => {
                let result = sources::time_machine::start().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
            Some(TimeMachineAction::Stop) => {
                let result = sources::time_machine::stop().await?;
                print_output(&serde_json::to_value(&result)?, cli.pretty)?;
            }
        },
        Commands::VoiceMemos => run_source!(sources::voice_memos::fetch(), cli.pretty),
        Commands::Weather => run_source!(sources::weather::fetch(), cli.pretty),
        Commands::Wifi { action } => match action {
            None | Some(WifiAction::Status) => {
                run_source!(sources::wifi::status(), cli.pretty)
            }
            Some(WifiAction::Networks) => {
                run_source!(sources::wifi::networks(), cli.pretty)
            }
        },
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match run().await {
        Ok(()) => Ok(()),
        Err(err) if is_broken_pipe(&err) => Ok(()),
        Err(err) => Err(err),
    }
}
