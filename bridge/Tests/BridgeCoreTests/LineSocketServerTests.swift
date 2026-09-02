import Foundation
import XCTest
import os
@testable import BridgeCore

final class LineSocketServerTests: XCTestCase {
    private var tempDir: URL?

    override func tearDown() {
        if let tempDir { try? FileManager.default.removeItem(at: tempDir) }
        super.tearDown()
    }

    private func makeServer(
        idleTimer: IdleTimer? = nil, peerPolicy: @escaping LineSocketServer.PeerPolicy = LineSocketServer.sameUserOnly
    ) async throws -> (LineSocketServer, String) {
        let (dir, path) = try temporarySocketPath()
        tempDir = dir
        let router = CommandRouter(version: "t")
        await router.register("home.scenes") { args in
            let home = Args(args).value("home")?.stringValue ?? "Primary"
            return [["id": "S1", "name": "Movie", "home": .string(home), "kind": "user_defined", "actions": 2]]
        }
        await router.register("slow") { _ in
            try await Task.sleep(for: .milliseconds(150))
            return "late"
        }
        return (LineSocketServer(path: path, router: router, idleTimer: idleTimer, peerPolicy: peerPolicy), path)
    }

    // MARK: peer uid

    func testSameUserPeerIsAcceptedAndSeenByThePolicy() async throws {
        let seen = OSAllocatedUnfairLock<[uid_t]>(initialState: [])
        let (server, path) = try await makeServer(peerPolicy: { uid in
            seen.withLock { $0.append(uid) }
            return LineSocketServer.sameUserOnly(uid)
        })
        try server.start()
        defer { server.stop() }

        let client = try LineSocketClient(path: path)
        XCTAssertEqual(try client.call(Request(id: 1, cmd: "ping")).ok, true)
        XCTAssertEqual(seen.withLock { $0 }, [getuid()])
        // The kernel reports our own uid for both ends of a local connection.
        XCTAssertEqual(LineSocketServer.peerUID(of: client.fd), getuid())
        XCTAssertNil(LineSocketServer.peerUID(of: -1))
    }

