# The theme

`theme.toml` is the one place a colour of any Ark app is written down. Every
app shares one calm neutral base and the same signals; an app differs only in
its accent: harken's gold, money's green. `theme.py` writes it out for each
platform's arkui:

| Platform | Generated | An app takes its accent with |
| --- | --- | --- |
| Android (Compose) | `kotlin/arkui/src/dev/arkui/Tokens.kt` | `arkuiScheme(Accent.Gold, dark)`, and `Accent.Gold.art(dark)` for the squares |
| iPhone (SwiftUI) | `swift/ArkUI/Sources/ArkUICore/Tokens.swift`, `swift/ArkUI/Sources/ArkUI/Colors.swift`, each app's `AccentColor.colorset` | `Accent.use(.gold)` at launch, then `.ink`, `.quiet`, `.accent`… |
| Desktop and web (iced) | `rust/arkui/src/tokens.rs` | `arkui::theme::install(arkui::tokens::GOLD)` |

After editing `theme.toml`, from the repository's root:

    nix run .#update-theme

`checks.theme` fails when a generated file is not what the tokens make, and
`checks.theme-contrast` fails when a pair is under its ratio. Both run in CI.
`python3 arkui/theme.py contrast` prints every pair and its ratio.

## The roles

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

## The contrast rule

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

## Adding an app's accent

1. Add an `[accent.<name>]` table: the five roles for `light` and for
   `dark`, `art` naming an `[art.<name>]` set (add one, or use `neutral`),
   and `colorsets` listing the iPhone app's
   `Assets.xcassets/AccentColor.colorset`, if it has one.
2. `nix run .#update-theme`, then `python3 arkui/theme.py contrast`, and
   adjust until every pair passes. The light accent has to be deep enough to
   be text on white and on the palest container; the dark one bright enough
   to read on the glass.
3. Draw the app in it: `arkuiScheme(Accent.<Name>, dark)` on Android,
   `Accent.use(.<name>)` on the iPhone, `arkui::tokens::<NAME>` on the
   desktop.
