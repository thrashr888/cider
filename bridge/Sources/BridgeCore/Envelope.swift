import Foundation

/// Bridge build info shared by the app, the CLI, and `ping`.
public enum BridgeInfo {
    public static let version = "0.1.0"
}

// MARK: - JSONValue

/// A minimal JSON model: enough to carry request args and response data
/// through the newline-delimited envelope without third-party dependencies.
public enum JSONValue: Equatable, Hashable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])
}

extension JSONValue: Codable {
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let bool = try? container.decode(Bool.self) {
            self = .bool(bool)
        } else if let number = try? container.decode(Double.self) {
            self = .number(number)
        } else if let string = try? container.decode(String.self) {
            self = .string(string)
        } else if let array = try? container.decode([JSONValue].self) {
            self = .array(array)
        } else if let object = try? container.decode([String: JSONValue].self) {
            self = .object(object)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "unsupported JSON value")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let bool): try container.encode(bool)
        case .number(let number):
            // Integral doubles print as integers (`50`, not `50.0`).
            if number.rounded() == number, abs(number) < 1e15 {
                try container.encode(Int64(number))
            } else {
                try container.encode(number)
            }
        case .string(let string): try container.encode(string)
        case .array(let array): try container.encode(array)
        case .object(let object): try container.encode(object)
        }
    }
}

extension JSONValue: ExpressibleByNilLiteral, ExpressibleByBooleanLiteral, ExpressibleByIntegerLiteral,
    ExpressibleByFloatLiteral, ExpressibleByStringLiteral, ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral
{
    public init(nilLiteral: ()) { self = .null }
    public init(booleanLiteral value: Bool) { self = .bool(value) }
    public init(integerLiteral value: Int) { self = .number(Double(value)) }
    public init(floatLiteral value: Double) { self = .number(value) }
    public init(stringLiteral value: String) { self = .string(value) }
    public init(arrayLiteral elements: JSONValue...) { self = .array(elements) }
    public init(dictionaryLiteral elements: (String, JSONValue)...) {
        self = .object(Dictionary(elements, uniquingKeysWith: { _, last in last }))
    }
}

extension JSONValue {
    public init(_ int: Int) { self = .number(Double(int)) }
    public init(_ double: Double) { self = .number(double) }
    public init(_ string: String) { self = .string(string) }
    public init(_ bool: Bool) { self = .bool(bool) }
    public init(_ string: String?) { self = string.map { .string($0) } ?? .null }

    /// Converts any `Encodable` (typically a row struct) into a `JSONValue`.
    public init(encoding value: some Encodable) throws {
        let data = try JSONEncoder().encode(value)
        self = try JSONDecoder().decode(JSONValue.self, from: data)
    }

    public var isNull: Bool { if case .null = self { return true } else { return false } }
    public var boolValue: Bool? { if case .bool(let v) = self { return v } else { return nil } }
    public var doubleValue: Double? { if case .number(let v) = self { return v } else { return nil } }
    public var intValue: Int? {
        guard case .number(let v) = self, v.rounded() == v, abs(v) < Double(Int.max) else { return nil }
        return Int(v)
    }
    public var stringValue: String? { if case .string(let v) = self { return v } else { return nil } }
    public var arrayValue: [JSONValue]? { if case .array(let v) = self { return v } else { return nil } }
    public var objectValue: [String: JSONValue]? { if case .object(let v) = self { return v } else { return nil } }

    public subscript(key: String) -> JSONValue? { objectValue?[key] }
    public subscript(index: Int) -> JSONValue? {
        guard let array = arrayValue, array.indices.contains(index) else { return nil }
        return array[index]
    }
}

// MARK: - Line coding

/// Single-line JSON encoding used for both directions of the socket protocol.
public enum JSONLine {
    public static func encode(_ value: some Encodable) throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(value)
        guard let line = String(data: data, encoding: .utf8) else {
            throw BridgeError.internalError("response is not UTF-8")
        }
        return line
    }

    public static func decode<T: Decodable>(_ type: T.Type, from line: String) throws -> T {
        try JSONDecoder().decode(type, from: Data(line.utf8))
    }
}

// MARK: - Request

/// `{"id": 1, "cmd": "home.scenes", "args": {"home": "2183 26th Ave"}}`
public struct Request: Codable, Equatable, Sendable {
    public var id: Int
    public var cmd: String
    public var args: [String: JSONValue]

    public init(id: Int, cmd: String, args: [String: JSONValue] = [:]) {
        self.id = id
        self.cmd = cmd
        self.args = args
    }

    private enum CodingKeys: String, CodingKey { case id, cmd, args }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(Int.self, forKey: .id)
        cmd = try container.decode(String.self, forKey: .cmd)
        args = try container.decodeIfPresent([String: JSONValue].self, forKey: .args) ?? [:]
    }

    /// Best-effort extraction of `id` from a line that failed to decode as a
    /// `Request`, so the error reply can still be correlated.
    public static func peekID(in line: String) -> Int? {
        struct IDOnly: Decodable { var id: Int? }
        return (try? JSONLine.decode(IDOnly.self, from: line))?.id
    }
}

// MARK: - Response

public struct ErrorBody: Codable, Equatable, Sendable {
    public var code: String
    public var message: String

    public init(code: String, message: String) {
        self.code = code
        self.message = message
    }
}

/// `{"id": 1, "ok": true, "data": ...}` or
/// `{"id": 1, "ok": false, "error": {"code": "not_found", "message": "..."}}`
public struct Response: Codable, Equatable, Sendable {
    public var id: Int
    public var ok: Bool
    public var data: JSONValue?
    public var error: ErrorBody?

    public static func success(id: Int, data: JSONValue) -> Response {
        Response(id: id, ok: true, data: data, error: nil)
    }

    public static func failure(id: Int, error: BridgeError) -> Response {
        Response(id: id, ok: false, data: nil, error: error.body)
    }

    public init(id: Int, ok: Bool, data: JSONValue?, error: ErrorBody?) {
        self.id = id
        self.ok = ok
        self.data = data
        self.error = error
    }

    private enum CodingKeys: String, CodingKey { case id, ok, data, error }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(Int.self, forKey: .id)
        ok = try container.decode(Bool.self, forKey: .ok)
        data = try container.decodeIfPresent(JSONValue.self, forKey: .data)
        error = try container.decodeIfPresent(ErrorBody.self, forKey: .error)
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(ok, forKey: .ok)
        if ok {
            try container.encode(data ?? .null, forKey: .data)
        } else {
            try container.encode(error ?? ErrorBody(code: "internal", message: "unknown error"), forKey: .error)
        }
    }
}
