# Build notes

The app was written without a local toolchain and compiled only on CI
(`reference-listen-shots`, macos-15, Xcode 26.3, iOS 26.2 / macOS 26.2
SDKs). The fixes each CI round needed, in order:

1. **No AppIcon** — XcodeGen's default `ASSETCATALOG_COMPILER_APPICON_NAME`
   is `AppIcon`, and actool fails without one. Added a procedurally drawn
   icon (coral gradient, a dot with sound rings either side): a 1024 pt
   universal iOS image and the Mac sizes.
2. **`AdaptableTabBarPlacement.sidebar` is unavailable on macOS** — the
   Library and Podcasts tabs (`.defaultVisibility(.hidden, for: .sidebar)`)
   are iOS-only now, inside `#if os(iOS)`; the Mac never has a tab bar, so
   it never needed them.
