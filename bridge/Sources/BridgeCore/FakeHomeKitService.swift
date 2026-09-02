import Foundation

/// In-memory `HomeKitService` for tests and for running the bridge without
/// HomeKit. Rows are the model; mutations edit them in place.
public actor FakeHomeKitService: HomeKitService {
    public struct Home: Sendable {
        public var row: HomeRow
        public var rooms: [RoomRow]
        public var accessories: [AccessoryRow]
        public var scenes: [SceneRow]
        public var triggers: [TriggerRow]

        public init(row: HomeRow, rooms: [RoomRow] = [], accessories: [AccessoryRow] = [],
                    scenes: [SceneRow] = [], triggers: [TriggerRow] = []) {
            self.row = row
            self.rooms = rooms
            self.accessories = accessories
            self.scenes = scenes
            self.triggers = triggers
        }
    }

    public var authorized: Bool
    public private(set) var homesData: [Home]

    public init(homes: [Home], authorized: Bool = true) {
        self.homesData = homes
        self.authorized = authorized
    }

    public func setAuthorized(_ value: Bool) { authorized = value }

    // MARK: HomeKitService

    public func status() async -> HomeKitStatus {
        HomeKitStatus(authorized: authorized, homes: authorized ? homesData.count : 0)
    }

    public func homes() async throws -> [HomeRow] {
        try checkAuthorized()
        return homesData.map(\.row)
    }

    public func rooms(home: String?) async throws -> [RoomRow] {
        try resolveHome(home).rooms
    }

    public func accessories(home: String?, room: String?) async throws -> [AccessoryRow] {
        let home = try resolveHome(home)
        guard let room else { return home.accessories }
        let resolved = try NameResolver.resolve(room, in: home.rooms, kind: "room", id: \.id, name: \.name)
        return home.accessories.filter { $0.room == resolved.name }
    }

    public func scenes(home: String?) async throws -> [SceneRow] {
        try resolveHome(home).scenes
    }

    public func triggers(home: String?) async throws -> [TriggerRow] {
        try resolveHome(home).triggers
    }

    // MARK: Internals

    private func checkAuthorized() throws {
        guard authorized else { throw BridgeError.homekitDenied("HomeKit access denied") }
    }

    private func homeIndex(_ query: String?) throws -> Int {
        try checkAuthorized()
        let resolved = try NameResolver.resolveHome(
            query, in: homesData, id: \.row.id, name: \.row.name, isPrimary: \.row.primary)
        return homesData.firstIndex { $0.row.id == resolved.row.id }!
    }

    private func resolveHome(_ query: String?) throws -> Home {
        homesData[try homeIndex(query)]
    }
}

// MARK: - Sample fixture

extension FakeHomeKitService {
    public enum SampleIDs {
        public static let loft = UUID(uuidString: "11111111-0000-4000-8000-000000000001")!
        public static let cabin = UUID(uuidString: "11111111-0000-4000-8000-000000000002")!
        public static let office = UUID(uuidString: "22222222-0000-4000-8000-000000000001")!
        public static let kitchen = UUID(uuidString: "22222222-0000-4000-8000-000000000002")!
        public static let officeLight = UUID(uuidString: "33333333-0000-4000-8000-000000000001")!
        public static let officeLamp = UUID(uuidString: "33333333-0000-4000-8000-000000000002")!
        public static let kitchenLamp = UUID(uuidString: "33333333-0000-4000-8000-000000000003")!
        public static let thermostat = UUID(uuidString: "33333333-0000-4000-8000-000000000004")!
        public static let officeLightPower = UUID(uuidString: "44444444-0000-4000-8000-000000000001")!
        public static let officeLightBrightness = UUID(uuidString: "44444444-0000-4000-8000-000000000002")!
        public static let movie = UUID(uuidString: "55555555-0000-4000-8000-000000000001")!
        public static let goodNight = UUID(uuidString: "55555555-0000-4000-8000-000000000002")!
        public static let cabinMovie = UUID(uuidString: "55555555-0000-4000-8000-000000000003")!
        public static let morningTrigger = UUID(uuidString: "66666666-0000-4000-8000-000000000001")!
        public static let motionTrigger = UUID(uuidString: "66666666-0000-4000-8000-000000000002")!
    }

