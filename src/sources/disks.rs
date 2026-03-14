use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total_bytes: i64,
    pub available_bytes: i64,
    pub used_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_type: Option<String>,
}

/// List mounted volumes with usage info.
pub async fn list() -> anyhow::Result<Vec<DiskInfo>> {
    let output =
        run_command_with_timeout("df", &["-Hl"], std::time::Duration::from_secs(5)).await?;

    let mut disks = Vec::new();
    for line in output.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }

        let filesystem = parts[0];
        // Skip pseudo-filesystems
        if filesystem.starts_with("devfs") || filesystem.starts_with("map ") || filesystem == "none"
        {
            continue;
        }

        let total = parse_size(parts[1]);
        let _used = parse_size(parts[2]);
        let available = parse_size(parts[3]);
        let percent_str = parts[4].trim_end_matches('%');
        let percent: f64 = percent_str.parse().unwrap_or(0.0);
        let mount = parts[8..].join(" ");

        if mount.starts_with("/private") || mount.is_empty() {
            continue;
        }

        let name = mount.rsplit('/').next().unwrap_or(&mount).to_string();
        let name = if name == "/" {
            "Macintosh HD".to_string()
        } else {
            name
        };

        disks.push(DiskInfo {
            name,
            mount_point: mount,
            file_system: filesystem.to_string(),
            total_bytes: total,
            available_bytes: available,
            used_percent: percent,
            disk_type: None,
        });
    }

    // Enrich with diskutil info for disk type
    if let Ok(diskutil_output) = run_command_with_timeout(
        "diskutil",
        &["list", "-plist"],
        std::time::Duration::from_secs(5),
    )
    .await
    {
        // Just check if it's an SSD via simple string matching
        let is_ssd = diskutil_output.contains("Solid State");
        for disk in &mut disks {
            disk.disk_type = Some(if is_ssd { "SSD" } else { "HDD" }.to_string());
        }
    }

    Ok(disks)
}

fn parse_size(s: &str) -> i64 {
    let s = s.trim();
    let (num, multiplier) = if let Some(n) = s.strip_suffix("Ti") {
        (n, 1_099_511_627_776i64)
    } else if let Some(n) = s.strip_suffix("Gi") {
        (n, 1_073_741_824)
    } else if let Some(n) = s.strip_suffix("Mi") {
        (n, 1_048_576)
    } else if let Some(n) = s.strip_suffix("Ki") {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix('T') {
        (n, 1_000_000_000_000)
    } else if let Some(n) = s.strip_suffix('G') {
        (n, 1_000_000_000)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1_000_000)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1000)
    } else {
        (s, 1)
    };

    num.parse::<f64>().unwrap_or(0.0) as i64 * multiplier
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("1G"), 1_000_000_000);
        assert_eq!(parse_size("500M"), 500_000_000);
        assert_eq!(parse_size("2T"), 2_000_000_000_000);
        assert_eq!(parse_size("100K"), 100_000);
    }
}
