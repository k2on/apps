# ArkUI's generic parts

The small pieces a page is laid out of that any Ark app can use unchanged.
Each has **one name and the same inputs in every toolkit**: Compose
(`ui/kotlin`, package `dev.arkui`), SwiftUI (`ui/swift`) and iced
(`ui/rust/arkui`, functions in snake case). A future client (the web) builds
the same list.

The rule for what goes here: a part goes in ArkUI only if another app could
use it as it is — no money word, no media word, no domain type in its
inputs. What an app is about (a transaction's row, a track's row, the
player) stays in the app and is listed in `spec/<app>-components.md`; where
such a component has a generic piece in it, it is made of the ArkUI part.

Every colour is the theme's (`ui/theme.toml`), every move one of the
shared springs (`ui/docs/motion.md`), every press `pressable` / `.press` /
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

## Figures

A page of figures — a budget, a register's balances, a report — is laid
out of these (`arkui::figures`, `FigureParts.swift`, `FigureParts.kt`).
Charts are Swift Charts on Apple's platforms, a Canvas on Android, plain
shapes and an SVG line on the desktop, in the theme's `[chart]` palette.

| Part | Inputs | Compose | SwiftUI | iced |
|---|---|---|---|---|
| StatusBadge | figure, tint (negative filled with a warning, caution a clock) | `StatusBadge` | `StatusBadge` | `figures::status_badge` |
| GoalBar | fraction, tint (`progress-width` wide beside columns) | `GoalBar` | `GoalBar` (`ProgressView`) | `figures::goal_bar` |
| Headline | label, figure, tint, caption, trailing | `Headline` | `Headline` (large, bold, rounded) | `figures::headline` |
| CapsuleButton | title, glyph?, prominent, action | `CapsuleButton` | `CapsuleButton` (bordered / prominent) | `figures::capsule_button` |
| NoticeBar | glyph, text, actions | `NoticeBar` | `NoticeBar` (on `.bar`) | `figures::notice_bar` |
| FigureStrip | (label, figure, tint)… | `FigureStrip` | `FigureStrip` | `figures::figure_strip` |
| StatTile | label, figure, caption? (its line kept, so tiles are one height) | `StatTile` | `StatTile` | `figures::stat_tile` |
| ChartCard | title, chart | `ChartCard` | `ChartCard` | `figures::chart_card` |
| LabelledBars | (label, value, said)… | `LabelledBars` | `LabelledBars` | `figures::labelled_bars` |
| ColumnChart | series, groups (label, values) | `ColumnChart` | `ColumnChart` | `figures::column_chart` |
| LineChart, Sparkline | points (oldest first); the area runs to the foot | `LineChart`, `Sparkline` | `LineChart`, `Sparkline` | `figures::line_chart`, `sparkline` |
| SummaryCard | label, cents, first/second (owned, owed), trend | `SummaryCard` | `SummaryCard` | the view's `Part::Summary` |
| Grid | items, columns, gap | a `Row` of weights | `LazyVGrid` | `figures::grid` |

The slider is called `Bar`, not `Slider`: `Slider` is SwiftUI's and
Material's own, and an app that imports both could not say which.

