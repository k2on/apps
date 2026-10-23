# A Bible as one scroll

An Android app whose whole surface is one column of text, Genesis 1:1 at
the top and Revelation 22:21 at the bottom, that scrolls like the system
scrolls and zooms like a map: pinch out and the words become lines, the
lines a column, the column — at the far end — a single white line the
height of the screen. Pinch in and it comes back. Going anywhere is the
camera move a map makes: out, across, in. This is the plan for building
it, the second app in this tree beside harken, on none of ArkDB.

## Decided by the owner

- **One continuous scroll** from Genesis to Revelation. No pages, no
  per-chapter screens; the chapter is a place in the scroll, not a view.
- **Zoom is continuous** and goes all the way out: the far end is the
  whole Bible as one thin white line. Every level between is a real
  state, not a snap point.
- **Scrolling is native.** The fling, the friction and the overscroll at
  either end are Android's own, not an imitation of them.
- **Navigation is the map move**: to go from here to there the camera
  zooms out, travels, and zooms back in, in one motion.
- **Not ArkDB.** No sync, no log, no domain. A reading app with its state
  on its own disk.
- **Not the standard widgets.** The owner's instinct is right that a
  `LazyColumn` or a `RecyclerView` cannot do this. The instinct that it
  wants a game engine is answered below: what a game engine would give
  — a camera, a scene redrawn under a transform, levels of detail — the
  hardware canvas gives already, and what a game engine would take —
  text rendering, the platform's physics, the battery — is exactly what
  this app lives on.

## Why not a game engine, and what instead

A game engine (libGDX, Godot, Unity, a Rust engine through the NDK) is a
render loop, a camera and batched drawing. Three things make it the
wrong tool here:

- **Text is its weakness.** Engines render text from bitmap or signed
  distance field atlases: fine at one size, blurry or ragged under
  continuous scale, with no system font fallback, no hinting, no
  complex shaping. Text at a readable size, held still, is this app's
  product, and the platform's text renderer is as good as text gets on
  the device.
- **The physics would be ours to fake.** An engine owns the touch
  stream; the fling curve, the overscroll stretch, the velocity
  tracking and the touch slop that make scrolling "native" are all in
  the Android view classes (`OverScroller`, `EdgeEffect`,
  `VelocityTracker`, `ViewConfiguration`) and nowhere else.
- **A render loop drains a battery** for an app whose screen is still
  while a person reads a verse.

What the engine was wanted for is a **custom `View`** drawing on the
hardware-accelerated `Canvas`: Skia on the GPU, text from a glyph atlas
under any transform, `drawLines` and `drawRects` for thousands of
primitives in one call, and a frame only when the camera moved. The
mental model is a **map engine**, not a game engine — levels of detail,
labels that appear by zoom, a camera with a fly-to — built in a few
hundred lines over primitives the platform already has. Compose draws
the chrome around it (the header, the picker, settings), the way harken
is Compose; the surface itself is one `View` inside `AndroidView`, since
the gesture and physics classes are the view system's and `onDraw`
draws a `CharArray` range without allocating.

If measurement says the canvas text cannot keep 120 fps during a pinch
over reading-size text (see *Risks*), the next step is to raster the
visible text to a layer at the pinch's start and scale that layer until
the fingers lift, which is what browsers do; a GL surface with an atlas
of our own is the step after, taken only on evidence.

## The design

### The text

One translation shipped as an asset, built by a tool in the tree from a
public-domain source: the King James Version (or the World English
Bible, public domain and modern; the format does not care). The asset
is the whole text as one UTF-8 file plus an index: for each of the
31,102 verses, its byte offset, and for each of the 1,189 chapters and
66 books, the verse it starts at. Loaded, the text is one `String` (some
4.3 million characters, under 10 MB) and the index three `IntArray`s. A
reference — `Ref(book, chapter, verse)` — is a verse index, and a verse
index is a character range. Full-text search is a scan of the string:
ten milliseconds, no index.

### The layout: measured once, broken ourselves

The document is laid out **once, at reading size**, and zoom is a
transform of that layout — the way a map is drawn at one scale and the
camera moves over it. Reading size is the unit: 1 document unit = 1 dp
at the chosen font size. From the layout comes the one fact everything
else is built on: **where every line is**, so where every verse is, so
how tall the whole Bible is.

`StaticLayout` over four million characters would be seconds of work;
instead the layout is ours, and cheap, because the Bible has few
distinct words:

- **Measure every distinct token once.** Split on spaces; the KJV has on
  the order of thirty thousand distinct tokens (words with their
  punctuation). `Paint.measureText` for each, with linear text and no
  hinting so that widths scale with the transform. Milliseconds.
