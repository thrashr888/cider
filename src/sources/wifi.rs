use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WifiStatus {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct KnownNetwork {
    pub ssid: String,
}

/// Show current Wi-Fi connection status.
pub async fn status() -> anyhow::Result<Vec<WifiStatus>> {
    let timeout = std::time::Duration::from_secs(5);

    // Get the Wi-Fi interface name
    let iface = run_command_with_timeout("networksetup", &["-listallhardwareports"], timeout)
        .await
        .ok()
        .and_then(|output| {
            let lines: Vec<&str> = output.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.contains("Wi-Fi") {
                    if let Some(device_line) = lines.get(i + 1) {
                        return device_line.split(':').nth(1).map(|s| s.trim().to_string());
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| "en0".to_string());

    // Use system_profiler for Wi-Fi info (works on all macOS versions)
    let output =
        run_command_with_timeout("system_profiler", &["SPAirPortDataType", "-json"], timeout).await;

    if let Ok(json_str) = output {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_str) {
            // Navigate to current network info
            if let Some(airport) = data
                .get("SPAirPortDataType")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
            {
                let interfaces = airport
                    .get("spairport_airport_interfaces")
                    .and_then(|i| i.as_array());

                if let Some(ifaces) = interfaces {
                    for iface_info in ifaces {
                        let current = iface_info.get("spairport_current_network_information");
                        if let Some(net) = current {
                            let ssid = net.get("_name").and_then(|n| n.as_str()).map(String::from);
                            let channel = net
                                .get("spairport_network_channel")
                                .and_then(|c| c.as_str())
                                .map(String::from);
                            let security = net
                                .get("spairport_security_mode")
                                .and_then(|s| s.as_str())
                                .map(String::from);
                            let signal = net.get("spairport_signal_noise").and_then(|s| s.as_i64());

                            return Ok(vec![WifiStatus {
                                connected: ssid.is_some(),
                                ssid,
                                bssid: None,
                                signal_strength: signal,
                                channel,
                                security,
                                interface: Some(iface.clone()),
                            }]);
                        }
                    }
                }
            }
        }
    }

    // Fallback: try ipconfig for basic connectivity
    let connected = run_command_with_timeout("ipconfig", &["getifaddr", &iface], timeout)
        .await
        .is_ok();

    Ok(vec![WifiStatus {
        connected,
        ssid: None,
        bssid: None,
        signal_strength: None,
        channel: None,
        security: None,
        interface: Some(iface),
    }])
}

/// List known/preferred Wi-Fi networks.
pub async fn networks() -> anyhow::Result<Vec<KnownNetwork>> {
    let output = run_command_with_timeout(
        "networksetup",
        &["-listpreferredwirelessnetworks", "en0"],
        std::time::Duration::from_secs(5),
    )
    .await?;

    Ok(output
        .lines()
        .skip(1) // First line is "Preferred networks on en0:"
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|ssid| KnownNetwork { ssid })
        .collect())
}
