# Screenshots That Do Not Depend On Time

Every app's screenshots (`.github/screens/apps.md`) are taken when the app
says its screen is ready, never after a sleep, never once "the picture has
stood still", and with nothing on the screen moving. A capture launches the
app in **shots mode**; the shared libraries do the rest, so an app gets it
by using them.

## Shots Mode

| Toolkit | How it is asked for | What it does |
|---|---|---|
| SwiftUI (`ui/swift`) | `-shots YES` among the launch arguments: `Shots.on` (ArkUICore) | `Springs.move`, `.size`, `.fade`, `.calm`, `released(…)` and `Animation.reduced(_:)` are `Springs.instant`; `.motionTransition` is `.identity`; `.arkShots()` on the app's root takes the animation out of every transaction under it, turns UIKit's animations off and clears the caret |
| Compose (`ui/kotlin`) | `-shots 1` in the intent's `ark.args`: `Shots.on`, read by `keepMotion()` | the activity's Compose runs on a duration scale of 0 (`ReducedMotion.scale`): every animation, arkui's and Material's, ends the frame it starts |
| iced (`arkui::motion`) | `ARK_SHOTS` set (app-desktop.sh sets it): `motion::shots()` | every `Slide` (and so every `Shown`) is on `INSTANT` |
| Browser (`arkui-web`) | `?shots=1`: `arkui_web::shots()` | `data-motion="shots"`: no animation or transition anywhere, the caret clear |
| Mac (`ui/swift`) | `-shots YES` (app-mac.sh) | as SwiftUI's |

This is not reduced motion. Reduced motion is a gentler motion; shots mode
is none, a fade included. `ALWAYS_ANIMATE` and `alwaysAnimate` are
untouched by it.

## The Ready Signal

The app is **ready** when nothing it said it waits for is still to come,
nothing on the screen is on its way anywhere, and a frame has been drawn
since. Whatever may change what is drawn (a touch, a key, a link, a route,
the peer's news, the theme, a turn on its side) takes the signal away and
grows a **generation**. A driver reads the generation, does something,
then waits for the app to be ready at a newer one, and only then takes the
screen.

What an app waits for is said to the library:

- **Its runtime**, the app's own signal: `ark_app::Runtime::is_idle` (no
  message being taken, no module's answer awaited, every view the screen
  needs held) at `Runtime::version` (which grows with every message taken:
  a newer version is a newer screen, waited for again). The bridges say
  both (`is_idle()`, `version()`); the browser's page says them on its
  root (`data-ark-idle`, `data-ark-version`); the desktop window hands them
  to arkui-shots (`Asked::run_ready`).
- **Its peer** (`ark_ffi::Peer::settled`: alone, or linked with nothing of
  its own unconfirmed and no call's answer waiting): the phones' bridges
  read it after every drain.
- **What is loading**: a picture (`Face`, harken's covers, on both phones),
  through `Ready.begin`/`end` (Swift), `Ready.waiting`/`still` (Kotlin), or
  the page's own fonts and images (the browser's, read by the driver).
- **What still moves**, which the library sees for itself: on the iPhone a
  layer's animation with an end (one that repeats, for ever or by being
  added afresh, or that turns a picture over in place — a spinner — has
  none, and is not waited for); on Android a window that drew since the
  last frame (Compose draws only while something changes; one that draws
  on for a second with nothing waited for, a spinner, is not waited for
  either); in the browser an animation with an end.

| Toolkit | Where the signal is | How the driver waits |
|---|---|---|
| iPhone, iPad | an accessibility element `ark.ready`, labelled `ready <generation> <route>`, only while ready | `Drive.swift` waits for it at a newer generation (`waitForExistence` and a predicate, a minute at most, which only fails) |
| Android | the log, tag `ArkReady`: `ready <generation> <route>` or `busy …`; and a 1 px view whose content description says the same | `app-android.sh` reads `adb logcat -s ArkReady` as it comes |
| Browser | the root's `data-ark-idle` and `data-ark-version` | `web-shots.js`, after every step: idle at one version over two frames, the fonts loaded, every image decoded, no animation with an end running |
| Desktop (iced) | the app's `is_idle`/`version`, handed to arkui-shots | `Window::until_ready`: idle at one version over two frames drawn with nothing else to do |
| Mac | none yet: the window is taken a few seconds after it opens | app-mac.sh |

