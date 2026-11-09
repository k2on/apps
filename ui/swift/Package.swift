// swift-tools-version: 5.9
// ArkUI for SwiftUI: what every Ark app on iOS shares, with nothing of any
// app in it (docs/plan-arkui-swift.md) — Android's ark-ui, said in SwiftUI.
//
// Two targets. ArkUICore is Foundation alone — the derived square's hash and
// palettes, the spring numbers — so Linux builds it and the apps' non-UI code
// and tests type-check against it there. ArkUI is the views and the
// animations, over it, and re-exports it.
import PackageDescription

let package = Package(
    name: "ArkUI",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "ArkUICore", targets: ["ArkUICore"]),
        .library(name: "ArkUI", targets: ["ArkUI"]),
    ],
    targets: [
        .target(name: "ArkUICore"),
        .target(name: "ArkUI", dependencies: ["ArkUICore"]),
    ]
)
