use clap::{Parser, Subcommand};

mod sources;

#[derive(Parser)]
#[command(name = "cider", about = "Read Apple app data from the command line")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pretty: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch reminders from Apple Reminders
    Reminders,
    /// Fetch notes from Apple Notes
    Notes,
    /// Fetch contacts from Apple Contacts
    Contacts,
    /// Fetch recent messages from iMessage/SMS
    Messages {
        /// Number of days to look back
        #[arg(long, default_value = "30")]
        days: u32,
    },
    /// Fetch books from Apple Books
    Books,
    /// Fetch items from Safari Reading List
    #[command(name = "reading-list")]
    ReadingList,
    /// Fetch everything from all sources
    All {
        /// Number of days for messages lookback
        #[arg(long, default_value = "30")]
        days: u32,
    },
}

fn print_json(value: &serde_json::Value, pretty: bool) {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        println!("{}", serde_json::to_string(value).unwrap());
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Reminders => {
            let records = sources::reminders::fetch().await?;
            print_json(&serde_json::to_value(&records)?, cli.pretty);
        }
        Commands::Notes => {
            let records = sources::notes::fetch().await?;
            print_json(&serde_json::to_value(&records)?, cli.pretty);
        }
        Commands::Contacts => {
            let records = sources::contacts::fetch().await?;
            print_json(&serde_json::to_value(&records)?, cli.pretty);
        }
        Commands::Messages { days } => {
            let records = sources::messages::fetch(days).await?;
            print_json(&serde_json::to_value(&records)?, cli.pretty);
        }
        Commands::Books => {
            let records = sources::books::fetch().await?;
            print_json(&serde_json::to_value(&records)?, cli.pretty);
        }
        Commands::ReadingList => {
            let records = sources::reading_list::fetch().await?;
            print_json(&serde_json::to_value(&records)?, cli.pretty);
        }
        Commands::All { days } => {
            let mut all = serde_json::Map::new();

            match sources::reminders::fetch().await {
                Ok(r) => {
                    all.insert("reminders".into(), serde_json::to_value(&r)?);
                }
                Err(e) => {
                    eprintln!("reminders: {e}");
                    all.insert("reminders".into(), serde_json::json!([]));
                }
            }

            match sources::notes::fetch().await {
                Ok(r) => {
                    all.insert("notes".into(), serde_json::to_value(&r)?);
                }
                Err(e) => {
                    eprintln!("notes: {e}");
                    all.insert("notes".into(), serde_json::json!([]));
                }
            }

            match sources::contacts::fetch().await {
                Ok(r) => {
                    all.insert("contacts".into(), serde_json::to_value(&r)?);
                }
                Err(e) => {
                    eprintln!("contacts: {e}");
                    all.insert("contacts".into(), serde_json::json!([]));
                }
            }

            match sources::messages::fetch(days).await {
                Ok(r) => {
                    all.insert("messages".into(), serde_json::to_value(&r)?);
                }
                Err(e) => {
                    eprintln!("messages: {e}");
                    all.insert("messages".into(), serde_json::json!([]));
                }
            }

            match sources::books::fetch().await {
                Ok(r) => {
                    all.insert("books".into(), serde_json::to_value(&r)?);
                }
                Err(e) => {
                    eprintln!("books: {e}");
                    all.insert("books".into(), serde_json::json!([]));
                }
            }

            match sources::reading_list::fetch().await {
                Ok(r) => {
                    all.insert("reading_list".into(), serde_json::to_value(&r)?);
                }
                Err(e) => {
                    eprintln!("reading-list: {e}");
                    all.insert("reading_list".into(), serde_json::json!([]));
                }
            }

            print_json(&serde_json::Value::Object(all), cli.pretty);
        }
    }

    Ok(())
}
