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
| iPhone, iPad | an accessibility element `ark.ready`, labelled `ready <generation> <route>`, only while ready, its value the theme the window draws in (`light`, `dark`) | `Drive.swift` waits for it at a newer generation (`waitForExistence` and a predicate, a minute at most, which only fails), and fails a shot drawn in the other theme |
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

## In the App

On the phones and the tablets an app built on ArkUI's runtime (money;
revelation, which takes no steps) walks its shots itself and photographs
itself: nothing outside it looks at the screen, finds a label or touches
it. A launch with `-shots` that finds a walk's plan in the app's own
directory — `Application Support/shots/plan.json` on an iOS simulator,
the app's external files' `shots/plan.json` on an Android emulator,
which only the app and the machine's own tools write — is a **walk**
(`Launch.walk`): ArkApp's `ShotsWalk` (ui/swift/Sources/ArkApp) and
ArkUI's `ShotsWalk` (ui/kotlin/shots) take it, then the process ends.

- **The steps are Rust's** (`ark_app::shots`, `ArkHost.shots_step`): a
  step reaches what it names in the ArkUI views on the screen now — the
  first run's page while it shows, else every runtime's view, each hosted
  one in its place, only what shows (a tab bar's selected tab, a pushed
  page while it is pushed, an inspector while it is open), and while a
  sheet, a dialog or an alert is up only what is in it — and sends the
  message that element's action sends, through the same runtime's `send`
  a finger's tap goes through.
