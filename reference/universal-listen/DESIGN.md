# Listen — design

A household's own music, podcasts and sermons, served from a home server
(`listen.home.arpa`) and played on any device. One SwiftUI target for
iPhone, iPad and the Mac (iOS 18 / macOS 15, built with the iOS 26 SDK),
designed after Music and Podcasts and the HIG: the system's containers,
type and materials, with as few platform and width checks as the
platforms allow.

## Navigation

One `TabView` with `.tabViewStyle(.sidebarAdaptable)` (RootView.swift):

| | iPhone (compact) | iPad, Mac (regular) |
|---|---|---|
| Top level | Tab bar: **Home, Library, Podcasts, Household, Search** | Sidebar: **Home, Household, Search** |
| Library | Library tab: a list of Playlists, Artists, Albums, Songs, Composers, then Recently Added | Sidebar section **Library**: Artists, Albums, Songs, Composers |
| Podcasts | Podcasts tab | Section **Podcasts & Sermons**: Shows, Downloaded |
| Playlists | Library ▸ Playlists | Section **Playlists**: Favorites, Recently Added, Most Played (smart), Sunday Morning, Deep Focus, Road Trip |

The Library and Podcasts tabs are hidden from the sidebar
(`.defaultVisibility(.hidden, for: .sidebar)`); the sections are hidden
from the tab bar (`.defaultVisibility(.hidden, for: .tabBar)`). Both
places show the same views (`ShowsView` is the Podcasts tab and the
Shows item). On iPad the user can still collapse the sidebar into the
floating tab bar. Each tab holds its own `NavigationStack` with a typed
`Route` path; the Navigator keeps one path per tab so switching tabs keeps
the place, and deep links (`-screen`) choose tab-or-sidebar by width — the
one place navigation reads `isCompact`.

## Containers

