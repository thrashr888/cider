import Foundation
import XCTest
@testable import BridgeCore

final class EnvelopeTests: XCTestCase {
    func testRequestRoundTrip() throws {
        let request = Request(id: 7, cmd: "home.scenes", args: ["home": "2183 26th Ave", "n": 3, "flag": true])
        let line = try JSONLine.encode(request)
        XCTAssertFalse(line.contains("\n"))
        XCTAssertEqual(line, #"{"args":{"flag":true,"home":"2183 26th Ave","n":3},"cmd":"home.scenes","id":7}"#)
        XCTAssertEqual(try JSONLine.decode(Request.self, from: line), request)
    }

    func testRequestWithoutArgsDecodes() throws {
        let request = try JSONLine.decode(Request.self, from: #"{"id": 1, "cmd": "ping"}"#)
        XCTAssertEqual(request, Request(id: 1, cmd: "ping"))
    }

    func testSuccessResponseRoundTrip() throws {
        let response = Response.success(id: 1, data: ["ran": true, "list": [1, 2.5, "x", nil]])
        let line = try JSONLine.encode(response)
        XCTAssertEqual(line, #"{"data":{"list":[1,2.5,"x",null],"ran":true},"id":1,"ok":true}"#)
        XCTAssertEqual(try JSONLine.decode(Response.self, from: line), response)
    }

    func testErrorResponseRoundTrip() throws {
        let response = Response.failure(id: 2, error: .notFound("scene 'Movie' not found"))
        let line = try JSONLine.encode(response)
        XCTAssertEqual(line, #"{"error":{"code":"not_found","message":"scene 'Movie' not found"},"id":2,"ok":false}"#)
        let decoded = try JSONLine.decode(Response.self, from: line)
        XCTAssertEqual(decoded, response)
        XCTAssertNil(decoded.data)
        XCTAssertEqual(BridgeError(body: decoded.error!), .notFound("scene 'Movie' not found"))
    }

    func testErrorCodes() {
        let cases: [(BridgeError, String)] = [
            (.notFound("a"), "not_found"), (.invalidArgs("a"), "invalid_args"),
            (.homekitDenied("a"), "homekit_denied"), (.homekitUnavailable("a"), "homekit_unavailable"),
            (.permissionDenied("a"), "permission_denied"),
            (.timeout("a"), "timeout"), (.internalError("a"), "internal"),
        ]
        for (error, code) in cases {
            XCTAssertEqual(error.code, code)
            XCTAssertEqual(BridgeError(body: error.body), error)
        }
        XCTAssertEqual(BridgeError(body: ErrorBody(code: "weird", message: "m")), .internalError("m"))
    }

    func testJSONValueDistinguishesBoolFromNumber() throws {
        let value = try JSONLine.decode(JSONValue.self, from: #"{"b":true,"one":1,"f":1.5,"s":"1","z":null}"#)
        XCTAssertEqual(value["b"], .bool(true))
        XCTAssertEqual(value["one"], .number(1))
        XCTAssertEqual(value["one"]?.intValue, 1)
        XCTAssertNil(value["one"]?.boolValue)
        XCTAssertEqual(value["f"], .number(1.5))
        XCTAssertNil(value["f"]?.intValue)
        XCTAssertEqual(value["s"], .string("1"))
        XCTAssertEqual(value["z"], .null)
        XCTAssertNil(value["missing"])
    }

    func testJSONValueFromEncodable() throws {
        struct Row: Encodable { let id: Int; let name: String; let when: String? }
        let value = try JSONValue(encoding: [Row(id: 1, name: "a", when: nil)])
        XCTAssertEqual(value, [["id": 1, "name": "a"]])
    }

    func testPeekID() {
        XCTAssertEqual(Request.peekID(in: #"{"id": 42, "cmd": 5}"#), 42)
        XCTAssertNil(Request.peekID(in: "not json"))
    }

    func testArgsTyping() async throws {
        let args = Args(["home": "Loft", "on": true, "scenes": ["A", "B"], "single": "S", "fire_at": "2026-09-01T07:30:00-07:00", "n": 2])
        XCTAssertEqual(try args.requiredString("home"), "Loft")
        XCTAssertNil(try args.string("missing"))
        XCTAssertEqual(try args.requiredBool("on"), true)
        XCTAssertEqual(try args.bool("n"), true)
        XCTAssertEqual(try args.requiredStringArray("scenes"), ["A", "B"])
        XCTAssertEqual(try args.stringArray("single"), ["S"])
        XCTAssertEqual(try args.requiredDate("fire_at").timeIntervalSince1970, 1_788_273_000)
        await XCTAssertBridgeError(try args.requiredString("missing"), code: "invalid_args", messageContains: "required")
        await XCTAssertBridgeError(try args.string("on"), code: "invalid_args", messageContains: "string")
        await XCTAssertBridgeError(try args.stringArray("n"), code: "invalid_args")
        await XCTAssertBridgeError(try Args(["fire_at": "tomorrow"]).date("fire_at"), code: "invalid_args", messageContains: "RFC 3339")
    }

    func testDateCoding() throws {
        let tz = TimeZone(identifier: "America/Los_Angeles")!
        let date = DateCoding.parse("2026-09-01T07:30:00-07:00")!
        XCTAssertEqual(DateCoding.format(date, timeZone: tz), "2026-09-01T07:30:00-07:00")
        XCTAssertEqual(DateCoding.parse("2026-09-01T14:30:00Z"), date)
        XCTAssertEqual(DateCoding.parse("2026-09-01T14:30:00.250Z")!.timeIntervalSince1970, date.timeIntervalSince1970 + 0.25, accuracy: 0.001)
        XCTAssertEqual(DateCoding.parse("2026-09-01T07:30", timeZone: tz), date)
        XCTAssertEqual(DateCoding.parse("2026-09-01 07:30:00", timeZone: tz), date)
        XCTAssertNil(DateCoding.parse("soon"))
    }
}
