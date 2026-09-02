#if canImport(HomeKit)
import Foundation
import HomeKit

/// The real `HomeKitService`, on `HMHomeManager`. Only compiles where HomeKit
/// exists (the Mac Catalyst app); SwiftPM on macOS skips this file's body.
///
/// Everything HomeKit hands out is a non-Sendable ObjC object whose delegate
/// callbacks arrive on the main queue, so the whole service lives on the main
/// actor. Callers await across the hop; nothing HomeKit-owned escapes.
@MainActor
public final class HMHomeKitService: NSObject, HomeKitService {
    private let manager: HMHomeManager
    private let loadLatch = Latch(count: 1)

    /// How long a `home.*` call waits for the first `homeManagerDidUpdateHomes`.
    public var loadTimeout: TimeInterval = 15
    /// Upper bound on the live characteristic read pass in `accessories`.
    public var readTimeout: TimeInterval = 6

    public override init() {
        manager = HMHomeManager()
        super.init()
        manager.delegate = self
    }

    // MARK: Readiness

    private func markLoaded() {
        loadLatch.signal()
    }

    /// Waits for HomeKit's first homes update, then checks authorization.
    private func authorizedHomes() async throws -> [HMHome] {
        guard await loadLatch.wait(timeout: loadTimeout) else {
            throw BridgeError.timeout("HomeKit did not report homes within \(Int(loadTimeout))s")
        }
        let status = manager.authorizationStatus
        if status.contains(.restricted) {
            throw BridgeError.homekitDenied(
                "HomeKit access is denied for Cider Bridge; allow it in System Settings > Privacy & Security > HomeKit")
        }
        guard status.contains(.authorized) else {
            throw BridgeError.homekitUnavailable("HomeKit authorization is not determined yet")
        }
        return manager.homes
    }

    // MARK: HomeKitService

    public func status() async -> HomeKitStatus {
        HomeKitStatus(authorized: manager.authorizationStatus.contains(.authorized), homes: manager.homes.count)
    }

    public func homes() async throws -> [HomeRow] {
        try await authorizedHomes().map(homeRow)
    }

    public func rooms(home: String?) async throws -> [RoomRow] {
        let home = try await resolveHome(home)
        return home.rooms.map { RoomRow(id: $0.uniqueIdentifier, name: $0.name, home: home.name) }
    }

    public func accessories(home: String?, room: String?) async throws -> [AccessoryRow] {
        let home = try await resolveHome(home)
        var accessories = home.accessories
        if let room {
            let resolved = try resolveRoom(room, in: home)
            accessories = accessories.filter { $0.room?.uniqueIdentifier == resolved.uniqueIdentifier }
        }
        await readLiveValues(of: accessories)
        return accessories.map(accessoryRow)
    }

    public func scenes(home: String?) async throws -> [SceneRow] {
        let home = try await resolveHome(home)
        return home.actionSets.map { sceneRow($0, home: home) }
    }

    public func triggers(home: String?) async throws -> [TriggerRow] {
        let home = try await resolveHome(home)
        return home.triggers.map { triggerRow($0, home: home) }
    }

    public func runScene(home: String?, scene: String) async throws {
        let home = try await resolveHome(home)
        let actionSet = try resolveActionSet(scene, in: home)
        try await mapping { try await home.executeActionSet(actionSet) }
    }

    public func set(home: String?, accessory: String, service: String?, characteristic: String,
                    value: JSONValue) async throws -> SetResult {
        let home = try await resolveHome(home)
        let target = try resolveAccessory(accessory, in: home)
        let match = try CharacteristicResolver.resolve(
            characteristic, service: service,
            services: target.services.filter { !HAPTypes.isAccessoryInformation(serviceType: $0.serviceType) },
            serviceID: \.uniqueIdentifier, serviceName: \.name, serviceType: \.serviceType,
            characteristics: \.characteristics, characteristicID: \.uniqueIdentifier,
            characteristicType: \.characteristicType)
        let hm = match.characteristic
        let name = HAPTypes.characteristicName(forType: hm.characteristicType)
        guard hm.properties.contains(HMCharacteristicPropertyWritable) else {
            throw BridgeError.invalidArgs("characteristic '\(name)' is read-only")
        }
        let native = try Self.nativeValue(value, for: hm)
        try await mapping { try await hm.writeValue(native) }
        let written = Self.jsonValue(of: hm)
        return SetResult(accessory: target.name, characteristic: name, value: written.isNull ? value : written)
    }

