import Foundation

/// Human names for Apple-defined HAP characteristic and service types.
///
/// HomeKit exposes types as UUID strings; Apple's own are
/// `0000XXXX-0000-1000-8000-0026BB765291` where `XXXX` is the HAP short id.
/// Vendor types (any other UUID) keep the UUID as their name.
public enum HAPTypes {
    public struct CharacteristicInfo: Equatable, Sendable {
        public let name: String
        public let unit: String?
        public init(name: String, unit: String? = nil) {
            self.name = name
            self.unit = unit
        }
    }

    static let appleBaseSuffix = "-0000-1000-8000-0026BB765291"

    /// `"00000025-0000-1000-8000-0026BB765291"`, `"25"`, `"0x25"` -> 0x25; vendor UUIDs -> nil.
    public static func shortID(_ type: String) -> Int? {
        var hex = type.trimmingCharacters(in: .whitespaces).uppercased()
        if hex.hasSuffix(appleBaseSuffix) {
            hex = String(hex.dropLast(appleBaseSuffix.count))
        } else if hex.contains("-") {
            return nil
        }
        if hex.hasPrefix("0X") { hex = String(hex.dropFirst(2)) }
        guard !hex.isEmpty, hex.count <= 8 else { return nil }
        return Int(hex, radix: 16)
    }

    public static func fullUUID(shortID: Int) -> String {
        String(format: "%08X", shortID) + appleBaseSuffix
    }

    // MARK: Characteristics

    public static func characteristic(forType type: String) -> CharacteristicInfo? {
        shortID(type).flatMap { characteristics[$0] }
    }

    public static func characteristicName(forType type: String) -> String {
        characteristic(forType: type)?.name ?? type
    }

    public static func characteristicUnit(forType type: String) -> String? {
        characteristic(forType: type)?.unit
    }

    /// Reverse lookup for `home.set`: a snake_case name (or alias) to the full UUID.
    public static func characteristicType(forName name: String) -> String? {
        let key = name.trimmingCharacters(in: .whitespaces).lowercased().replacingOccurrences(of: " ", with: "_")
        let canonical = characteristicAliases[key] ?? key
        return characteristicsByName[canonical].map(fullUUID(shortID:))
    }

    public static let characteristicAliases: [String: String] = [
        "on": "power_state", "power": "power_state", "off": "power_state",
        "temperature": "target_temperature", "position": "target_position",
        "lock": "lock_target_state", "door": "target_door_state",
        "mode": "target_heating_cooling_state", "color_temp": "color_temperature",
        "speed": "rotation_speed", "humidity": "current_relative_humidity",
    ]

