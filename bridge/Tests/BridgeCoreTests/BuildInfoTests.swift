import Foundation
import XCTest
@testable import BridgeCore

final class BuildInfoTests: XCTestCase {
    func testKindFromProfileShape() {
        XCTAssertEqual(BridgeBuild.kind(profile: ["ProvisionsAllDevices": true], signer: nil), .developerID)
        XCTAssertEqual(BridgeBuild.kind(profile: ["ProvisionedDevices": ["00006030-001C"]], signer: nil), .development)
        // A Developer ID profile wins over whatever the certificate says.
        XCTAssertEqual(BridgeBuild.kind(profile: ["ProvisionsAllDevices": true], signer: "Apple Development: X"), .developerID)
        // An empty or shapeless profile falls through to the signer.
        XCTAssertEqual(BridgeBuild.kind(profile: ["ProvisionedDevices": []], signer: nil), .unsigned)
        XCTAssertEqual(BridgeBuild.kind(profile: [:], signer: "Developer ID Application: Paul Thrasher (5T4QSYSNP2)"), .developerID)
    }

    func testKindFromSigner() {
        XCTAssertEqual(BridgeBuild.kind(profile: nil, signer: "Developer ID Application: Paul Thrasher (5T4QSYSNP2)"), .developerID)
        XCTAssertEqual(BridgeBuild.kind(profile: nil, signer: "Apple Development: Paul Thrasher (272R94Z2BK)"), .development)
        XCTAssertEqual(BridgeBuild.kind(profile: nil, signer: nil), .unsigned)
        XCTAssertEqual(BridgeBuild.kind(profile: nil, signer: "Something Else"), .unsigned)
    }

    func testPlistIsExtractedFromACMSWrapper() throws {
        let xml = """
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0"><dict>
            <key>Name</key><string>Cider Bridge Developer ID</string>
            <key>ProvisionsAllDevices</key><true/>
            </dict></plist>
            """
        var blob = Data([0x30, 0x82, 0x0e, 0x81, 0x06, 0x09, 0x2a])  // DER-ish prefix
        blob.append(Data(xml.utf8))
        blob.append(Data([0x00, 0xa0, 0x82, 0x05, 0xc8]))  // signature-ish suffix
        let profile = try XCTUnwrap(BridgeBuild.plist(inProfile: blob))
        XCTAssertEqual(profile["Name"] as? String, "Cider Bridge Developer ID")
        XCTAssertEqual(BridgeBuild.kind(profile: profile, signer: nil), .developerID)

        XCTAssertNil(BridgeBuild.plist(inProfile: Data("no plist here".utf8)))
        XCTAssertNil(BridgeBuild.plist(inProfile: Data("<?xml truncated".utf8)))
    }

    func testPingFieldsShape() {
        let build = BridgeBuild(kind: .developerID, homekitEntitled: false, weatherkitEntitled: true, bundlePath: "/x/Cider Bridge.app")
        XCTAssertEqual(build.pingFields, [
            "build": "developer-id", "homekit_entitled": false, "weatherkit_entitled": true, "bundle_path": "/x/Cider Bridge.app",
        ])
    }

    func testCurrentProcessIsUnsignedAndUnentitledUnderSwiftTest() {
        // The test runner has no HomeKit/WeatherKit entitlement and no profile;
        // whatever signs xctest, it is neither a development nor a Developer ID
        // bridge certificate.
        let build = BridgeBuild.current()
        XCTAssertFalse(build.homekitEntitled)
        XCTAssertFalse(build.weatherkitEntitled)
        XCTAssertEqual(build.kind, .unsigned)
        XCTAssertFalse(build.bundlePath.isEmpty)
    }
}
