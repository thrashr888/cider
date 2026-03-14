use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BluetoothDevice {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_level: Option<String>,
}

/// List paired/connected Bluetooth devices.
pub async fn list() -> anyhow::Result<Vec<BluetoothDevice>> {
    let output = run_command_with_timeout(
        "system_profiler",
        &["SPBluetoothDataType", "-json"],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let data: serde_json::Value = serde_json::from_str(&output)?;

    let mut devices = Vec::new();

    // Navigate the system_profiler JSON structure
    let bt_data = data
        .get("SPBluetoothDataType")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first());

    if let Some(bt) = bt_data {
        // Connected/paired devices are in device_connected or device_not_connected
        for section_key in &["device_connected", "device_not_connected", "device_title"] {
            if let Some(section) = bt.get(section_key).and_then(|s| s.as_array()) {
                for device_wrapper in section {
                    if let Some(obj) = device_wrapper.as_object() {
                        for (name, info) in obj {
                            let connected = *section_key == "device_connected";
                            let address = info
                                .get("device_address")
                                .and_then(|a| a.as_str())
                                .map(String::from);
                            let device_type = info
                                .get("device_minorType")
                                .and_then(|t| t.as_str())
                                .map(String::from);
                            let battery = info
                                .get("device_batteryLevelMain")
                                .and_then(|b| b.as_str())
                                .map(String::from);

                            devices.push(BluetoothDevice {
                                name: name.clone(),
                                address,
                                connected,
                                device_type,
                                battery_level: battery,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(devices)
}
