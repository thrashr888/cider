import BridgeCore
import Foundation

// Native CLI stub (RFC phase 4 adds EventKit/Contacts). Invocation shape is
// already the RFC's: `cider-bridge <cmd> [json-args]`, one envelope on stdout.
let arguments = CommandLine.arguments.dropFirst()
let cmd = arguments.first ?? "ping"
var args: [String: JSONValue] = [:]
if let rawArgs = arguments.dropFirst().first {
    do {
        args = try JSONLine.decode([String: JSONValue].self, from: rawArgs)
    } catch {
        let response = Response.failure(id: 0, error: .invalidArgs("args must be a JSON object: \(error.localizedDescription)"))
        print(try JSONLine.encode(response))
        exit(2)
    }
}

let router = CommandRouter()
let response = await router.dispatch(Request(id: 0, cmd: cmd, args: args))
print(try JSONLine.encode(response))
exit(response.ok ? 0 : 1)
