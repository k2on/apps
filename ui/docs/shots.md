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
| iced (`arkui::motion`) | `ARK_SHOTS=1` (app-desktop.sh sets it): `motion::shots()` | every `Slide` (and so every `Shown`) is on `INSTANT` |
| Browser (`arkui-web`) | `?shots=1`: `arkui_web::shots()` | `data-motion="shots"`: no animation or transition anywhere, the caret clear |

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

- **Its runtime** (`ark_app::Runtime::idle`: no message being taken, no
  module's answer awaited) and **its peer** (`ark_ffi::Peer::settled`:
  alone, or linked with nothing of its own unconfirmed and no call's answer
  waiting): the phones' bridges read both after every drain.
- **What is loading**: a picture (`Face`, harken's covers), through
  `Ready.begin`/`end` (Swift), `Ready.waiting`/`still` (Kotlin), or the
  page's own fetches and images (the browser's, watched by itself).

| Toolkit | Where the signal is | How the driver waits |
|---|---|---|
| iPhone, iPad | an accessibility element `ark.ready`, labelled `ready <generation> <route>`, only while ready | `Drive.swift` waits for it at a newer generation (`waitForExistence` and a predicate, a minute at most, which only fails) |
| Android | the log, tag `ArkReady`: `ready <generation> <route>` or `busy …`; and a 1 px view whose content description says the same | `app-android.sh` reads `adb logcat -s ArkReady` as it comes |
| Browser | `window.__ark.after(g)`, a promise of the first ready moment after generation `g` | `web-shots.js` awaits it after every step |

A wait's limit is a guard: reaching it fails the shot, with the screen and
its tree kept beside it. It is never how long a shot waits when all is well.

## What Else Holds Still

- The day and the clock: the demo's fixed day (`-demo <day>`, the
  showcase's time warp), the system's status bar overridden
  (`simctl status_bar`, Android's demo mode).
- The caret: clear in shots mode.
- Where a list is scrolled: where a shot's steps put it, or its top.

## Navigation

A shot opened by a `route` is opened as a link (`<scheme>://<route>`) in
the running app; one opened by launch arguments, or with neither, is opened
afresh, as before. Taps remain for what only a finger reaches (a menu, a
gesture), and each is followed by the ready signal too.