A wait's limit is a guard: reaching it fails the shot, with the screen and
its tree kept beside it. It is never how long a shot waits when all is well.

## What Else Holds Still

- The day and the clock: the demo's fixed day (`-demo <day>`, the
  showcase's time warp), the system's status bar overridden
  (`simctl status_bar`, Android's demo mode). A time the app works out
  from the clock against its server's seed is said in hours: money's demo
  household is made an hour and a minute before it is seeded, so "Synced
  1 hour ago" holds for the hour after (in the day's first hour, minutes
  still).
- A system banner over the app (the simulator's own notices, "Ready for
  Apple Intelligence"): the iPhone's and the iPad's driver looks for one in
  SpringBoard before every picture, swipes it away and waits it out, and
  fails the shot if it stays; the Android emulator's are hidden by demo
  mode (`notifications visible false`).
- What the system shows that is not the app: Android's own animations
  off, its status bar's icons all said, the launcher's home laid out
  before the app is installed; each iOS device pass on a server of its own
  (the iPad's not left what the iPhone's played).

Held still since dry runs 42971809544 and 42977527249 (37 of 81 Android
shots byte-identical between them): the navigation bars hidden in shots
mode (ui/kotlin/shots: the gesture handle, and the launcher's taskbar with
the apps it predicts in its own order on the tablet and the open Fold),
with the system's "Viewing full screen" told as seen already
(`immersive_mode_confirmations`, which had come up over the first launch,
dry run 42984084410); harken's album credits in the order the album plays,
not the order a scan added its tracks (`browse::albums`: a tie between
three soloists went to a different one each run, on every phone and the
desktop); and a cover decoded for each size it is drawn at (harken's
`Covers`, ArkUI's `CoverLoader.cached(url, side)`: the full player drew
whichever size of its cover was decoded first, the mini player's blurred
or its own sharp one). Not held still: one to three levels of antialiasing
at rounded edges and in the status bar's icons, the emulator's GPU, and
where SystemUI puts the status bar's clock (a few pixels to the side, one
run to the next). Two runs are held to each other by
`.github/screens/same.py`: byte-identical, the same within three levels
(the antialiasing), or different, and where.
- The caret: clear in shots mode; and no field takes the keys by
  itself (SwiftUI's `AmountEntry` and `NameField` focus as they appear
  only outside shots mode), so no keyboard covers a sheet unless a shot's
  step taps a field to type in it.
- Where a list is scrolled: where a shot's steps put it, or its top.

## Navigation

Every driver opens the app once for each device and theme and walks its
shots in order in that one launch (`.github/screens/apps.md`). A shot is
reached in the running app: by its launch arguments as a **launch link**
(`<scheme>://?screen=…&inspector=1`, the browser's spelling of them, read by
`Navigator.launchArgs` / `Navigator.launchArgsOf`), which an app's shell
takes as a launch — everything it showed put back where a launch leaves it
(ark_app's `Launch` puts back the shell's own state; the platform puts away
its sheets and pages), then that screen; by its `route` as a link
(`<scheme>://<route>`); or by its steps from where the last shot left it.
Only a shot marked `fresh`, or one whose launch-time arguments change, has
the app launched again. In the browser the same is the address moved in
place (`history.pushState` and `popstate`): arkui-web puts the app back
(`WebApp::rewind`) and its own menus, popovers, command box and focus with
it, then arrives at the new address. Taps remain for what only a finger
reaches (a menu, a gesture), and each is followed by the ready signal too.
