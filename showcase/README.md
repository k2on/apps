# showcase

The shared components on their own, one app per phone, for the docs'
captures (`docs/site`, `.github/workflows/screens.yaml`). Neither has any
app's code in it, nor ArkDB's kernel: each is the shared components and a
screen that draws one shot of them by name.

- `android/` — a Gradle project compiling ark-ui (today the sources under
  `harken/android/app/src/main/kotlin/dev/harken/android/ui`, until it is a
  library of its own) and `Showcase.kt`. `gradle -p showcase/android
  :app:assembleDebug`; open a shot with
  `adb shell am start -n dev.arkui.showcase/.ShowcaseActivity --es shot cards`.
- `ios/` — an XcodeGen project over ArkUI for SwiftUI (`swift/ArkUI`).
  `xcodegen generate`, build for the simulator, and launch with
  `xcrun simctl launch booted dev.arkui.showcase -showcase art`.

A shot's id is the docs' (`docs/site/src/components.rs`), which also says
which phones it is taken on.