- **Songs**: `Table` with sortable columns Title (artwork, favourite star,
  playing indicator), Artist, Album, Composer, Time, Plays; multi-select;
  `contextMenu(forSelectionType:primaryAction:)` — double-click / Return
  (or tap on iPhone) plays from that row. On iPhone the table shows its
  first column, so the title cell adds “Artist · Album” and the time
  (Apple's Food Truck pattern). A Sort pop-up menu sits in the toolbar too.
- **Albums**, **Shows**: adaptive `LazyVGrid` (150–230 pt tiles),
  procedurally drawn covers; shows carry an unplayed-count badge.
- **Album**: `List` (plain) with a header that `ViewThatFits` lays out
  beside the artwork when it fits and stacked/centred when it doesn't;
  numbered rows with the playing indicator, composer lines for mixed
  classical albums, swipe actions, and a footer with count/length/year.
- **Artists / Composers**: plain lists; artist pages show Top Songs and an
  album grid; composer pages group works by recording.
- **Playlist**: a header (artwork, smart badge, summary, Play / Shuffle)
  over the same songs `Table`.
- **Show**: header (`ViewThatFits` again), All / Unplayed / Downloaded
  segmented filter, episode rows like Podcasts: date (and show) in caps,
  title, two-line summary with scripture reference for sermons, a tinted
  play capsule with progress and time left, download state, More menu.
- **Episode**: details, Play/Resume, and the full transcript: tap any line
  to play from there; the current line is tinted.
- **Household**: “Listening Now” cards (member avatar, device, item,
  progress, Playing/Paused badge, Listen Here Too / Take Over) and a Hand
  Off grid of devices (the selected one filled with the accent).
- **Search**: `.searchable` with scopes (All, Music, Podcasts & Sermons)
  and suggestions; empty state is a Browse grid of genre tiles; results
  are sections: Albums (horizontal), Artists, Songs, Episodes;
  `ContentUnavailableView.search` when nothing matches.
- **Settings**: five panes (Server, Household, Playback, Downloads,
  Appearance), grouped `Form`s shared by the Mac's `Settings` window
  (tabbed) and the iOS sheet (a list of panes under the owner's card,
  opened from Home's avatar button).

## Player

- **iPhone / iPad**: a mini player in the tab view's bottom accessory
  (iOS 26 `tabViewBottomAccessory`; the tab bar minimises on scroll):
  artwork, title, “Playing on Living Room”, play/pause, next (skip 30 for
  spoken items). Tap opens the full player as a full-screen cover. (Before
  iOS 26 a material bar sits above the content.)
- **Mac**: the window toolbar is the player, as in Music: previous /
  play / next at the leading edge, a centred now-playing display (artwork
  — click for the full player — title, subtitle, progress line, time
  left), then volume, output, Lyrics and Up Next.
- **iPad toolbar** (regular width) has the same output, Lyrics and Up Next
  buttons; Lyrics opens the **inspector** (Up Next | Lyrics/Transcript,
  segmented), Up Next a **popover**.
- **Full player**: a dark mesh gradient from the artwork's colours. Portrait:
  close chevron + grabber, large artwork (shrinks when paused), title and
  subtitle with Favorite and More, a capsule scrubber (thickens while
  dragged; elapsed / remaining), transport (previous / play / next, or −15 /
  play / +30 for spoken items), volume, and a bottom row: Lyrics
  (Transcript) · speed pill (spoken only, 0.75–2×) · output · Up Next.
  Choosing Lyrics or Up Next folds the artwork into a header row. Wide
  (iPad landscape, Mac): artwork and controls left, the lyrics/transcript
  or Up Next panel right.
- **Lyrics / transcript**: lines in large bold type, the current one
  primary, past secondary, future tertiary; scrolls to keep the current
  line centred; tap a line to seek. Transcripts label speaker changes with
  the time.
- **Output picker**: “Playing On” with every household device (this
  device, Living Room, Kitchen, Studio, Iris's iPad, Den TV), each with its
  room or who is using it, a checkmark for the current one, and volume. A
  device someone else is playing on asks first (confirmation dialog: Move
  Playback / Play on Both / Cancel). It's a popover from the toolbar (iPad,
  Mac) or the full player, a sheet on iPhone.
- **Up Next**: reorderable, deletable queue; header with Shuffle and
  Repeat toggles and “Playing on …”; History below.

## Menus and transient UI

Every item has the same actions everywhere (Menus.swift): its context menu,
its More (…) menu and the full player's More menu. Songs: Play, Play Next,
Play Later · Add to Playlist ▸ (New Playlist…, the household's playlists) ·
Favorite · Go to Album / Artist / Composer. Albums: Play, Shuffle, Play
Next, Play Later · Add to Playlist ▸ · Go to Artist. Episodes: Play /
Resume, Play Next, Play Later · Mark as Played · Download / Stop / Remove
Download… (confirmation) · Go to Show, Episode Details. Sheets: New
Playlist, Settings (iOS). Dialogs: hand-off, remove download, sign out.

## Keyboard

Space plays/pauses (`onKeyPress`, so it never steals a space from a text
field). The **Controls** menu (Mac menu bar; iPad's menu with a keyboard):
Play/Pause ⌘↩, Next ⌘→, Previous ⌘←, Skip Back 15 ⌥⌘←, Skip Forward 30
⌥⌘→, Go to Current Item ⌘L, Volume ⌘↑ / ⌘↓, Shuffle ⌥⌘S, Repeat ▸,
Playback Speed ▸. Edit ▸ Find ⌘F jumps to Search. View: Full Player ⇧⌘F,
Show Lyrics ⌥⌘L, Show Up Next ⌥⌘U, plus the system sidebar and inspector
commands. Tables, lists and grids get arrow-key selection from the system.

## Typography, colour, spacing

- SF Pro at system text styles throughout (Dynamic Type): large navigation
  titles on iOS; shelf headings `.title2.bold`; album/show titles
  `.title.bold` with the artist in `.title2` accent; rows `.body` with
  `.subheadline` secondary lines; captions in caps for dates and kinds;
  monospaced digits for times and counts. Set-type covers use New York
  (`.serif`).
- Accent: coral, `#E7513C` light / `#FF6F57` dark (AccentColor); all else is
  semantic (`.primary/.secondary/.tertiary`, `.background.secondary` cards,
  materials). The full player is always dark.
- 20 pt content margins, 16–18 pt grid gaps, 28–32 pt between shelves;
  continuous corner radii (4.5% of artwork size, 14–18 pt cards).
- Covers: gradients of two hues with rings, blurred blobs, stripes or set
  type and an SF Symbol — deterministic per album, no artwork assets.

## Motion

One spring for panels and the player (`Animation.listen`), a light spring
press on custom buttons (`PressableStyle`), symbol replace transitions on
play/pause and favourite, variable-colour waveform on the playing row.
With Reduce Motion: no scale on press, a short ease instead of springs,
no animated scrolling of lyrics. Sensory feedback: a light impact on
transport actions, selection on output and favourite changes.

## Accessibility

Icon buttons all have labels (Label + `.iconOnly`); rows combine their
children; the scrubber is one adjustable element (±15 s) with a value; the
playing state is announced; selected outputs and panel toggles carry
`.isSelected`; artwork is hidden from VoiceOver. Colour is never the only
signal (unplayed has a dot *and* the count; downloads have distinct
symbols).

## Trade-offs

- The Mac's player is in the toolbar (Music) rather than a bottom bar; the
  iPad keeps the mini player plus toolbar buttons rather than a toolbar
  transport, which would crowd the floating tab bar.
- Space is handled with `onKeyPress` rather than a menu key equivalent;
  the Controls menu shows ⌘↩ for Play.
- The Library/Podcasts tab pair exists only because the tab bar has five
  slots; the sidebar shows their contents directly.
- No audio: the playhead is a clock; with `-screen` it is frozen.
