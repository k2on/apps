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
`server/` and `demo/`, and the one Android debug key its app.toml names.
Nothing native of an app is committed: its phones' shell is the app
library's (ArkApp's and ark-app-android's `ArkShell`), drawn from app.toml's
`[shell]` names and its Rust views; its theme, glyphs and marks are arkui's;
its page and its desktop window are generated from app.toml too (`.web/`,
`.desktop/`: arkui-web's and arkui's hosts over its `Windowed`).
Its `bridge/`, `views/`, `native/`, `iced/` and `web/` are being removed
and must not grow (money has none of them: its folder is app.toml,
icon.svg, core/, app/, server/, demo/, README.md, .gitignore and the
debug key; harken still has its own): a
screen goes in `app/` as a `#[view]`, what the app's host does with what
its screens ask (routing between runtimes, its month, its bank, its
session's facts; a route is one of the shell's places on every host) in
`app/` as its `ark_app::host::HostedApp`, a device feature in an `ark-*`
module behind an `ark-module` port, domain rules in `core/`, and anything
of a shell another app could share in ArkApp, ark-app-android or arkui. An
app takes a module on by depending on its crate from `app/`; nothing lists
modules anywhere else. No app has a `main.rs` or names a toolkit crate
(`arkui_web`, `arkui`, `leptos`): its page and its web export are
generated from app.toml (`nix run .#<id>-export-web`), and what its pages
load besides is its `public/`, named by `[web]`. A static app with no data
(the docs, `docs/site`) is `app.toml`, `icon.svg`, `public/` and its crate
in the one directory. `ui/docs/app.md` ("Generated, Not Committed") says
what to do in each case.
