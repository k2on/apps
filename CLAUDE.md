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
`server/` and `demo/`. Its `native/` and `iced/` are being removed and
must not grow; `bridge/`, `views/` and `web/` are gone (the phones'
library and the browser's page are generated from app.toml by
ui/prebuild.py and nix/ark-app.nix). A screen goes in `app/` as a
`#[view]`, what the app's host does with what its screens ask (routing
between runtimes, its month, its bank, its session's facts) in `app/` as
its `ark_app::host::HostedApp`, what a window draws of the app's own (a
name its shell hosts, its Settings, its address) in `app/` as its
`ark_app::window::Windowed`, a device feature in an `ark-*` module behind
an `ark-module` port (its browser device in module.toml's `[web]`),
domain rules in `core/`. An app takes a
module on by depending on its crate from `app/`; nothing lists modules
anywhere else. `ui/docs/app.md` ("Generated, Not Committed") says what to
do in each case.
