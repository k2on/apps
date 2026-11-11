# Working in this repository

## UI and Animation

Before any change to UI, motion, gestures, press feedback or haptics — in
Compose (`ui/kotlin`, the apps' `android/`), SwiftUI (`ui/swift`, the
apps' `ios/`) or iced (`ui/rust/arkui`, the apps' `iced/`) — read
`ui/docs/motion.md` and follow it. Use the shared tokens it names (`Springs`,
`Press`/`.press`, `SensoryFeedback`/`.sensoryFeedback`, the reduced-motion
helpers, `arkui::motion`) rather than literal curves or springs, and keep
the three toolkits in step.

Run the doc's Review Checklist on the diff before pushing, and say in the
commit what was verified and what has not been seen on a device. Leave
`ALWAYS_ANIMATE` / `alwaysAnimate` as they are unless the owner asks.

Everything shared lives in arkui. Every motion value (stiffness, damping,
fade, cap, flick speed, deceleration, press scale, entrance scale) is
written once, in `ui/theme.toml`'s `[motion]`, and generated into
`MotionTokens` for Kotlin and Swift and `arkui::motion_tokens` for Rust by
`nix run .#update-theme` (`checks.theme` fails if they drift). The
libraries' `Springs` / `arkui::motion` are built on those tokens, and an
app uses only the libraries' springs, transitions, press and haptics: no
number, curve, spring or component of its own that another app could
share. One that is missing is added to arkui (and to `theme.toml` if it is a
value), then used from there.

## An App's Directory

An ark app's committed shape is `app.toml`, `icon.svg`, `core/`, `app/`,
`server/` and `demo/`. Its `bridge/`, `views/`, `native/`, `iced/` and
`web/` are being removed and must not grow: a screen goes in `app/` as a
`#[view]`, a device feature in an `ark-*` module behind an `ark-module`
port, domain rules in `core/`. `ui/docs/app.md` ("Generated, Not
Committed") says what to do in each case.
