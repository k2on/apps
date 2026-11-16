# Layout Primitives Under the Parts

`ark_app::view` (ui/rust/app/src/view.rs) lays a screen out of ArkUI's
**primitives**, which the generator writes out as each toolkit's own
layout, and its **native containers and controls**, each written by hand
once per toolkit — SwiftUI, Compose, iced and the browser — and called by
name. Everything else is a **component**: a Rust composition of those,
written once and drawn natively everywhere.

The rule that decides which is which:

- **A container or control that carries platform behaviour stays a native
  part of ArkUI's**: lists and sections (an inset-grouped `List` on the
  iPhone, with its separators, swipe, reorder and VoiceOver's grouping),
  navigation, tabs, sheets, menus, forms, toolbars, pickers, fields, a
  press with its menu, a scrubber or a slider, the now-playing accessory.
  Rebuilding one from stacks would lose what the platform gives it.
- **What is only drawn is a composition of the primitives**: a header
  card's face, a goal's bar and note, an empty list's sentence, a block of
  titles, a cover, a row of a song. Written once in Rust from stacks,
  text, glyphs, pictures and frames, with the theme's tokens.
- **A component lives with who uses it.** One app's own is that app's
  (`harken-app`'s `parts`: its covers, tiles, rows, headers and player
  controls); one several apps really share is ark-app's
  (`ark_app::view`'s components: `title_stack`, `empty_state`). None is
  ever hand-written per toolkit, and ArkUI holds none of an app's.

## What Harken Lays Out

Harken's screens are Rust now (harken-app's `#[view]`s, ui/docs/app.md,
"harken's Port"), drawn by every toolkit from these parts. What it lays
out, and what that asked of ArkUI:

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

What falls out is harken's own, so it is harken's: Rust compositions in
`harken/app/src/parts.rs`, used by its `#[view]`s like any part. Each is
the primitives below and ArkUI's press; nothing of it is per toolkit.

