use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ScreenSharingStatus {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vnc_port: Option<String>,
}

pub async fn status() -> anyhow::Result<Vec<ScreenSharingStatus>> {
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

/// Enable screen sharing by loading the launchd plist.
/// Note: this requires sudo privileges.
pub async fn enable() -> anyhow::Result<ActionResult> {
    run_command_with_timeout(
        "sudo",
        &[
            "launchctl",
            "load",
            "-w",
            "/System/Library/LaunchDaemons/com.apple.screensharing.plist",
        ],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(ActionResult::success_with_message(
        "enable_screen_sharing",
        "Screen sharing enabled (sudo may be required)",
    ))
}

/// Disable screen sharing by unloading the launchd plist.
/// Note: this requires sudo privileges.
pub async fn disable() -> anyhow::Result<ActionResult> {
    run_command_with_timeout(
        "sudo",
        &[
            "launchctl",
            "unload",
            "-w",
            "/System/Library/LaunchDaemons/com.apple.screensharing.plist",
        ],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(ActionResult::success_with_message(
        "disable_screen_sharing",
        "Screen sharing disabled (sudo may be required)",
    ))
}
