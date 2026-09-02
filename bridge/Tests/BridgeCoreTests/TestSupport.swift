import Foundation
import XCTest
@testable import BridgeCore

/// Blocking Unix-socket client for end-to-end tests: one line out, one line in.
final class LineSocketClient {
    private let fd: Int32
    private var pending: [UInt8] = []

    init(path: String) throws {
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw LineSocketServer.SocketError.posix("socket", errno) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8CString)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            for (i, byte) in bytes.enumerated() { raw[i] = UInt8(bitPattern: byte) }
        }
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else {
            let code = errno
            close(fd)
            throw LineSocketServer.SocketError.posix("connect", code)
        }
        var timeout = timeval(tv_sec: 5, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
    }

    deinit { close(fd) }

    func send(_ line: String) {
        let bytes = Array((line + "\n").utf8)
        var offset = 0
        while offset < bytes.count {
            let n = bytes.withUnsafeBytes { raw in write(fd, raw.baseAddress! + offset, raw.count - offset) }
            guard n > 0 else { return }
            offset += n
        }
    }

    func sendRaw(_ bytes: [UInt8]) {
        _ = bytes.withUnsafeBytes { raw in write(fd, raw.baseAddress!, raw.count) }
    }

    func readLine() throws -> String {
        while true {
            if let newline = pending.firstIndex(of: 0x0A) {
                let line = String(decoding: pending[..<newline], as: UTF8.self)
                pending.removeSubrange(...newline)
                return line
            }
            var chunk = [UInt8](repeating: 0, count: 65536)
            let n = read(fd, &chunk, chunk.count)
            guard n > 0 else { throw LineSocketServer.SocketError.posix("read", n == 0 ? 0 : errno) }
            pending.append(contentsOf: chunk[0..<n])
        }
    }

    func call(_ request: Request) throws -> Response {
        send(try JSONLine.encode(request))
        return try JSONLine.decode(Response.self, from: try readLine())
    }
}

/// A short socket path (sockaddr_un caps at 104 bytes) under a fresh temp dir.
func temporarySocketPath() throws -> (dir: URL, path: String) {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("cb-\(UUID().uuidString.prefix(8))", isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return (dir, dir.appendingPathComponent("b.sock").path)
}

func XCTAssertBridgeError(
    _ expression: @autoclosure () async throws -> some Any,
    code: String,
    messageContains: String? = nil,
    file: StaticString = #filePath,
    line: UInt = #line
) async {
    do {
        _ = try await expression()
        XCTFail("expected \(code) error", file: file, line: line)
    } catch let error as BridgeError {
        XCTAssertEqual(error.code, code, "unexpected code for \(error)", file: file, line: line)
        if let fragment = messageContains {
            XCTAssertTrue(error.message.localizedCaseInsensitiveContains(fragment),
                          "'\(error.message)' does not mention '\(fragment)'", file: file, line: line)
        }
    } catch {
        XCTFail("expected BridgeError \(code), got \(error)", file: file, line: line)
    }
}
