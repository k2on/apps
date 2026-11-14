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

    app.toml                        what it is to Android: its id, its theme, its dependencies; its
                                    gradle project is generated from it (ui/docs/app.md,
                                    "Generated, Not Committed"): `nix run .#revelation-prebuild`
    icon.svg                        its icon (the launcher's adaptive and themed icon are made from it)
    native/android/
      assets/bsb.txt                the Berean Standard Bible, as published
      res/values/                   its own theme and colours (the first frame black)
      main/dev/revelation/android/
        Bible.kt      the text as one String with IntArray indices; Ref <-> verse; search; typed references
        Layout.kt     the reference layout at reading size: tokens measured once, greedy breaks, the line table
        Camera.kt     (y, scale), the screen transform, the clamps, pinch about a focal point, the rubber band
        FlyTo.kt      van Wijk and Nuijten's smooth zoom-and-pan in one dimension (d3's interpolateZoom)
        Levels.kt     the level-of-detail decisions, bars, the column and its rules, labels, the header text
        Gestures.kt   the gesture state machine: idle, dragging, flinging, pinching, animating
        BibleView.kt  the View: draws what Levels decides, and turns touches into camera moves
        Shell.kt      the Compose chrome: header, picker, search, About
        Theme.kt      the one palette, white on black, shared by the View and Compose
        MainActivity.kt
      debug.keystore                the key every build signs with (see below)
      deps.json                     the Maven graph gradle replays offline
    fdroid/                         the F-Droid metadata

The pieces that decide things (`Bible`, `Layout`, `Camera`, `FlyTo`,
`Levels`, `Gestures`) are pure Kotlin with no Android types. `BibleView`
and the activity are kept thin.

## Building on a laptop

You need JDK 21, Gradle 8.x (there is no wrapper, as in harken), and an
Android SDK with **platform 35** and **build-tools 35.0.0**. The toolchain
is the same as `harken/android`'s: AGP 8.7.3, Kotlin 2.0.21, Compose BOM
2024.12.01. `nix run .#revelation-prebuild`, then from
`revelation/.native/android`:

    export ANDROID_HOME=/path/to/sdk ANDROID_SDK_ROOT=/path/to/sdk
    gradle :app:assembleRelease   # app/build/outputs/apk/release/app-release.apk
    adb install app/build/outputs/apk/release/app-release.apk

`versionCode` and `versionName` come from gradle properties
(`-PversionCode=…`, `-PversionName=…`) and default to 1 and 0.1, as in
harken. AGP reads `-Pandroid.aapt2FromMavenOverride=…` itself, so a nix
derivation can point it at an `aapt2` it built.

**One key, committed, and public on purpose.** `native/android/debug.keystore`
signs both the debug and the release builds, so that one APK installs over
the previous one. The reason is the same as harken's, and is written next
to `signingConfigs` in `app/build.gradle.kts`. Anyone can read the key, so
anyone can sign an APK as this app. It must never sign anything published
as trusted.

## Built, not run

`nix build .#revelation-apk` compiles it and assembles the APK, and
nothing more is checked by a build.

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
- Double-tap, the two-finger tap and taps on labels.

## Not done in the prototype

- **Reflow.** A pinch past reading size rubber-bands and springs back to
  scale 1. It does not lay the text out again at a larger font. There is
  no font-size setting.
- **A bundled serif.** The text is in `Typeface.SERIF`, the system's
  serif.
- **No accessibility yet.** TalkBack cannot read a canvas.
  `ExploreByTouchHelper` over the visible verses is the planned answer.
- Selection, bookmarks, highlights and a second translation.
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
