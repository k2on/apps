// swift-tools-version:5.8
import PackageDescription

// The test target is an executable rather than an XCTest bundle: the nix
// `swift` devshell this is built in carries no XCTest, so the runner exits
// non-zero on any failing vector instead. `swift run ArkDBTests` runs it.
let package = Package(
    name: "ArkDB",
    products: [
        .library(name: "ArkDB", targets: ["ArkDB"]),
        .executable(name: "ArkDBTests", targets: ["ArkDBTests"]),
    ],
    targets: [
        .target(name: "ArkDB", dependencies: [], path: "Sources/ArkDB"),
        .executableTarget(name: "ArkDBTests", dependencies: ["ArkDB"], path: "Tests/ArkDBTests"),
    ]
)
