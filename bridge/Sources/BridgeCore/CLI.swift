import Foundation

// The native CLI's protocol pieces, kept here so they are testable without
// running the executable: argument parsing, envelope printing, `watch`
// arguments and change coalescing. `Sources/cider-bridge/main.swift` wires
// these to the real stores.

// MARK: - Invocation

/// `cider-bridge <cmd> [json-args | -]`: one JSON object argument, or `-` to
/// read it from stdin.
public struct CLIInvocation: Equatable, Sendable {
    public static let usage = """
        usage: cider-bridge <cmd> [json-args | -]

        Prints one JSON envelope line on stdout ({"id":0,"ok":true,"data":...} or
        {"id":0,"ok":false,"error":{"code":...,"message":...}}); exit 0 on ok, 1 on error.
        Commands: ping, calendar.{calendars,list,get,create,update,delete},
        reminders.{lists,list,create,update,complete,reopen,delete},
        contacts.{list,get}, watch {"sources":["calendar","reminders","contacts"],"once":false}
        """

    public var cmd: String
    public var args: [String: JSONValue]

    public init(cmd: String, args: [String: JSONValue] = [:]) {
        self.cmd = cmd
        self.args = args
    }

    public var isHelp: Bool { ["help", "-h", "--help"].contains(cmd) }

    /// - Parameter arguments: the process arguments without the executable path.
    /// - Parameter readStdin: called only when the args argument is `-`.
    public static func parse(_ arguments: [String], readStdin: () -> String) throws -> CLIInvocation {
        guard let cmd = arguments.first, !cmd.isEmpty else {
            throw BridgeError.invalidArgs("missing command; usage: cider-bridge <cmd> [json-args | -]")
        }
        guard arguments.count <= 2 else {
            throw BridgeError.invalidArgs("too many arguments; usage: cider-bridge <cmd> [json-args | -]")
        }
        guard arguments.count == 2 else { return CLIInvocation(cmd: cmd) }
        let raw = arguments[1] == "-" ? readStdin() : arguments[1]
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return CLIInvocation(cmd: cmd) }
        do {
            return CLIInvocation(cmd: cmd, args: try JSONLine.decode([String: JSONValue].self, from: trimmed))
        } catch {
            throw BridgeError.invalidArgs("args must be a JSON object: \(error.localizedDescription)")
        }
    }
}

extension Response {
    /// The stdout line for a response; encoding cannot realistically fail, but
    /// if it does the line is still a valid error envelope.
    public var wireLine: String {
        (try? JSONLine.encode(self))
            ?? #"{"error":{"code":"internal","message":"response encoding failed"},"id":\#(id),"ok":false}"#
    }
}

// MARK: - watch

public enum WatchSource: String, CaseIterable, Sendable, Codable {
    case calendar, reminders, contacts
}

public struct WatchRequest: Equatable, Sendable {
    public var sources: [WatchSource]
    public var once: Bool

    public init(sources: [WatchSource], once: Bool = false) {
        self.sources = sources
        self.once = once
    }

    public static func parse(_ args: Args) throws -> WatchRequest {
        let names = try args.stringArray("sources") ?? WatchSource.allCases.map(\.rawValue)
        var sources: [WatchSource] = []
        for name in names {
            guard let source = WatchSource(rawValue: name.lowercased()) else {
                throw BridgeError.invalidArgs(
                    "unknown source '\(name)'; 'sources' may contain \(WatchSource.allCases.map(\.rawValue).joined(separator: ", "))")
            }
            if !sources.contains(source) { sources.append(source) }
        }
        guard !sources.isEmpty else { throw BridgeError.invalidArgs("'sources' must name at least one source") }
        return WatchRequest(sources: sources, once: try args.bool("once") ?? false)
    }
}

/// One `watch` line: `{"source":"calendar","at":"…","kind":"store_changed"}`.
public struct WatchEvent: Codable, Equatable, Sendable {
    public var source: WatchSource
    public var at: Date
    public var kind: String

    public init(source: WatchSource, at: Date, kind: String = "store_changed") {
        self.source = source
        self.at = at
        self.kind = kind
    }

