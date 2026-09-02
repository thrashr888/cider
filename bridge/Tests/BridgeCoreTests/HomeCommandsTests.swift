import Foundation
import XCTest
@testable import BridgeCore

/// Every `home.*` command through the router against `FakeHomeKitService`.
final class HomeCommandsTests: XCTestCase {
    typealias IDs = FakeHomeKitService.SampleIDs

    private var router: CommandRouter!
    private var service: FakeHomeKitService!

    static let build = BridgeBuild(
        kind: .development, homekitEntitled: true, weatherkitEntitled: true,
        bundlePath: "/Users/me/Applications/Cider Bridge.app")

    override func setUp() async throws {
        service = FakeHomeKitService.sample()
        router = CommandRouter(version: "test")
        await registerHomeCommands(router, service: service, build: Self.build)
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

    // MARK: ping

    func testPingReportsAuthorizationHomeCountAndBuild() async throws {
        let authorized = await call("ping")
        XCTAssertEqual(authorized, .success(id: 1, data: [
            "version": "test", "homekit_authorized": true, "homes": 2,
            "build": "development", "homekit_entitled": true, "weatherkit_entitled": true,
            "bundle_path": "/Users/me/Applications/Cider Bridge.app",
        ]))
        await service.setAuthorized(false)
        let denied = await call("ping")
        XCTAssertEqual(denied.data?["homekit_authorized"], false)
        XCTAssertEqual(denied.data?["homes"], 0)
    }

    func testPingOnADeveloperIDBuildSaysNoHomeKit() async throws {
        let router = CommandRouter(version: "test")
        await registerHomeCommands(router, service: FakeHomeKitService(homes: [], authorized: false), build: BridgeBuild(
            kind: .developerID, homekitEntitled: false, weatherkitEntitled: true, bundlePath: "/opt/homebrew/opt/cider/libexec/Cider Bridge.app"))
        let response = await router.dispatch(Request(id: 1, cmd: "ping"))
        XCTAssertEqual(response.data?["build"], "developer-id")
        XCTAssertEqual(response.data?["homekit_entitled"], false)
        XCTAssertEqual(response.data?["weatherkit_entitled"], true)
        XCTAssertEqual(response.data?["homekit_authorized"], false)
        XCTAssertEqual(response.data?["homes"], 0)
        XCTAssertEqual(response.data?["bundle_path"], "/opt/homebrew/opt/cider/libexec/Cider Bridge.app")
    }

    func testDeniedHomeKitIsHomekitDenied() async {
        await service.setAuthorized(false)
        await expectError("home.homes", code: "homekit_denied")
        await expectError("home.scenes", ["home": "Loft"], code: "homekit_denied")
    }

    // MARK: reads

    func testHomes() async throws {
        let homes = try await data([HomeRow].self, "home.homes")
        XCTAssertEqual(homes, [
            HomeRow(id: IDs.loft, name: "Loft", primary: true),
            HomeRow(id: IDs.cabin, name: "Cabin", primary: false),
        ])
        let raw = await call("home.homes").data
        XCTAssertEqual(raw?[0]?["id"], .string(IDs.loft.uuidString))
    }

    func testRoomsDefaultToPrimaryHomeAndResolveByNameOrUUID() async throws {
        let byDefault = try await data([RoomRow].self, "home.rooms")
        XCTAssertEqual(byDefault.map(\.name), ["Office", "Kitchen"])
        XCTAssertEqual(byDefault.first?.home, "Loft")

        let byName = try await data([RoomRow].self, "home.rooms", ["home": "loft"])
        XCTAssertEqual(byName, byDefault)
        let byUUID = try await data([RoomRow].self, "home.rooms", ["home": .string(IDs.loft.uuidString.lowercased())])
        XCTAssertEqual(byUUID, byDefault)

        let cabin = try await data([RoomRow].self, "home.rooms", ["home": "CABIN"])
        XCTAssertEqual(cabin, [])

        await expectError("home.rooms", ["home": "Barn"], code: "not_found", messageContains: "home 'Barn'")
        await expectError("home.rooms", ["home": 7], code: "invalid_args", messageContains: "string")
    }

    func testAccessoriesCarryLiveValuesAndFilterByRoom() async throws {
        let all = try await data([AccessoryRow].self, "home.accessories")
        XCTAssertEqual(all.map(\.name), ["Office Light", "Lamp", "Lamp", "Thermostat"])

        let light = all[0]
        XCTAssertEqual(light.room, "Office")
        XCTAssertEqual(light.manufacturer, "Philips")
        XCTAssertTrue(light.reachable)
        XCTAssertEqual(light.services.count, 1)
        XCTAssertEqual(light.services[0].type, "lightbulb")
        let power = light.services[0].characteristics[0]
        XCTAssertEqual(power.name, "power_state")
        XCTAssertEqual(power.value, .bool(true))
        XCTAssertTrue(power.writable)
        XCTAssertTrue(power.readable)
        let brightness = light.services[0].characteristics[1]
        XCTAssertEqual(brightness.name, "brightness")
        XCTAssertEqual(brightness.value, 80)
        XCTAssertEqual(brightness.unit, "%")

        let thermostat = all[3]
        XCTAssertFalse(thermostat.reachable)
        XCTAssertEqual(thermostat.services[0].type, "thermostat")
        XCTAssertEqual(thermostat.services[0].characteristics.map(\.name),
                       ["current_temperature", "target_temperature", "target_heating_cooling_state"])
        XCTAssertEqual(thermostat.services[0].characteristics[0].value, 21.5)
        XCTAssertEqual(thermostat.services[0].characteristics[0].unit, "°C")

        let kitchen = try await data([AccessoryRow].self, "home.accessories", ["room": "kitchen"])
        XCTAssertEqual(kitchen.map(\.name), ["Lamp", "Thermostat"])
        let office = try await data([AccessoryRow].self, "home.accessories", ["home": "Loft", "room": .string(IDs.office.uuidString)])
        XCTAssertEqual(office.map(\.id), [IDs.officeLight, IDs.officeLamp])

        await expectError("home.accessories", ["room": "Attic"], code: "not_found", messageContains: "room 'Attic'")

        // Wire shape: snake_case keys, type UUID + human name on characteristics.
        let raw = await call("home.accessories", ["room": "Office"]).data
        let characteristic = raw?[0]?["services"]?[0]?["characteristics"]?[0]
        XCTAssertEqual(characteristic?["type"], "00000025-0000-1000-8000-0026BB765291")
        XCTAssertEqual(characteristic?["name"], "power_state")
        XCTAssertEqual(raw?[0]?["services"]?[0]?["type_id"], "00000043-0000-1000-8000-0026BB765291")
    }

    func testScenes() async throws {
        let loft = try await data([SceneRow].self, "home.scenes")
        XCTAssertEqual(loft, [
            SceneRow(id: IDs.movie, name: "Movie", home: "Loft", kind: "user_defined", actions: 3),
            SceneRow(id: IDs.goodNight, name: "Good Night", home: "Loft", kind: "sleep", actions: 5),
        ])
        let cabin = try await data([SceneRow].self, "home.scenes", ["home": "Cabin"])
        XCTAssertEqual(cabin.map(\.name), ["Movie"])
    }

    func testTriggers() async throws {
        let triggers = try await data([TriggerRow].self, "home.triggers")
        XCTAssertEqual(triggers.map(\.name), ["Morning", "Hall motion"])
        XCTAssertEqual(triggers[0].kind, "timer")
        XCTAssertEqual(triggers[0].recurrence, .daily)
        XCTAssertEqual(triggers[0].fireDate, DateCoding.parse("2026-09-02T07:00:00-07:00"))
        XCTAssertEqual(triggers[0].scenes, ["Good Night"])
        XCTAssertEqual(triggers[1].kind, "event")
        XCTAssertFalse(triggers[1].enabled)
        XCTAssertNil(triggers[1].fireDate)

        let raw = await call("home.triggers").data
        XCTAssertEqual(raw?[0]?["recurrence"], "daily")
        XCTAssertEqual(raw?[0]?["fire_date"], .string(DateCoding.format(triggers[0].fireDate!)))
        XCTAssertNil(raw?[1]?["fire_date"])
        XCTAssertNil(raw?[1]?["last_fire"])
    }

    // MARK: resolution

    func testNameResolverAmbiguityAndDefaults() throws {
        let rows = [
            HomeRow(id: IDs.loft, name: "Loft", primary: false),
            HomeRow(id: IDs.cabin, name: "loft", primary: false),
        ]
        XCTAssertThrowsError(try NameResolver.resolve("LOFT", in: rows, kind: "home", id: \.id, name: \.name)) { error in
            XCTAssertEqual((error as? BridgeError)?.code, "invalid_args")
            XCTAssertTrue("\(error)".contains("ambiguous"))
        }
        // A UUID cuts through the ambiguity.
        XCTAssertEqual(try NameResolver.resolve(IDs.cabin.uuidString, in: rows, kind: "home", id: \.id, name: \.name).name, "loft")
        // No primary and several homes: the caller must choose.
        XCTAssertThrowsError(try NameResolver.resolveHome(nil, in: rows, id: \.id, name: \.name, isPrimary: \.primary)) { error in
            XCTAssertEqual((error as? BridgeError)?.code, "invalid_args")
        }
        XCTAssertEqual(try NameResolver.resolveHome(nil, in: [rows[1]], id: \.id, name: \.name, isPrimary: \.primary).name, "loft")
        XCTAssertThrowsError(try NameResolver.resolveHome(nil, in: [HomeRow](), id: \.id, name: \.name, isPrimary: \.primary)) { error in
            XCTAssertEqual((error as? BridgeError)?.code, "not_found")
        }
    }

    func testRecurrenceCoding() throws {
        XCTAssertEqual(Recurrence(dateComponents: DateComponents(day: 1)), .daily)
        XCTAssertEqual(Recurrence(dateComponents: DateComponents(weekOfYear: 1)), .weekly)
        XCTAssertEqual(Recurrence(dateComponents: DateComponents(minute: 30)), .minutes(30))
        XCTAssertEqual(Recurrence(dateComponents: DateComponents(hour: 2)), .minutes(120))
        XCTAssertNil(Recurrence(dateComponents: DateComponents(month: 1)))
        XCTAssertEqual(Recurrence.minutes(45).dateComponents, DateComponents(minute: 45))
        XCTAssertEqual(try Recurrence.parse("daily"), .daily)
        XCTAssertEqual(try Recurrence.parse("Weekly"), .weekly)
        XCTAssertEqual(try Recurrence.parse(["minutes": 15]), .minutes(15))
        XCTAssertNil(try Recurrence.parse(nil))
        XCTAssertNil(try Recurrence.parse(.null))
        XCTAssertThrowsError(try Recurrence.parse("hourly"))
        XCTAssertThrowsError(try Recurrence.parse(["minutes": 0]))
    }
}