    public static let characteristics: [Int: CharacteristicInfo] = [
        0x08: .init(name: "brightness", unit: "%"),
        0x0D: .init(name: "cooling_threshold_temperature", unit: "°C"),
        0x0E: .init(name: "current_door_state"),
        0x0F: .init(name: "current_heating_cooling_state"),
        0x10: .init(name: "current_relative_humidity", unit: "%"),
        0x11: .init(name: "current_temperature", unit: "°C"),
        0x12: .init(name: "heating_threshold_temperature", unit: "°C"),
        0x13: .init(name: "hue", unit: "°"),
        0x14: .init(name: "identify"),
        0x1D: .init(name: "lock_current_state"),
        0x1E: .init(name: "lock_target_state"),
        0x20: .init(name: "manufacturer"),
        0x21: .init(name: "model"),
        0x22: .init(name: "motion_detected"),
        0x23: .init(name: "name"),
        0x24: .init(name: "obstruction_detected"),
        0x25: .init(name: "power_state"),
        0x26: .init(name: "outlet_in_use"),
        0x28: .init(name: "rotation_direction"),
        0x29: .init(name: "rotation_speed", unit: "%"),
        0x2F: .init(name: "saturation", unit: "%"),
        0x30: .init(name: "serial_number"),
        0x32: .init(name: "target_door_state"),
        0x33: .init(name: "target_heating_cooling_state"),
        0x34: .init(name: "target_relative_humidity", unit: "%"),
        0x35: .init(name: "target_temperature", unit: "°C"),
        0x36: .init(name: "temperature_display_units"),
        0x52: .init(name: "firmware_revision"),
        0x53: .init(name: "hardware_revision"),
        0x66: .init(name: "security_system_current_state"),
        0x67: .init(name: "security_system_target_state"),
        0x68: .init(name: "battery_level", unit: "%"),
        0x69: .init(name: "carbon_monoxide_detected"),
        0x6A: .init(name: "contact_state"),
        0x6B: .init(name: "current_ambient_light_level", unit: "lux"),
        0x6D: .init(name: "current_position", unit: "%"),
        0x6F: .init(name: "hold_position"),
        0x70: .init(name: "leak_detected"),
        0x71: .init(name: "occupancy_detected"),
        0x72: .init(name: "position_state"),
        0x73: .init(name: "programmable_switch_event"),
        0x75: .init(name: "status_active"),
        0x76: .init(name: "smoke_detected"),
        0x77: .init(name: "status_fault"),
        0x79: .init(name: "status_low_battery"),
        0x7A: .init(name: "status_tampered"),
        0x7C: .init(name: "target_position", unit: "%"),
        0x8F: .init(name: "charging_state"),
        0x92: .init(name: "carbon_dioxide_detected"),
        0x93: .init(name: "carbon_dioxide_level", unit: "ppm"),
        0x95: .init(name: "air_quality"),
        0xA7: .init(name: "lock_physical_controls"),
        0xA8: .init(name: "target_air_purifier_state"),
        0xA9: .init(name: "current_air_purifier_state"),
        0xAB: .init(name: "filter_life_level", unit: "%"),
        0xAC: .init(name: "filter_change_indication"),
        0xAF: .init(name: "current_fan_state"),
        0xB0: .init(name: "active"),
        0xB1: .init(name: "current_heater_cooler_state"),
        0xB2: .init(name: "target_heater_cooler_state"),
        0xB6: .init(name: "swing_mode"),
        0xBF: .init(name: "target_fan_state"),
        0xCE: .init(name: "color_temperature", unit: "mired"),
        0xD1: .init(name: "program_mode"),
        0xD2: .init(name: "in_use"),
        0xD3: .init(name: "set_duration", unit: "s"),
        0xD4: .init(name: "remaining_duration", unit: "s"),
        0xD5: .init(name: "valve_type"),
        0xD6: .init(name: "is_configured"),
        0xDB: .init(name: "input_source_type"),
        0xDC: .init(name: "input_device_type"),
        0xDD: .init(name: "closed_captions"),
        0xDF: .init(name: "power_mode_selection"),
        0xE0: .init(name: "current_media_state"),
        0xE1: .init(name: "remote_key"),
        0xE2: .init(name: "picture_mode"),
        0xE3: .init(name: "configured_name"),
        0xE6: .init(name: "identifier"),
        0xE7: .init(name: "active_identifier"),
        0xE8: .init(name: "sleep_discovery_mode"),
        0xE9: .init(name: "volume_control_type"),
        0xEA: .init(name: "volume_selector"),
        0x119: .init(name: "volume", unit: "%"),
        0x11A: .init(name: "mute"),
        0x134: .init(name: "target_visibility_state"),
        0x135: .init(name: "current_visibility_state"),
        0x137: .init(name: "target_media_state"),
    ]

    private static let characteristicsByName: [String: Int] = Dictionary(
        characteristics.map { ($0.value.name, $0.key) }, uniquingKeysWith: { first, _ in first })

    // MARK: Services

    public static let accessoryInformationServiceID = 0x3E

    public static func serviceName(forType type: String) -> String {
        shortID(type).flatMap { services[$0] } ?? type
    }

    public static func serviceType(forName name: String) -> String? {
        let key = name.trimmingCharacters(in: .whitespaces).lowercased().replacingOccurrences(of: " ", with: "_")
        return servicesByName[key].map(fullUUID(shortID:))
    }

    public static func isAccessoryInformation(serviceType type: String) -> Bool {
        shortID(type) == accessoryInformationServiceID
    }

    public static let services: [Int: String] = [
        0x3E: "accessory_information",
        0x40: "fan",
        0x41: "garage_door_opener",
        0x43: "lightbulb",
        0x44: "lock_management",
        0x45: "lock_mechanism",
        0x47: "outlet",
        0x49: "switch",
        0x4A: "thermostat",
        0x7E: "security_system",
        0x7F: "carbon_monoxide_sensor",
        0x80: "contact_sensor",
        0x81: "door",
        0x82: "humidity_sensor",
        0x83: "leak_sensor",
        0x84: "light_sensor",
        0x85: "motion_sensor",
        0x86: "occupancy_sensor",
        0x87: "smoke_sensor",
        0x89: "stateless_programmable_switch",
        0x8A: "temperature_sensor",
        0x8B: "window",
        0x8C: "window_covering",
        0x8D: "air_quality_sensor",
        0x96: "battery",
        0x97: "carbon_dioxide_sensor",
        0xB7: "fan_v2",
        0xB9: "slats",
        0xBA: "filter_maintenance",
        0xBB: "air_purifier",
        0xBC: "heater_cooler",
        0xBD: "humidifier_dehumidifier",
        0xCC: "service_label",
        0xCF: "irrigation_system",
        0xD0: "valve",
        0xD7: "faucet",
        0xD8: "television",
        0xD9: "input_source",
        0x110: "camera_rtp_stream_management",
        0x112: "microphone",
        0x113: "speaker",
        0x121: "doorbell",
    ]

    private static let servicesByName: [String: Int] = Dictionary(
        services.map { ($0.value, $0.key) }, uniquingKeysWith: { first, _ in first })
}
