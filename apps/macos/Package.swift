// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "ActRealmMac",
    platforms: [
        .macOS(.v26)
    ],
    products: [
        .library(name: "ActRealmKit", targets: ["ActRealmKit"]),
        .executable(name: "ActRealmApp", targets: ["ActRealmApp"])
    ],
    targets: [
        .target(
            name: "ActRealmKit"
        ),
        .target(
            name: "ActRealmUI",
            dependencies: ["ActRealmKit"]
        ),
        .executableTarget(
            name: "ActRealmApp",
            dependencies: ["ActRealmKit", "ActRealmUI"]
        ),
        .executableTarget(
            name: "SnapshotTool",
            dependencies: ["ActRealmKit", "ActRealmUI"]
        ),
        .testTarget(
            name: "ActRealmKitTests",
            dependencies: ["ActRealmKit"],
            // CommandLineTools ships Swift Testing outside SwiftPM's default
            // macro search path. Full Xcode ignores these compatible paths.
            swiftSettings: [
                .unsafeFlags([
                    "-Xfrontend", "-disable-cross-import-overlays",
                    "-plugin-path", "/Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/testing"
                ])
            ],
            linkerSettings: [
                .unsafeFlags([
                    "-F", "/Library/Developer/CommandLineTools/Library/Developer/Frameworks",
                    "-Xlinker", "-rpath",
                    "-Xlinker", "/Library/Developer/CommandLineTools/Library/Developer/Frameworks",
                    "-Xlinker", "-rpath",
                    "-Xlinker", "/Library/Developer/CommandLineTools/Library/Developer/usr/lib"
                ])
            ]
        )
    ]
)
