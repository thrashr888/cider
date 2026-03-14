use super::util::run_jxa_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Track {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    pub duration_seconds: f64,
    pub play_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    pub rating: i64,
    pub loved: bool,
}

pub async fn fetch() -> anyhow::Result<Vec<Track>> {
    // Use JXA with bulk property access for speed.
    // Handle the case where Music.app has no library.
    let script = r#"
const app = Application("Music");
let lib;
try { lib = app.libraryPlaylists[0]; } catch(e) { ""; }
if (!lib) { ""; } else {
    const tracks = lib.tracks;
    let count;
    try { count = tracks.length; } catch(e) { count = 0; }
    if (count === 0) { ""; } else {
        const limit = Math.min(count, 500);
        const names = tracks.name().slice(0, limit);
        const artists = tracks.artist().slice(0, limit);
        const albums = tracks.album().slice(0, limit);
        const genres = tracks.genre().slice(0, limit);
        const durations = tracks.duration().slice(0, limit);
        const playCounts = tracks.playedCount().slice(0, limit);
        const years = tracks.year().slice(0, limit);
        const ratings = tracks.rating().slice(0, limit);
        const loveds = tracks.loved().slice(0, limit);
        const results = [];
        for (let i = 0; i < limit; i++) {
            results.push([
                (names[i] || "").replace(/[\t\n\r]/g, " "),
                (artists[i] || "").replace(/[\t\n\r]/g, " "),
                (albums[i] || "").replace(/[\t\n\r]/g, " "),
                (genres[i] || "").replace(/[\t\n\r]/g, " "),
                durations[i] || 0,
                playCounts[i] || 0,
                years[i] || 0,
                ratings[i] || 0,
                loveds[i] ? "1" : "0"
            ].join("\t"));
        }
        results.join("\n");
    }
}
"#;

    let output = run_jxa_with_timeout(script, std::time::Duration::from_secs(120)).await?;
    if output.is_empty() {
        anyhow::bail!("Music library is empty or Music.app is not configured");
    }
    Ok(parse_output(&output))
}

fn parse_output(output: &str) -> Vec<Track> {
    let mut records = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }
        let name = parts[0].trim();
        if name.is_empty() {
            continue;
        }
        let artist = parts.get(1).copied().unwrap_or("").trim();
        let album = parts.get(2).copied().unwrap_or("").trim();
        let genre = parts.get(3).copied().unwrap_or("").trim();
        let duration: f64 = parts
            .get(4)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let play_count: i64 = parts
            .get(5)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let year: i64 = parts
            .get(6)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let rating: i64 = parts
            .get(7)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let loved = parts.get(8).map(|s| s.trim() == "1").unwrap_or(false);

        records.push(Track {
            name: name.to_string(),
            artist: if artist.is_empty() {
                None
            } else {
                Some(artist.to_string())
            },
            album: if album.is_empty() {
                None
            } else {
                Some(album.to_string())
            },
            genre: if genre.is_empty() {
                None
            } else {
                Some(genre.to_string())
            },
            duration_seconds: duration,
            play_count,
            year: if year == 0 { None } else { Some(year) },
            rating,
            loved,
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
            "Bohemian Rhapsody\tQueen\tA Night at the Opera\tRock\t354.32\t42\t1975\t80\t1\n\
                       Yesterday\tThe Beatles\tHelp!\tPop\t125.0\t10\t1965\t60\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Bohemian Rhapsody");
        assert_eq!(records[0].artist.as_deref(), Some("Queen"));
        assert_eq!(records[0].play_count, 42);
        assert!(records[0].loved);
        assert!(!records[1].loved);
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}
