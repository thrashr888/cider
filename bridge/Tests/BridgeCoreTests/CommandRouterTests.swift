import Foundation
import XCTest
@testable import BridgeCore

final class CommandRouterTests: XCTestCase {
    func testPingBuiltin() async throws {
        let router = CommandRouter(version: "9.9.9")
        let response = await router.dispatch(Request(id: 1, cmd: "ping"))
        XCTAssertEqual(response, .success(id: 1, data: ["version": "9.9.9", "homekit_authorized": false, "homes": 0]))
    }

    func testQuitCallsInjectedClosure() async throws {
        let quit = expectation(description: "quit called")
        let router = CommandRouter { quit.fulfill() }
        let response = await router.dispatch(Request(id: 3, cmd: "quit"))
        XCTAssertEqual(response, .success(id: 3, data: ["bye": true]))
        await fulfillment(of: [quit], timeout: 1)
    }

    func testUnknownCommandIsInvalidArgs() async {
        let router = CommandRouter()
        let response = await router.dispatch(Request(id: 5, cmd: "home.nope"))
        XCTAssertEqual(response.ok, false)
        XCTAssertEqual(response.error?.code, "invalid_args")
        XCTAssertTrue(response.error!.message.contains("home.nope"))
    }

    func testHandlerReceivesArgsAndReturnsData() async throws {
        let router = CommandRouter()
        await router.register("echo") { args in .object(args) }
        let response = await router.dispatch(Request(id: 8, cmd: "echo", args: ["a": 1]))
        XCTAssertEqual(response, .success(id: 8, data: ["a": 1]))
        let commands = await router.commands
        XCTAssertEqual(commands, ["echo", "ping", "quit"])
    }

    func testThrownBridgeErrorKeepsCode() async throws {
        let router = CommandRouter()
        await router.register("boom") { _ in throw BridgeError.homekitDenied("nope") }
        let response = await router.dispatch(Request(id: 9, cmd: "boom"))
        XCTAssertEqual(response, .failure(id: 9, error: .homekitDenied("nope")))
    }

    func testThrownForeignErrorIsInternal() async throws {
        struct Weird: Error {}
        let router = CommandRouter()
        await router.register("boom") { _ in throw Weird() }
        let response = await router.dispatch(Request(id: 10, cmd: "boom"))
        XCTAssertEqual(response.error?.code, "internal")
        XCTAssertTrue(response.error!.message.contains("Weird"))
    }

    func testLineDispatch() async throws {
        let router = CommandRouter(version: "1")
        let ok = await router.dispatch(line: #"{"id": 4, "cmd": "ping"}"#)
        XCTAssertEqual(ok, #"{"data":{"homekit_authorized":false,"homes":0,"version":"1"},"id":4,"ok":true}"#)

        let malformed = await router.dispatch(line: #"{"id": 11, "cmd": 5}"#)
        let response = try JSONLine.decode(Response.self, from: malformed!)
        XCTAssertEqual(response.id, 11)
        XCTAssertEqual(response.error?.code, "invalid_args")

        let garbage = await router.dispatch(line: "hello")
        XCTAssertEqual(try JSONLine.decode(Response.self, from: garbage!).id, 0)

        let blank = await router.dispatch(line: "   ")
        XCTAssertNil(blank)
    }
}
