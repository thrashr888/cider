import Foundation

// MARK: - Rows (the RFC's `data` shapes)

public struct HomeKitStatus: Equatable, Sendable {
    public var authorized: Bool
    public var homes: Int
    public init(authorized: Bool, homes: Int) {
        self.authorized = authorized
        self.homes = homes
    }
}

public struct HomeRow: Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var primary: Bool
    public init(id: UUID, name: String, primary: Bool) {
        self.id = id
        self.name = name
        self.primary = primary
    }
}

public struct RoomRow: Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var home: String
    public init(id: UUID, name: String, home: String) {
        self.id = id
        self.name = name
        self.home = home
    }
}

public struct CharacteristicRow: Codable, Equatable, Sendable {
    public var id: UUID
    /// HomeKit type UUID.
    public var type: String
    /// Human name from `HAPTypes`, or the UUID for vendor types.
    public var name: String
    public var value: JSONValue
    public var unit: String?
    public var writable: Bool
    public var readable: Bool

    public init(id: UUID, type: String, name: String, value: JSONValue, unit: String?, writable: Bool, readable: Bool) {
        self.id = id
        self.type = type
        self.name = name
        self.value = value
        self.unit = unit
        self.writable = writable
        self.readable = readable
    }

    /// Builds a row with `name`/`unit` looked up from `HAPTypes`.
    public static func make(id: UUID = UUID(), type: String, value: JSONValue, writable: Bool, readable: Bool = true,
                            unit: String? = nil) -> CharacteristicRow {
        CharacteristicRow(
            id: id, type: type, name: HAPTypes.characteristicName(forType: type), value: value,
            unit: unit ?? HAPTypes.characteristicUnit(forType: type), writable: writable, readable: readable)
    }
}

public struct ServiceRow: Codable, Equatable, Sendable {
    public var id: UUID
    /// User-visible service name (HomeKit's `HMService.name`).
    public var name: String
    /// Human service type (`lightbulb`), or the UUID for vendor types.
    public var type: String
    /// HomeKit type UUID.
    public var typeID: String
    public var characteristics: [CharacteristicRow]

    private enum CodingKeys: String, CodingKey { case id, name, type, typeID = "type_id", characteristics }

    public init(id: UUID, name: String, typeID: String, characteristics: [CharacteristicRow]) {
        self.id = id
        self.name = name
        self.type = HAPTypes.serviceName(forType: typeID)
        self.typeID = typeID
        self.characteristics = characteristics
    }
}

public struct AccessoryRow: Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var room: String?
    public var manufacturer: String?
    public var model: String?
    public var reachable: Bool
    public var services: [ServiceRow]

    public init(id: UUID, name: String, room: String?, manufacturer: String?, model: String?, reachable: Bool,
                services: [ServiceRow]) {
        self.id = id
        self.name = name
        self.room = room
        self.manufacturer = manufacturer
        self.model = model
        self.reachable = reachable
        self.services = services
    }
}

public struct SceneRow: Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var home: String
    /// `user_defined`, `wake_up`, `sleep`, `home_departure`, `home_arrival`, `trigger_owned`.
    public var kind: String
    public var actions: Int

    public init(id: UUID, name: String, home: String, kind: String, actions: Int) {
        self.id = id
        self.name = name
        self.home = home
        self.kind = kind
        self.actions = actions
    }
}

/// `"daily"`, `"weekly"`, or `{"minutes": n}` on the wire; `DateComponents` for HomeKit.
public enum Recurrence: Equatable, Sendable {
    case daily
    case weekly
    case minutes(Int)

    public var jsonValue: JSONValue {
        switch self {
        case .daily: "daily"
        case .weekly: "weekly"
        case .minutes(let n): ["minutes": .number(Double(n))]
        }
    }

    public var dateComponents: DateComponents {
        switch self {
        case .daily: DateComponents(day: 1)
        case .weekly: DateComponents(weekOfYear: 1)
        case .minutes(let n): DateComponents(minute: n)
        }
    }

    /// Recognizes the three shapes HomeKit uses for timer recurrence; anything
    /// else (hours, months, combinations) is `nil`.
    public init?(dateComponents c: DateComponents) {
        let day = c.day ?? 0, week = c.weekOfYear ?? 0, minute = c.minute ?? 0
        let hour = c.hour ?? 0
        if day == 1, week == 0, minute == 0, hour == 0 {
            self = .daily
        } else if week == 1, day == 0, minute == 0, hour == 0 {
            self = .weekly
        } else if day == 0, week == 0, minute + hour * 60 > 0 {
            self = .minutes(minute + hour * 60)
        } else {
            return nil
        }
    }

    public init?(jsonValue: JSONValue) {
        switch jsonValue {
        case .string(let s) where s.lowercased() == "daily": self = .daily
        case .string(let s) where s.lowercased() == "weekly": self = .weekly
        case .object(let o):
            guard let minutes = o["minutes"]?.intValue, minutes > 0 else { return nil }
            self = .minutes(minutes)
        default: return nil
        }
    }