- **Break lines greedily**, no hyphenation, each verse its own paragraph
  (the KJV's own tradition, and it keeps every verse addressable by its
  first line). Pure arithmetic over 783,000 words: well under a hundred
  milliseconds in Kotlin.
- **The line table**: parallel arrays, one entry per line — character
  start, character end, measured width — and, since every text line has
  the same height, its `y` is its index times the line height plus the
  headings before it. On the order of 120,000 lines, a couple of
  megabytes.
- **Blocks** between the lines: a book title (a tall block, its name in
  display type), a chapter number (a short block), and the verse
  paragraphs; verse numbers are drawn small at a verse's first line.
  Each block's height is fixed in document units, so the whole document
  is a sum.

The reference layout is a function of (text, font, font size, column
width); it is recomputed when any of those change — rotation, a font
size setting — and is fast enough that nothing is cached to disk.

The estimates the plan is sized by, to be replaced by the layout's own
numbers once the text is in (18 dp serif, 27 dp line height, a 360 dp
column):

| quantity                     | about            |
|------------------------------|------------------|
| lines                        | 120,000          |
| document height              | 3,000,000 dp     |
| screens of reading           | 4,000            |
| scale at the far end         | 1 / 4,000        |
| column width at the far end  | 0.1 dp → 1 px    |

The last row is the owner's "single white line": the column zoomed out
until its width is under a pixel, drawn clamped to one. The geometry
gives it for free; the renderer only has to not lose it.

### The camera

Two numbers: `y`, the document coordinate at the screen's vertical
centre, and `scale`. The column is always horizontally centred — the
document is one-dimensional, so there is no horizontal pan — and

    screen_y = (doc_y − camera.y) · scale + H/2

Zoom is bounded by `scale_min = (H − 2·pad) / D` (the whole document
fits with a little air) and `scale_max = 1` (reading size; see
*Reflow* for pinching past it). `camera.y` is clamped so the document
never leaves the screen; when the document fits, it is centred.
A pinch about a focal point keeps the document point under the fingers
still — `y' = y_f − (f − H/2)/scale'` — and the focal point's own drift
pans. Nothing else in the app knows about pixels: everything is a
document coordinate and a scale.

### Levels of detail

What is drawn depends on one quantity, the rendered height of a text
line, `h = lineHeight · scale · density` in pixels, and on the rendered
height of a chapter and a book. Each level is what a reader would see
from that far away, with the transitions faded over a band of zoom so
nothing pops:

- **Glyphs** (`h ≥ 5 px`). The visible lines, found by binary search in
  the line table, drawn with `drawText(chars, start, count, x, y)` under
  `canvas.scale`. Forty calls on a reading screen; a few hundred at the
  small end. Verse numbers and chapter numbers in their blocks, book
  titles in theirs.
- **Greeked** (`1.2 ≤ h < 5 px`). Each line a bar of its measured width,
  half the line height thick, white: text as seen from across a room,
  with the short last lines of verses and the gaps of chapter blocks
  still visible. One `drawLines` call over a float array per frame; a
  few thousand bars at most.
- **Column** (`h < 1.2 px`). The lines have merged; the column is drawn
  as rectangles at the coverage the bars had (about half intensity, so
  the brightness is continuous with the level above), one per chapter,
  with a one-pixel dark separator between chapters while a chapter is
  at least 6 px tall, then one per book with a separator between books
  while a book is at least 4 px tall, then one rectangle. The column's
  width is clamped to one pixel, which is the line.
- **Labels**, in screen-space type that does not scale, beside the
  column like a map's: chapter numbers down the right while a chapter
  is at least 24 px tall, book names down the left while a book is at
  least 20 px tall, and when they are not, as many book names as fit
  without colliding, longest books first — Psalms, Genesis, Jeremiah,
  Isaiah — the way a map keeps the big cities. At the far end: the line,
  a few book names, and a tick where the Testaments meet. Every label is
  a target: a tap flies there.

A frame at any level is under a millisecond of CPU and a handful of
draw calls; the view is invalidated only while the camera moves.

### Scrolling, native

- **Drag** moves `camera.y` by the finger's travel over `scale`, after
  the platform's touch slop.
- **Fling** is `OverScroller.fling` in screen pixels — the same spline
  and friction `RecyclerView` and `ScrollView` use, which is what
  "native" means — read each frame and converted to document units at
  the current scale, so a fling across a zoom change stays right.
- **Overscroll** at Genesis 1:1 and Revelation 22:21 is `EdgeEffect`,
  the stretch on Android 12 and later and the glow before it.
- **Pinch** is `ScaleGestureDetector` with the focal anchoring above;
  double-tap zooms in a step toward reading size at the tapped place,
  two-finger tap zooms out a step (the map conventions). On release, a
  scale within a few percent of reading size snaps to it, so text at
  rest is always crisp, and a scale near the far end snaps to it.