    public func createTimerTrigger(home: String?, name: String, fireAt: Date, recurrence: Recurrence?,
                                   scenes: [String]) async throws -> TriggerRow {
        let home = try await resolveHome(home)
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else {
            throw BridgeError.invalidArgs("'name' is required")
        }
        guard !scenes.isEmpty else { throw BridgeError.invalidArgs("'scenes' must name at least one scene") }
        let actionSets = try scenes.map { try resolveActionSet($0, in: home) }
        // HomeKit keeps timer fire dates at minute precision and rejects the past.
        let fireDate = Date(timeIntervalSinceReferenceDate: (fireAt.timeIntervalSinceReferenceDate / 60).rounded(.down) * 60)
        guard fireDate > Date() else {
            throw BridgeError.invalidArgs("'fire_at' must be in the future (got \(DateCoding.format(fireAt)))")
        }

        let trigger = HMTimerTrigger(name: name, fireDate: fireDate, recurrence: recurrence?.dateComponents)
        try await mapping { try await home.addTrigger(trigger) }
        do {
            for actionSet in actionSets {
                try await mapping { try await trigger.addActionSet(actionSet) }
            }
            try await mapping { try await trigger.enable(true) }
        } catch {
            // Do not leave a half-built automation behind.
            try? await home.removeTrigger(trigger)
            throw error
        }
        return triggerRow(trigger, home: home)
    }

    public func setTriggerEnabled(home: String?, trigger: String, enabled: Bool) async throws -> TriggerRow {
        let home = try await resolveHome(home)
        let hm = try resolveTrigger(trigger, in: home)
        try await mapping { try await hm.enable(enabled) }
        return triggerRow(hm, home: home)
    }

    public func deleteTrigger(home: String?, trigger: String) async throws {
        let home = try await resolveHome(home)
        let hm = try resolveTrigger(trigger, in: home)
        try await mapping { try await home.removeTrigger(hm) }
    }

    // MARK: Errors

    /// Runs a HomeKit call and rewrites its `HMError` into the RFC's codes.
    private func mapping<T>(_ body: () async throws -> T) async throws -> T {
        do {
            return try await body()
        } catch let error as BridgeError {
            throw error
        } catch {
            throw Self.bridgeError(from: error)
        }
    }

    static func bridgeError(from error: Error) -> BridgeError {
        let ns = error as NSError
        let message = "HomeKit: \(ns.localizedDescription)"
        guard ns.domain == HMErrorDomain, let code = HMError.Code(rawValue: ns.code) else {
            return .internalError(message)
        }
        switch code {
        case .notFound:
            return .notFound(message)
        case .invalidParameter, .invalidValueType, .readOnlyCharacteristic, .writeOnlyCharacteristic, .fireDateInPast:
            return .invalidArgs(message)
        case .homeAccessNotAuthorized:
            return .homekitDenied(message)
        case .accessoryNotReachable, .communicationFailure:
            return .homekitUnavailable(message)
        case .operationTimedOut:
            return .timeout(message)
        default:
            return .internalError(message)
        }
    }

    /// Converts a wire value to what `writeValue` expects, using the
    /// characteristic's metadata format.
    static func nativeValue(_ value: JSONValue, for characteristic: HMCharacteristic) throws -> Any {
        let name = HAPTypes.characteristicName(forType: characteristic.characteristicType)
        let format = characteristic.metadata?.format
        func bad(_ expected: String) -> BridgeError {
            .invalidArgs("'\(name)' expects \(expected), got \(value)")
        }
        switch format {
        case HMCharacteristicMetadataFormatBool:
            if let b = value.boolValue { return b }
            if let n = value.doubleValue { return n != 0 }
            if let s = value.stringValue?.lowercased() {
                if ["true", "on", "yes", "1"].contains(s) { return true }
                if ["false", "off", "no", "0"].contains(s) { return false }
            }
            throw bad("a boolean")
        case HMCharacteristicMetadataFormatInt, HMCharacteristicMetadataFormatUInt8, HMCharacteristicMetadataFormatUInt16,
             HMCharacteristicMetadataFormatUInt32, HMCharacteristicMetadataFormatUInt64:
            if let i = value.intValue { return i }
            if let b = value.boolValue { return b ? 1 : 0 }
            if let s = value.stringValue, let i = Int(s) { return i }
            throw bad("an integer")
        case HMCharacteristicMetadataFormatFloat:
            if let d = value.doubleValue { return d }
            if let s = value.stringValue, let d = Double(s) { return d }
            throw bad("a number")
        case HMCharacteristicMetadataFormatString:
            if let s = value.stringValue { return s }
            if let d = value.doubleValue { return d.rounded() == d ? String(Int(d)) : String(d) }
            if let b = value.boolValue { return b ? "true" : "false" }
            throw bad("a string")
        default:
            switch value {
            case .bool(let b): return b
            case .number(let n): return n.rounded() == n ? Int(n) : n
            case .string(let s): return s
            default: throw bad("a scalar")
            }
        }
    }

