use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TimeMachineInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_backup: Option<String>,
    pub backups: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

pub async fn status() -> anyhow::Result<Vec<TimeMachineInfo>> {
    let timeout = std::time::Duration::from_secs(10);

    let dest_info = run_command_with_timeout("tmutil", &["destinationinfo"], timeout)
        .await
        .ok();

    let destination = dest_info.as_ref().and_then(|output| {
        output
            .lines()
            .find(|l| l.contains("Name"))
            .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
    });

    let latest = run_command_with_timeout("tmutil", &["latestbackup"], timeout)
        .await
        .ok()
        .map(|s| s.trim().to_string());

    let backup_list = list().await.unwrap_or_default();

    let tm_status = run_command_with_timeout("tmutil", &["status"], timeout)
        .await
        .ok()
        .and_then(|output| {
            output
                .lines()
                .find(|l| l.contains("Running"))
                .map(|l| l.trim().to_string())
        });

    Ok(vec![TimeMachineInfo {
        destination,
        latest_backup: latest,
        backups: backup_list,
        status: tm_status,
    }])
}

/// List Time Machine backup paths.
pub async fn list() -> anyhow::Result<Vec<String>> {
    let timeout = std::time::Duration::from_secs(10);

    let output = run_command_with_timeout("tmutil", &["listbackups"], timeout).await?;

    Ok(output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Start a Time Machine backup.
pub async fn start() -> anyhow::Result<ActionResult> {
    run_command_with_timeout(
        "tmutil",
        &["startbackup"],
        std::time::Duration::from_secs(30),
    )
    .await?;

    Ok(ActionResult::success("start_backup"))
}

/// Stop a running Time Machine backup.
pub async fn stop() -> anyhow::Result<ActionResult> {
    run_command_with_timeout(
        "tmutil",
        &["stopbackup"],
        std::time::Duration::from_secs(30),
    )
    .await?;

    Ok(ActionResult::success("stop_backup"))
}
