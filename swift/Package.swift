// swift-tools-version:5.8
import PackageDescription

// The test target is an executable rather than an XCTest bundle: the nix
// `swift` devshell this is built in carries no XCTest, so the runner exits
// non-zero on any failing vector instead. `swift run ArkDBTests` runs it.
let package = Package(
    name: "ArkDB",
    // The client library uses Swift concurrency and URLSessionWebSocketTask,
    // which need these; ignored on Linux.
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [
        .library(name: "ArkDB", targets: ["ArkDB"]),
        // The authoring vocabulary (spec/AUTHORING.md): a domain written in
        // Swift, emitted as a module or run natively. A module of its own
        // because its `Bool`, `Int` and `Id<T>` shadow the standard
        // library's and ArkDB's in every file that imports it.
        .library(name: "ArkAuthoring", targets: ["ArkAuthoring"]),
        // Everything an app needs around the sans-io machines: the WebSocket
        // link, the session over the replicas, files, and an in-process
        // exchange for tests. Foundation (+ FoundationNetworking on Linux).
        .library(name: "ArkDBClient", targets: ["ArkDBClient"]),
        .executable(name: "ArkDBTests", targets: ["ArkDBTests"]),
    ],
    targets: [
        .target(name: "ArkDB", dependencies: [], path: "Sources/ArkDB"),
        .target(name: "ArkAuthoring", dependencies: ["ArkDB"], path: "Sources/ArkAuthoring"),
        .target(name: "ArkDBClient", dependencies: ["ArkDB"], path: "Sources/ArkDBClient"),
        // spec/AUTHORING.md Appendix B, authored in Swift: the text `arkc gen
        // swift` prints for the demo module.
        .target(name: "ArkDemo", dependencies: ["ArkAuthoring"], path: "Tests/Demo"),
        // harken's domain as the phone is built with it — the printed
        // authoring form in ../harken/domain/gen/swift (a link), compiled
        // here so that Linux proves it builds against ArkAuthoring.
        .target(name: "HarkenDomain", dependencies: ["ArkAuthoring"], path: "Tests/HarkenDomain"),
        // The rest of the vocabulary, in one small domain the tests hold to Eval.
        .target(name: "Kitchen", dependencies: ["ArkAuthoring"], path: "Tests/Kitchen"),
        // Everything below the iOS app's views: the same four domain files
        // and ../harken/ios/Harken/Rows.swift (links), so the phone's bridge
        // is compiled and driven on Linux.
        .target(name: "HarkenPhone", dependencies: ["ArkDB", "ArkDBClient", "ArkAuthoring"], path: "Tests/HarkenPhone"),
        .executableTarget(name: "ArkDBTests", dependencies: ["ArkDB", "ArkDBClient", "ArkAuthoring", "ArkDemo", "HarkenDomain", "HarkenPhone", "Kitchen"], path: "Tests/ArkDBTests"),
    ]
)
