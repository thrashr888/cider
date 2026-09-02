import Foundation
import XCTest
@testable import BridgeCore

/// `home.run_scene`, `home.set`, and the trigger mutations against the fake.
final class HomeWriteCommandsTests: XCTestCase {
    typealias IDs = FakeHomeKitService.SampleIDs

    private var router: CommandRouter!
    private var service: FakeHomeKitService!

    override func setUp() async throws {
        service = FakeHomeKitService.sample()
        router = CommandRouter(version: "test")
        await registerHomeCommands(router, service: service)
    }

    private func call(_ cmd: String, _ args: [String: JSONValue] = [:]) async -> Response {
        await router.dispatch(Request(id: 1, cmd: cmd, args: args))
    }

    private func data<T: Decodable>(_ type: T.Type, _ cmd: String, _ args: [String: JSONValue] = [:],
                                    file: StaticString = #filePath, line: UInt = #line) async throws -> T {
        let response = await call(cmd, args)
        guard response.ok, let data = response.data else {
            XCTFail("\(cmd) failed: \(response.error?.code ?? "?") \(response.error?.message ?? "")", file: file, line: line)
            throw BridgeError(body: response.error ?? ErrorBody(code: "internal", message: "no data"))
        }
        return try JSONDecoder().decode(type, from: try JSONEncoder().encode(data))
    }

    private func expectError(_ cmd: String, _ args: [String: JSONValue] = [:], code: String,
                             messageContains fragment: String? = nil,
                             file: StaticString = #filePath, line: UInt = #line) async {
        let response = await call(cmd, args)
        XCTAssertFalse(response.ok, "\(cmd) unexpectedly succeeded", file: file, line: line)
        XCTAssertEqual(response.error?.code, code, response.error?.message ?? "", file: file, line: line)
        if let fragment {
            XCTAssertTrue(response.error?.message.localizedCaseInsensitiveContains(fragment) ?? false,
                          "'\(response.error?.message ?? "")' lacks '\(fragment)'", file: file, line: line)
        }
    }

    // MARK: run_scene

    func testRunSceneByNameCaseInsensitiveAndByUUID() async throws {
        let ran = await call("home.run_scene", ["scene": "movie"])
        XCTAssertEqual(ran, .success(id: 1, data: ["ran": true]))
        let byUUID = await call("home.run_scene", ["home": "Cabin", "scene": .string(IDs.cabinMovie.uuidString)])
        XCTAssertTrue(byUUID.ok)
        let byName = await call("home.run_scene", ["home": "cabin", "scene": "Movie"])
        XCTAssertTrue(byName.ok)
        let executed = await service.executedScenes
        XCTAssertEqual(executed, [IDs.movie, IDs.cabinMovie, IDs.cabinMovie])
    }

    func testRunSceneErrors() async {
        await expectError("home.run_scene", code: "invalid_args", messageContains: "'scene' is required")
        await expectError("home.run_scene", ["scene": "Party"], code: "not_found", messageContains: "scene 'Party'")
        await expectError("home.run_scene", ["home": "Barn", "scene": "Movie"], code: "not_found")
        await service.setAuthorized(false)
        await expectError("home.run_scene", ["scene": "Movie"], code: "homekit_denied")
    }

    // MARK: set

    func testSetPowerStateAndReadBack() async throws {
        let result = try await data(SetResult.self, "home.set",
                                    ["accessory": "office light", "characteristic": "power_state", "value": false])
        XCTAssertEqual(result, SetResult(accessory: "Office Light", characteristic: "power_state", value: false))

        let rows = try await data([AccessoryRow].self, "home.accessories", ["room": "Office"])
        XCTAssertEqual(rows[0].services[0].characteristics[0].value, false)

        // Alias, string coercion, and a numeric characteristic.
        let on = try await data(SetResult.self, "home.set", ["accessory": "Office Light", "characteristic": "on", "value": "on"])
        XCTAssertEqual(on.value, true)
        let dim = try await data(SetResult.self, "home.set",
                                 ["accessory": .string(IDs.officeLight.uuidString), "characteristic": "brightness", "value": 25])
        XCTAssertEqual(dim, SetResult(accessory: "Office Light", characteristic: "brightness", value: 25))
    }