    private enum CodingKeys: String, CodingKey { case source, at, kind }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        source = try c.decode(WatchSource.self, forKey: .source)
        at = try c.decodeDate(forKey: .at) ?? Date()
        kind = try c.decode(String.self, forKey: .kind)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(source, forKey: .source)
        try c.encodeDate(at, forKey: .at)
        try c.encode(kind, forKey: .kind)
    }

    public var wireLine: String { Response.success(id: 0, data: (try? JSONValue(encoding: self)) ?? .null).wireLine }
}

/// Folds a burst of store notifications into one event per source: the first
/// notification opens a window (500 ms by default) and everything else for
/// that source inside it is dropped; the event is emitted when the window
/// closes, stamped with the first notification's time.
public actor WatchCoalescer {
    public typealias Emit = @Sendable (WatchEvent) -> Void

    private let window: Duration
    private let emit: Emit
    private var pending: [WatchSource: Task<Void, Never>] = [:]

    public init(window: Duration = .milliseconds(500), emit: @escaping Emit) {
        self.window = window
        self.emit = emit
    }

    public func note(_ source: WatchSource, at: Date = Date()) {
        guard pending[source] == nil else { return }
        let window = self.window
        let emit = self.emit
        pending[source] = Task { [weak self] in
            try? await Task.sleep(for: window)
            guard !Task.isCancelled else { return }
            await self?.close(source)
            emit(WatchEvent(source: source, at: at))
        }
    }

    private func close(_ source: WatchSource) {
        pending[source] = nil
    }

    public func cancel() {
        for task in pending.values { task.cancel() }
        pending.removeAll()
    }
}

// MARK: - Lifetime

/// When a `watch` ends, besides `once`, SIGINT, and the launcher exiting.
///
/// The contract is "stream until stdin closes", and that only means something
/// when stdin is a pipe, a socket, or a terminal: something the launcher holds
/// and can close. `/dev/null` (what `Stdio::null()` and a detached shell hand
/// us) is at EOF before the first read, a regular file reaches it after a few
/// reads, and a closed descriptor never reads at all. Treating those as "stop"
/// ended every such watch within milliseconds of the `watching` line, with
/// exit 0 and no event ever emitted.
public enum WatchLifetime {
    /// Whether EOF on `fileDescriptor` should end the watch.
    public static func stdinEndsWatch(fileDescriptor fd: Int32 = STDIN_FILENO) -> Bool {
        var status = stat()
        guard fstat(fd, &status) == 0 else { return false }
        switch status.st_mode & S_IFMT {
        case S_IFIFO, S_IFSOCK: return true
        case S_IFCHR: return isatty(fd) == 1
        default: return false
        }
    }
}

// MARK: - Store observation

#if canImport(EventKit) && canImport(Contacts)
import Contacts
import EventKit

/// Subscribes to `EKEventStoreChanged` / `CNContactStoreDidChange` and feeds
/// a `WatchCoalescer`. `EKEventStoreChanged` does not say whether an event or
/// a reminder changed, so it is reported under every EventKit source that was
/// requested.
@MainActor
public final class StoreWatcher {
    private let coalescer: WatchCoalescer
    private var observers: [NSObjectProtocol] = []

    public init(request: WatchRequest, eventStore: EKEventStore, coalescer: WatchCoalescer) {
        self.coalescer = coalescer
        let eventKitSources = request.sources.filter { $0 != .contacts }
        if !eventKitSources.isEmpty {
            observers.append(NotificationCenter.default.addObserver(
                forName: .EKEventStoreChanged, object: eventStore, queue: .main
            ) { _ in
                let at = Date()
                Task { for source in eventKitSources { await coalescer.note(source, at: at) } }
            })
        }
        if request.sources.contains(.contacts) {
            observers.append(NotificationCenter.default.addObserver(
                forName: .CNContactStoreDidChange, object: nil, queue: .main
            ) { _ in
                let at = Date()
                Task { await coalescer.note(.contacts, at: at) }
            })
        }
    }

    public func stop() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll()
        Task { await coalescer.cancel() }
    }
}
#endif
