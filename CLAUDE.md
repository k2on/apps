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

arkui holds primitives, native containers and native controls, nothing
else. A primitive is what the generator writes out as each toolkit's own
layout (the stacks, spacer, scroll, text, glyph, image, swatch, grid and
their modifiers, `ui/docs/primitives.md`); a native container or control
is what carries the platform's behaviour and cannot be composed (a list, a
section, navigation, tabs, sheets, menus, forms, fields, pickers, a press,
the scrubber and volume, the now-playing accessory). A component is a Rust
composition of those, never hand-written per toolkit: one app's own is a
`#[view]`-able function in that app's `app/` crate (harken's covers and
rows are `harken-app`'s `parts`), and one several apps really share is a
composition in ark-app (`ark_app::view`'s components). Every value is a
token: every motion value (stiffness, damping, fade, cap, flick speed,
deceleration, press scale, entrance scale) is written once, in
`ui/theme.toml`'s `[motion]`, every gap, corner and measure in its
`[layout]`, and generated into `MotionTokens`/`LayoutTokens` for Kotlin
and Swift and `arkui::motion_tokens`/`layout_tokens` for Rust by `nix run
.#update-theme` (`checks.theme` fails if they drift). The libraries'
`Springs` / `arkui::motion` are built on those tokens, and an app uses
only the libraries' springs, transitions, press and haptics: no number,
curve or spring of its own. A primitive, container or control that is
missing is added to arkui on every toolkit (and to `theme.toml` if it is
a value), then used from there.

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
