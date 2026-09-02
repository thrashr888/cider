import Foundation
import XCTest
@testable import BridgeCore

final class HAPTypesTests: XCTestCase {
    func testShortIDParsing() {
        XCTAssertEqual(HAPTypes.shortID("00000025-0000-1000-8000-0026BB765291"), 0x25)
        XCTAssertEqual(HAPTypes.shortID("00000025-0000-1000-8000-0026bb765291"), 0x25)
        XCTAssertEqual(HAPTypes.shortID("25"), 0x25)
        XCTAssertEqual(HAPTypes.shortID("0x25"), 0x25)
        XCTAssertEqual(HAPTypes.shortID("00000119-0000-1000-8000-0026BB765291"), 0x119)
        XCTAssertNil(HAPTypes.shortID("E863F10F-079E-48FF-8F27-9C2605A29F52"))  // vendor (Eve)
        XCTAssertNil(HAPTypes.shortID(""))
        XCTAssertEqual(HAPTypes.fullUUID(shortID: 0x25), "00000025-0000-1000-8000-0026BB765291")
    }

    func testCharacteristicNamesAndUnits() {
        let cases: [(Int, String, String?)] = [
            (0x25, "power_state", nil), (0x08, "brightness", "%"), (0x13, "hue", "°"), (0x2F, "saturation", "%"),
            (0xCE, "color_temperature", "mired"), (0x11, "current_temperature", "°C"),
            (0x35, "target_temperature", "°C"), (0x33, "target_heating_cooling_state", nil),
            (0x1D, "lock_current_state", nil), (0x1E, "lock_target_state", nil), (0x22, "motion_detected", nil),
            (0x6A, "contact_state", nil), (0x71, "occupancy_detected", nil), (0x68, "battery_level", "%"),
            (0x6D, "current_position", "%"), (0x7C, "target_position", "%"),
        ]
        for (id, name, unit) in cases {
            let type = HAPTypes.fullUUID(shortID: id)
            XCTAssertEqual(HAPTypes.characteristicName(forType: type), name, type)
            XCTAssertEqual(HAPTypes.characteristicUnit(forType: type), unit, type)
        }
    }

    func testUnknownCharacteristicKeepsUUID() {
        let vendor = "E863F10F-079E-48FF-8F27-9C2605A29F52"
        XCTAssertEqual(HAPTypes.characteristicName(forType: vendor), vendor)
        XCTAssertNil(HAPTypes.characteristicUnit(forType: vendor))
        let unmapped = HAPTypes.fullUUID(shortID: 0xFFFF)
        XCTAssertEqual(HAPTypes.characteristicName(forType: unmapped), unmapped)
    }

    func testReverseCharacteristicLookupAndAliases() {
        XCTAssertEqual(HAPTypes.characteristicType(forName: "power_state"), HAPTypes.fullUUID(shortID: 0x25))
        XCTAssertEqual(HAPTypes.characteristicType(forName: "Power State"), HAPTypes.fullUUID(shortID: 0x25))
        XCTAssertEqual(HAPTypes.characteristicType(forName: "on"), HAPTypes.fullUUID(shortID: 0x25))
        XCTAssertEqual(HAPTypes.characteristicType(forName: "brightness"), HAPTypes.fullUUID(shortID: 0x08))
        XCTAssertNil(HAPTypes.characteristicType(forName: "warp_drive"))
    }

    func testServiceNames() {
        let cases: [(Int, String)] = [
            (0x43, "lightbulb"), (0x49, "switch"), (0x47, "outlet"), (0x4A, "thermostat"), (0x45, "lock_mechanism"),
            (0x85, "motion_sensor"), (0x80, "contact_sensor"), (0x86, "occupancy_sensor"),
            (0x8A, "temperature_sensor"), (0x82, "humidity_sensor"), (0x8C, "window_covering"),
            (0x41, "garage_door_opener"), (0x40, "fan"), (0x113, "speaker"), (0xD8, "television"),
        ]
        for (id, name) in cases {
            XCTAssertEqual(HAPTypes.serviceName(forType: HAPTypes.fullUUID(shortID: id)), name)
        }
        let vendor = "E863F007-079E-48FF-8F27-9C2605A29F52"
        XCTAssertEqual(HAPTypes.serviceName(forType: vendor), vendor)
        XCTAssertTrue(HAPTypes.isAccessoryInformation(serviceType: HAPTypes.fullUUID(shortID: 0x3E)))
        XCTAssertFalse(HAPTypes.isAccessoryInformation(serviceType: HAPTypes.fullUUID(shortID: 0x43)))
        XCTAssertEqual(HAPTypes.serviceType(forName: "Lightbulb"), HAPTypes.fullUUID(shortID: 0x43))
    }

    func testCharacteristicRowMake() {
        let row = CharacteristicRow.make(type: HAPTypes.fullUUID(shortID: 0x08), value: 40, writable: true)
        XCTAssertEqual(row.name, "brightness")
        XCTAssertEqual(row.unit, "%")
        XCTAssertTrue(row.readable)
        let json = try! JSONValue(encoding: row)
        XCTAssertEqual(json["value"], 40)
        XCTAssertEqual(json["unit"], "%")
        XCTAssertEqual(json["type"], "00000008-0000-1000-8000-0026BB765291")
    }
}