    /// Two homes. "Loft" (primary) has two rooms, a lightbulb, two accessories
    /// both named "Lamp" (for ambiguity tests), a thermostat, two scenes and two
    /// triggers. "Cabin" has one scene.
    public static func sample() -> FakeHomeKitService {
        let ids = SampleIDs.self
        let lightbulb = HAPTypes.fullUUID(shortID: 0x43)
        let thermostat = HAPTypes.fullUUID(shortID: 0x4A)
        let power = HAPTypes.fullUUID(shortID: 0x25)
        let brightness = HAPTypes.fullUUID(shortID: 0x08)

        func lamp(id: UUID, name: String, room: String, on: Bool) -> AccessoryRow {
            AccessoryRow(
                id: id, name: name, room: room, manufacturer: "Acme", model: "L1", reachable: true,
                services: [
                    ServiceRow(id: UUID(), name: name, typeID: lightbulb, characteristics: [
                        .make(type: power, value: .bool(on), writable: true),
                    ]),
                ])
        }

        let loft = Home(
            row: HomeRow(id: ids.loft, name: "Loft", primary: true),
            rooms: [
                RoomRow(id: ids.office, name: "Office", home: "Loft"),
                RoomRow(id: ids.kitchen, name: "Kitchen", home: "Loft"),
            ],
            accessories: [
                AccessoryRow(
                    id: ids.officeLight, name: "Office Light", room: "Office", manufacturer: "Philips", model: "LCA001",
                    reachable: true,
                    services: [
                        ServiceRow(id: UUID(), name: "Office Light", typeID: lightbulb, characteristics: [
                            .make(id: ids.officeLightPower, type: power, value: .bool(true), writable: true),
                            .make(id: ids.officeLightBrightness, type: brightness, value: 80, writable: true),
                        ]),
                    ]),
                lamp(id: ids.officeLamp, name: "Lamp", room: "Office", on: false),
                lamp(id: ids.kitchenLamp, name: "Lamp", room: "Kitchen", on: true),
                AccessoryRow(
                    id: ids.thermostat, name: "Thermostat", room: "Kitchen", manufacturer: "ecobee", model: "EB3",
                    reachable: false,
                    services: [
                        ServiceRow(id: UUID(), name: "Thermostat", typeID: thermostat, characteristics: [
                            .make(type: HAPTypes.fullUUID(shortID: 0x11), value: 21.5, writable: false),
                            .make(type: HAPTypes.fullUUID(shortID: 0x35), value: 20, writable: true),
                            .make(type: HAPTypes.fullUUID(shortID: 0x33), value: 1, writable: true),
                        ]),
                    ]),
            ],
            scenes: [
                SceneRow(id: ids.movie, name: "Movie", home: "Loft", kind: "user_defined", actions: 3),
                SceneRow(id: ids.goodNight, name: "Good Night", home: "Loft", kind: "sleep", actions: 5),
            ],
            triggers: [
                TriggerRow(
                    id: ids.morningTrigger, name: "Morning", home: "Loft", kind: "timer", enabled: true,
                    fireDate: DateCoding.parse("2026-09-02T07:00:00-07:00"), recurrence: .daily, scenes: ["Good Night"]),
                TriggerRow(id: ids.motionTrigger, name: "Hall motion", home: "Loft", kind: "event", enabled: false,
                           scenes: ["Movie"]),
            ])

        let cabin = Home(
            row: HomeRow(id: ids.cabin, name: "Cabin", primary: false),
            scenes: [SceneRow(id: ids.cabinMovie, name: "Movie", home: "Cabin", kind: "user_defined", actions: 1)])

        return FakeHomeKitService(homes: [loft, cabin])
    }
}
