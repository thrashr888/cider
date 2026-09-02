import Foundation

/// The RFC's error codes. Anything else thrown by a handler becomes `internal`.
public enum BridgeError: Error, Equatable, Sendable {
    case notFound(String)
    case invalidArgs(String)
    case homekitDenied(String)
    case homekitUnavailable(String)
    case timeout(String)
    case internalError(String)

    public var code: String {
        switch self {
        case .notFound: "not_found"
        case .invalidArgs: "invalid_args"
        case .homekitDenied: "homekit_denied"
        case .homekitUnavailable: "homekit_unavailable"
        case .timeout: "timeout"
        case .internalError: "internal"
        }
    }

    public var message: String {
        switch self {
        case .notFound(let m), .invalidArgs(let m), .homekitDenied(let m),
             .homekitUnavailable(let m), .timeout(let m), .internalError(let m):
            m
        }
    }

    public var body: ErrorBody { ErrorBody(code: code, message: message) }

    /// Rebuilds a `BridgeError` from a wire `ErrorBody`; unknown codes map to `internal`.
    public init(body: ErrorBody) {
        switch body.code {
        case "not_found": self = .notFound(body.message)
        case "invalid_args": self = .invalidArgs(body.message)
        case "homekit_denied": self = .homekitDenied(body.message)
        case "homekit_unavailable": self = .homekitUnavailable(body.message)
        case "timeout": self = .timeout(body.message)
        default: self = .internalError(body.message)
        }
    }
}

extension BridgeError: CustomStringConvertible, LocalizedError {
    public var description: String { "\(code): \(message)" }
    public var errorDescription: String? { description }
}

// MARK: - Args

/// Typed access to a request's `args`, throwing `invalid_args` on the wrong shape.
public struct Args: Sendable {
    public let raw: [String: JSONValue]

    public init(_ raw: [String: JSONValue]) { self.raw = raw }

    public func value(_ key: String) -> JSONValue? {
        guard let v = raw[key], !v.isNull else { return nil }
        return v
    }

    public func string(_ key: String) throws -> String? {
        guard let v = value(key) else { return nil }
        guard let s = v.stringValue else { throw BridgeError.invalidArgs("'\(key)' must be a string") }
        return s
    }

    public func requiredString(_ key: String) throws -> String {
        guard let s = try string(key), !s.isEmpty else { throw BridgeError.invalidArgs("'\(key)' is required") }
        return s
    }

    public func bool(_ key: String) throws -> Bool? {
        guard let v = value(key) else { return nil }
        if let b = v.boolValue { return b }
        if let s = v.stringValue?.lowercased() {
            if ["true", "yes", "on", "1"].contains(s) { return true }
            if ["false", "no", "off", "0"].contains(s) { return false }
        }
        if let n = v.doubleValue { return n != 0 }
        throw BridgeError.invalidArgs("'\(key)' must be a boolean")
    }

    public func requiredBool(_ key: String) throws -> Bool {
        guard let b = try bool(key) else { throw BridgeError.invalidArgs("'\(key)' is required") }
        return b
    }

    public func stringArray(_ key: String) throws -> [String]? {
        guard let v = value(key) else { return nil }
        if let s = v.stringValue { return [s] }
        guard let array = v.arrayValue else { throw BridgeError.invalidArgs("'\(key)' must be an array of strings") }
        return try array.map { item in
            guard let s = item.stringValue else { throw BridgeError.invalidArgs("'\(key)' must be an array of strings") }
            return s
        }
    }

    public func requiredStringArray(_ key: String) throws -> [String] {
        guard let a = try stringArray(key), !a.isEmpty else { throw BridgeError.invalidArgs("'\(key)' is required") }
        return a
    }

    public func requiredValue(_ key: String) throws -> JSONValue {
        guard let v = raw[key] else { throw BridgeError.invalidArgs("'\(key)' is required") }
        return v
    }

    public func date(_ key: String) throws -> Date? {
        guard let s = try string(key) else { return nil }
        guard let d = DateCoding.parse(s) else {
            throw BridgeError.invalidArgs("'\(key)' must be an RFC 3339 date, got '\(s)'")
        }
        return d
    }

    public func requiredDate(_ key: String) throws -> Date {
        guard let d = try date(key) else { throw BridgeError.invalidArgs("'\(key)' is required") }
        return d
    }
}

// MARK: - Dates

/// RFC 3339 in the local time zone, the RFC's date format for the wire.
public enum DateCoding {
    public static func format(_ date: Date, timeZone: TimeZone = .current) -> String {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        f.timeZone = timeZone
        return f.string(from: date)
    }

    /// Accepts RFC 3339 with or without fractional seconds, and a bare local
    /// `yyyy-MM-dd'T'HH:mm[:ss]` (no offset) interpreted in the local time zone.
    public static func parse(_ string: String, timeZone: TimeZone = .current) -> Date? {
        let trimmed = string.trimmingCharacters(in: .whitespaces)
        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = iso.date(from: trimmed) { return d }
        iso.formatOptions = [.withInternetDateTime]
        if let d = iso.date(from: trimmed) { return d }

        let local = DateFormatter()
        local.locale = Locale(identifier: "en_US_POSIX")
        local.timeZone = timeZone
        for format in ["yyyy-MM-dd'T'HH:mm:ss", "yyyy-MM-dd'T'HH:mm", "yyyy-MM-dd HH:mm:ss", "yyyy-MM-dd HH:mm"] {
            local.dateFormat = format
            if let d = local.date(from: trimmed) { return d }
        }
        return nil
    }
}
