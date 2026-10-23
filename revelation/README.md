# Revelation

The Bible as one scroll, zoomed like a map. An Android app whose whole
surface is one column of text, Genesis 1:1 at the top and Revelation 22:21
at the bottom. It scrolls the way Android scrolls and zooms continuously:
pinch out and the words become lines, the lines become a column, and at the
far end the column is a single white line the height of the screen. Going
anywhere is a map's camera move: out, across and back in. The design is
`docs/plan-bible.md`; this directory implements it as a prototype.

It is Kotlin and nothing else. There is no Rust, no C, no JNI, and no build
tool for the text. The translation ships as the file it was published as,
and the app parses it at launch.

## What is here

    android/                        the gradle project (one module, :app)
      app/src/main/assets/bsb.txt   the Berean Standard Bible, as published
      app/src/main/kotlin/dev/revelation/android/
        Bible.kt      the text as one String with IntArray indices; Ref <-> verse; search; typed references
        Layout.kt     the reference layout at reading size: tokens measured once, greedy breaks, the line table
        Camera.kt     (y, scale), the screen transform, the clamps, pinch about a focal point, the rubber band
        FlyTo.kt      van Wijk and Nuijten's smooth zoom-and-pan in one dimension (d3's interpolateZoom)
        Levels.kt     the level-of-detail decisions, bars, the column and its rules, labels, the header text
        Gestures.kt   the gesture state machine: idle, dragging, flinging, pinching, animating
        BibleView.kt  the View: draws what Levels decides, and turns touches into camera moves
        Shell.kt      the Compose chrome: header, picker, search, About
        Theme.kt      the two palettes, shared by the View and Compose
        MainActivity.kt
      app/src/test/kotlin/dev/revelation/android/   JUnit 4 tests and the Paparazzi screens
      debug.keystore                the key every build signs with (see below)
      fdroid/                       the F-Droid metadata and icon

The pieces that decide things (`Bible`, `Layout`, `Camera`, `FlyTo`,
`Levels`, `Gestures`) are pure Kotlin with no Android types, and are tested
on the JVM. `BibleView` and the activity are kept thin.

## Building on a laptop

You need JDK 21, Gradle 8.x (there is no wrapper, as in harken), and an
Android SDK with **platform 35** and **build-tools 35.0.0**. The toolchain
is the same as `harken/android`'s: AGP 8.7.3, Kotlin 2.0.21, Compose BOM
2024.12.01 and Paparazzi 1.3.5. From `revelation/android`:

    export ANDROID_HOME=/path/to/sdk ANDROID_SDK_ROOT=/path/to/sdk
    gradle :app:testDebugUnitTest :app:assembleRelease   # the tests, and app/build/outputs/apk/release/app-release.apk
    gradle :app:recordPaparazziDebug                     # the screens, app/src/test/snapshots/images/*.png
    adb install app/build/outputs/apk/release/app-release.apk

`versionCode` and `versionName` come from gradle properties
(`-PversionCode=…`, `-PversionName=…`) and default to 1 and 0.1, as in
harken. AGP reads `-Pandroid.aapt2FromMavenOverride=…` itself, so a nix
derivation can point it at an `aapt2` it built.

The screenshots take under a minute. Layoutlib's text measurement is
slow, so the whole Bible is measured with it once, and every later
surface shot reuses that layout. `BibleView` keeps the last layout for the
process.

**One key, committed, and public on purpose.** `android/debug.keystore`
signs both the debug and the release builds, so that one APK installs over
the previous one. The reason is the same as harken's, and is written next
to `signingConfigs` in `app/build.gradle.kts`. Anyone can read the key, so
anyone can sign an APK as this app. It must never sign anything published
as trusted.

## What is proven on the JVM

`gradle :app:testDebugUnitTest` checks the following:

- **The text.** 66 books, 1,189 chapters and 31,102 verses. Genesis 1:1
  begins "In the beginning". The last verse is Revelation 22:21. John 3:16
  and Psalm 23 are where their references say. Every verse's reference
  round-trips through its index. The two attribution lines are kept.
  Search ignores case, finds each verse once, never matches across two
  verses, and scans the whole text in milliseconds. Typed references work
  ("john 3:16", "Ps 23", "1 cor 13:4").