- **Frames** come from `postInvalidateOnAnimation`, under the
  Choreographer, only while a scroller, a gesture or an animation is
  live. Still text costs nothing.
- The gesture code is one state machine — idle, dragging, flinging,
  pinching, animating — and a touch during any animation ends it and
  takes the camera from where it is.

### The fly-to

Going from here to there is the smooth zoom-and-pan of van Wijk and
Nuijten, the path d3's `interpolateZoom` and Google Maps' camera
animate on, in one dimension: between (`y0`, `w0`) and (`y1`, `w1`),
where `w = H / scale` is the visible height of document, with the
constant ρ = √2. It zooms out exactly as far as the distance needs —
across a chapter, barely; from Genesis to Revelation, all the way to
the line — and back in, along a hyperbolic path with no visible seam
between the three phases. The degenerate case (no distance) is a plain
interpolation of `w`. Duration grows with the path length and is
clamped, about 300 ms for a chapter and no more than 1.2 s across the
book. Every way of going somewhere uses it: a tap on a label, the
picker, a search result, a typed reference, a bookmark, back.

### Wayfinding

A header over the surface, always the reference under the screen's
centre — "Psalms 23" while reading, "Psalms" when the chapters are
bars, "Psalms – Isaiah" when they have merged. Tapping it opens the
picker, a grid of 66 books and then of chapters, and a chosen chapter
flies. Search is a field over the string; a result flies to its verse
and the verse is held for a moment. There is no separate map or
minimap: the zoom is the map.

### Reflow

Pinching past reading size enlarges the text past crisp, and on release
the layout is recomputed at a font size nearer the new scale and the
camera animates to scale 1 at it — so a person with the common instinct
to pinch text bigger gets bigger text, and the layout is always drawn
at its own size. The same reflow serves the font size setting and
rotation.

### Look

White text on near-black by default — the "white line" is the owner's
image, and it is the night theme's — with a light theme that inverts
(the line is black). A serif face bundled under an open license
(Literata or EB Garamond), 18 dp, a column no wider than about 36 em
and centred on wide screens. Verse numbers small and dim, chapter
numbers large, book titles with air around them. Nothing else on the
surface: the header, and the labels when zoomed out.

### State

The camera (`y`, `scale`) and the font size, saved on pause and
restored on launch, in DataStore. Bookmarks and highlights, when they
come, as rows in a small Room table keyed by verse index. Nothing
syncs.

## The work

Each piece lands with its tests and is reviewable on its own.

1. **The text.** The build tool (`bible/text/`) that turns a
   public-domain source into the asset and its index; the loader; `Ref`
   to verse index to character range and back; search. Tests over the
   counts (66, 1,189, 31,102) and a few known verses.
2. **The layout.** The token measurer behind an interface, the greedy
   breaker, the line table and the blocks. Pure, tested on the JVM with
   a fake measurer: every character in exactly one line, no line wider
   than the column, `y` monotonic, a verse's first line is where its
   `Ref` says. A timing test over the whole text.
3. **The surface at reading size.** The `View`, the camera and its
   transform, the glyph level, drag, fling, overscroll. The gesture
   state machine, tested. This is the app's first screenshot.
4. **Zoom.** The pinch, the levels of detail and their fades, the
   labels and their collision rule, the snaps, the line at the far end.
   Screenshots at each level.
5. **The fly-to.** `interpolateZoom` in one dimension and its tests
   (endpoints exact, `w` rising then falling, the degenerate case), the
   header, the picker, taps on labels, search, go-to.
6. **Reflow and settings.** Pinch past reading size, the font size
   setting, rotation, the two themes, the saved camera.
7. **The build.** `bible/android` as a gradle project on the SDK and
   `aapt2` the flake already assembles (`nix/android-sdk.nix`,
   `nix/aapt2.nix`), its own recorded Maven graph (`deps.json`), and
   three outputs beside harken's: `bible-apk`, `bible-apk-deps`,
   `bible-android-screens`. `bible/README.md` says how to build and
   what has and has not been seen on a phone.

Later, and not in this plan's scope: accessibility — TalkBack cannot
read a canvas, so an `ExploreByTouchHelper` exposes the visible verses
as virtual views; selection — a long press selects a verse, with copy
and share, which the line table makes a lookup; bookmarks and
highlights; a second translation in the same asset format; paragraph
and poetry setting where the source carries it; and iOS, which mirrors
the design over `UIView` and `UIScrollView`'s deceleration the way
harken's iOS mirrors its Android.

## Proving it without a phone

