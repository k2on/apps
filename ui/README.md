# ArkUI

Everything an Ark app is drawn with, on every platform, in one place: the
theme, the toolkits' libraries, the runtime that runs an app's screens in
Rust and the code generator that writes them out for the phones. The
device modules an app can ask for (haptics, the clipboard, the camera, …)
are packages of their own, beside this one, under `modules/`.

```
ui/
├── README.md         this file: what is where, and the theme
├── theme.toml        every colour, measure and motion value (below)
├── theme.py          writes theme.toml out for each toolkit
├── prebuild.py       every Ark app's native projects, from its app.toml (ui/docs/app.md)
├── docs/
│   ├── design.md     how every Ark app is designed: Mac-first, then Rust, then its domain
│   ├── apps/         each app's design (money.md)
│   ├── app.md        the whole app in Rust: the runtime, #[view], ark-ui-gen, modules, generated native projects
│   ├── motion.md     how everything moves and answers a touch (a standing rule)
│   └── parts.md      ArkUI's generic parts, one name in Compose, SwiftUI and iced
├── rust/
│   ├── arkui/        crate `arkui`: the iced components, the desktop
│   ├── web/          crate `arkui-web`: the same parts in the browser, Leptos over the DOM
│   ├── glyphs/       crate `arkui-glyphs`: the glyph table, toolkit-free
│   ├── arkui-macros/ crate `arkui-macros`: `#[view]`
│   ├── router/       crate `ark-router`: a route as one path string
│   ├── router-macros/ crate `ark-router-macros`: `#[derive(Routes)]`
│   ├── app/          crate `ark-app`: an app's runtime and the view vocabulary
│   ├── gen/          crate `ark-ui-gen`: SwiftUI and Compose from a recorded view
│   ├── shots/        crate `arkui-shots`: an iced window's screenshots, no window
│   └── module/       crate `ark-module`: the plumbing every device module shares
├── swift/            the Swift package `ArkUI` (targets `ArkUI`, `ArkUICore`)
└── kotlin/           the Compose library, package `dev.arkui`
```

The Rust crates are members of the one workspace at `rust/Cargo.toml`
(by path, `../ui/rust/…`), beside the kernel. The apps take the Swift
package by path in their `project.yml` (`../../ui/swift`) and the
Kotlin sources by path in their `build.gradle.kts` (`../../ui/kotlin/src`).

Before a change to motion, a gesture, a press or a haptic in any of them,
read `docs/motion.md` (the repository's `CLAUDE.md` says so too).

## The theme

`theme.toml` is the one place a colour of any Ark app is written down. Every
app shares one calm neutral base and the same signals; an app differs only in
its accent: harken's gold, money's green. `theme.py` writes it out for each
platform's arkui:

| Platform | Generated | An app takes its accent with |
| --- | --- | --- |
| Android (Compose) | `ui/kotlin/src/dev/arkui/Tokens.kt` | `arkuiScheme(Accent.Gold, dark)`, and `Accent.Gold.art(dark)` for the squares |
| iPhone (SwiftUI) | `ui/swift/Sources/ArkUICore/Tokens.swift`, `ui/swift/Sources/ArkUI/Colors.swift`, each app's `AccentColor.colorset` | `Accent.use(.gold)` at launch, then `.ink`, `.quiet`, `.accent`… |
| Desktop (iced) | `ui/rust/arkui/src/tokens.rs` | `arkui::theme::install(arkui::tokens::GOLD)` |
| Browser (arkui-web) | `ui/rust/web/theme.css` (CSS variables, the springs as `linear()`, the size classes' container queries) | `arkui_web::run(app, "gold", css)` (`data-accent`) |

After editing `theme.toml`, from the repository's root:

    nix run .#update-theme

`checks.theme` fails when a generated file is not what the tokens make, and
`checks.theme-contrast` fails when a pair is under its ratio. Both run in CI.
`python3 ui/theme.py contrast` prints every pair and its ratio.

### The roles

Named by what they are for, never by hue.

- **background**: the page. **surface**: a plane over it (a sheet, a bar).
- **container-lowest … container-highest**: the steps a container takes off
  the page, from a card's faint fill to a pressed row or a meter's track.
- **glass**: the fill of a glass chip, menu and sheet (Material's
  `surfaceBright`, which `Glass.fill()` reads).
- **ink**: text. **quiet**: secondary text, a subtitle or a count.
  **disabled**: what cannot be pressed, and a transcript's past lines.
- **outline**: a control's edge, 3:1. **hairline**: a divider.
- **scrim**, **inverse-surface**, **inverse-ink**: a dialog's dim and a snackbar.
- The signals: **error**, **caution** and **positive**, with **on-** colours
  and quiet **-container**s for error and caution.
- An accent: **accent** (a filled button, a slider, a selected tab),
  **on-accent**, **accent-text** (a link, the row playing),
  **accent-container** and **on-accent-container**.
- **art**: the derived squares' gradients, each a pair of stops for each side.
- **wash**: the full player's blurred cover and how much of it shows.

In Material, `primary` is the accent, `secondary` is a neutral grey (so a
tonal button still shows on an accent card), `tertiary` is the caution and
`onSurfaceVariant` is quiet. Nothing is tinted by elevation.

### The contrast rule

WCAG 2's ratio, as `[contrast]` lists it:

- every text role (ink, quiet, accent, accent-text and the signals) on every
  ground (background, surface, each container, glass) at **4.5:1**;
- ink, quiet and accent-text on the player's wash, a pale cream cover at the
  wash's opacity over the page, at **4.5:1**;
- each colour on the fill it is written on (on-accent on accent and the
  rest) at **4.5:1**;
- `large` pairs, for large text, icons and a control's edge, at **3:1**.

Disabled is exempt, as WCAG has it. A colour that is only ever drawn large
goes in `large`, never in `text`.

### Adding an app's accent

1. Add an `[accent.<name>]` table: the five roles for `light` and for
   `dark`, `art` naming an `[art.<name>]` set (add one, or use `neutral`),
   and `colorsets` listing the iPhone app's
   `Assets.xcassets/AccentColor.colorset`, if it has one.
2. `nix run .#update-theme`, then `python3 ui/theme.py contrast`, and
   adjust until every pair passes. The light accent has to be deep enough to
   be text on white and on the palest container; the dark one bright enough
   to read on the glass.
3. Draw the app in it: `arkuiScheme(Accent.<Name>, dark)` on Android,
   `Accent.use(.<name>)` on the iPhone, `arkui::tokens::<NAME>` on the
   desktop.
