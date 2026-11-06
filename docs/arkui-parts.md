# ArkUI's generic parts

The small pieces a page is laid out of that any Ark app can use unchanged.
Each has **one name and the same inputs in every toolkit**: Compose
(`kotlin/arkui`, package `dev.arkui`), SwiftUI (`swift/ArkUI`) and iced
(`rust/arkui`, functions in snake case). A future client (the web) builds
the same list.

The rule for what goes here: a part goes in ArkUI only if another app could
use it as it is — no money word, no media word, no domain type in its
inputs. What an app is about (a transaction's row, a track's row, the
player) stays in the app and is listed in `spec/<app>-components.md`; where
such a component has a generic piece in it, it is made of the ArkUI part.

Every colour is the theme's (`arkui/theme.toml`), every move one of the
shared springs (`docs/motion.md`), every press `pressable` / `.press` /
`.pressRow`. The desktop keeps the desktop's measures — a pointer is
precise and a window wide — so a card is rounded by 8 there and by 20 on a
phone: what is the same is the part and what it is handed.

| Part | Inputs | Compose | SwiftUI | iced |
|---|---|---|---|---|
| Tint | neutral, positive, caution, negative, accent | `Tint`, `tintColors`, `tintInk` | `Tint` | `parts::Tint` |
| PageHeader | kind, title, facts, by?, onBy?, cover, actions | `PageHeader` | `PageHeader` | `parts::page_header` |
| SectionHeader | title, trailing | `SectionHeader` | `SectionHeader` | `parts::section_header` |
| GroupCard | rows (content) | `GroupCard` (a `ListRow` in it is a grouped row) | `GroupCard` | `parts::group_card` (`style::card`) |
| RowDivider | inset | `RowDivider` | `RowDivider` | `parts::row_divider` |
| Banner | glyph, text | `Banner(icon, text)` | `Banner(symbol:text:)` | `parts::banner` |
| Pill | tint, content | `Pill` | `Pill` | `parts::pill` (`style::pill`) |
| Chip | label, selected, action | `Chip` | `Chip` | `parts::chip` (`style::chip`) |
| Badge | count, said | `Badge` | `Badge` | `parts::badge` |
| Tile | glyph, colour, side | `Tile` | `Tile` | `parts::tile` |
| ProgressBar | fraction, colour or tint, thickness | `ProgressBar` | `ProgressBar` | `parts::progress_bar` (`style::progress`) |
| Bars | parts (label, value, colour), description, value writer | `Bars` | `Bars` | `parts::bars` |
| Donut | parts, description, thickness, centre | `Donut` | `Donut` | — (iced's canvas is not built) |
| SearchField | query, onQuery, placeholder, onSearch | `SearchField` | `SearchField` | `parts::search_field` (`style::field`) |
| NameField | name, onName, problem, hint, label | `NameField` | `NameField` | `parts::name_field` (`style::field`) |
| Pager | title, back, forward, home?, homeLabel | `Pager` | `Pager` | `parts::pager` |
| Scrubber | value, onValue, onDone, left, right, enabled, label | `Scrubber` | `Scrubber` | `parts::scrubber` |
| Bar (the slider) | value, onValue, onDone | `Bar` | `Bar` | `parts::bar` |
| VolumeSlider | level, muted, about, onLevel, onDone, onMute | `VolumeSlider` | `VolumeSlider` | — (the desktop's mute and bar are two controls) |
| Glyphs | a Lucide name | `ArkGlyphs.named` | `ArkGlyphs.symbol` (an SF Symbol) | `glyphs::named` |

The slider is called `Bar`, not `Slider`: `Slider` is SwiftUI's and
Material's own, and an app that imports both could not say which.

**Glyphs.** The shared names are Lucide's: `chevron-left`,
`chevron-right`, `chevron-down`, `ellipsis-vertical`, `x`, `plus`, `minus`,
`check`, `circle-check`, `circle`, `search`, `wifi-off`, `volume-2`,
`volume-x`, `log-out`. Android and the desktop draw Lucide's paths; the
iPhone draws the SF Symbol each maps to.

## A page about somebody

`rust/arkui/src/hero.rs`'s catalogue page has its twins on the phones:

| Part | Inputs | Compose | SwiftUI | iced |
|---|---|---|---|---|
| HeroBanner | picture, kind, name | `HeroBanner(seed, art, kind, name)`, `HeroBanner.height` | `HeroBanner(image:kind:name:)`, `HeroBanner.height` | `hero::banner` |
| HeroHeading | title | `HeroHeading` | `HeroHeading` | `hero::heading` |
| HeroEntry | over, title, under, count, open, picture | `HeroEntry` | `HeroEntry` | `hero::entry` / `hero::listing` |
| HeroMore | label, open | `HeroMore` | `HeroMore` | `hero::more` |