    // MARK: Resolution

    func resolveHome(_ query: String?) async throws -> HMHome {
        try NameResolver.resolveHome(
            query, in: try await authorizedHomes(), id: \.uniqueIdentifier, name: \.name, isPrimary: \.isPrimary)
    }

    func resolveRoom(_ query: String, in home: HMHome) throws -> HMRoom {
        try NameResolver.resolve(query, in: home.rooms, kind: "room", id: \.uniqueIdentifier, name: \.name)
    }

    func resolveActionSet(_ query: String, in home: HMHome) throws -> HMActionSet {
        try NameResolver.resolve(query, in: home.actionSets, kind: "scene", id: \.uniqueIdentifier, name: \.name)
    }

    func resolveTrigger(_ query: String, in home: HMHome) throws -> HMTrigger {
        try NameResolver.resolve(query, in: home.triggers, kind: "trigger", id: \.uniqueIdentifier, name: \.name)
    }

    func resolveAccessory(_ query: String, in home: HMHome) throws -> HMAccessory {
        try NameResolver.resolve(query, in: home.accessories, kind: "accessory", id: \.uniqueIdentifier, name: \.name)
    }

    // MARK: Live values

    /// Issues `readValue` for every readable characteristic of every reachable
    /// accessory at once and waits for all of them, bounded by `readTimeout`.
    /// Failures keep the cached value; HomeKit updates `characteristic.value`
    /// as reads complete. Completion-handler form rather than task groups: the
    /// HomeKit objects are non-Sendable and must not leave the main actor.
    private func readLiveValues(of accessories: [HMAccessory]) async {
        let characteristics = accessories
            .filter(\.isReachable)
            .flatMap(\.services)
            .filter { !HAPTypes.isAccessoryInformation(serviceType: $0.serviceType) }
            .flatMap(\.characteristics)
            .filter { $0.properties.contains(HMCharacteristicPropertyReadable) }
        guard !characteristics.isEmpty else { return }

        let latch = Latch(count: characteristics.count)
        Self.startReads(characteristics, signaling: latch)
        _ = await latch.wait(timeout: readTimeout)
    }

    /// Synchronous on purpose: the completion-handler form is the intended API
    /// here, and calling it from an async context only draws a warning.
    private static func startReads(_ characteristics: [HMCharacteristic], signaling latch: Latch) {
        for characteristic in characteristics {
            characteristic.readValue { _ in
                Task { @MainActor in latch.signal() }
            }
        }
    }

    // MARK: Rows

    private func homeRow(_ home: HMHome) -> HomeRow {
        HomeRow(id: home.uniqueIdentifier, name: home.name, primary: home.isPrimary)
    }

    func accessoryRow(_ accessory: HMAccessory) -> AccessoryRow {
        AccessoryRow(
            id: accessory.uniqueIdentifier, name: accessory.name, room: accessory.room?.name,
            manufacturer: accessory.manufacturer, model: accessory.model, reachable: accessory.isReachable,
            services: accessory.services
                .filter { !HAPTypes.isAccessoryInformation(serviceType: $0.serviceType) }
                .map(serviceRow))
    }

    private func serviceRow(_ service: HMService) -> ServiceRow {
        ServiceRow(
            id: service.uniqueIdentifier, name: service.name, typeID: service.serviceType,
            characteristics: service.characteristics.map(characteristicRow))
    }

    func characteristicRow(_ characteristic: HMCharacteristic) -> CharacteristicRow {
        let type = characteristic.characteristicType
        return CharacteristicRow.make(
            id: characteristic.uniqueIdentifier, type: type,
            value: Self.jsonValue(of: characteristic),
            writable: characteristic.properties.contains(HMCharacteristicPropertyWritable),
            readable: characteristic.properties.contains(HMCharacteristicPropertyReadable),
            unit: HAPTypes.characteristicUnit(forType: type) ?? Self.unitName(characteristic.metadata?.units))
    }