    /// Parses an optional `recurrence` arg, throwing `invalid_args` on nonsense.
    public static func parse(_ value: JSONValue?) throws -> Recurrence? {
        guard let value, !value.isNull else { return nil }
        guard let recurrence = Recurrence(jsonValue: value) else {
            throw BridgeError.invalidArgs("'recurrence' must be \"daily\", \"weekly\", or {\"minutes\": n}")
        }
        return recurrence
    }
}

extension Recurrence: Codable {
    public init(from decoder: Decoder) throws {
        let value = try JSONValue(from: decoder)
        guard let recurrence = Recurrence(jsonValue: value) else {
            throw DecodingError.dataCorruptedError(
                in: try decoder.singleValueContainer(), debugDescription: "unrecognized recurrence")
        }
        self = recurrence
    }

    public func encode(to encoder: Encoder) throws {
        try jsonValue.encode(to: encoder)
    }
}

public struct TriggerRow: Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var home: String
    /// `timer` or `event`.
    public var kind: String
    public var enabled: Bool
    public var fireDate: Date?
    public var recurrence: Recurrence?
    /// Names of the action sets (scenes) the trigger runs.
    public var scenes: [String]
    public var lastFire: Date?

    public init(id: UUID, name: String, home: String, kind: String, enabled: Bool, fireDate: Date? = nil,
                recurrence: Recurrence? = nil, scenes: [String], lastFire: Date? = nil) {
        self.id = id
        self.name = name
        self.home = home
        self.kind = kind
        self.enabled = enabled
        self.fireDate = fireDate
        self.recurrence = recurrence
        self.scenes = scenes
        self.lastFire = lastFire
    }

    private enum CodingKeys: String, CodingKey {
        case id, name, home, kind, enabled, scenes, recurrence
        case fireDate = "fire_date"
        case lastFire = "last_fire"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        home = try c.decode(String.self, forKey: .home)
        kind = try c.decode(String.self, forKey: .kind)
        enabled = try c.decode(Bool.self, forKey: .enabled)
        scenes = try c.decode([String].self, forKey: .scenes)
        recurrence = try c.decodeIfPresent(Recurrence.self, forKey: .recurrence)
        fireDate = try c.decodeIfPresent(String.self, forKey: .fireDate).flatMap { DateCoding.parse($0) }
        lastFire = try c.decodeIfPresent(String.self, forKey: .lastFire).flatMap { DateCoding.parse($0) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(name, forKey: .name)
        try c.encode(home, forKey: .home)
        try c.encode(kind, forKey: .kind)
        try c.encode(enabled, forKey: .enabled)
        try c.encode(scenes, forKey: .scenes)
        try c.encodeIfPresent(recurrence, forKey: .recurrence)
        try c.encodeIfPresent(fireDate.map { DateCoding.format($0) }, forKey: .fireDate)
        try c.encodeIfPresent(lastFire.map { DateCoding.format($0) }, forKey: .lastFire)
    }
}

// MARK: - Service protocol

/// The `home.*` command surface from the RFC table. Every name argument is a
/// name or UUID (names case-insensitive; ambiguity is `invalid_args`); a nil
/// `home` means the primary home (or the only home).
public protocol HomeKitService: Sendable {
    /// Cheap, never blocks on HomeKit: feeds `ping`.
    func status() async -> HomeKitStatus
    func homes() async throws -> [HomeRow]
    func rooms(home: String?) async throws -> [RoomRow]
    /// Accessories with live characteristic values.
    func accessories(home: String?, room: String?) async throws -> [AccessoryRow]
    func scenes(home: String?) async throws -> [SceneRow]
    func triggers(home: String?) async throws -> [TriggerRow]
}

// MARK: - Name / UUID resolution

public enum NameResolver {
    /// UUID match first (exact), then case-insensitive name. Zero matches is
    /// `not_found`; several is `invalid_args`.
    public static func resolve<T>(_ query: String, in items: [T], kind: String,
                                  id: (T) -> UUID, name: (T) -> String) throws -> T {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        if let uuid = UUID(uuidString: trimmed), let hit = items.first(where: { id($0) == uuid }) {
            return hit
        }
        let matches = items.filter { name($0).caseInsensitiveCompare(trimmed) == .orderedSame }
        switch matches.count {
        case 0:
            throw BridgeError.notFound("\(kind) '\(query)' not found")
        case 1:
            return matches[0]
        default:
            let ids = matches.map { id($0).uuidString }.joined(separator: ", ")
            throw BridgeError.invalidArgs("\(kind) '\(query)' is ambiguous (\(matches.count) matches); use a UUID: \(ids)")
        }
    }

    /// nil query -> the primary home, else the only home; otherwise resolve by name/UUID.
    public static func resolveHome<T>(_ query: String?, in homes: [T],
                                      id: (T) -> UUID, name: (T) -> String, isPrimary: (T) -> Bool) throws -> T {
        guard let query, !query.trimmingCharacters(in: .whitespaces).isEmpty else {
            if let primary = homes.first(where: isPrimary) { return primary }
            if homes.count == 1 { return homes[0] }
            if homes.isEmpty { throw BridgeError.notFound("no HomeKit homes") }
            throw BridgeError.invalidArgs("no primary home; pass 'home' (one of: \(homes.map(name).joined(separator: ", ")))")
        }
        return try resolve(query, in: homes, kind: "home", id: id, name: name)
    }
}
