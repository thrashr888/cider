import Foundation

/// Unix-domain socket server speaking the newline-delimited JSON envelope.
///
/// One request line in, one response line out, per connection, in order.
/// Multiple clients are served concurrently. All mutable state is confined to
/// the serial `queue` (accept/read events are `DispatchSource`s targeting it),
/// which is why the class is `@unchecked Sendable`.
///
/// Trust model: the bridge is single-user. The socket is 0600 in a 0700
/// directory, and every accepted connection is additionally checked with
/// `getpeereid`: a peer whose uid is not ours is dropped before a byte is
/// read (defense in depth against a permissive umask or a moved socket).
public final class LineSocketServer: @unchecked Sendable {
    /// Decides whether a connecting peer (by effective uid) may talk to the
    /// bridge. The default accepts only the server's own uid.
    public typealias PeerPolicy = @Sendable (uid_t) -> Bool

    public static let sameUserOnly: PeerPolicy = { $0 == getuid() }

    public enum SocketError: Error, CustomStringConvertible {
        case pathTooLong(String)
        case posix(String, Int32)

        public var description: String {
            switch self {
            case .pathTooLong(let p): "socket path exceeds sockaddr_un limit: \(p)"
            case .posix(let call, let code): "\(call) failed: \(String(cString: strerror(code))) (\(code))"
            }
        }
    }

    /// The RFC path: `$HOME/Library/Application Support/cider/bridge.sock`.
    public static var defaultPath: String {
        // NSHomeDirectory() rather than homeDirectoryForCurrentUser: the latter
        // is unavailable on Mac Catalyst, and the app is not sandboxed.
        (NSHomeDirectory() as NSString).appendingPathComponent("Library/Application Support/cider/bridge.sock")
    }

    public let path: String
    private let router: CommandRouter
    private let idleTimer: IdleTimer?
    private let peerPolicy: PeerPolicy
    private let queue = DispatchQueue(label: "dev.thrasher.cider.bridge.socket")

    // Confined to `queue`.
    private var listenFD: Int32 = -1
    private var listenSource: DispatchSourceRead?
    private var connections: [UUID: Connection] = [:]

    public init(
        path: String = LineSocketServer.defaultPath, router: CommandRouter, idleTimer: IdleTimer? = nil,
        peerPolicy: @escaping PeerPolicy = LineSocketServer.sameUserOnly
    ) {
        self.path = path
        self.router = router
        self.idleTimer = idleTimer
        self.peerPolicy = peerPolicy
    }

    public var isRunning: Bool { queue.sync { listenFD >= 0 } }

    /// Creates the parent directory, unlinks a stale socket, binds, chmods the
    /// socket to 0600, and begins accepting. Starts the idle timer if any.
    public func start() throws {
        try queue.sync { try startOnQueue() }
    }

    /// Stops accepting, drops every client, and unlinks the socket path.
    public func stop() {
        queue.sync { stopOnQueue() }
    }