    private func sceneRow(_ actionSet: HMActionSet, home: HMHome) -> SceneRow {
        SceneRow(
            id: actionSet.uniqueIdentifier, name: actionSet.name, home: home.name,
            kind: Self.sceneKind(actionSet.actionSetType), actions: actionSet.actions.count)
    }

    func triggerRow(_ trigger: HMTrigger, home: HMHome) -> TriggerRow {
        let timer = trigger as? HMTimerTrigger
        // `lastFireDate` is deprecated ("no longer supported") since iOS 17, so
        // `last_fire` is always null from the real service.
        return TriggerRow(
            id: trigger.uniqueIdentifier, name: trigger.name, home: home.name,
            kind: timer == nil ? "event" : "timer", enabled: trigger.isEnabled,
            fireDate: timer?.fireDate,
            recurrence: timer?.recurrence.flatMap(Recurrence.init(dateComponents:)),
            scenes: trigger.actionSets.map(\.name), lastFire: nil)
    }

    // MARK: Value mapping

    static func jsonValue(of characteristic: HMCharacteristic) -> JSONValue {
        guard let value = characteristic.value else { return .null }
        switch value {
        case let number as NSNumber:
            let format = characteristic.metadata?.format
            if format == HMCharacteristicMetadataFormatBool || number.isBooleanNumber {
                return .bool(number.boolValue)
            }
            return .number(number.doubleValue)
        case let string as String:
            return .string(string)
        case let data as Data:
            return .string(data.base64EncodedString())
        default:
            return .string(String(describing: value))
        }
    }

    static func unitName(_ units: String?) -> String? {
        switch units {
        case HMCharacteristicMetadataUnitsCelsius: "°C"
        case HMCharacteristicMetadataUnitsFahrenheit: "°F"
        case HMCharacteristicMetadataUnitsPercentage: "%"
        case HMCharacteristicMetadataUnitsArcDegree: "°"
        case HMCharacteristicMetadataUnitsSeconds: "s"
        case HMCharacteristicMetadataUnitsLux: "lux"
        case HMCharacteristicMetadataUnitsPartsPerMillion: "ppm"
        case HMCharacteristicMetadataUnitsMicrogramsPerCubicMeter: "µg/m³"
        default: nil
        }
    }

    static func sceneKind(_ type: String) -> String {
        switch type {
        case HMActionSetTypeWakeUp: "wake_up"
        case HMActionSetTypeSleep: "sleep"
        case HMActionSetTypeHomeDeparture: "home_departure"
        case HMActionSetTypeHomeArrival: "home_arrival"
        case HMActionSetTypeUserDefined: "user_defined"
        case HMActionSetTypeTriggerOwned: "trigger_owned"
        default: type
        }
    }
}

extension HMHomeKitService: HMHomeManagerDelegate {
    // HomeKit delivers delegate callbacks on the main queue; the protocol
    // requirements are nonisolated, so hop back onto the main actor explicitly.
    nonisolated public func homeManagerDidUpdateHomes(_ manager: HMHomeManager) {
        MainActor.assumeIsolated { markLoaded() }
    }

    nonisolated public func homeManager(_ manager: HMHomeManager, didUpdate status: HMHomeManagerAuthorizationStatus) {
        // A denied/restricted answer never produces a homes update; unblock waiters.
        if status.contains(.restricted) {
            MainActor.assumeIsolated { markLoaded() }
        }
    }
}

extension NSNumber {
    /// True for `@YES`/`@NO`-style boxed booleans (CFBoolean), not for 0/1 ints.
    var isBooleanNumber: Bool {
        CFGetTypeID(self) == CFBooleanGetTypeID()
    }
}

/// Main-actor countdown latch with a timeout: `wait` returns `true` once
/// `signal()` has been called `count` times, `false` on timeout. Resumes at
/// most once, so late signals after a timeout are harmless.
@MainActor
final class Latch {
    private var remaining: Int
    private var continuation: CheckedContinuation<Bool, Never>?

    init(count: Int) { remaining = count }

    var isOpen: Bool { remaining <= 0 }

    func signal() {
        remaining -= 1
        if remaining <= 0 { resume(true) }
    }

    func wait(timeout: TimeInterval) async -> Bool {
        if isOpen { return true }
        return await withCheckedContinuation { continuation in
            self.continuation = continuation
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(timeout))
                self.resume(false)
            }
        }
    }

    private func resume(_ value: Bool) {
        continuation?.resume(returning: value)
        continuation = nil
    }
}
#endif
