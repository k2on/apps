# Layout Primitives Under the Parts

`ark_app::view` (ui/rust/app/src/view.rs) lays a screen out of ArkUI's
parts (ui/docs/parts.md): a tab's page, a card's section, a list's row, a
sheet. Each part is written by hand four times — SwiftUI, Compose, iced and
the browser — and the generator only calls it by name. This page adds a
layer under them: a few **primitives** that the generator writes out as
each toolkit's own layout, so a purely visual part is written once, in
Rust, and drawn natively everywhere.

The rule that decides which is which:

- **A container that carries platform behaviour stays a native part**:
  lists and sections (an inset-grouped `List` on the iPhone, with its
  separators, swipe, reorder and VoiceOver's grouping), navigation, tabs,
  sheets, menus, forms, toolbars, pickers, fields, a scrubber or a slider.
  Rebuilding one from stacks would lose what the platform gives it.
- **What is only drawn moves to the primitives**: a header card's face, a
  goal's bar and note, an empty list's sentence, a block of titles. Written
  once in Rust from stacks, text and glyphs, with the theme's tokens.

## What Harken Lays Out

Harken's phones are still hand-written (harken/ios, SwiftUI; harken/android,
Compose); its port to Rust comes later. What it lays out, and what that
asks of ArkUI:

| Screen | Native containers (stay parts) | Stack-composed visuals | Components that fall out |
|---|---|---|---|
| **Full player** (`PlayerViews.swift` `FullPlayer`, `PlayerScreens.kt` `FullFace`) | the sheet it is presented in, pull-to-close, `Menu` of the track's actions, the scrubber (`Scrubber`, a drag with haptics), `VolumeSlider`, the queue/devices/transcript buttons' sheets | a `VStack` of a bar row (chevron, the album's name, the menu), the cover sized to the room (`Spacer`s either side), a leading `VStack` of title (bold, `.title2`) and artist (accent, linked), the scrubber, the transport `HStack`, a row of chips | **transport controls** (shuffle · back · a large prominent play · forward · repeat, `Spacer`s between); **title stack** (title over a linked subtitle); **artwork** at a size, its corner a token |
| **Now-playing bar** (`MiniPlayer`, `MiniFace`) | the tab view's bottom accessory, its sideways swipe (`Momentum`, `Springs.released`) | `HStack(spacing: m)` of a small cover, a leading `VStack(spacing: 0)` (title semibold, the line under it quiet, one line each), a play/pause glyph button, a next glyph button; a hairline of progress at its foot | **media row** (artwork, title stack, trailing glyph buttons); **progress hairline** |
| **Album / artist headers** (`Hero.swift` `RecordHeader`, `PersonBanner`, `Featured`; `Components.kt` `CoverHeader`, `Featured`) | the inline header's fold into the navigation bar (`inlineHeader`, a scroll-geometry read), `HeroBanner` under the status bar (safe areas) | `HStack(spacing: l + 4)` of the cover and a leading `VStack(spacing: xs)` of kicker (caption, quiet), title (`.title2` bold), by (headline, accent), small (caption); `PlayShuffle` under it | **media header** (cover beside a title stack, two wide buttons under it); **title stack**; **play/shuffle pair** (a `WideButton` pair) |
| **Track rows** (`TrackRow`, `EpisodeRow`, `QueueRow`; `TrackRow` in Kotlin) | the row's press (`.pressRow`, `Press.Highlight`), its context menu, swipe, the list's separators | `HStack(spacing: m)`: leading cover or number at `thumb`, a leading `VStack(spacing: 2)` of title (body, accent while playing) and its line (subheadline, quiet), `Spacer(minLength: 0)`, a length in monospaced caption, the `…` menu | **media row** (above); **figure text** (monospaced digits); an episode's **people line** (overlapping avatars + words) |
| **Library grid** (`AlbumsGrid`, `ShowsGrid`; `Albums` in Kotlin) | `LazyVGrid(.adaptive(minimum: 150))` / `LazyVerticalGrid` — a lazy grid is a container (it recycles), `.zoomSource` for the navigation zoom | each cell a leading `VStack(spacing: 2)`: square cover, title (subheadline semibold, one line), a quiet caption | **artwork tile** (artwork over a title and a caption: `CoverCard`), round for a person |
| **Home shelves** (`CoverShelf`, `Heading`, `LastCard`) | `ScrollView(.horizontal)` + `LazyHStack` (lazy, a container); the shelf's heading's action button | `Heading`: `HStack` of a `.title2` bold title, `Spacer`, a borderless action; `LastCard`: `HStack` of cover, a title stack of three lines, `Spacer`, a round play glyph — on a glass card (`surface`, `card-corner`) | **section heading** (title, trailing action); **artwork tile**; **media row** on a card |
| **Search results** (`SearchView`, `SearchScreen.kt`) | `List` + `.searchable`, `Section`s, recent searches as rows, `ContentUnavailableView.search` | the albums and artists shelves inside a section (`listRowInsets(EdgeInsets())` to break out of the row's inset) | **empty state**; **artwork tile**; **track row** |
| **Devices sheet** (`DevicesSheet`, `DeviceLine`; `Sheets.kt`) | the sheet and its navigation bar (Done), the `List`, each line's press, the join button, `VolumeSlider` | a line: `HStack(spacing: l)` of a device glyph (24 wide), a leading `VStack` of name and a caption, `Spacer`, a playing glyph | **glyph row** (glyph, title stack, trailing mark) — ArkUI's `list_row` already covers it |
| **Queue** (`QueueSheet`, `QueueSheet` Kotlin) | the sheet, `Reorderable` (drag to reorder, handles), the context menu | headings, "Nothing after this one." as a quiet sentence | **empty state** (one sentence); **section heading** |
| **Car** (`Car.kt`) | none drawn by the app: Android Auto draws its own templates from the media browse tree (`MediaItem`s, `EXTRAS_KEY_ROOT_CHILDREN_LIMIT`) | — | none: a car is data, not layout |
| **Watch** | harken has no watch app on either platform | — | — |
| **Onboarding** (`OnboardingViews.swift` `OnboardingPage`, `Onboarding.kt`) | the fields (`AddressField`), `WideButton`, the scan button's camera sheet — already ArkUI's `first_run_page`, `address_field`, `wide_button` | a centred `VStack(spacing: s)`: the mark on a rounded tile, a large-title title, a quiet sentence, the content; at most 420 wide | **title stack** (centred); **glyph tile** |
| **Settings, diagnostics** (`SettingsView.swift`, `SettingsScreen.kt`) | `Form`, sections, toggles, pushes | `Number`: a label over a figure | **figure with label** (`Number`, money's summary facts) |
| **Empty and waiting** (`ContentUnavailableView` ×8 on the iPhone, ArkUI's Kotlin `Empty`; `Fetching`) | — | a centred `VStack` of a large quiet glyph, a title, a quiet sentence; for `Fetching` a spinner under the words | **empty state** |
| **Avatar** (`Avatar`, `PeopleLine`, `AccountButton`) | the account sheet; the face's picture loading (`Covers`) | a `ZStack` of a circle in the name's colour, the initials, the picture over them, clipped round | **avatar** — ArkUI already has it (`AccountFace` / `Part::Account`, money's toolbar); harken's `Avatar` is the same face at another size |

What falls out, and who uses it:

| Component | What it is | harken | money |
|---|---|---|---|
| **Empty state** (`empty_state`) | a quiet glyph, a title, a sentence, centred | Home, Library's tabs, Search, the queue, Devices | an empty register, no transactions to review, nothing spent yet |
| **Title stack** (`title_stack`) | a kicker over a title over a subtitle over a caption, leading or centred, each only while it has words | the players, record headers, `Featured`, `LastCard`, rows, onboarding | the header card (title, line, caption) |
| Glyph tile (`Part::Tile`, now a leaf the primitives can hold) | a glyph on a tinted rounded tile | onboarding's mark, the new-playlist row | the header card's glyph |
| Status badge (`Part::StatusBadge`, a leaf) | a figure in a capsule of its tint | a download's state | a category's available |
| Meter (`Part::Meter`, a leaf) | a bar of how far, in a tint | an episode's progress, the now-playing hairline | a goal's progress |
| Artwork tile | artwork over a title and a caption | the shelves, the grids, search | — (receipts' photos, later) |
| Media header | artwork beside a title stack, actions under | album, artist, show, playlist, recording | — |
| Media row | artwork, a title stack, trailing glyph buttons | tracks, episodes, the now-playing bar, the queue | — (`list_row` is money's) |
| Transport controls | shuffle, back, play, forward, repeat | the full player | — |
| Progress scrubber | ArkUI's `Scrubber` (a native part: a drag) | the full player | — |
| Chip row | ArkUI's `chip_row` (native: it wraps) | Library's tabs | filters |
| Figure with label | a quiet label over a figure | diagnostics, a record's foot | the summary card's two facts |
| Avatar | ArkUI's `Part::Account` face, generic | people, the account | the toolbar's account |

The first two are built here and used by money now; the leaves are what
the rebuilt parts needed. The rest wait for harken's port: each is a
`fn` in `ark_app::view` returning primitives, like the first two.

## The Primitives

All in `ark_app::view`, used inside a `#[view]` like any part:

```rust
vstack((
    glyph("target").style(TextStyle::Title).ink(Ink::Accent),
    text(c.name).style(TextStyle::Headline).if_any(),
    text(c.figure).style(TextStyle::Figure).ink(Ink::Figure(c.tint.into())),
))
.spacing(Space::Xs)
.align(Align::Center)
.padding(Edges::Vertical, Space::Xl)
.fill_width()
.combine()
```

| Primitive | Inputs | Methods |
|---|---|---|
| `vstack(content)`, `hstack(content)`, `zstack(content)` | what it holds | `.spacing(Space)` (none: 0), `.align(Align)` across its axis (none: centred) |
| `spacer()`, `spacer_min(Space)` | — | takes what is left along its stack's axis |
| `scroll(content)`, `scroll_across(content)` | what scrolls | vertical, or sideways |
| `text(words)` | words read or written | `.style(TextStyle)`, `.ink(Ink)`, `.lines(n)`, `.centred()`, `.if_any()` (nothing while empty) |
| `glyph(name)` | a Lucide name (an SF Symbol on Apple's) | `.style(TextStyle)` (its size), `.ink(Ink)` |
| `image(url, label)` | a picture by URL | as `photo` |
| `tile(glyph, tint)`, `status_badge(figure, tint)`, `meter(fraction, tint)` | leaves the primitives hold: ArkUI's `Tile`, `StatusBadge`, a bar | — |

And on every primitive (the `Modify` trait):

| Modifier | What |
|---|---|
| `.padding(Edges, Space)` | `Edges::All`, `Horizontal`, `Vertical`, `Top`, `Bottom`, `Leading`, `Trailing` |
| `.fill_width()`, `.fill_width_at(Align)`, `.fill_height()` | as wide (tall) as it is given, its content centred or at `Align` |
| `.background(Surface)` | `Surface::Card` (the card's container), `Surface::Raised` (a container above it) |
| `.corner(Corner)` | `Corner::Card` (`card-corner`), `Corner::Small` (`corner`): clips |
| `.combine()` | one element to a screen reader, its words in order |

## Tokens Only

A primitive takes no number. Every gap and inset is a `Space`, every corner
a `Corner`, every colour an `Ink` or a `Surface`, every face a `TextStyle`:

| Token | Is | SwiftUI | Compose | iced | Browser |
|---|---|---|---|---|---|
| `Space::Xs` … `Xxl`, `Page`, `Section` | `[layout]`'s `space-*`, `page-margin`, `section-gap` | `Space.xs` … | `Space.xs` … | `layout_tokens::SPACE_XS` … | `var(--ark-space-xs)` … |
| `Corner::Card`, `Small` | `card-corner`, `corner` | `LayoutTokens.cardCorner` | `LayoutTokens.CARD_CORNER.dp` | `CARD_CORNER` — the desktop's card is 8 (`style::card`) | `var(--ark-card-corner)` |
| `TextStyle::LargeTitle`, `Title`, `Headline`, `Body`, `Subheadline`, `Footnote`, `Caption`, `Figure` | the system's named styles on the phones (they follow Dynamic Type and the font scale); the desktop's sizes, `[layout]`'s `type-*` | `.arkType(_:)`: `.largeTitle`, `.title2`, `.headline`, `.body`, `.subheadline`, `.footnote`, `.caption`, the large rounded bold figure | `ArkType.*.style`: `headlineLarge`, `titleLarge`, `titleMedium`, `bodyLarge`, `bodyMedium`, `bodySmall`, `labelSmall`, `displaySmall` bold | `layout_tokens::TYPE_*`, bold for the headline, titles and the figure | `.ark-type-*` over `var(--ark-type-*)` |
| `Ink::Ink`, `Quiet`, `Accent`, `Tint(name)`, `Figure(name)` | the theme's roles; a tint by name (`neutral` is quiet), or as a figure (`neutral` is the ink) | `Color.ink`, `.quiet`, `.accentText`, `Tint(named:).color`, `.figure` | `ArkInk.*` over the scheme, `tintInk` | `theme::of(t)…`, `parts::Tint` | `.ark-ink-*`, `.ark-tint-*` |
| `Surface::Card`, `Raised` | `container-low`, `container-high` | `Color.surfaceContainerLow`… via `ArkSurface` | `surfaceContainerLow`, `surfaceContainerHigh` | `style::card`'s colours | `var(--ark-container-low)` |

So CLAUDE.md's rule holds for layout as it does for motion: a value is
written once, in `ui/theme.toml`, and a view names it.

## The Boundary

| Stays a native part | Why |
|---|---|
| `tab_page`, `detail_page` | the large title, pull to refresh, the inset-grouped `List` and its scroll |
| `card_section` | a `Section`: its card, separators, header, footer, the list's VoiceOver grouping, swipe and reorder on its rows |
| `list_row`, `meter_row`, `table_row`, `data_table` | presses, menus, the disclosure chevron, the table that becomes a list |
| `form_sheet`, `form_section`, the fields, pickers, `switch_row`, `date_field` | the keyboard, focus, the system's pickers |
| `tabs`, `toolbar`, `split`, `push`, `inspector`, `searchable` | navigation |
| `confirm`, `alert`, `prompt` | the system's dialogs |
| `photo`, `zoom_photo`, `scrubber`, the charts | gestures, decoding, drawing |

| Moves to the primitives | Built in `view.rs` as |
|---|---|
| `header_card` | a `card_section` holding a centred `vstack` (tile, title stack, figure, line, caption), then its actions as rows |
| `goal_progress` | a leading `vstack` of a `meter` and a quiet footnote, at the row's inset |
| `empty_state` (new) | a centred `vstack` of a quiet glyph and a title stack |
| `title_stack` (new) | a `vstack` of up to four `text`s, each only while it has words |
| next: `summary_card`, `hero_figure`, `headline`, `paper` | they need what is below under Hard Cases (a figure in cents, "fits", parallel lists) |

## How Each Maps

| Primitive | SwiftUI | Compose | iced | Browser |
|---|---|---|---|---|
| `vstack` | `VStack(alignment:spacing:)` | `Column(verticalArrangement = spacedBy, horizontalAlignment)` | `column().spacing().align_x()` | `div.ark-stack.ark-v` (flex column, `gap`) |
| `hstack` | `HStack(alignment:spacing:)` | `Row(horizontalArrangement = spacedBy, verticalAlignment)` | `row().spacing().align_y()` | `div.ark-stack.ark-h` |
| `zstack` | `ZStack(alignment:)` | `Box(contentAlignment)` | `stack![]`, each centred | `div.ark-stack.ark-z` (grid, one cell) |
| `spacer` | `Spacer(minLength:)` | `Spacer(Modifier.weight(1f))` (and `widthIn`/`heightIn(min)`) | `Space` filling its stack's axis | `div.ark-spacer` (`flex: 1`) |
| `scroll` | `ScrollView` (`.horizontal`) | `verticalScroll` / `horizontalScroll(rememberScrollState())` | `scrollable` | `overflow: auto` |
| `text` | `Text(_)` `.arkType` `.foregroundStyle` `.lineLimit` `.multilineTextAlignment` | `Text(style, color, maxLines, overflow, textAlign)` | `text().size().font().style()` | `div.ark-text.ark-type-*` |
| `glyph` | `Image(systemName: ArkGlyphs.sf(_))` `.arkType` | `ArkGlyph(name, ArkType.*)` (Lucide, sized by the style) | `icon::tinted` at the style's size | the inline SVG |
| `.padding` | `.padding(.horizontal, Space.l)` | `Modifier.padding(horizontal = Space.l)` | `container().padding()` | `.ark-pad-*` |
| `.fill_width` | `.frame(maxWidth: .infinity, alignment:)` | `.fillMaxWidth().wrapContentWidth(align)` | `width(Fill)`, aligned | `.ark-fill-w` |
| `.background`, `.corner` | `.background(…)`, `.clipShape(.rect(cornerRadius:))` | `.clip(RoundedCornerShape).background()` | `container::Style` | `.ark-surface-*`, `.ark-corner-*` |
| `.combine` | `.accessibilityElement(children: .combine)` | `.semantics(mergeDescendants = true) {}` | — (iced has no screen reader yet) | `role="group"` |

SwiftUI applies modifiers inside out and Compose's `Modifier` chain
outside in, so the generator writes a primitive's modifiers in its order
for Swift and reversed for Kotlin: `.padding().background().corner()` is
padded, filled, then clipped on both.

A primitive written straight into a part's slot (a `card_section`'s rows,
a page's content) is a row of the iPhone's `List`; the generator gives it
`.listRowInsets(EdgeInsets())` so its own padding is the only inset on
both phones, as Compose's `GroupCard` adds none.

## Hard Cases

- **Text sizing and Dynamic Type.** The phones take a `TextStyle` by name,
  so Dynamic Type (iOS) and the font scale (Android) grow it; a figure
  that must stay on one line says `.lines(1)` (SwiftUI shrinks nothing on
  its own: the summary card's `minimumScaleFactor` is not a primitive yet).
  The desktop's sizes are fixed (`type-*`). A glyph is sized by its style,
  so it grows with the words beside it.
- **Flexible frames.** Only "as wide as given" (`fill_width`) and its
  alignment. SwiftUI's `ViewThatFits` (the summary card's facts beside or
  under the sparkline) and Compose's `FlowRow` are not alike enough to be
  one primitive; the summary card stays a part until a `fits(wide, narrow)`
  primitive is designed (SwiftUI `ViewThatFits`, Compose a
  `SubcomposeLayout` that measures the first).
- **Alignment guides.** Cross-axis alignment only (`Align::Start`,
  `Center`, `End`). Baselines (`.lastTextBaseline`, harken's `WorkHeading`)
  and custom guides are not offered; Compose's `alignByBaseline` exists, so
  a `Align::Baseline` for `hstack` is the likely next one.
- **Safe areas.** A primitive never ignores one; a part that reaches under
  the status bar (`HeroBanner`, the player's backdrop) stays native.
- **Scroll and list interplay.** `scroll` is for a sideways shelf or a page
  that is not a list. A vertical `scroll` inside a `tab_page` (a `List`)
  would nest scrolling; laziness (`LazyVGrid`, `LazyHStack`) is a
  container's, so the grid and the shelf stay parts. Inside a list a
  primitive is a row (above).
- **Accessibility grouping.** `.combine()` makes one element of a face (a
  header card reads "Groceries, $40.00, Overspent by $21.02"). A list's
  grouping and rows' actions are the native parts'.
- **Right to left.** `Align::Start`/`End` and `Edges::Leading`/`Trailing`
  are leading and trailing (SwiftUI `.leading`, Compose `Start`, the
  browser's flex start), so they flip in a right-to-left language; nothing
  says left or right. iced does not lay out right to left yet.
- **Values in cents.** A `text` takes words; a figure in cents is formatted
  by the app's record (money's figures already are), or, for a part like
  the summary card, by the toolkit's own formatter — not yet a primitive.
- **Parallel lists.** A `for` walks one list; a receipt's names, figures
  and notes are three. `paper` stays a part until its record gives a list
  of lines.

## What Is Verified

The generated SwiftUI and Compose are held by `ui/rust/gen/tests/snapshots`
(a view of primitives, and the rebuilt header card); the view model by
`ui/rust/app/src/tests.rs`. The phones' code is compiled in CI (mac-compile,
build); the look against the hand-written parts was not compared on a
device.