    func testSetResolvesByTypeUUIDCharacteristicUUIDAndService() async throws {
        let byType = try await data(SetResult.self, "home.set", [
            "accessory": "Office Light", "characteristic": "00000025-0000-1000-8000-0026BB765291", "value": false,
        ])
        XCTAssertEqual(byType.characteristic, "power_state")
        let byID = try await data(SetResult.self, "home.set", [
            "accessory": "Office Light", "characteristic": .string(IDs.officeLightBrightness.uuidString), "value": 5,
        ])
        XCTAssertEqual(byID.characteristic, "brightness")
        let scoped = try await data(SetResult.self, "home.set", [
            "accessory": "Office Light", "service": "lightbulb", "characteristic": "brightness", "value": 60,
        ])
        XCTAssertEqual(scoped.value, 60)
        await expectError("home.set", ["accessory": "Office Light", "service": "thermostat", "characteristic": "brightness", "value": 1],
                          code: "not_found", messageContains: "service 'thermostat'")
    }

    func testSetAmbiguousAccessoryNeedsUUID() async throws {
        await expectError("home.set", ["accessory": "Lamp", "characteristic": "power_state", "value": true],
                          code: "invalid_args", messageContains: "ambiguous")
        let result = try await data(SetResult.self, "home.set", [
            "accessory": .string(IDs.kitchenLamp.uuidString), "characteristic": "power_state", "value": false,
        ])
        XCTAssertEqual(result.accessory, "Lamp")
        let kitchen = try await data([AccessoryRow].self, "home.accessories", ["room": "Kitchen"])
        XCTAssertEqual(kitchen[0].services[0].characteristics[0].value, false)
        let office = try await data([AccessoryRow].self, "home.accessories", ["room": "Office"])
        XCTAssertEqual(office[1].services[0].characteristics[0].value, false, "other Lamp untouched")
    }

    func testSetErrors() async {
        await expectError("home.set", ["characteristic": "power_state", "value": true], code: "invalid_args", messageContains: "'accessory'")
        await expectError("home.set", ["accessory": "Office Light", "value": true], code: "invalid_args", messageContains: "'characteristic'")
        await expectError("home.set", ["accessory": "Office Light", "characteristic": "power_state"], code: "invalid_args", messageContains: "'value'")
        await expectError("home.set", ["accessory": "Toaster", "characteristic": "power_state", "value": true], code: "not_found", messageContains: "accessory 'Toaster'")
        await expectError("home.set", ["accessory": "Office Light", "characteristic": "hue", "value": 1], code: "not_found", messageContains: "characteristic 'hue'")
        await expectError("home.set", ["accessory": "Thermostat", "characteristic": "current_temperature", "value": 30],
                          code: "invalid_args", messageContains: "read-only")
    }

    // MARK: timer triggers

    func testCreateTimerTriggerReturnsRowAndAppearsInTriggers() async throws {
        let row = try await data(TriggerRow.self, "home.trigger_create_timer", [
            "name": "Lights off", "fire_at": "2026-09-03T22:30:00-07:00", "recurrence": "daily",
            "scenes": ["good night", .string(IDs.movie.uuidString)],
        ])
        XCTAssertEqual(row.name, "Lights off")
        XCTAssertEqual(row.home, "Loft")
        XCTAssertEqual(row.kind, "timer")
        XCTAssertTrue(row.enabled)
        XCTAssertEqual(row.fireDate, DateCoding.parse("2026-09-03T22:30:00-07:00"))
        XCTAssertEqual(row.recurrence, .daily)
        XCTAssertEqual(row.scenes, ["Good Night", "Movie"])
        XCTAssertNil(row.lastFire)

        let raw = await call("home.trigger_create_timer", [
            "name": "Every 30", "fire_at": "2026-09-03T08:00:00-07:00", "recurrence": ["minutes": 30], "scenes": "Movie",
        ]).data
        XCTAssertEqual(raw?["recurrence"], ["minutes": 30])
        XCTAssertEqual(raw?["fire_date"], .string(DateCoding.format(DateCoding.parse("2026-09-03T08:00:00-07:00")!)))
        XCTAssertEqual(raw?["scenes"], ["Movie"])

        let once = try await data(TriggerRow.self, "home.trigger_create_timer", [
            "name": "Once", "fire_at": "2026-09-04T06:00", "scenes": ["Movie"],
        ])
        XCTAssertNil(once.recurrence)

        let all = try await data([TriggerRow].self, "home.triggers")
        XCTAssertEqual(all.map(\.name), ["Morning", "Hall motion", "Lights off", "Every 30", "Once"])
        XCTAssertEqual(all[2].id, row.id)
    }