- **The layout.** These checks run over the whole text with a fake
  measurer. Every character of every verse is in exactly one line. No
  line is wider than the column. Breaks are greedy. `y` is monotonic and
  the blocks add up. Each verse's first line is where its `Ref` says.
  Each distinct token is measured once (about 35,000 of them). The whole
  Bible lays out in about 100 ms on this machine, with a 300 ms bound in
  the test. At a Pixel 5's width that is about 99,600 lines and 2.76
  million dp of document.
- **The camera.** Screen and document coordinates round-trip. Zoom stops
  at its bounds. The document never leaves the screen, and is centred
  with its pad when it fits. A drag past an end reports the overscroll
  for the edge effect. A pinch keeps the document point under the
  fingers still, and the focal drift pans. The rubber band gives a
  little and no more.
- **The fly-to.** The endpoints are exact. `w` rises and then falls,
  zooming out about as far as the whole Bible to cross it. `y` only moves
  toward the target. A short move barely zooms out. The degenerate case
  (no distance) is a plain zoom. The duration is clamped to 300–1200 ms.
- **The levels.** The thresholds are 5, 1.2, 6, 4, 24 and 20 px. The
  crossfades sum to one, and no level moves by a visible step across the
  whole range of zoom. The column's ink is the bars' coverage (0.5) and
  rises to full as it narrows to the line. The line at the far end is one
  whole pixel, centred. Rules appear only where their thresholds allow.
  Labels never overlap, and the longest books win. The header reads
  "Psalm 23", then "Psalms", then "Job – Isaiah"-like ranges, and
  "Genesis – Revelation" at the far end.
- **The gestures.** The state machine's transitions are tested: a touch
  stops a fling or an animation where it is, the finger left after a
  pinch neither drags nor flings, and no animation starts while a finger
  is driving the camera.
- **The screens.** Paparazzi draws the real `BibleView` over the real
  text inside the real Compose shell, at a Pixel 5's size, light and
  dark. These are the six camera states (`01_reading`, `02_small_glyphs`,
  `03_greeked`, `04_chapters`, `05_books`, `06_line`) and the chrome
  (`07_picker`, `08_picker_chapters`, `09_search`, `10_search_reference`,
  `11_about`). Each has a `-light` and a `-dark`.

## What only a phone can show

No phone or emulator has run this APK. The following are unknown until
one does:

- Frame times during a fling, and during a pinch over reading-size text
  (`FrameMetrics`, Perfetto). This is the plan's first risk: every pinch
  frame re-rasterises the glyphs at a new size. The first thing to do
  with the APK is to pinch over Psalms with the frame-time overlay on.
- How long the layout takes at launch with the real `Paint.measureText`.
  It runs off the main thread, so the screen is bare ground until it
  lands. The JVM's 100 ms with a fake measurer suggests a few hundred ms
  on a mid-range phone.
- How the fling feels next to `ScrollView`'s. It is the same
  `OverScroller` running in screen pixels, so it should feel the same.
- The stretch overscroll on Android 12 and later, applied by `EdgeEffect`
  to the view's render node, and the glow before Android 12.
- Whether the fly-to reads as one motion, and whether the crossfades
  between levels pop under a moving pinch.
- Double-tap, the two-finger tap and taps on labels. The touch routing in
  `BibleView` has no automated test, only the state machine under it does.

## Not done in the prototype

- **Reflow.** A pinch past reading size rubber-bands and springs back to
  scale 1. It does not lay the text out again at a larger font. There is
  no font-size setting.
- **A bundled serif.** The text is in `Typeface.SERIF`, the system's
  serif. It renders under layoutlib, so the screenshots use it too.
- **No accessibility yet.** TalkBack cannot read a canvas.
  `ExploreByTouchHelper` over the visible verses is the planned answer.
- Selection, bookmarks, highlights and a second translation.
- **Popping book labels.** Book labels placed by the collision rule appear
  and disappear whole as the zoom changes. Chapter labels and every level
  fade instead.
- **The saved place.** The camera is saved to SharedPreferences, not
  DataStore. It stores the verse under the centre, the offset from its
  first line and the scale, so it survives a change of width.

## The text

The Berean Standard Bible, as published at BereanBible.com
(`app/src/main/assets/bsb.txt`, unchanged). Its attribution, shown in the
app's About sheet:

> The Holy Bible, Berean Standard Bible, BSB is produced in cooperation
> with Bible Hub, Discovery Bible, unfoldingWord, Bible Aquifer,
> OpenBible.com, and the Berean Bible Translation Committee.
>
> This text of God's Word has been dedicated to the public domain. Free
> resources and databases are available at BereanBible.com.

The app's own code is MIT OR Apache-2.0.
