pub mod activity_monitor;
pub mod apps;
pub mod automator;
pub mod bluetooth;
pub mod books;
pub mod calendar;
pub mod clock;
pub mod console_logs;
pub mod contacts;
pub mod disks;
pub mod facetime;
pub mod find_my;
pub mod fonts;
pub mod home;
pub mod journal;
pub mod keychain;
pub mod mail;
pub mod maps;
pub mod messages;
pub mod music;
pub mod news;
pub mod notes;
pub mod passwords;
pub mod photo_booth;
pub mod photos;
pub mod reading_list;
pub mod reminders;
pub mod safari;
pub mod screen_sharing;
pub mod screenshots;
pub mod shortcuts;
pub mod spotlight;
pub mod stickies;
pub mod stocks;
pub mod system_info;
pub mod time_machine;
mod util;

/// The result every mutating call returns (`create`, `complete`, `update`, …).
/// Re-exported because those signatures name it: the rest of `util` is
/// AppleScript/subprocess plumbing that callers have no business reaching.
pub use util::ActionResult;
pub mod voice_memos;
pub mod weather;
pub mod wifi;