    func testCreateTimerTriggerErrors() async {
        let good: [String: JSONValue] = ["name": "T", "fire_at": "2026-09-03T22:30:00-07:00", "scenes": ["Movie"]]
        var args = good
        args["name"] = nil
        await expectError("home.trigger_create_timer", args, code: "invalid_args", messageContains: "'name'")
        args = good
        args["fire_at"] = nil
        await expectError("home.trigger_create_timer", args, code: "invalid_args", messageContains: "'fire_at'")
        args = good
        args["fire_at"] = "next tuesday"
        await expectError("home.trigger_create_timer", args, code: "invalid_args", messageContains: "RFC 3339")
        args = good
        args["scenes"] = []
        await expectError("home.trigger_create_timer", args, code: "invalid_args", messageContains: "'scenes'")
        args = good
        args["scenes"] = ["Party"]
        await expectError("home.trigger_create_timer", args, code: "not_found", messageContains: "scene 'Party'")
        args = good
        args["recurrence"] = "hourly"
        await expectError("home.trigger_create_timer", args, code: "invalid_args", messageContains: "recurrence")
        args = good
        args["home"] = "Barn"
        await expectError("home.trigger_create_timer", args, code: "not_found")
    }

    func testSetTriggerEnabledAndDelete() async throws {
        let disabled = try await data(TriggerRow.self, "home.trigger_set_enabled", ["trigger": "morning", "enabled": false])
        XCTAssertEqual(disabled.id, IDs.morningTrigger)
        XCTAssertFalse(disabled.enabled)
        let enabled = try await data(TriggerRow.self, "home.trigger_set_enabled",
                                     ["trigger": .string(IDs.motionTrigger.uuidString), "enabled": "true"])
        XCTAssertTrue(enabled.enabled)
        let listed = try await data([TriggerRow].self, "home.triggers")
        XCTAssertEqual(listed.map(\.enabled), [false, true])

        await expectError("home.trigger_set_enabled", ["trigger": "Morning"], code: "invalid_args", messageContains: "'enabled'")
        await expectError("home.trigger_set_enabled", ["trigger": "Nope", "enabled": true], code: "not_found", messageContains: "trigger 'Nope'")

        let deleted = await call("home.trigger_delete", ["trigger": "Hall Motion"])
        XCTAssertEqual(deleted, .success(id: 1, data: ["deleted": true]))
        let remaining = try await data([TriggerRow].self, "home.triggers")
        XCTAssertEqual(remaining.map(\.name), ["Morning"])
        await expectError("home.trigger_delete", ["trigger": "Hall motion"], code: "not_found")
        await expectError("home.trigger_delete", [:], code: "invalid_args", messageContains: "'trigger'")
    }

    func testCommandSurfaceMatchesRFC() async {
        let commands = await router.commands
        XCTAssertEqual(commands, [
            "home.accessories", "home.homes", "home.rooms", "home.run_scene", "home.scenes", "home.set",
            "home.trigger_create_timer", "home.trigger_delete", "home.trigger_set_enabled", "home.triggers",
            "ping", "quit",
        ])
    }
}
