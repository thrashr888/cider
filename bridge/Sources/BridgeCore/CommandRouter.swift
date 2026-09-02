import Foundation

/// Maps `cmd` names to async handlers and turns their results (or thrown
/// errors) into response envelopes. Unknown commands are `invalid_args`,
/// `BridgeError`s keep their code, anything else is `internal`.
public actor CommandRouter {
    public typealias Handler = @Sendable ([String: JSONValue]) async throws -> JSONValue
    public typealias QuitHandler = @Sendable () -> Void

    public let version: String
    private var handlers: [String: Handler] = [:]

    /// - Parameters:
    ///   - version: reported by `ping`.
    ///   - quit: invoked by the `quit` command (after the reply is queued).
    public init(version: String = BridgeInfo.version, quit: @escaping QuitHandler = {}) {
        self.version = version
        handlers["ping"] = { _ in
            ["version": .string(version), "homekit_authorized": false, "homes": 0]
        }
        handlers["quit"] = { _ in
            quit()
            return ["bye": true]
        }
    }

    public func register(_ cmd: String, _ handler: @escaping Handler) {
        handlers[cmd] = handler
    }

    public var commands: [String] { handlers.keys.sorted() }

    public func dispatch(_ request: Request) async -> Response {
        guard let handler = handlers[request.cmd] else {
            return .failure(id: request.id, error: .invalidArgs("unknown command '\(request.cmd)'"))
        }
        do {
            return .success(id: request.id, data: try await handler(request.args))
        } catch let error as BridgeError {
            return .failure(id: request.id, error: error)
        } catch is CancellationError {
            return .failure(id: request.id, error: .timeout("cancelled"))
        } catch {
            return .failure(id: request.id, error: .internalError(String(describing: error)))
        }
    }

    /// One wire line in, one wire line out. Malformed JSON is `invalid_args`
    /// with the request `id` recovered when possible. Blank lines yield `nil`.
    public func dispatch(line: String) async -> String? {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return nil }
        let response: Response
        do {
            response = await dispatch(try JSONLine.decode(Request.self, from: trimmed))
        } catch {
            response = .failure(
                id: Request.peekID(in: trimmed) ?? 0,
                error: .invalidArgs("malformed request: \(error.localizedDescription)"))
        }
        return (try? JSONLine.encode(response))
            ?? #"{"error":{"code":"internal","message":"response encoding failed"},"id":\#(response.id),"ok":false}"#
    }
}