    private func startOnQueue() throws {
        guard listenFD < 0 else { return }

        let directory = (path as NSString).deletingLastPathComponent
        try FileManager.default.createDirectory(
            atPath: directory, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        unlink(path)

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw SocketError.posix("socket", errno) }

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8CString)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard pathBytes.count <= capacity else {
            close(fd)
            throw SocketError.pathTooLong(path)
        }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            for (i, byte) in pathBytes.enumerated() { raw[i] = UInt8(bitPattern: byte) }
        }
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)

        let bound = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0 else {
            let code = errno
            close(fd)
            throw SocketError.posix("bind", code)
        }
        chmod(path, 0o600)
        guard listen(fd, 16) == 0 else {
            let code = errno
            close(fd)
            unlink(path)
            throw SocketError.posix("listen", code)
        }
        _ = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK)

        let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        source.setEventHandler { [weak self] in self?.acceptPending() }
        source.setCancelHandler { close(fd) }
        source.resume()

        listenFD = fd
        listenSource = source
        idleTimer?.start()
    }

    private func stopOnQueue() {
        idleTimer?.stop()
        listenSource?.cancel()
        listenSource = nil
        if listenFD >= 0 {
            listenFD = -1
            unlink(path)
        }
        for connection in connections.values { connection.close() }
        connections.removeAll()
    }

    private func acceptPending() {
        while listenFD >= 0 {
            let fd = accept(listenFD, nil, nil)
            if fd < 0 {
                if errno == EINTR { continue }
                return  // EAGAIN: drained
            }
            let peer = Self.peerUID(of: fd)
            guard let uid = peer, peerPolicy(uid) else {
                Self.log("rejected connection from uid \(peer.map(String.init) ?? "unknown") (server uid \(getuid()))")
                close(fd)
                continue
            }
            // Accepted sockets inherit O_NONBLOCK on BSD; reads are readiness-driven
            // and writes are small, so blocking mode is what we want.
            _ = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) & ~O_NONBLOCK)
            var one: Int32 = 1
            setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))

            let id = UUID()
            let connection = Connection(fd: fd, queue: queue)
            connections[id] = connection
            connection.start(
                handle: { [router, idleTimer] line in
                    idleTimer?.touch()
                    return await router.dispatch(line: line)
                },
                onClose: { [weak self] in
                    guard let self else { return }
                    self.queue.async { self.connections[id] = nil }
                })
        }
    }

    /// The effective uid of the process at the other end of a Unix socket, or
    /// `nil` when the kernel will not say (treated as untrusted).
    static func peerUID(of fd: Int32) -> uid_t? {
        var uid: uid_t = 0
        var gid: gid_t = 0
        guard getpeereid(fd, &uid, &gid) == 0 else { return nil }
        return uid
    }

    private static func log(_ message: String) {
        FileHandle.standardError.write(Data("cider-bridge: \(message)\n".utf8))
    }
}

/// One client. Reads happen on the server queue via a `DispatchSourceRead`;
/// complete lines are fed to a single `Task` that dispatches them in order and
/// writes replies. The task owns the fd and closes it once the stream ends.
private final class Connection: @unchecked Sendable {
    private let fd: Int32
    private let source: DispatchSourceRead
    private let stream: AsyncStream<String>
    private let continuation: AsyncStream<String>.Continuation
    private var buffer: [UInt8] = []  // confined to the server queue
    private var task: Task<Void, Never>?

    init(fd: Int32, queue: DispatchQueue) {
        self.fd = fd
        source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        (stream, continuation) = AsyncStream.makeStream(of: String.self)
    }

    func start(
        handle: @escaping @Sendable (String) async -> String?,
        onClose: @escaping @Sendable () -> Void
    ) {
        source.setEventHandler { [weak self] in self?.readAvailable() }
        source.setCancelHandler { [continuation] in continuation.finish() }
        source.resume()
        task = Task { [fd, stream] in
            for await line in stream {
                if let reply = await handle(line) {
                    Connection.write(reply + "\n", to: fd)
                }
            }
            shutdown(fd, SHUT_RDWR)
            Darwin.close(fd)
            onClose()
        }
    }

    func close() {
        source.cancel()
    }

    private func readAvailable() {
        var chunk = [UInt8](repeating: 0, count: 65536)
        let count = read(fd, &chunk, chunk.count)
        guard count > 0 else {
            source.cancel()  // EOF or error: finish the stream, task closes the fd
            return
        }
        buffer.append(contentsOf: chunk[0..<count])
        while let newline = buffer.firstIndex(of: 0x0A) {
            let lineBytes = buffer[buffer.startIndex..<newline]
            buffer.removeSubrange(buffer.startIndex...newline)
            continuation.yield(String(decoding: lineBytes, as: UTF8.self))
        }
    }

    private static func write(_ string: String, to fd: Int32) {
        var bytes = Array(string.utf8)
        var offset = 0
        while offset < bytes.count {
            let written = bytes.withUnsafeMutableBytes { raw in
                Foundation.write(fd, raw.baseAddress! + offset, raw.count - offset)
            }
            if written < 0 {
                if errno == EINTR { continue }
                return  // peer gone; SO_NOSIGPIPE keeps us alive
            }
            offset += written
        }
    }
}
