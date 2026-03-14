use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computer_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_build: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_gb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_resolution: Option<String>,
}

pub async fn show() -> anyhow::Result<Vec<SystemInfo>> {
    let timeout = std::time::Duration::from_secs(10);

    let computer_name = run_command_with_timeout("scutil", &["--get", "ComputerName"], timeout)
        .await
        .ok();
    let hostname = run_command_with_timeout("hostname", &[], timeout)
        .await
        .ok();

    let os_version = run_command_with_timeout("sw_vers", &["-productVersion"], timeout)
        .await
        .ok();
    let os_build = run_command_with_timeout("sw_vers", &["-buildVersion"], timeout)
        .await
        .ok();

    // Hardware info via sysctl (fast, unlike system_profiler)
    let hardware_model = run_command_with_timeout("sysctl", &["-n", "hw.model"], timeout)
        .await
        .ok();
    let chip = run_command_with_timeout("sysctl", &["-n", "machdep.cpu.brand_string"], timeout)
        .await
        .ok();
    let serial_number = run_command_with_timeout("ioreg", &["-l", "-d", "2"], timeout)
        .await
        .ok()
        .and_then(|output| {
            output
                .lines()
                .find(|l| l.contains("IOPlatformSerialNumber"))
                .and_then(|l| l.split('"').nth(3))
                .map(String::from)
        });

    let memory_bytes: Option<u64> =
        run_command_with_timeout("sysctl", &["-n", "hw.memsize"], timeout)
            .await
            .ok()
            .and_then(|s| s.trim().parse().ok());
    let memory_gb = memory_bytes.map(|b| format!("{} GB", b / 1_073_741_824));

    let uptime = run_command_with_timeout("uptime", &[], timeout).await.ok();
    let shell = std::env::var("SHELL").ok();

    let display_resolution =
        run_command_with_timeout("system_profiler", &["SPDisplaysDataType"], timeout)
            .await
            .ok()
            .and_then(|output| {
                output
                    .lines()
                    .find(|l| l.contains("Resolution"))
                    .map(|l| l.trim().to_string())
            });

    Ok(vec![SystemInfo {
        computer_name,
        hostname,
        os_version,
        os_build,
        hardware_model,
        chip,
        serial_number,
        memory_gb,
        uptime: uptime.map(|u| u.trim().to_string()),
        shell,
        display_resolution,
    }])
}

/// Set the computer name and local hostname.
pub async fn set_computer_name(name: &str) -> anyhow::Result<ActionResult> {
    let timeout = std::time::Duration::from_secs(10);

    run_command_with_timeout("scutil", &["--set", "ComputerName", name], timeout).await?;
    run_command_with_timeout("scutil", &["--set", "LocalHostName", name], timeout).await?;

    Ok(ActionResult::success_with_message(
        "set_computer_name",
        &format!("Computer name set to '{name}'"),
    ))
}

/// Read a defaults domain (optionally a specific key).
pub async fn defaults_read(domain: &str, key: Option<&str>) -> anyhow::Result<String> {
    let timeout = std::time::Duration::from_secs(10);

    let mut args = vec!["read", domain];
    if let Some(k) = key {
        args.push(k);
    }

    run_command_with_timeout("defaults", &args, timeout).await
}

/// Write a defaults value.
pub async fn defaults_write(domain: &str, key: &str, value: &str) -> anyhow::Result<ActionResult> {
    let timeout = std::time::Duration::from_secs(10);

    run_command_with_timeout("defaults", &["write", domain, key, value], timeout).await?;

    Ok(ActionResult::success_with_message(
        "defaults_write",
        &format!("Set {domain} {key}"),
    ))
}
