# Working in this repository

## UI and Animation

Before any change to UI, motion, gestures, press feedback or haptics — in
Compose (`kotlin/arkui`, the apps' `android/`), SwiftUI (`swift/ArkUI`, the
apps' `ios/`) or iced (`rust/arkui`, the apps' `iced/`) — read
`docs/motion.md` and follow it. Use the shared tokens it names (`Springs`,
`Press`/`.press`, `SensoryFeedback`/`.sensoryFeedback`, the reduced-motion
helpers, `arkui::motion`) rather than literal curves or springs, and keep
the three toolkits in step.

Run the doc's Review Checklist on the diff before pushing, and say in the
commit what was verified and what has not been seen on a device. Leave
`ALWAYS_ANIMATE` / `alwaysAnimate` as they are unless the owner asks.
