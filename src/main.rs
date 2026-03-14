use clap::{Parser, Subcommand};

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
    /// Fetch books from Apple Books
    Books,
    /// Fetch calendar events (past 7 days + next 30 days)
    Calendar,
    /// Show local time, world clocks, and alarms (Clock)
    Clock,
    /// Show recent system log entries (Console)
    Console {
        /// Minutes of logs to show
        #[arg(long, default_value = "30")]
        minutes: u32,
    },
    /// Fetch contacts from Apple Contacts
    Contacts,
    /// Fetch devices from Find My
    #[command(name = "find-my")]
    FindMy,
    /// List installed fonts (Font Book)
    Fonts,
    /// List HomeKit accessories (Home)
    Home,
    /// Fetch journal entries
    Journal,
    /// Fetch recent mail from inbox
    Mail,
    /// Fetch bookmarked places from Maps
    Maps,
    /// Fetch recent messages from iMessage/SMS
    Messages {
        /// Number of days to look back
        #[arg(long, default_value = "30")]
        days: u32,
    },
    /// Fetch tracks from Music library
    Music,
    /// Fetch saved articles from Apple News
    News,
    /// Fetch notes from Apple Notes
    Notes,
    /// List photos/videos from Photo Booth
    #[command(name = "photo-booth")]
    PhotoBooth,
    /// Fetch recent photos metadata from Photos
    Photos,
    /// Fetch items from Safari Reading List
    #[command(name = "reading-list")]
    ReadingList,
    /// Fetch reminders from Apple Reminders
    Reminders,
    /// Show screen sharing status
    #[command(name = "screen-sharing")]
    ScreenSharing,
    /// List recent screenshots
    Screenshots,
    /// List Siri Shortcuts
    Shortcuts,
    /// Fetch sticky notes from Stickies
    Stickies,
    /// Fetch stock watchlist from Stocks
    Stocks,
    /// Show system information (System Settings)
    #[command(name = "system-info")]
    SystemInfo,
    /// Show Time Machine backup info
    #[command(name = "time-machine")]
    TimeMachine,
    /// Fetch voice memos
    #[command(name = "voice-memos")]
    VoiceMemos,
    /// Fetch weather data
    Weather,
}

fn print_json(value: &serde_json::Value, pretty: bool) {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        println!("{}", serde_json::to_string(value).unwrap());
    }
}

macro_rules! run_source {
    ($source:expr, $pretty:expr) => {{
        let records = $source.await?;
        print_json(&serde_json::to_value(&records)?, $pretty);
    }};
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ActivityMonitor => run_source!(sources::activity_monitor::fetch(), cli.pretty),
        Commands::Apps => run_source!(sources::apps::fetch(), cli.pretty),
        Commands::Automator => run_source!(sources::automator::fetch(), cli.pretty),
        Commands::Books => run_source!(sources::books::fetch(), cli.pretty),
        Commands::Calendar => run_source!(sources::calendar::fetch(), cli.pretty),
        Commands::Clock => run_source!(sources::clock::fetch(), cli.pretty),
        Commands::Console { minutes } => {
            run_source!(sources::console_logs::fetch(minutes), cli.pretty)
        }
        Commands::Contacts => run_source!(sources::contacts::fetch(), cli.pretty),
        Commands::FindMy => run_source!(sources::find_my::fetch(), cli.pretty),
        Commands::Fonts => run_source!(sources::fonts::fetch(), cli.pretty),
        Commands::Home => run_source!(sources::home::fetch(), cli.pretty),
        Commands::Journal => run_source!(sources::journal::fetch(), cli.pretty),
        Commands::Mail => run_source!(sources::mail::fetch(), cli.pretty),
        Commands::Maps => run_source!(sources::maps::fetch(), cli.pretty),
        Commands::Messages { days } => run_source!(sources::messages::fetch(days), cli.pretty),
        Commands::Music => run_source!(sources::music::fetch(), cli.pretty),
        Commands::News => run_source!(sources::news::fetch(), cli.pretty),
        Commands::Notes => run_source!(sources::notes::fetch(), cli.pretty),
        Commands::PhotoBooth => run_source!(sources::photo_booth::fetch(), cli.pretty),
        Commands::Photos => run_source!(sources::photos::fetch(), cli.pretty),
        Commands::ReadingList => run_source!(sources::reading_list::fetch(), cli.pretty),
        Commands::Reminders => run_source!(sources::reminders::fetch(), cli.pretty),
        Commands::ScreenSharing => run_source!(sources::screen_sharing::fetch(), cli.pretty),
        Commands::Screenshots => run_source!(sources::screenshots::fetch(), cli.pretty),
        Commands::Shortcuts => run_source!(sources::shortcuts::fetch(), cli.pretty),
        Commands::Stickies => run_source!(sources::stickies::fetch(), cli.pretty),
        Commands::Stocks => run_source!(sources::stocks::fetch(), cli.pretty),
        Commands::SystemInfo => run_source!(sources::system_info::fetch(), cli.pretty),
        Commands::TimeMachine => run_source!(sources::time_machine::fetch(), cli.pretty),
        Commands::VoiceMemos => run_source!(sources::voice_memos::fetch(), cli.pretty),
        Commands::Weather => run_source!(sources::weather::fetch(), cli.pretty),
    }

    Ok(())
}
