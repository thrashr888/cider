use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct App {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<App>> {
    // Use mdfind to quickly enumerate .app bundles, then read version from Info.plist.
    // This is much faster than system_profiler SPApplicationsDataType.
    let output = run_command_with_timeout(
        "mdfind",
        &[
            "kMDItemContentType == 'com.apple.application-bundle'",
            "-onlyin",
            "/Applications",
        ],
        std::time::Duration::from_secs(15),
    )
    .await?;

    let mut apps = Vec::new();
    for line in output.lines() {
        let path = line.trim();
        if path.is_empty() || !path.ends_with(".app") {
            continue;
        }

        let name = path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .strip_suffix(".app")
            .unwrap_or(path)
            .to_string();

        let plist_path = format!("{path}/Contents/Info.plist");
        let version = run_command_with_timeout(
            "defaults",
            &["read", &plist_path, "CFBundleShortVersionString"],
            std::time::Duration::from_secs(2),
        )
        .await
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

        apps.push(App {
            name,
            path: path.to_string(),
            version,
        });
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(apps)
}

#[cfg(test)]
mod tests {
    // Integration tests only — requires filesystem access
}
