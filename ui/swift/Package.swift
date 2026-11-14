// swift-tools-version: 5.9
// ArkUI for SwiftUI: what every Ark app on iOS shares, with nothing of any
// app in it (docs/plan-arkui-swift.md) — Android's ark-ui, said in SwiftUI.
//
// Two targets. ArkUICore is Foundation alone — the derived square's hash and
// palettes, the spring numbers — so Linux builds it and the apps' non-UI code
// and tests type-check against it there. ArkUI is the views and the
// animations, over it, and re-exports it.
//
// Beside them, Sources/ArkApp is the app library (ui/docs/app.md, "The App
// Library"): what every Ark app is underneath its screens. It is written over
// an app's own bindings (ark-ffi's `ArkHost`, `ArkProcess`), which no package
// here has, so it is no target of this package: nix builds it per app and
// platform with `swiftModule` (flake.nix, `arkAppModule`), and an app's
// generated Xcode project compiles it into the app's target.
import PackageDescription

let package = Package(
    name: "ArkUI",
    platforms: [.iOS("26.0"), .macOS("26.0")],
    products: [
        .library(name: "ArkUICore", targets: ["ArkUICore"]),
        .library(name: "ArkUI", targets: ["ArkUI"]),
    ],
    targets: [
        .target(name: "ArkUICore"),
        .target(name: "ArkUI", dependencies: ["ArkUICore"]),
    ]
)
