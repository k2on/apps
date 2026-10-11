// swift-tools-version:5.8
import PackageDescription

// The test target is an executable rather than an XCTest bundle: the nix
// `swift` devshell this is built in carries no XCTest, so the runner exits
// non-zero on any failing vector instead. `swift run ArkDBTests` runs it.
//
// A phone's runtime is a client (docs/plan-phones.md): it interprets the
// module `arkc` emits and authors none, so there is no authoring vocabulary
// and no printed domain here.
let package = Package(
    name: "ArkDB",
    // The client library uses Swift concurrency and URLSessionWebSocketTask,
    // which need these; ignored on Linux.
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [
        .library(name: "ArkDB", targets: ["ArkDB"]),
        .library(name: "ArkDBClient", targets: ["ArkDBClient"]),
        .executable(name: "ArkDBTests", targets: ["ArkDBTests"]),
    ],
    targets: [
        .target(name: "ArkDB", dependencies: [], path: "Sources/ArkDB"),
        .target(name: "ArkDBClient", dependencies: ["ArkDB"], path: "Sources/ArkDBClient"),
        .executableTarget(name: "ArkDBTests", dependencies: ["ArkDB", "ArkDBClient"], path: "Tests/ArkDBTests"),
    ]
)
