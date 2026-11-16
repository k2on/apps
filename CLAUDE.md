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
`#[view]`-able function in that app's `src/components/` (harken's covers
and rows are `harken-app`'s `components`), and one several apps really share is a
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

An ark app's committed shape is `app.toml`, `icon.svg`, `Cargo.toml` (its
crate, at its root), `src/` and `server/`, and the one Android debug key
its app.toml names:

```
money/
  app.toml  icon.svg  Cargo.toml  debug.keystore
  src/lib.rs        the host's part and what is shared, minus the screens
  src/app/          its routes and screens, Expo Router's way (`ark_router::routes!`)
  src/components/   its own components, Rust compositions of arkui's primitives
  src/core/         its domain: a crate of its own
  src/demo/         its demo's household (a crate), shots.json, server.sh
  server/           its server, for now
```

`src/app` is read as the crate compiles (ark_router's crate docs say the
conventions): a file is a route (its path the folders but a `(group)`,
`[id].rs` a typed field of its `#[derive(ark_router::Route)]` struct,
`index.rs` the folder's own); a `_layout.rs` is a screen that lays out its
folder (the root's the shell, `(modals)`' the sheets, each group in
`(tabs)` a tab), its own modules in `_layout/`; `+not-found.rs` says where
an address that names nothing lands. From it the macro writes the `Route`
enum, its parsing and printing, the app's declaration (what its manifest
and every generated target are written from) and what a route does to the
screens (`ark_app::host::Routed`: a tab selected, a screen told, a page
pushed). A new screen is a `_layout.rs` with its runtime marked
`#[ark_router::screen(view = …)]` and a field of the host; a new route is
a file. Nothing native of an app is committed: its phones' shell is the
app library's (ArkApp's and ark-app-android's `ArkShell`), drawn from its
declaration and its Rust views; its theme, glyphs and marks are arkui's;
its page and its desktop window are generated too (`.web/`, `.desktop/`:
arkui-web's and arkui's hosts over its `Windowed`). app.toml is
configuration only (name, id, version, accent, platforms, icon, Apple and
Android settings, web modes, store listing, channels, background,
keystore). What is said in Rust or Cargo is never said in it again: its
domain and demo are its `src/core` and `src/demo` crates, its modules its
crate's dependencies; ui/prebuild.py reads the declaration as the manifest
its generator prints (`<id>-gen manifest`), and its check refuses the old
keys and an `app/`, `core/` or `demo/` committed beside app.toml.
A screen goes in `src/app` as a layout with a `#[view]`, a component in
`src/components/`, what the app's host does with what its screens ask
that is not routing (its month, its bank, its session's facts, a route
the shell shows outside a tab: an inspector, Settings) in `src/host.rs` as
its `ark_app::host::HostedApp`, a device feature in an `ark-*` module
behind an `ark-module` port, domain rules in `src/core/`, and anything of
a shell another app could share in ArkApp, ark-app-android or arkui. An
app takes a module on by depending on its crate from its `Cargo.toml`;
nothing lists modules anywhere else. No app has a `main.rs` or names a
toolkit crate (`arkui_web`, `arkui`, `leptos`): its page and its web
export are generated from app.toml (`nix run .#<id>-export-web`), and what
its pages load besides is its `public/`, named by `[web]`. A static app
with no data (the docs, `docs/site`) is `app.toml`, `icon.svg`, `public/`
and its crate in the one directory, its pages `src/app/index.rs` and
`src/app/[tag].rs`. `ui/docs/app.md` ("Generated, Not Committed") says
what to do in each case.
