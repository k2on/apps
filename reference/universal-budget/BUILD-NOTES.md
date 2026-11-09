# Build notes

What it took to get the reference compiling and screenshotted on GitHub's
macOS runner (`.github/workflows/reference-shots.yaml`, Xcode 26.3,
macos-15 image). Every change is the smallest one that builds; none changes
the design.

## Build setup (CI only, not the project)

- Built with `ENABLE_DEBUG_DYLIB=NO ENABLE_PREVIEWS=NO` (one plain binary).
- iOS Simulator: `CODE_SIGNING_ALLOWED=NO`.
- macOS: ad hoc (`CODE_SIGN_IDENTITY=-`), `ENABLE_HARDENED_RUNTIME=NO`, so
  the app launches unsigned by a team.
- Screens are opened with SCREENS.md's launch arguments; the iPad's
  landscape shots go through a small UI test (`ci/landscape/`) because
  `simctl` cannot rotate a simulator.

## Compile fixes

1. **`defaultAdaptableTabBarPlacement` is iOS-only.** The iOS build
   succeeded first time; the macOS build failed with
   `'defaultAdaptableTabBarPlacement' is unavailable in macOS` and
   `'sidebar' is unavailable in macOS` (ContentView.swift). Wrapped the one
   modifier in `#if os(iOS)`. The Mac always shows the sidebar anyway, so
   `-columns all` meant nothing there.
