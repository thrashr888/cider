// swift-tools-version: 6.0
import PackageDescription

// BridgeCore builds as a plain macOS library (every HomeKit/WeatherKit path is
// behind `#if canImport(...)`), so `swift build` / `swift test` work without
// Xcode signing. The Catalyst app in Sources/CiderBridgeApp is built by
// XcodeGen + xcodebuild (see project.yml) and links this same package, where
// HomeKit is importable and the real service compiles in.
//
// The `cider-bridge` executable embeds an Info.plist section so TCC can show
// its usage strings and attribute Calendar/Reminders/Contacts consent to it.
// `unsafeFlags` makes this package unusable as a *versioned* dependency; the
// app consumes it by local path (project.yml), which SwiftPM allows.
let package = Package(
    name: "cider-bridge",
    platforms: [.macOS(.v14), .macCatalyst(.v17), .iOS(.v17)],
    products: [
        .library(name: "BridgeCore", targets: ["BridgeCore"]),
        .executable(name: "cider-bridge", targets: ["cider-bridge"]),
    ],
    targets: [
        .target(name: "BridgeCore"),
        .executableTarget(
            name: "cider-bridge",
            dependencies: ["BridgeCore"],
            linkerSettings: [
                .unsafeFlags([
                    "-Xlinker", "-sectcreate",
                    "-Xlinker", "__TEXT",
                    "-Xlinker", "__info_plist",
                    "-Xlinker", "\(Context.packageDirectory)/Resources/cider-bridge-Info.plist",
                ]),
            ]),
        .testTarget(name: "BridgeCoreTests", dependencies: ["BridgeCore"]),
    ]
)
