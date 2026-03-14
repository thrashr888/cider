use super::util::{run_command_with_timeout, APPLE_EPOCH};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Photo {
    pub uuid: String,
    pub filename: String,
    pub kind: String,
    pub width: i64,
    pub height: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    pub duration: f64,
    pub is_favorite: bool,
    pub is_hidden: bool,
}

pub async fn fetch() -> anyhow::Result<Vec<Photo>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = format!("{home}/Pictures/Photos Library.photoslibrary/database/Photos.sqlite");

    if tokio::fs::metadata(&db_path).await.is_err() {
        anyhow::bail!("Photos database not found at {db_path}");
    }

    let query = r#"
SELECT
    ZUUID,
    ZFILENAME,
    ZKIND,
    ZWIDTH,
    ZHEIGHT,
    ZDATECREATED,
    ZLATITUDE,
    ZLONGITUDE,
    ZDURATION,
    ZFAVORITE,
    ZHIDDEN
FROM ZASSET
WHERE ZTRASHEDSTATE = 0
ORDER BY ZDATECREATED DESC
LIMIT 200;
"#;

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", &db_path, query.trim()],
        std::time::Duration::from_secs(15),
    )
    .await?;

    Ok(parse_output(&stdout))
}

fn parse_output(output: &str) -> Vec<Photo> {
    let mut records = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }

        let uuid = parts[0].trim();
        let filename = parts[1].trim();
        if filename.is_empty() {
            continue;
        }

        let kind_num: i64 = parts[2].trim().parse().unwrap_or(0);
        let width: i64 = parts[3].trim().parse().unwrap_or(0);
        let height: i64 = parts[4].trim().parse().unwrap_or(0);
        let date_ts: f64 = parts[5].trim().parse().unwrap_or(0.0);
        let lat: f64 = parts
            .get(6)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let lon: f64 = parts
            .get(7)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let duration: f64 = parts
            .get(8)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let is_favorite = parts.get(9).map(|s| s.trim() == "1").unwrap_or(false);
        let is_hidden = parts.get(10).map(|s| s.trim() == "1").unwrap_or(false);

        let date = if date_ts != 0.0 {
            DateTime::from_timestamp(date_ts as i64 + APPLE_EPOCH, 0)
        } else {
            None
        };

        let kind = match kind_num {
            0 => "photo",
            1 => "video",
            _ => "other",
        };

        records.push(Photo {
            uuid: uuid.to_string(),
            filename: filename.to_string(),
            kind: kind.to_string(),
            width,
            height,
            date,
            latitude: if lat == 0.0 { None } else { Some(lat) },
            longitude: if lon == 0.0 { None } else { Some(lon) },
            duration,
            is_favorite,
            is_hidden,
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output =
            "ABC-123\tIMG_0001.HEIC\t0\t4032\t3024\t726000000\t45.123\t-122.456\t0\t1\t0\n\
                       DEF-456\tVID_0002.MOV\t1\t1920\t1080\t726000000\t0\t0\t15.5\t0\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].kind, "photo");
        assert!(records[0].is_favorite);
        assert!(records[0].latitude.is_some());
        assert_eq!(records[1].kind, "video");
        assert_eq!(records[1].duration, 15.5);
        assert!(records[1].latitude.is_none());
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}
