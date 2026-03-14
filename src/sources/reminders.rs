use super::util::{run_command_with_timeout, slug, truncate_for_title, APPLE_EPOCH};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub list: String,
    pub priority: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<chrono::Utc>>,
}

pub async fn fetch() -> anyhow::Result<Vec<Reminder>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let stores_dir =
        format!("{home}/Library/Group Containers/group.com.apple.reminders/Container_v1/Stores");

    let listing =
        run_command_with_timeout("ls", &[&stores_dir], std::time::Duration::from_secs(5)).await?;

    let db_files: Vec<String> = listing
        .lines()
        .filter(|l| l.starts_with("Data-") && l.ends_with(".sqlite"))
        .map(|l| format!("{stores_dir}/{l}"))
        .collect();

    if db_files.is_empty() {
        anyhow::bail!("No Reminders database found");
    }

    let mut all = Vec::new();

    for db_path in &db_files {
        let query = r#"
SELECT
    COALESCE(r.ZEXTERNALIDENTIFIER, r.ZCKIDENTIFIER, CAST(r.Z_PK AS TEXT)),
    COALESCE(l.ZNAME, ''),
    COALESCE(r.ZTITLE, ''),
    COALESCE(r.ZPRIORITY, 0),
    r.ZDUEDATE,
    COALESCE(r.ZFLAGGED, 0)
FROM ZREMCDREMINDER r
LEFT JOIN ZREMCDBASELIST l ON r.ZLIST = l.Z_PK
WHERE r.ZCOMPLETED = 0
ORDER BY r.ZDUEDATE ASC
LIMIT 500;
"#;

        match run_command_with_timeout(
            "sqlite3",
            &["-separator", "\t", db_path, query.trim()],
            std::time::Duration::from_secs(10),
        )
        .await
        {
            Ok(stdout) => all.extend(parse_output(&stdout)),
            Err(e) => eprintln!("Skipping reminders DB {db_path}: {e}"),
        }
    }

    Ok(all)
}

fn parse_output(output: &str) -> Vec<Reminder> {
    let mut records = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let reminder_id = parts[0].trim();
        let list_name = parts.get(1).copied().unwrap_or("").trim();
        let name = parts.get(2).copied().unwrap_or("").trim();
        let priority_str = parts.get(3).copied().unwrap_or("0").trim();
        let due_str = parts.get(4).copied().unwrap_or("").trim();

        if name.is_empty() {
            continue;
        }

        let priority: i32 = priority_str.parse().unwrap_or(0);

        let due_date = if due_str.is_empty() {
            None
        } else if let Ok(core_data_ts) = due_str.parse::<f64>() {
            DateTime::from_timestamp(core_data_ts as i64 + APPLE_EPOCH, 0)
        } else {
            super::util::parse_plist_date(due_str)
        };

        let id = if reminder_id.is_empty() {
            slug(name)
        } else {
            slug(reminder_id)
        };

        records.push(Reminder {
            id,
            title: truncate_for_title(name),
            list: list_name.to_string(),
            priority,
            due_date,
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "ABC-123-DEF\tShopping\tBuy groceries\t5\t793900800.0\t0\n\
             GHI-456-JKL\tHealth\tCall dentist\t0\t\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Buy groceries");
        assert_eq!(records[0].list, "Shopping");
        assert_eq!(records[0].priority, 5);
        assert!(records[0].due_date.is_some());
        assert_eq!(records[1].title, "Call dentist");
        assert!(records[1].due_date.is_none());
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}
