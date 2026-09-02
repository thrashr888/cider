// swift-tools-version: 6.0
import PackageDescription

// BridgeCore builds as a plain macOS library (every HomeKit path is behind
// `#if canImport(HomeKit)`), so `swift build` / `swift test` work without Xcode
// signing. The Catalyst app in Sources/CiderBridgeApp is built by XcodeGen +
// xcodebuild (see project.yml) and links this same package, where HomeKit is
// importable and the real service compiles in.
let package = Package(
    name: "cider-bridge",
    platforms: [.macOS(.v14), .macCatalyst(.v17), .iOS(.v17)],
    products: [
        .library(name: "BridgeCore", targets: ["BridgeCore"]),
        .executable(name: "cider-bridge", targets: ["cider-bridge"]),
    ],
    targets: [
        .target(name: "BridgeCore"),
        .executableTarget(name: "cider-bridge", dependencies: ["BridgeCore"]),
        .testTarget(name: "BridgeCoreTests", dependencies: ["BridgeCore"]),
    ]
)