As harken's: no phone or emulator runs here, so what can be proved on
the JVM is, and what only a phone can show is named.

- **On the JVM.** The layout, the camera, the levels' thresholds, the
  fly-to path, the gesture state machine and the search are pure code
  with unit tests. The surface renders through Paparazzi at six camera
  states — reading, glyphs at the small end, greeked, chapters, books,
  the line — in both themes, as PNGs under `bible-android-screens`:
  what the owner looks at, and what a change to the drawing is reviewed
  by.
- **Only a phone.** Frame times during a fling and during a pinch over
  reading text (`FrameMetrics`, Perfetto), the feel of the fling against
  `ScrollView`'s, the stretch effect, and whether the fly-to reads as
  one motion. The first thing to do with the first APK is to pinch over
  Psalms with the frame-time overlay on.

## Risks, and what decides them

- **Glyphs under continuous scale.** Every pinch frame is a new glyph
  size, so the GPU glyph cache re-rasterises the visible glyphs each
  frame. Chrome and the system's own pinch-zoomed views do this at
  frame rate for a screen of text, and under 5 px we stop drawing
  glyphs at all, so the budget is a few thousand glyphs. Measured in
  piece 4 on a mid-range phone; the fallbacks are in order above.
- **A line drawn is a line measured.** A line's width as the sum of its
  tokens may differ by a pixel or two from the shaped line, where a
  font kerns across a space. A test measures the deviation over the
  whole text; if it shows, lines are drawn token by token at their
  measured `x`, which the canvas batches anyway.
- **Layout at launch.** Must be under a quarter second for the whole
  text or the app opens on a spinner. The token approach is what
  guarantees it; the timing test in piece 2 holds it.
- **Zoom while flinging.** The scroller runs in screen pixels; a pinch
  during a fling ends the fling, which is what a scroll view does.
- **The seam between levels.** A pop where bars become glyphs would make
  the zoom feel like a slideshow. The fades over a band of zoom and the
  continuity of brightness between levels are the answer; the
  screenshots at the thresholds are how it is checked.

## Open to the owner

- The app's name, and so its directory and package; `bible/` here is a
  placeholder.
- The translation: KJV, WEB, or both from the start.
- Verse-per-line, as planned, or paragraphs where the source marks them.
- Whether a pinch past reading size reflows (as planned) or stops.

## Landed

The app is **Revelation**, `revelation/` (`revelation/README.md`), and
it is Kotlin and nothing else, by the owner's decision: no Rust, no
native library, no build tool for the text. The sharing question raised
between this plan and the build — a Rust core over UniFFI drawing
natively on each platform, or Kotlin Multiplatform with Skia everywhere
— is answered for now by that decision: the engine is pure Kotlin with
no Android types in it (`Bible`, `Layout`, `Camera`, `FlyTo`, `Levels`,
`Gestures`), so it is the part a later port would carry.

Of the open points: the translation is the Berean Standard Bible,
public domain, shipped as the file it was published as and parsed at
launch; each verse is its own paragraph; a pinch past reading size
rubber-bands back rather than reflowing.

What landed, against the pieces above: the text (1), the layout (2),
the surface with the platform's scroller and edge effects (3), the
levels down to the one-pixel line with their fades and labels (4), the
fly-to, the header, the picker and search (5), the theme and the
saved place (6, without the font-size setting), and the build (7):
`revelation-apk`, `revelation-apk-deps` and `revelation-android-screens`
beside harken's, and one F-Droid repository for both apps (`fdroid-repo`,
`apps-fdroid`, `nix/fdroid.nix`). Forty-seven JVM tests and eleven
screenshot pairs; the whole text lays out in about a hundred
milliseconds with the fake measurer, as 99,563 lines and 2,761,398 dp at
a Pixel 5's width, from 34,883 distinct tokens.

Three choices made in the building: the header and the saved place read
from a point a quarter of the way down the screen below the header, not
its centre, since a fly-to lands a chapter just under the header and a
short psalm can end before the centre; the header shows a range once
more than two books are on screen; and `BibleView` keeps the last
layout for the process, which is what makes the screenshots take under
a minute under layoutlib.

Not yet, and still as planned: reflow by pinch and the font size
setting, a bundled serif (`Typeface.SERIF` for now), accessibility,
selection, bookmarks, a second translation. No phone has run it: the
touch routing, the fling against `ScrollView`'s, the stretch effect,
frame times during a pinch over reading text and the layout's time with
the real `Paint` are the first things to look at with the first APK.

Since: the app is dark always, whatever the phone's theme, on a black
ground rather than near-black, and the light theme is gone. Book and
chapter names fade in and out over a fifth of a second as the collision
rule places and drops them, instead of popping; a name fading out stays
on the text it named.
