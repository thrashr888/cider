import BridgeCore
import EventKit
import Foundation

// cider-bridge: the native (non-Catalyst) half of the bridge. EventKit and
// Contacts through the RFC envelope:
//
//   cider-bridge <cmd> [json-args | -]
//
// One envelope line on stdout, diagnostics on stderr, exit 0 on ok and 1 on
// error. `watch` streams one envelope line per store change until stdin
// closes (or SIGINT). Everything below runs on the main actor; the stores'
// objects never leave it.

// First thing, before any store is touched: with CIDER_BRIDGE_DISCLAIM=1,
// become our own TCC client (see Responsibility.swift for why that is opt-in).
Responsibility.disclaimAndReexecIfNeeded()

func emit(_ line: String) {
    FileHandle.standardOutput.write(Data((line + "\n").utf8))
}

func fail(_ error: BridgeError) -> Never {
    emit(Response.failure(id: 0, error: error).wireLine)
    exit(1)
}

let invocation: CLIInvocation
do {
    invocation = try CLIInvocation.parse(Array(CommandLine.arguments.dropFirst())) {
        String(decoding: FileHandle.standardInput.readDataToEndOfFile(), as: UTF8.self)
    }
} catch let error as BridgeError {
    if CommandLine.arguments.count == 1 {
        FileHandle.standardError.write(Data((CLIInvocation.usage + "\n").utf8))
    }
    fail(error)
} catch {
    fail(.invalidArgs(String(describing: error)))
}

if invocation.isHelp {
    print(CLIInvocation.usage)
    exit(0)
}

let eventKit = EKEventKitService()
let contacts = CNContactsService()
let router = CommandRouter(version: BridgeInfo.version)
await registerEventKitCommands(router, service: eventKit)
await registerContactsCommands(router, service: contacts)
await router.register("ping") { _ in
    [
        "version": .string(BridgeInfo.version),
        "build": .string(BridgeBuild.current().kind.rawValue),
        "calendar": .string(EKEventKitService.authorizationName(for: .event)),
        "reminders": .string(EKEventKitService.authorizationName(for: .reminder)),
        "contacts": .string(CNContactsService.authorizationName),
        "executable": .string(PermissionHelp.executablePath),
        "tcc_subject": .string(Responsibility.isDisclaimed ? "cider-bridge" : "launcher"),
    ]
}

if invocation.cmd == "watch" {
    let request: WatchRequest
    do {
        request = try WatchRequest.parse(Args(invocation.args))
        // Consent up front, so a denial is one envelope rather than silence.
        if request.sources.contains(.calendar) { try await eventKit.authorize(.event) }
        if request.sources.contains(.reminders) { try await eventKit.authorize(.reminder) }
        if request.sources.contains(.contacts) { try await contacts.authorize() }
    } catch let error as BridgeError {
        fail(error)
    } catch {
        fail(.internalError(String(describing: error)))
    }

    // Runs until stdin closes, SIGINT, or (with `once`) the first event.
    let finished = AsyncStream<Void>.makeStream()
    let coalescer = WatchCoalescer { event in
        emit(event.wireLine)
        if request.once { finished.continuation.yield() }
    }
    let watcher = StoreWatcher(request: request, eventStore: eventKit.store, coalescer: coalescer)

    let stdin = DispatchSource.makeReadSource(fileDescriptor: STDIN_FILENO, queue: .main)
    stdin.setEventHandler {
        var buffer = [UInt8](repeating: 0, count: 4096)
        let n = read(STDIN_FILENO, &buffer, buffer.count)
        if n <= 0 { finished.continuation.yield() }
    }
    stdin.resume()

    FileHandle.standardError.write(Data("cider-bridge: watching \(request.sources.map(\.rawValue).joined(separator: ", "))\n".utf8))
    for await _ in finished.stream { break }
    stdin.cancel()
    watcher.stop()
    exit(0)
}

let response = await router.dispatch(Request(id: 0, cmd: invocation.cmd, args: invocation.args))
emit(response.wireLine)
exit(response.ok ? 0 : 1)
