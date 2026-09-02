import Foundation
import Security

/// What kind of bridge this process is: which entitlements it actually
/// carries and how it was signed. Reported by `ping` so cider can tell a
/// Homebrew (Developer ID, WeatherKit-only) bridge from a personal build
/// with HomeKit, and so a missing capability is a typed answer, not a crash.
public struct BridgeBuild: Equatable, Sendable {
    public enum Kind: String, Sendable {
        /// Signed with an Apple Development certificate and a device-listed
        /// profile: a personal `cider bridge build --install`.
        case development
        /// Signed with Developer ID, notarized, a `ProvisionsAllDevices`
        /// profile: the Homebrew tarball. Apple does not grant HomeKit here.
        case developerID = "developer-id"
        /// Ad-hoc or not signed at all (CI, `swift build`).
        case unsigned
    }

    public var kind: Kind
    public var homekitEntitled: Bool
    public var weatherkitEntitled: Bool
    public var bundlePath: String

    public init(kind: Kind, homekitEntitled: Bool, weatherkitEntitled: Bool, bundlePath: String) {
        self.kind = kind
        self.homekitEntitled = homekitEntitled
        self.weatherkitEntitled = weatherkitEntitled
        self.bundlePath = bundlePath
    }

    /// The `ping.data` fields this contributes.
    public var pingFields: [String: JSONValue] {
        [
            "build": .string(kind.rawValue),
            "homekit_entitled": .bool(homekitEntitled),
            "weatherkit_entitled": .bool(weatherkitEntitled),
            "bundle_path": .string(bundlePath),
        ]
    }

    public static let homekitEntitlement = "com.apple.developer.homekit"
    public static let weatherkitEntitlement = "com.apple.developer.weatherkit"

    // MARK: Detection

    /// The running process, from its entitlements (`SecTask`), its embedded
    /// provisioning profile, and failing that its signing certificate.
    public static func current(bundle: Bundle = .main) -> BridgeBuild {
        let profile = bundle.url(forResource: "embedded", withExtension: "provisionprofile")
            .flatMap { try? Data(contentsOf: $0) }
            .flatMap(plist(inProfile:))
        return BridgeBuild(
            kind: kind(profile: profile, signer: signerSummary()),
            homekitEntitled: hasEntitlement(homekitEntitlement),
            weatherkitEntitled: hasEntitlement(weatherkitEntitlement),
            bundlePath: bundle.bundlePath)
    }

    /// Classifies by the profile when there is one (Developer ID profiles
    /// provision all devices; development profiles list them), else by the
    /// leaf certificate's subject (`Developer ID Application: …`,
    /// `Apple Development: …`), else `unsigned`.
    static func kind(profile: [String: Any]?, signer: String?) -> Kind {
        if let profile {
            if profile["ProvisionsAllDevices"] as? Bool == true { return .developerID }
            if let devices = profile["ProvisionedDevices"] as? [Any], !devices.isEmpty { return .development }
        }
        if let signer {
            if signer.hasPrefix("Developer ID Application") { return .developerID }
            if signer.hasPrefix("Apple Development") || signer.hasPrefix("Mac Developer") { return .development }
        }
        return .unsigned
    }

    /// The plist inside a CMS-wrapped `.provisionprofile`. The XML is carried
    /// verbatim in the DER payload, so it is found by delimiters rather than
    /// decoded (CMSDecoder is not available to Catalyst).
    static func plist(inProfile data: Data) -> [String: Any]? {
        guard let start = data.range(of: Data("<?xml".utf8)),
              let end = data.range(of: Data("</plist>".utf8), in: start.lowerBound..<data.endIndex)
        else { return nil }
        let xml = data[start.lowerBound..<end.upperBound]
        return (try? PropertyListSerialization.propertyList(from: xml, format: nil)) as? [String: Any]
    }

    /// Reads the running process's own entitlement; `false` when unsigned.
    static func hasEntitlement(_ name: String) -> Bool {
        guard let task = SecTaskCreateFromSelf(nil) else { return false }
        let value = SecTaskCopyValueForEntitlement(task, name as CFString, nil)
        return (value as? Bool) == true || (value as? NSNumber)?.boolValue == true
    }

    /// Subject summary of the leaf certificate that signed this code, or `nil`
    /// when there is none (ad-hoc, unsigned).
    static func signerSummary() -> String? {
        var code: SecCode?
        guard SecCodeCopySelf([], &code) == errSecSuccess, let code else { return nil }
        var staticCode: SecStaticCode?
        guard SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess, let staticCode else { return nil }
        var info: CFDictionary?
        let flags = SecCSFlags(rawValue: UInt32(kSecCSSigningInformation))
        guard SecCodeCopySigningInformation(staticCode, flags, &info) == errSecSuccess,
              let dictionary = info as? [String: Any],
              let certificates = dictionary[kSecCodeInfoCertificates as String] as? [SecCertificate],
              let leaf = certificates.first
        else { return nil }
        return SecCertificateCopySubjectSummary(leaf) as String?
    }
}
