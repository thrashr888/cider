use super::util::run_jxa_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct HomeAccessory {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_name: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<HomeAccessory>> {
    // HomeKit doesn't have great JXA/AppleScript support.
    // Try using the shortcuts CLI to run a HomeKit query if available,
    // or attempt to read the HomeKit database.
    let home = std::env::var("HOME").unwrap_or_default();
    let hk_dir = format!("{home}/Library/Containers/com.apple.Home/Data/Library");

    // Try JXA first (may not work on all systems)
    let script = r#"
const app = Application("Home");
const results = [];
try {
    const homes = app.homes();
    for (let h = 0; h < homes.length; h++) {
        const home = homes[h];
        const homeName = home.name();
        const rooms = home.rooms();
        for (let r = 0; r < rooms.length; r++) {
            const room = rooms[r];
            const roomName = room.name();
            const accessories = room.accessories();
            for (let a = 0; a < accessories.length; a++) {
                const acc = accessories[a];
                results.push([acc.name(), roomName, homeName].join("\t"));
            }
        }
    }
} catch(e) {}
results.join("\n")
"#;

    match run_jxa_with_timeout(script, std::time::Duration::from_secs(15)).await {
        Ok(output) if !output.is_empty() => return Ok(parse_output(&output)),
        _ => {}
    }

    // Fallback: check if Home container exists but JXA didn't work
    if tokio::fs::metadata(&hk_dir).await.is_ok() {
        anyhow::bail!(
            "Home app data exists but JXA access failed. HomeKit scripting may not be supported on this system."
        );
    }

    anyhow::bail!("Home app not configured or not accessible")
}

fn parse_output(output: &str) -> Vec<HomeAccessory> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let name = parts.first()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(HomeAccessory {
                name: name.to_string(),
                room: parts
                    .get(1)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                home_name: parts
                    .get(2)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "Living Room Light\tLiving Room\tMy Home\nKitchen Speaker\tKitchen\tMy Home\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Living Room Light");
        assert_eq!(records[0].room.as_deref(), Some("Living Room"));
    }
}