| Component (`harken_app::parts`) | API | Composed of |
|---|---|---|
| Artwork | `artwork(url, seed, label).size(ArtSize::Row \| Shelf \| Header \| Fill).round()` | `zstack((swatch(seed), image(url, label).fill().decode(m)))`, then the square first — `.side(m)` at `art-row`, `art-shelf` or `art-header`, or `.square()` of the width it is given — then `.corner(Corner::Small)` or `Corner::Circle` (a person). The picture covers its square and is cut to it on every toolkit; the derived square shows until it comes; a screenshot waits for it |
| Artwork tile | `artwork_tile(art, title, caption, tap)` | `pressable` (a give) round a leading `vstack` of the cover, a one-line subheadline and a quiet one-line caption, `.width(m)` its cover's measure, or `.fill_width_at(Start)` in a grid's cell |
| Artwork shelf | `artwork_shelf(tiles)` | `scroll_across(hstack(tiles).spacing(M).align(Start).padding(Horizontal, Page))` |
| Artwork grid | `artwork_grid(tiles)` | `grid(Measure::ArtShelf, tiles).spacing(M).line_spacing(L).padding(Horizontal, Page)` |
| Media row | `media_row(title, tap).art(..).under(..).trailing(..).menu(..)` | `press_row` (a row's light, its menu) round an `hstack` of the art, a plain one-line `title_stack`, `spacer_min(Xs)` and the end |
| Media header | `media_header(art, titles, actions)` | a `vstack` of an `hstack` of the cover at `art-header` and the titles, the actions under both, at the page's margins |
| Play/shuffle pair | `play_shuffle(play, shuffle)` | two `wide_button`s in an `hstack` |
| Glyph button | `glyph_button(name, label, style, ink, tap)` | `pressable` round a padded `glyph` |
| Transport controls | `transport(play_glyph, play_label, toggle, back, forward).shuffle(tint, tap).repeat(glyph, tint, tap)` | glyph buttons and `spacer`s across the width |
| Figure with label | `figure_label(label, figure)` | a quiet caption over a one-line headline, `.combine()`d |

What stays ArkUI's, because it is native:

| Native | API (`ark_app::view`) | Why it is not composed |
|---|---|---|
| Pressed as one | `pressable(label, tap, content)` (a give), `press_row(label, tap, content)` (a row's light), `.menu(title, glyph, tap)` | `Part::Press`: the platform's button and context menu (SwiftUI's `Pressable`, a `Button` in `.press`/`.pressRow`; Compose's `Modifier.pressable` with `Press.Give`/`Highlight`, its menu held; iced's bare button; the browser's `<button class="ark-press">`); its walk id a row's by its label when it lights, else what it sends |
| Scrubber, hairline | `scrubber(at_ms, since_ms, playing, length_ms, seek)`, `hairline(..)` | `Part::Timeline`: counted from its clock as it is drawn (`TimelineView`, a ticking state, iced's tick while one plays, an animation frame in the DOM), a drag with a `Selection` haptic at either end, `seek` once on release — a time-driven control and a gesture |
| Volume | `volume(label, level, muted, change, mute)` | `Part::Volume`: `VolumeSlider` on the phones (a level from elsewhere sprung to), iced's slider, the browser's `<input type="range">` — a slider |
| Now-playing accessory | `adaptive_tabs(..).accessory(bar, full, open, close).showing(on).expanded(on).skips(next, previous)` | the shell's `Part::Tabs` with an `Accessory`: SwiftUI's `expandingCard` (`tabViewBottomAccessory` zooming into a `fullScreenCover`), Compose's `ExpandingCard`, iced's bar at the foot and the full window over it, the browser's — a shell container |

ui/rust/gen/tests/snapshots holds what the emitters write for the
primitives a cover and a grid are made of, and for the native controls
(`MediaView.swift`, `MediaScreen.kt`, `TabsView.swift`, `TabsScreen.kt`);
harken's compositions are checked by app-screens' sheets, not by tests of
their views.

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
| `text(words)` | words read or written (a field, never a `for`'s item itself: `#[view]` takes a bare binding for a sub-view's record) | `.style(TextStyle)`, `.ink(Ink)`, `.lines(n)`, `.centred()`, `.if_any()` (nothing while empty) |
| `glyph(name)` | a Lucide name (an SF Symbol on Apple's) | `.style(TextStyle)` (its size), `.ink(Ink)` |
| `image(url, label)` | a picture by URL (`file:`, `data:`, `https:`), said as `label` (nothing when empty) | fitted until told: its own shape at the width given, as `photo`. `.fill()`: exactly the frame it is given, the picture covering it and cut to it — never its own aspect — fetched and kept by the toolkit, decoded once per size, nothing drawn until it has come (what is under it shows), a screenshot waiting for it to come or fail. `.decode(Measure)`: decoded for that side, not the most a filling picture is drawn at |
| `swatch(seed)` | a name | the theme's derived square for it (`art` gradients, `arkui::art::hash` on every toolkit), filling the frame it is given: what stands under a picture until it comes |
| `grid(min: Measure, content)` | its cells | a native, lazy where the toolkit has one, container of as many equal columns as fit at least `min` wide, each cell a column's width; `.spacing(Space)` between columns (`space-m` until told), `.line_spacing(Space)` between rows (`space-l`) |
| `tile(glyph, tint)`, `status_badge(figure, tint)`, `meter(fraction, tint)` | leaves the primitives hold: ArkUI's `Tile`, `StatusBadge`, a bar | — |

And on every primitive (the `Modify` trait):

| Modifier | What |
|---|---|
| `.padding(Edges, Space)` | `Edges::All`, `Horizontal`, `Vertical`, `Top`, `Bottom`, `Leading`, `Trailing` |
| `.fill_width()`, `.fill_width_at(Align)`, `.fill_height()` | as wide (tall) as it is given, its content centred or at `Align` |
| `.background(Surface)` | `Surface::Card` (the card's container), `Surface::Raised` (a container above it) |
| `.corner(Corner)` | `Corner::Card` (`card-corner`), `Corner::Small` (`corner`), `Corner::Circle` (round): clips |
| `.width(Measure)` | as wide as a measure, what it holds kept to it (a tile's words under its cover) |
| `.side(Measure)` | a square the measure a side; what it holds is laid into it |
| `.square()` | as tall as it is wide, of the width it is given; what it holds is laid into that square (iced, which has no aspect-ratio layout, draws an `art-shelf` square) |
| `.combine()` | one element to a screen reader, its words in order |

A method of another primitive (`.spacing` on words) panics when the view
is built or recorded, so a test of the view finds it.

## Tokens Only

A primitive takes no number. Every gap and inset is a `Space`, every corner
a `Corner`, every colour an `Ink` or a `Surface`, every face a `TextStyle`:

| Token | Is | SwiftUI | Compose | iced | Browser |
|---|---|---|---|---|---|
| `Space::Xs` … `Xxl`, `Page`, `Section` | `[layout]`'s `space-*`, `page-margin`, `section-gap` | `Space.xs` … | `Space.xs` … | `layout_tokens::SPACE_XS` … | `var(--ark-space-xs)` … |
| `Corner::Card`, `Small`, `Circle` | `card-corner`, `corner`, round | `LayoutTokens.cardCorner` | `LayoutTokens.CARD_CORNER.dp` | `CARD_CORNER` — the desktop's card is 8 (`style::card`) | `var(--ark-card-corner)` |
| `Measure::ArtRow`, `ArtShelf`, `ArtHeader` | `[layout]`'s `art-row`, `art-shelf`, `art-header` | `ArkMeasure.artRow` … | `ArkMeasure.artRow` … | `layout_tokens::ART_ROW` … | `var(--ark-art-row)` … |
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
| `photo`, `zoom_photo`, `scrubber`, `hairline`, `volume`, the charts | gestures, a clock, decoding, drawing |
| `pressable`, `press_row` (`Part::Press`) | the platform's button, its press feedback and its context menu |
| the shell's accessory (`Tabs::accessory`), `ExpandingCard` | the tab view's bottom accessory, its zoom, the system's sheet |

| Moves to the primitives | Built in `view.rs` as |
|---|---|
| `header_card` (money-app's `parts`) | a `card_section` holding a centred `vstack` (tile, title stack, figure, line, caption), then its actions as rows |
| `goal_progress` (money-app's `parts`) | a leading `vstack` of a `meter` and a quiet footnote, at the row's inset |
| `empty_state` (new) | a centred `vstack` of a quiet glyph and a title stack |
| `title_stack` (new) | a `vstack` of up to four `text`s, each only while it has words |
| next: `summary_card`, `hero_figure`, `headline`, `paper` | they need what is below under Hard Cases (a figure in cents, "fits", parallel lists) |

## How Each Maps

| Primitive | SwiftUI | Compose | iced | Browser |
|---|---|---|---|---|
| `vstack` | `VStack(alignment:spacing:)` | `Column(verticalArrangement = spacedBy, horizontalAlignment)` | `column().spacing().align_x()` | `div.ark-stack.ark-v` (flex column, `gap`) |
| `hstack` | `HStack(alignment:spacing:)` | `Row(horizontalArrangement = spacedBy, verticalAlignment)` | `row().spacing().align_y()` | `div.ark-stack.ark-h` |
| `zstack` | `ZStack(alignment:)` | `Box(contentAlignment)` | `stack![]`, each centred | `div.ark-stack.ark-z` (grid, one cell) |
| `spacer` | `Spacer(minLength:)` | `Spacer(Modifier.weight(1f).widthIn(min = …))` (a weight is measured after the rest, so the least is kept only while there is room) | `Space` filling its stack's axis | `div.ark-spacer` (`flex: 1`) |
| `scroll` | `ScrollView` (`.horizontal`) | `verticalScroll` / `horizontalScroll(rememberScrollState())` | `scrollable` | `overflow: auto` |
| `text` | `Text(_)` `.arkType` `.foregroundStyle` `.lineLimit` `.multilineTextAlignment` | `Text(style, color, maxLines, overflow, textAlign)` | `text().size().font().style()` | `div.ark-text.ark-type-*` |
| `glyph` | `Image(systemName: ArkGlyphs.sf(_))` `.arkType` | `ArkGlyph(name, ArkType.*)` (Lucide, sized by the style) | `icon::tinted` at the style's size | the inline SVG |
| `.padding` | `.padding(.horizontal, Space.l)` | `Modifier.padding(horizontal = Space.l)` | `container().padding()` | `.ark-pad-*` |
| `.fill_width` | `.frame(maxWidth: .infinity, alignment:)` | `.fillMaxWidth().wrapContentWidth(align)` | `width(Fill)`, aligned | `.ark-fill-w` |
| `.background`, `.corner` | `.background(…)`, `.clipShape(.rect(cornerRadius:))` | `.clip(RoundedCornerShape).background()` | `container::Style` | `.ark-surface-*`, `.ark-corner-*` |
| `.combine` | `.accessibilityElement(children: .combine)` | `.semantics(mergeDescendants = true) {}` | — (iced has no screen reader yet) | `role="group"` |
| `image(..).fill()` | `ArkPicture`: `Color.clear.overlay { picture.scaledToFill() }.clipped()` (so it is the frame proposed, never the picture's aspect), `PictureCache` (ImageIO's thumbnail at the size drawn), `readyWhile` | `ArkPicture`: a `Box` of the modifier's size, `Image(ContentScale.Crop)` clipped to it, over `UrlCovers` (disk and memory, decoded by a power-of-two shrink, `Ready.waiting`) | `images::remote` drawn `ContentFit::Cover` at the square it is in, rounded in its pixels for a circle | `<img class="ark-picture">` `object-fit: cover`, absolutely over its box |
| `swatch` | `Square(seed:)` (a `Canvas`) | `Square(seed, Modifier.fillMaxSize())` | `art::square` at the square it is in | `div.ark-swatch.ark-square-<n>` |
| `grid` | `LazyVGrid(.adaptive(minimum:))` | `ArkGrid` (a `Layout` of equal columns, each cell measured a column's width; inside a page's lazy list) | a wrapped `row` of cells a column's least width | `display: grid`, `repeat(auto-fill, minmax(…, 1fr))` |
| `.width`, `.side`, `.square` | `.frame(width:)`, `.frame(width:height:)`, `.aspectRatio(1, contentMode: .fit)` | `.width()`, `.size()`, `.aspectRatio(1f)` | a fixed `container` (`.square` an `art-shelf` one) | `.ark-w-*`, `.ark-side-*`, `.ark-square` (`aspect-ratio: 1`) |
| `Corner::Circle` | `.clipShape(Circle())` | `.clip(CircleShape)` | the leaves' radius half the side | `.ark-corner-circle` |

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

## Inventory

Every part of ArkUI's, and every helper of the hand-written toolkits, by
what it is. *Primitive*: written out by the generator as the toolkit's
own layout. *Native*: a container or control written once per toolkit
because it carries the platform's behaviour. *Shared*: a Rust composition
in ark-app that several apps use. *One app's*: a component only one app
uses, which belongs in that app's `app/` crate as a Rust composition of
the primitives — moved, or still to move.

| Part / helper (`ark_app::view`) | Toolkits' own | Is | Who uses it |
|---|---|---|---|
| `vstack`, `hstack`, `zstack`, `spacer`, `scroll`, `text`, `glyph`, `image` (fit and fill), `swatch`, `grid`, and the modifiers | SwiftUI's and Compose's own layout; `ArkPicture`, `PictureCache`, `UrlCovers`, `ArkGrid`, `Square` | primitive | every app |
| `tile`, `status_badge`, `meter` (`Part::Tile`, `StatusBadge`, `Meter`) | `Tile`, `StatusBadge`, `MeterBar` | primitive (leaves) | money (a header card's glyph, a category's badge, a goal's bar), harken (an episode's progress) |
| `tab_page`, `detail_page` (`Part::Page`), `Part::Detail` | `TabPage`, `DetailPage` | native container | every app |
| `card_section` (`Part::Section`), `list_row` (`Part::Row`), `chip_row`/`chip` | `CardSection`, `ListRow`, `ChipRow`, `Chip` | native container | every app |
| `form_sheet`, `form_section`, `prompt`, `confirm`, `alert` | `FormSheet`, `FormSection`, the system's dialogs | native container | every app |
| `name_field`, `amount_field`, `date_field`, `address_field`, `picker`/`choice`, `search_picker`, `switch_row`, `form_button`, `wide_button`, `status_line` | the fields, pickers, toggles and buttons | native control | money, harken, the first run |
| `tabs`, `adaptive_tabs`, `section_tab`, `tab_group`, `push`, `split`, `toolbar`, `bar_item`, `account_item`, `inspector`, `searchable`, `hosted`, `settings_panes` | ArkShell's, `ExpandingCard`, the accessory | native container (the shell) | every app |
| `pressable`, `press_row` (`Part::Press`) | `Pressable` (Controls.swift, Controls.kt), a bare button, `<button class="ark-press">` | native control | harken; any app |
| `scrubber`, `hairline` (`Part::Timeline`), `volume` (`Part::Volume`) | `ClockScrubber`, `ClockHairline`, `VolumePart`, `Scrubber`, `VolumeSlider` | native control | harken |
| `photo`, `zoom_photo` (`Part::Photo`) | `Photo`, `Zoomed` | native control (decoding, pinch to zoom) | money (a receipt) |
| `code_image` (`Part::CodeImage`) | `QrCode` | native (drawn) | the first run's pairing |
| `bar_chart`, `data_table`, `table_row`, `table_heading` (`Part::BarChart`, `Table`, `TableRow`, `TableHeading`) | `Charts`, `TableParts` | native (drawn; the table that becomes a list) | money |
| `writing`, `specimen` (`Part::Writing`, `Specimen`) | the markdown and the toolkit's specimens | native | the docs |
| `first_run_page` (`Part::FirstRunPage`) | `FirstRunParts` | native container (the first run's page) | every app's first run |
| `title_stack`, `empty_state` | — | shared composition | harken, money |
| `artwork`, `artwork_tile`, `artwork_shelf`, `artwork_grid`, `media_row`, `media_header`, `play_shuffle`, `glyph_button`, `transport`, `figure_label` | — (the hand-written `Artwork`, `ArtworkGrid`, `ArtSize`, `artWidth`, `ArtworkCache`, `LocalGridCell`, `art::picture` callers and `.ark-art*` are gone) | one app's: **moved**, `harken-app`'s `parts` | harken |
| `header_card` (`Part::Section` of a composed face), `goal_progress` | — | one app's: **moved**, `money-app`'s `parts` | money |
| `summary_card` (`Part::Summary`), `hero_figure` (`Part::Hero`), `headline` (`Part::Headline`), `meter_row` (`Part::MeterRow`), `paper` (`Part::Paper`), `amount_entry` (`Part::AmountEntry`'s face) | `Summary`/`SummaryCard`, `HeroFigure`, `Headline`, `MeterRow`, `Paper` (FigureParts, PhotoParts, Parts) | one app's, hand-written per toolkit: **still to move** to money-app as compositions (they need what Hard Cases names: a figure in cents, "fits", parallel lists) | money |
| `Cover`, `CoverLoader`, `HeroBanner`, `Components.kt`'s covers (Kotlin), `HeroParts.swift`/`.kt` | — | one app's (harken's old native screens'), hand-written: **still to move or delete** once nothing calls them | harken (old) |
| `MarkTile`, the account face (`Part::Account`) | `MarkTile`, `AccountFace` | native (the shell's mark and account) | every app |

## What Is Verified

The view model and the components by `cargo test -p ark-app`
(`view/layout.rs`, `view/components.rs`); harken's compositions (a cover
its square first, the picture filled over the derived square) by
app-screens' sheets on every platform; the generated SwiftUI and Compose
by `ui/rust/gen/tests/snapshots` (`CardView.swift`, `CardScreen.kt`: the
rebuilt header card and goal, an empty state, a row of a glyph, words and a
spacer, a sideways shelf, an image — and a test that neither holds a number);
iced's and the browser's drawing by `cargo check` and clippy. The phones'
code is compiled in CI (mac-compile, build). Not compared on a device or in
a screenshot against the hand-written parts: the header card's face is now
`space-xl` above and below and `space-l` at its sides on both phones (it
was about 19 pt on the iPhone, 24 dp all round on Android).
