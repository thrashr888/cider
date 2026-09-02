#if os(macOS)
import Darwin
import Foundation

// TCC attributes a command-line tool's consent to its *responsible process*:
// Terminal, or whatever launched cider. The prompt names that app, uses that
// app's usage strings, and the grant lands on that app. That is how every
// CLI on macOS behaves, and it means cider-bridge inherits the Calendar /
// Reminders / Contacts access the user already gave Terminal.
//
// Alternatively the tool can *disclaim* responsibility and become its own
// TCC client (the private-but-stable spawn attribute Chromium and Electron
// use): the prompt then names Cider Bridge and the grant follows this
// binary. That is opt-in (`CIDER_BRIDGE_DISCLAIM=1`) because on macOS 27
// tccd refuses to show Calendar and Contacts prompts to a disclaimed
// command-line process (Reminders does prompt), so by default the launcher's
// consent is the one that counts.
@_silgen_name("responsibility_spawnattrs_setdisclaim")
private func responsibility_spawnattrs_setdisclaim(
    _ attrs: UnsafeMutablePointer<posix_spawnattr_t?>, _ disclaim: Int32) -> Int32

public enum Responsibility {
    /// Opt in to self-attribution.
    public static let optIn = "CIDER_BRIDGE_DISCLAIM"
    /// Set in the re-executed process so it does not re-exec again.
    public static let marker = "CIDER_BRIDGE_DISCLAIMED"

    public static func wantsDisclaim(_ environment: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
        guard let value = environment[optIn]?.trimmingCharacters(in: .whitespaces).lowercased() else { return false }
        return !["", "0", "false", "no", "off"].contains(value)
    }

    /// True in a process that re-executed itself as its own TCC client.
    public static var isDisclaimed: Bool {
        ProcessInfo.processInfo.environment[marker] != nil
    }

    /// With `CIDER_BRIDGE_DISCLAIM` set, replaces the current process image
    /// with itself, responsibility disclaimed (`POSIX_SPAWN_SETEXEC` keeps the
    /// pid and every fd). Returns when that was not requested, already done,
    /// or failed; the caller carries on in-process.
    public static func disclaimAndReexecIfNeeded(environment: [String: String] = ProcessInfo.processInfo.environment) {
        guard wantsDisclaim(environment), environment[marker] == nil else { return }
        let path = PermissionHelp.executablePath

        var attrs: posix_spawnattr_t?
        guard posix_spawnattr_init(&attrs) == 0 else { return }
        defer { posix_spawnattr_destroy(&attrs) }
        guard responsibility_spawnattrs_setdisclaim(&attrs, 1) == 0,
              posix_spawnattr_setflags(&attrs, Int16(POSIX_SPAWN_SETEXEC)) == 0
        else { return }

        setenv(marker, "1", 1)
        var pid: pid_t = 0
        let status = posix_spawn(&pid, path, nil, &attrs, CommandLine.unsafeArgv, environ)
        // Only reached if the exec failed.
        unsetenv(marker)
        FileHandle.standardError.write(Data(
            "cider-bridge: could not re-exec with responsibility disclaimed (\(String(cString: strerror(status)))); TCC will attribute consent to the launching app\n".utf8))
    }
}
#endif
