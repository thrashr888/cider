import Foundation
import XCTest
@testable import BridgeCore

final class CLITests: XCTestCase {
    private func parse(_ args: [String], stdin: String = "") throws -> CLIInvocation {
        try CLIInvocation.parse(args) { stdin }
    }

    func testJSONArgument() throws {
        let invocation = try parse(["calendar.list", #"{"since":"2026-08-01T00:00:00Z","limit":2}"#])
        XCTAssertEqual(invocation, CLIInvocation(cmd: "calendar.list", args: ["since": "2026-08-01T00:00:00Z", "limit": 2]))
    }

    func testCommandWithoutArgs() throws {
        XCTAssertEqual(try parse(["ping"]), CLIInvocation(cmd: "ping"))
        XCTAssertEqual(try parse(["ping", ""]), CLIInvocation(cmd: "ping"))
        XCTAssertEqual(try parse(["ping", "  \n"]), CLIInvocation(cmd: "ping"))
    }

    func testDashReadsStdin() throws {
        var stdinReads = 0
        let invocation = try CLIInvocation.parse(["reminders.list", "-"]) {
            stdinReads += 1
            return "{\"list\": \"Todo\"}\n"
        }
        XCTAssertEqual(invocation, CLIInvocation(cmd: "reminders.list", args: ["list": "Todo"]))
        XCTAssertEqual(stdinReads, 1)

        // Empty stdin is no args, and stdin is untouched without `-`.
        XCTAssertEqual(try parse(["reminders.list", "-"], stdin: ""), CLIInvocation(cmd: "reminders.list"))
        _ = try CLIInvocation.parse(["ping", "{}"]) {
            XCTFail("stdin read without '-'")
            return ""
        }
    }

    func testParseErrors() async {
        await XCTAssertBridgeError(try parse([]), code: "invalid_args", messageContains: "missing command")
        await XCTAssertBridgeError(try parse(["a", "{}", "extra"]), code: "invalid_args", messageContains: "too many")
        await XCTAssertBridgeError(try parse(["a", "[1,2]"]), code: "invalid_args", messageContains: "JSON object")
        await XCTAssertBridgeError(try parse(["a", "{nope"]), code: "invalid_args", messageContains: "JSON object")
        await XCTAssertBridgeError(try parse(["a", "-"], stdin: "not json"), code: "invalid_args")
    }

    func testHelp() throws {
        XCTAssertTrue(try parse(["--help"]).isHelp)
        XCTAssertTrue(try parse(["help"]).isHelp)
        XCTAssertFalse(try parse(["ping"]).isHelp)
    }

    func testWireLine() {
        XCTAssertEqual(Response.success(id: 0, data: ["deleted": true]).wireLine, #"{"data":{"deleted":true},"id":0,"ok":true}"#)
        XCTAssertEqual(
            Response.failure(id: 0, error: .permissionDenied("no")).wireLine,
            #"{"error":{"code":"permission_denied","message":"no"},"id":0,"ok":false}"#)
    }

    // MARK: watch

    func testWatchRequestParsing() async throws {
        XCTAssertEqual(try WatchRequest.parse(Args([:])), WatchRequest(sources: [.calendar, .reminders, .contacts]))
        XCTAssertEqual(try WatchRequest.parse(Args(["sources": ["Reminders", "calendar", "reminders"], "once": true])),
                       WatchRequest(sources: [.reminders, .calendar], once: true))
        XCTAssertEqual(try WatchRequest.parse(Args(["sources": "contacts"])), WatchRequest(sources: [.contacts]))
        await XCTAssertBridgeError(try WatchRequest.parse(Args(["sources": ["mail"]])), code: "invalid_args", messageContains: "mail")
        await XCTAssertBridgeError(try WatchRequest.parse(Args(["sources": []])), code: "invalid_args", messageContains: "at least one")
        await XCTAssertBridgeError(try WatchRequest.parse(Args(["sources": 3])), code: "invalid_args")
    }

    func testWatchEventLine() throws {
        let at = DateCoding.parse("2026-09-02T12:00:00-07:00")!
        let event = WatchEvent(source: .calendar, at: at)
        let line = event.wireLine
        XCTAssertEqual(line, #"{"data":{"at":"\#(DateCoding.format(at))","kind":"store_changed","source":"calendar"},"id":0,"ok":true}"#)
        let decoded = try JSONLine.decode(Response.self, from: line)
        XCTAssertEqual(decoded.data?["source"], "calendar")
    }

    func testCoalescerFoldsBurstsPerSource() async throws {
        let box = EventBox()
        let coalescer = WatchCoalescer(window: .milliseconds(60)) { event in Task { await box.append(event) } }
        let first = Date()
        for _ in 0..<5 { await coalescer.note(.calendar, at: first) }
        await coalescer.note(.contacts)
        await coalescer.note(.calendar, at: first.addingTimeInterval(1))

        try await Task.sleep(for: .milliseconds(150))
        var events = await box.events
        XCTAssertEqual(events.map(\.source).sorted { $0.rawValue < $1.rawValue }, [.calendar, .contacts])
        // The burst is stamped with its first notification.
        XCTAssertEqual(events.first { $0.source == .calendar }?.at, first)
        XCTAssertEqual(events.first?.kind, "store_changed")

        // A later change, after the window closed, is a new event.
        await coalescer.note(.calendar)
        try await Task.sleep(for: .milliseconds(150))
        events = await box.events
        XCTAssertEqual(events.filter { $0.source == .calendar }.count, 2)
        XCTAssertEqual(events.filter { $0.source == .contacts }.count, 1)
    }

    func testCoalescerCancelDropsPending() async throws {
        let box = EventBox()
        let coalescer = WatchCoalescer(window: .milliseconds(40)) { event in Task { await box.append(event) } }
        await coalescer.note(.reminders)
        await coalescer.cancel()
        try await Task.sleep(for: .milliseconds(120))
        let events = await box.events
        XCTAssertEqual(events, [])
    }
}

private actor EventBox {
    var events: [WatchEvent] = []
    func append(_ event: WatchEvent) { events.append(event) }
}