**Glyphs.** The shared names are Lucide's: `chevron-left`,
`chevron-right`, `chevron-down`, `ellipsis-vertical`, `x`, `plus`, `minus`,
`check`, `circle-check`, `circle`, `search`, `wifi-off`, `volume-2`,
`volume-x`, `log-out`, and for a row's menu `pencil`, `copy`, `archive`,
`undo-2`, `badge-check`, `circle-plus`, `trash`, `folder`, `lock`, `receipt`; for the shell and the figures
`panel-right` (the inspector's toggle), `wand-sparkles`, `refresh-cw`,
`list-filter`, `mail`, `chart-column`, `list`, `settings`, `inbox`,
`triangle-alert`, `clock`, `dollar-sign`, `banknote`, `cloud`, `target`,
`arrow-left-right`, `users`, `house`, `landmark`. Android and the desktop
draw Lucide's paths; the iPhone draws the SF Symbol each maps to.

## A screen laid out in Rust

The parts `ark_app::view` composes (ui/docs/app.md): hand-written in
`ScreenParts.swift`, `ScreenParts.kt` and `ui/rust/arkui/src/view.rs`, called
by the generated views by these names.

| Part | Inputs | Compose | SwiftUI | iced |
|---|---|---|---|---|
| TabPage | title, onRefresh, top, content | the app's frame (`LocalTabFrame`) over a scrolling column | `List` (inset grouped) under the large title | a scrolling column under the title |
| CardSection | title, total, showsTotal, footer, rows | `SectionHeader` + `GroupCard` | `Section` | `section_header` + `group_card` |
| ListRow | title, subtitle, tile/mark/glyph, badge, amount, value (words at its end), quiet, accent, **pushes**, said, menu, onTap | a `Press.Highlight` row; held, a `DropdownMenu` (lit as it is held); a `Chevron` when it pushes | a button row; held, `.contextMenu`; when it pushes, a `NavigationLink`'s look (the list's highlight, the disclosure chevron) | a bare button; right click, a long press or the menu key, `context_menu` (ArkUI's menu at the pointer, every item); a chevron when it pushes |
| DetailPage | content | a scrolling column of cards | `List` (grouped) | a scrolling column |
| HeaderCard | title, figure, tint, glyph?, badge, line, caption, actions | `GroupCard`, centred, its actions as rows | the list's first `Section`, centred | `style::card`, centred, a rule over the actions |
| GoalProgress | fraction, tint, note | `ProgressBar` and its note | `ProgressView` and its note | `progress_bar` and its note |
| MenuAction | title, glyph, destructive, shown, onTap | `DropdownMenuItem` | `Button(role:)` with a `Label` | `context_menu::Item` |
| FormSheet | title, onDismiss, confirm, canConfirm, onConfirm, content | `ModalBottomSheet`, the confirm a full-width button under the form | `.sheet` over a `Form`, Cancel ✕ and the confirming ✓ in its bar | a dialog with Cancel and the confirm |
| FormSection | header, footer, content | a column, its label over and its note under | `Section(header:footer:)` | a column |
| OptionPicker / PickerChoice | label, selection, onChange; title, glyph, tag | `FilterChip`s in a `FlowRow` | `Picker` (a menu in the form's row), each a tagged `Label` (`\.choiceSymbol` for the app's own symbols) | chips that wrap |
| AmountEntry | label, prompt, text, cents, problem, onType, onCents | `AmountField` (typed) | ArkUI's `AmountEntry` (the figure over the keypad, in cents) | a text field (typed) |
| SwitchRow | title, note?, isOn, onChange | `ListItem` with a `Switch` | `Toggle` | `checkbox` |
| Prompt, NameField, AmountField, ChipRow, Chip, FormButton, HeroFigure | as they read | | | |
| FirstRunPage | glyph, title, sentence, content | `OnboardingPage` (the mark in the primary, centred, scrolling over the keyboard) | `WelcomePage` (the mark on glass in the accent) | a centred column under the mark's tile; the browser's `.ark-welcome` card |
| AddressField | label, prompt, text, note, problem, enabled, onChange, onSubmit | `OutlinedTextField` (the URI keyboard, Go) | a `TextField` on glass (`.URL`, no capitals or corrections, Go) | a `text_input`, Enter submits; the browser's `<input type=url>` |
| WideButton | title, prominent, enabled, busy, onTap | `Button` / `OutlinedButton`, a spinner while busy | `.glassProminent` in the accent / `.glass`, a `ProgressView` while busy | `style::action` / `style::quiet`; the browser's `.ark-push` (`.ark-default`) |
| StatusLine | text?, busy, problem? | a polite / assertive live region | the footnote quiet, the problem in `Color.error` | dim words, the problem in the danger colour; `role=status` / `alert` |
| CodeImage | size, dark | `QrCode` | `QrCode` | `arkui::qr::qr_code`; the browser's inline SVG |

## Tables

A table of rows (a register, a list of transactions) is laid out once
(`data_table`, `table_column`, `table_row`, `table_heading` in
`ark_app::view`): its columns, each a heading, a width (points, or a share
of what is left), an alignment and where its cell goes when compact; its
rows, each its cells' words with their tints and glyphs beside them (three
lists of strings, `ark_app::view::Cells`), a dot while `marked`, lit while
`selected`, its tap and its menu; headings among them (a day). From a
regular width it is rows of cells under the headings; compact, each row is
a list's two lines — the title and the words under it at the leading end,
the figure and the words under it at the trailing end, by the columns'
slots, the rest left out. The parts decide which, never the app.

| Part | Inputs | Compose | SwiftUI | iced | Browser |
|---|---|---|---|---|---|
| DataTable | columns (`DataColumn`: heading, width, share, align, slot), rows | a `GroupCard`, its headings a row, compact below `COMPACT_WIDTH` (`TableParts.kt`) | a list's `Section` with the headings as its header, compact by the size class (`TableParts.swift`) | `arkui::table`'s headings and rows | `arkui_web::table` (`.ark-table`, `.ark-listed` when compact) |
| DataTableRow | cells, tints, glyphs, marked, selected, said, menu, onTap | a `Press.Highlight` row, a `DropdownMenu` held | a button row, `.contextMenu` | a full-width row, its menu at the pointer | a grid row, its menu on right click |
| DataTableHeading | title | a quiet label | a footnote header row | `table::section` | a heading row |
| DateField | label, day (`2026-11-11`), onChange | a row, Material's `DatePickerDialog` | `DatePicker(.date)` | a text field | `fields::date_field` |

## A page about somebody

`ui/rust/arkui/src/hero.rs`'s catalogue page has its twins on the phones:

| Part | Inputs | Compose | SwiftUI | iced |
|---|---|---|---|---|
| HeroBanner | picture, kind, name | `HeroBanner(seed, art, kind, name)`, `HeroBanner.height` | `HeroBanner(image:kind:name:)`, `HeroBanner.height` | `hero::banner` |
| HeroHeading | title | `HeroHeading` | `HeroHeading` | `hero::heading` |
| HeroEntry | over, title, under, count, open, picture | `HeroEntry` | `HeroEntry` | `hero::entry` / `hero::listing` |
| HeroMore | label, open | `HeroMore` | `HeroMore` | `hero::more` |