- **A step names an element by its id**, never by what it says, so no
  copy change, no count in a row's words and no other language moves it.
  Ids are derived, not written (`shots::id_of`): a tab by its section's
  tag (`tab:budget`; a child section by its parent and title,
  `tab:accounts/checking`); a row by its title as a slug (`row:checking`,
  `row:dining-out`; rows of one title by their place after the first,
  `row:target~2`); anything else by the message it sends, its variant in
  kebab case (`auto-assign`, `new-transaction`, `dismiss`; a field by what
  its input sends, `typed`), with its words too when the message carries a
  value (`toggle:bills`, `inspector:category-details`). Each but a tab's
  has its screen's name before it (`budget.auto-assign`,
  `accounts.row:checking`). Two elements of one id fail the step that
  names it, naming both. `tap` (a field tapped takes what is typed next),
  `type`, `press` with its menu's `item` id, `await`; `{"text":
  "<label>"}` reaches an element by what it says, for one with nothing
  else to name it by (no walk uses it).
- **A step that finds nothing fails at once** at a ready screen (what it
  names will not come), with the ids on the screen beside what each says.
  While the screen is still on its way it is looked for again each time it
  is ready anew: the first run's work off the shell's thread (a server
  checked, a code traded, a session opened) keeps the host from idle.
- **After each step the app waits for its own ready signal** at a newer
  generation (`Ready.untilReady`, `Ready.awaitReady`): its limit only
  guards. What held it is logged, each probe saying what it waits on
  (`ArkHost.busy`: work off the shell's thread, the peer's own intents
  unconfirmed or a call's answer to take, each runtime's message running,
  module answers, messages after the draw, views not held). While the
  screen Rust says is up has a sheet, a dialog or an alert in it (its
  tree's `(modal)`), the iPhone's and the iPad's is not ready until the
  platform has presented one; and the system's material settling into a
  theme (Liquid Glass's backdrop, which the windows' speed does not
  reach) is put at its end at once.
- **Then it draws itself**: its own windows at the screen's scale (iOS:
  every window of its scenes by `drawHierarchy`, lowest level first, so a
  sheet, an alert, a confirmation dialog, a menu or a popover is over
  what it covers; Android: every window of the process by PixelCopy, a
  dialog's dim drawn as the system dims it). The status bar is the
  system's and in neither. Each picture is `<id>.png` under its theme,
  with what a step can name beside it (`<id>.tree.txt`).
- **Every way in one launch, a walk a way.** The shots before the
  sign-in's (`welcome`) are taken every way first, one switch apart. Then
  the rest of the list is walked whole once a way — light, then dark; on
  the iPad then dark on its side, then light on its side — each walk after
  the first begun by a reset inside the app (`ArkHost.shots_reset`: the
  app's part made anew over the same session, `Hosting::reopen`, so every
  screen's state is what a launch already signed in shows — no filter,
  search, toggle, selection, inspector, sheet, menu or pushed page, the
  first section, the day's month — with nothing of any screen listed: a
  field a screen adds is put back with the rest; then a link with no
  arguments, which puts away the platform's own pages; the session and
  its data kept. A step that sets something sets it, where it can, rather
  than turning it over: `transactions.show:all` chooses All) and one
  switch (the theme: ArkUI's `ArkAppearance` on every window of every
  scene and every window shown after, an alert's and a menu's, with each
  bar's appearance handed back and each material's effect set afresh, as
  Settings' Appearance does it while the app runs — iOS 26's glass bars
  kept the old theme otherwise, 43318181211 — and Android 12's per-app
  night mode, which an Ark app's activity takes in place, `configChanges`
  `uiMode`; the iPad's way up: `requestGeometryUpdate`, which iPadOS 26
  allows only an app that takes the whole screen, so apps-ios.sh writes
  `UIRequiresFullScreen` into the simulator's build it installs on the
  iPad, and no build for people has it). Before each picture the walk
  checks the switch happened: the middle of the tab bar (else the top
  navigation bar) on the iPhone and the iPad, the top of the app's window
  on Android, is dark in a dark picture and light in a light one, or the
  walk fails. The sign-in's
  steps (its shot says `signIn`) are the first walk's alone. Within a walk
  two shots of one picture fail (a step that went nowhere); at the end, so
  do two ways of one shot that drew the same picture, unless the plan says
  the app draws one theme (`"theme": "dark"`). The log has each reset and
  each walk's whole time. Android's tablet is taken in landscape alone, as
  before; the Fold's postures are the emulator's, each a walk of its own.
- **The sign-in.** The walk goes through the first run as a new person
  does. The system's browser sheet is not the app's and no step reaches
  it, so a walk's launch reads the dev server's answer to the sign-in
  itself (`ark_auth::client::straight_back`, as the desktop does) and
  takes the code only when it comes straight back to the app's own
  `<scheme>://auth`: a real server answers with a page for a person, and
  then the sheet opens as ever. Only a simulator's build (`target_abi =
  "sim"`) or Android's on an emulator can.
- **Times.** Each shot's steps (each its Rust call and its ready wait),
  switches and pictures are logged (`shots/out/walk.log`, `ArkShots` in
  Android's log, `Shots:` on iOS).
- **Ids outside the walk.** The same ids are not yet written into each
  toolkit's accessibility identifiers (SwiftUI's
  `.accessibilityIdentifier`, Compose's `testTag`, the browser's
  `data-ark-id`): the walk reads them from the views, and nothing outside
  the app needs them yet.

An app whose screens are its own rather than ArkUI's views (harken's)
is still driven from outside, as below.

## Navigation

A walk (above) reaches every shot by its steps from where the last left
it, from a new install. A driver from outside opens the app once for each
device and theme and walks its shots in order in that one launch
(`.github/screens/apps.md`), each by its steps; only a shot marked
`fresh` has the app launched again. The Mac's and the browser's screens are
each opened by its launch arguments (the browser's address): read by
`Navigator.launchArgs` / `Navigator.launchArgsOf`, taken by an app's
shell as a launch. In the browser the same is the address moved in
place (`history.pushState` and `popstate`): arkui-web puts the app back
(`WebApp::rewind`) and its own menus, popovers, command box and focus with
it, then arrives at the new address. Taps remain for what only a finger
reaches (a menu, a gesture), and each is followed by the ready signal too.