    func testRejectedPeerIsDroppedWithoutAReply() async throws {
        let (server, path) = try await makeServer(peerPolicy: { _ in false })
        try server.start()
        defer { server.stop() }

        // connect(2) completes in the kernel before accept, so the client sees
        // the drop as EOF on its first read rather than a refused connection.
        let rejected = try LineSocketClient(path: path)
        rejected.send(#"{"id": 1, "cmd": "ping"}"#)
        XCTAssertThrowsError(try rejected.readLine())

        // The server is still serving: a later peer gets the same policy answer,
        // and stop/start with the default policy admits us again.
        XCTAssertThrowsError(try LineSocketClient(path: path).call(Request(id: 2, cmd: "ping")))
        server.stop()
        let (open, openPath) = try await makeServer()
        try open.start()
        defer { open.stop() }
        XCTAssertEqual(try LineSocketClient(path: openPath).call(Request(id: 3, cmd: "ping")).ok, true)
    }

    func testPingAndFakeScenesEndToEnd() async throws {
        let (server, path) = try await makeServer()
        try server.start()
        defer { server.stop() }

        // Socket file is private to the user and the parent dir was created.
        let attrs = try FileManager.default.attributesOfItem(atPath: path)
        XCTAssertEqual((attrs[.posixPermissions] as? Int).map { $0 & 0o777 }, 0o600)

        let client = try LineSocketClient(path: path)
        client.send(#"{"id": 1, "cmd": "ping"}"#)
        XCTAssertEqual(try client.readLine(), #"{"data":{"homekit_authorized":false,"homes":0,"version":"t"},"id":1,"ok":true}"#)

        let scenes = try client.call(Request(id: 2, cmd: "home.scenes", args: ["home": "Loft"]))
        XCTAssertEqual(scenes, .success(id: 2, data: [["id": "S1", "name": "Movie", "home": "Loft", "kind": "user_defined", "actions": 2]]))

        let unknown = try client.call(Request(id: 3, cmd: "nope"))
        XCTAssertEqual(unknown.error?.code, "invalid_args")
    }

    func testMultipleClientsAndOrderedRepliesPerConnection() async throws {
        let (server, path) = try await makeServer()
        try server.start()
        defer { server.stop() }

        let a = try LineSocketClient(path: path)
        let b = try LineSocketClient(path: path)
        // Two requests pipelined on one connection come back in order even when
        // the first is slower; the second client is unaffected.
        a.send(#"{"id": 1, "cmd": "slow"}"#)
        a.send(#"{"id": 2, "cmd": "ping"}"#)
        let bReply = try b.call(Request(id: 9, cmd: "ping"))
        XCTAssertEqual(bReply.id, 9)
        XCTAssertEqual(try JSONLine.decode(Response.self, from: try a.readLine()).id, 1)
        XCTAssertEqual(try JSONLine.decode(Response.self, from: try a.readLine()).id, 2)
    }

    func testSplitAndMalformedLines() async throws {
        let (server, path) = try await makeServer()
        try server.start()
        defer { server.stop() }

        let client = try LineSocketClient(path: path)
        // A request delivered in two writes, followed by garbage and a blank line.
        client.sendRaw(Array(#"{"id": 5, "cm"#.utf8))
        client.sendRaw(Array(#"d": "ping"}"#.utf8) + [0x0A])
        XCTAssertEqual(try JSONLine.decode(Response.self, from: try client.readLine()).id, 5)

        client.send("")
        client.send(#"{"id": 6, "cmd": 12}"#)
        let bad = try JSONLine.decode(Response.self, from: try client.readLine())
        XCTAssertEqual(bad.id, 6)
        XCTAssertEqual(bad.error?.code, "invalid_args")

        client.send("garbage")
        let garbage = try JSONLine.decode(Response.self, from: try client.readLine())
        XCTAssertEqual(garbage.id, 0)
        XCTAssertEqual(garbage.error?.code, "invalid_args")
    }

    func testStaleSocketIsReplacedOnStart() async throws {
        let (server, path) = try await makeServer()
        FileManager.default.createFile(atPath: path, contents: Data("stale".utf8))
        try server.start()
        defer { server.stop() }
        XCTAssertEqual(try LineSocketClient(path: path).call(Request(id: 1, cmd: "ping")).ok, true)
    }

    func testStopUnlinksAndRestarts() async throws {
        let (server, path) = try await makeServer()
        try server.start()
        XCTAssertTrue(server.isRunning)
        server.stop()
        XCTAssertFalse(server.isRunning)
        XCTAssertFalse(FileManager.default.fileExists(atPath: path))
        XCTAssertThrowsError(try LineSocketClient(path: path))

        try server.start()
        defer { server.stop() }
        XCTAssertEqual(try LineSocketClient(path: path).call(Request(id: 1, cmd: "ping")).ok, true)
    }

    func testIdleTimerFiresAfterQuietPeriodAndRequestsResetIt() async throws {
        let idle = expectation(description: "idle fired")
        let fireCount = OSAllocatedUnfairLock(initialState: 0)
        let timer = IdleTimer(timeout: 0.3) {
            fireCount.withLock { $0 += 1 }
            idle.fulfill()
        }
        let (server, path) = try await makeServer(idleTimer: timer)
        try server.start()
        defer { server.stop() }

        let client = try LineSocketClient(path: path)
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertEqual(try client.call(Request(id: 1, cmd: "ping")).ok, true)
        try await Task.sleep(for: .milliseconds(200))
        // 400 ms elapsed but only 200 ms since the last request: not idle yet.
        XCTAssertEqual(fireCount.withLock { $0 }, 0)
        await fulfillment(of: [idle], timeout: 1)
        XCTAssertEqual(fireCount.withLock { $0 }, 1)
    }
}

final class IdleTimerTests: XCTestCase {
    func testFires() async {
        let fired = expectation(description: "fired")
        let timer = IdleTimer(timeout: 0.05) { fired.fulfill() }
        timer.start()
        await fulfillment(of: [fired], timeout: 1)
    }

    func testTouchDefersAndStopCancels() async throws {
        let fired = expectation(description: "fired")
        fired.isInverted = true
        let timer = IdleTimer(timeout: 0.15) { fired.fulfill() }
        timer.start()
        try await Task.sleep(for: .milliseconds(100))
        timer.touch()
        try await Task.sleep(for: .milliseconds(100))
        timer.stop()
        await fulfillment(of: [fired], timeout: 0.3)
    }
}
