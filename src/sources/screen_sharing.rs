use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ScreenSharingStatus {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vnc_port: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<ScreenSharingStatus>> {
    let timeout = std::time::Duration::from_secs(5);

    // Check if screen sharing is enabled by looking at the launchd service
    let enabled = run_command_with_timeout(
        "launchctl",
        &["print", "system/com.apple.screensharing"],
        timeout,
    )
    .await
    .is_ok();

    // Check if VNC port is listening
    let vnc_port = run_command_with_timeout("lsof", &["-i", ":5900", "-sTCP:LISTEN"], timeout)
        .await
        .ok()
        .filter(|s| !s.is_empty())
        .map(|_| "5900".to_string());

    Ok(vec![ScreenSharingStatus {
        enabled: enabled || vnc_port.is_some(),
        vnc_port,
    }])
}
