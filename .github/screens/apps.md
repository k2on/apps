# The apps' screenshots

Each app's screens, taken from the builds that were published (the APK in
the F-Droid repository, the commit sent to TestFlight), signed in to the
app's own server with sample data. An app built on ArkUI's runtime walks
its shots on the phones and tablets itself, from a new install, and draws
its own pictures (shots.json's `walk`, ui/docs/shots.md "In the App"); the
others are driven from outside as a person would. No code in any app
knows it is being photographed: what does it is ArkUI's and ark-app's. The same system
the docs' arkui shots use (`screens.yaml`): a real Android emulator and a
real iOS simulator, light and dark, onto the `screens` branch with a
manifest. Beside them a third "phone", `desktop`: the app's iced window,
built by nix at the commit and run with no window ([below](#the-desktop)).
And the larger screens, each a "phone" of its own, in the same jobs: an
Android tablet and a Fold, an iPad upright and on its side when the app's
build takes the iPad, and the Mac's own window when the commit has a Mac
build ([below](#the-larger-screens)). On a pull request, the same shots
are compared with main's and the ones that changed are shown on the PR.

`.github/workflows/app-screens.yaml` runs it. Nothing there, or in the
scripts here, names an app.

## An app's part

The apps are the flake's one list (`flake.nix`, `publish`: its `apks`, its
`ios` and its `desktop`); an app on it with `<app>/demo/shots.json` is
photographed on each phone it is listed for, and on the desktop when its
shots.json has `desktop`. Every app's id, its Android applicationId
and its iPhone bundle id, is `bundlePrefix` (`us.koon`) and its name, so
neither driver is told it: Android reads it from the APK, iOS from the app.

All of it is in `<app>/demo/`:

- **`shots.json`'s `walk`**: the app's server, and the walk its phones
  and tablets take inside the app (money's, revelation's):

  ```json
  {
    "server": { "port": 8788, "user": "alex", "name": "Alex" },
    "walk": {
      "phone": [
        { "id": "welcome", "steps": [] },
        { "id": "budget", "steps": [{ "tap": "welcome.next" }, { "tap": "welcome.typed" }, { "type": "{server}" },
                                    { "tap": "welcome.check" }, { "await": "tab:budget" }] },
        { "id": "category", "steps": [{ "tap": "budget.row:dining-out" }] }
      ],
      "large": [ … ]
    }
  }
  ```

  One list for both phones (`phone`: the iPhone, Android's phone), one for
  the larger screens (`large`: the iPad, which turns on its side at every
  stop inside the walk, Android's tablet, the Fold open and folded), the
  same ids. The app is installed afresh on each device (and in each of the
  Fold's postures) and opened with its list as a plan in its own
  directory: it shows its first run (`welcome`), signs in through it
  (`{server}` is this device's server's address), lands where it lands,
  and reaches each shot after by its steps from where the last left it:
  tabs, rows, toolbar buttons, a sheet's dismissal, a dialog's actions.
  A step names an element by its id (ui/docs/shots.md, "In the App":
  `tab:budget`, `budget.row:dining-out`, `shell.new-transaction`,
  `budget.cancel-auto`): `{"tap": "<id>"}`, `{"type": "<text>"}` (into the
  field tapped last), `{"press": "<id>", "item": "<item id>"}`, `{"await":
  "<id>"}`. Each shot's `<id>.tree.txt` lists every id on its screen beside
  what it says, and a step that finds nothing fails with that list
  (`stuck.why.txt`). Each stop is taken in both themes (and on the iPad
  both ways up), one switch at a time. `"theme": "dark"`: an app that draws
  one theme (revelation), each screen taken once as both.

  Not reached by a tap, and so not taken on the phones: the Mac's and the
  browser's own (a row's context menu open, a date picker's popover, the
  command box), which only a pointer or keys open; and nothing yet of
  Settings past its first pane (Settings is put up by the platform's own
  sheet, which the views do not hold, so the walk ends there).

- **`shots.json`'s `shots`**: an app whose screens are its own, not
  ArkUI's views (harken's), and so driven from outside. **The app is
  opened once for each device and theme, and its shots are taken in that
  one launch, in order**: each shot's `steps` done from where the last shot
  left the app, and the screen taken. `"fresh": true` on a shot opens the
  app again for it (harken's `player`, which plays from Home's Recently
  added and needs the mini player there from the start). A step's
  `{"tap": "<label>"}` taps the element whose accessibility label or
  identifier is `<label>` (an iOS `accessibilityIdentifier`, an Android
  resource id or exposed Compose `testTag`), `"role"` as above;
  `{"press": "<label>"}` holds it; `{"type": "<text>"}` types into what has
  focus; `{"wait": <seconds>}` waits; `"where": "bottom"`, `"unless":
  "<label>"` (skip when that label is on the screen already), `{"await":
  "<label>"}` (up to 20 s, `a|b` either), `{"relaunch": true}`, and
  `"hold": true` on a tap (a long press). Each shot is saved with the tree
  of the screen it was taken from (`<id>.tree.txt`: on Android `ui.py
  --tree`'s outline of uiautomator's dump; on iOS XCUITest's description);
  a step that finds nothing leaves `stuck.png` and `stuck.tree.txt`.

  `server` (optional, either kind): the port its `server.sh` listens on,
  the id the server's dev sign-in is given (`user`, lowercase: the login
  and what entries carry), and the name that person goes by (`name`,
  `ARK_DEV_NAME`: what Settings, the account button and "Entered by"
  show), and their picture.

- **`shots.json`'s `screens`** (optional): the Mac's and the browser's
  screens, opened by their launch arguments alone, no steps, on the sizes
  each is `on` (`mac`, and the browser's `ios`, `ipad-portrait`,
  `ipad-landscape`):

  ```json
  "launch": { "mac": ["-demo", "2026-11-18", "-window", "1440x900"] },
  "screens": [
    { "id": "category", "args": ["-screen", "category:dining"], "on": ["ios", "android"] },
    { "id": "category", "args": ["-screen", "category:dining", "-inspector", "1"], "on": ["ipad-portrait", "ipad-landscape", "android-tablet", "fold-unfolded", "fold-folded", "mac"] }
  ]
  ```

  **One list, one id for a screen on every device.** The phones take the
  same screens in the same order, and so do the larger screens (the iPad
  either way up, Android's tablet, the Fold open and folded); a screen
  that each shows its own way (money's category: Category Details on a
  phone, the inspector on a larger screen) is two entries under one id,
  so the same screen is the same file on every device and `compare.py`
  and the docs hold it against itself. money's: `budget`, `category`,
  `accounts`, `account` (Checking's register), `review`, `transaction`
  (the newest with a receipt), `add-transaction`, `insights`, `settings`,
  `auto-assign-menu`, then `welcome`; the Mac's are the same but the last
  two.

  Every Ark app's shell reads them (`ark_app::shell::Launch`: `-screen
  <place>[:item]`, `-inspector 1`, `-appearance`, `-pane`), the iPhone's
  and the Mac's from the process's arguments (XCUITest's
  `launchArguments`, a Mac app's command line), Android's from the launch
  intent's string array `ark.args` (`am start --esa ark.args
  -screen,budget`), the desktop's from its command line. Every phone and
  tablet is signed in to the app's server as `server.user`, so every shot
  of a run is of one household, on one day (`DEMO_DAY`, `server.sh`
  below). `launch.mac` goes before each screen's on the Mac, which no
  browser signs in for: money's opens its demo household, the one its
  server is seeded with, and `app-mac.sh` gives its `-demo` the run's day.

- **`server.sh`** (only for an app with a server):
  `server.sh <dir> <listen address>`, run from the repository's root,
  starts the app's server with dev auth and something in it, writes its pid
  to `<dir>/pid` and its log to `<dir>/server.log`, and returns. Each
  seeds a fresh data directory first with the app's seed, which lives
  beside the app, not in it (`docs/seed.md`): `nix build .#<app>-seed`, then
  `<app>-seed <dir>/data`, then the server from `nix build .#<app>-server`
  on it. money's writes the demo household as the log the server boots on
  (`money-seed`), up to the run's day: the workflow's `which` says it once
  (`day`, UTC) and every job has it as `DEMO_DAY`, so the phones, the
  tablets, the desktop and the Mac's demo (`app-mac.sh` gives `-demo` that
  day) are one household on one day, however long a job waited for its
  runner (a device's own clock still says which day is today: a job that
  starts after midnight shows the household a day old); harken's writes the demo catalogue as a media root of
  tagged FLAC files and covers, `<dir>/data/media`, for the server's own
  scanner to read (`harken-seed`, no ffmpeg).

- **`shots.json`'s `desktop`**: the window's size, its scale, and its
  shots, each an `id` (the phones' where the screen is the same) and steps:

  ```json
  "desktop": {
    "window": [1440, 900], "scale": 2,
    "shots": [
      { "id": "library", "steps": [{ "click": "Albums" }] },
      { "id": "search", "steps": [{ "key": "ctrl+k" }, { "type": "Elise" }, { "await": "Für Elise" }] }
    ]
  }
  ```

  A window has no accessibility tree to read, so a step names its target by
  the text the window lays out (iced's own selector, as iced_test finds
  text): `{"click": "<text>"}` clicks the middle of the text that says
  exactly that (`a|b`, `"where": "bottom"`, `"optional"` and `"unless"` as
  the phones' taps have them; `"button": "right"` for its context menu);
  `{"hover": "<text>"}` moves the pointer over
  it and leaves it there, so the shot is drawn pointed at it (a shot is
  otherwise drawn with no pointer); `{"scroll": "<text>", "by": <pixels>}`
  turns the wheel over it, `by` down (negative: up), for a page no key
  scrolls; `{"type": "<text>"}` types into what has the
  keys, and a key no widget takes goes on to the app's own, the vim keys;
  `{"key": "ctrl+k"}` presses one (`Enter`, `Escape`, `Tab`, `Backspace`,
  `Space`, the arrows, or a character, after `ctrl+`, `shift+`, `alt+`,
  `super+`); `{"await": …}`, `{"wait": …}` and `{"relaunch": true}` as the
  phones'. Each shot's `<id>.tree.txt` is every text on that screen and
  where it is, which is what a step can name. harken has no `player` on the
  desktop: nothing there can play ("the desktop build has no audio
  device"). money's `accounts` is Checking's register, its `transactions`
  all accounts'.

And the builds: on Android the artifact `apk-<app>` of `build.yaml`
(already the case for every APK in the flake's `publish`); on the iPhone
the flake's `<app>-ios-simulator`, and on the desktop its `<app>-iced`,
which nix builds at the commit (see below).

## What is shared

- **`app-server.sh`**: runs an app's `server.sh` on the port its
  `shots.json` names, then waits until `/healthz` answers and the log's
  head stands still (a scanner done adding).
- **Signing in**: a walk signs in through arkui's first run by its own
  steps; the browser's sign-in sheet is the system's, so a walk's launch
  takes a dev server's answer to it straight back without the sheet
  (ui/docs/shots.md, "In the App"; only a simulator's or an emulator's
  walk can). An app driven from outside signs in through arkui's
  onboarding the same way for all of them. They type the server's
  address into "Server address", tap Continue (and "Sign in" if it is
  there), give the server's dev sign-in page the name, and allow a
  permission the app asks for. The login is kept, so each shot after opens
  signed in.
- **`app-android.sh`** (with `ui.py`, which finds a label's place in
  uiautomator's dump): installs the APK; for a walk, pushes its plan into
  the app's external files (`/sdcard/Android/data/<package>/files/shots`),
  opens it, waits until the app says the walk is `done` (or `failed`,
  why), pulls the pictures and its `walk.log` (each shot's step, switch and
  picture times); for an app driven from outside, signs
  in through Chrome, and does each shot's steps with `adb input`. The emulator reaches the server on
  the host's loopback as `10.0.2.2`. Each shot opens the app as the
  launcher does (`am start -W` of its launcher activity) and waits until
  its own window has the focus (`dumpsys window`); one that never comes is
  a failure with its why beside it (`stuck.why.txt`, the step's
  `*-failed.txt`: the focus, the resumed activity, the crash log). The
  theme is set with the app stopped, and the app opened only once the
  night mode says it back and the launcher, which the change re-creates,
  is in front again.
- **`apps-ios.sh`**: for a walk, each app installed afresh (the
  simulator's keychain emptied), its plan
  written into its data container (`simctl get_app_container`), opened
  with `-shots YES`, and waited for until it says it is over; its pictures
  and `walk.log` copied out. One command per device, its own everything:
  `PORT_OFFSET` (its servers' instances, `app-server.sh <app> <dir>
  <port>`), `RESULTS` and `PARTIAL`, so the iPhone and the iPad
  can be taken at once. The iPad's walk writes upright and on its side
  apart (ipad-portrait/, ipad-landscape/). No UI-test driver is built:
  every app the phones take walks itself. harken and revelation are left
  out of the screenshots (app-screens.yaml's `which`) until they are
  ported to Ark apps; their shots.json stay.
- **`app-desktop.sh`**: runs the window nix built with `ARK_SHOTS` set,
  in a home of its own, given `--server` and `--user`; it signs in, does
  each shot's steps and writes its PNGs ([the desktop](#the-desktop)).
- **`apps-android.sh`**, **`apps-desktop.sh`**, **`apps-mac.sh`**: each
  app in turn, so one that fails does not stop the
  others, and the step fails at the end if any did (Android's too: a job
  whose shots failed is red, not only its log); `apps-android.sh` on the emulator profile `DEVICE` says.
  `apps-ios.sh` takes every app at once, on the iPhone or (`FAMILY=ipad`)
  the iPad, and fails at its end the same way.
- **`ios-family.sh`**: of the apps built, those whose build takes the iPad
  (the built `.app`'s `UIDeviceFamily`, which `TARGETED_DEVICE_FAMILY`
  writes, has 2).
- **`app-mac.sh`** (with `window.swift`, a window's id by its process):
  opens the Mac build and takes its window ([the larger
  screens](#the-larger-screens)).
- **`ios-inputs.sh`**: the hash of what an iPhone app's shots are taken
  from, said on Linux ([when it runs](#when-it-runs)).
- **`published.sh`**: each APK as the F-Droid repository offers it, for a
  run that builds nothing (a dry run: `build`'s at the commit), fetched
  once by the workflow's `apks` job and handed to every emulator's job;
  **`await-apk.sh`**: an APK from a `build` run still going, as soon as it
  is uploaded.
- **`ios-build.sh`**: each iPhone app as nix builds it at the commit:
  the cache's when its inputs have not moved, or built with the runner's
  own Xcode where it is installed. **`desktop-build.sh`**:
  each desktop window as nix builds it at the commit, the cache's when its
  inputs have not moved.
- **`apps-lay.sh`**: lays a run's captures over a tree. A phone taken only
  in part keeps what the tree had.
- **`manifest.py`**: the docs' manifest, written for each app
  (`apps/<app>/manifest.json`).
- **`hero.py`**: the README's picture of an app (`apps/<app>/hero-light.png`
  and `hero-dark.png`), drawn on main's publish from the captures just laid
  out: its logo and name, and one screen (its shots.json's `hero`) on the
  desktop in a laptop and on each phone in a phone, the devices drawn by
  the script from their `device.json`. A device with no capture of that
  screen is left out. Pillow and Inter from the flake's nixpkgs
  (`.#screens-python`, `nixpkgs#inter`).
- **`compare.py`**: a PR's shots against main's (ImageMagick, more than a
  thousandth of the pixels more than a few percent off), as the PR's
  comment. A phone is a directory under `apps/<app>/`; the comment's
  columns are its list of them (`PHONES`: Android, Android tablet, Fold
  open, Fold folded, iPhone, iPad, iPad landscape, Desktop, Mac), each pair
  there only when a screen shown has it on main or on the PR; a phone main
  has none of says "new". `manifest.py` lists whatever phones an app's
  directory has, so the new ones are in `apps/<app>/manifest.json` too.

## When it runs

On Android and the desktop each app is a job of its own, so the apps are
taken at once. The iPhone's are one job, on one Mac, the scarce runner:
one Xcode selected, one nix installed, one simulator booted, every app
built and every server started under the boot, then each app taken on it
in turn, the driver built once (`apps-ios.sh`). One that fails is said,
the others still taken and kept, and the job fails at its end. Each job
starts its apps' servers first, in the background (each `server.sh`,
nix's build of the server: the cache's, or built while the rest goes on),
under whatever is slow anyway: the apps' build on the iPhone and the
desktop, the system image and the emulator's boot on Android.

**An iPhone app whose shots would be the same is not taken.** What they
are taken from is hashed on Linux, before any Mac is asked for
(`ios-inputs.sh`, in `which`): the app's `<app>-ios-simulator` store path,
evaluated at the commit and not built (it moves only with what it is built
from), the packages its `server.sh` builds, its `demo/` (shots.json, server.sh), its sign-in's
picture, and the iPhone's scripts here. Each app taken whole keeps the hash
beside its shots, `<app>/ios/inputs`; the next run with the same one, on
main against what is published, on a PR against its own last shots or
else main's (shown as "not rebuilt"), passes it over, and when it passes
over every app no Mac starts. Android has the same from `build`, which
builds an APK only when its inputs moved.

**A PR merged or closed** is taken nothing of: `which` asks whether it is
still open, and the iPhone's job asks again as it starts (it may have
waited an hour for a Mac), then does nothing, and nothing is compared.

- **On main**: Android once `build` has ended and its deploy has
  succeeded, from that run's APKs: an APK is there only when its inputs
  moved and it went to F-Droid. The iPhone once `ios` has ended, each
  app whose hash moved built by nix at that commit: the cache's if its
  build has not moved, and the kernel's simulator slice the one `ios`
  built for its tests and pushed. The
  desktop as soon as `build` has started, the same way: every window built
  by nix at that commit (build.yaml's own build of it fills the cache).
  Each phone taken whole replaces what the `screens` branch had at
  `apps/<app>/<phone>/`.
- **On a pull request**: Android as soon as `build` starts, the iPhone
  once `ios` has ended. `build`
  builds the APKs whose inputs differ from what main published, and each
  app's job, its emulator booted and its server up meanwhile, takes its
  APK the moment the run uploads it (`await-apk.sh`), before the run's
  tests have ended; an app with no new APK is passed over. The iPhone apps
  are built by nix at the PR's commit after `ios`, which for an app with
  tests has built its kernel's simulator slice and its app at that commit
  and pushed them to the cache (a branch of this repository's), so the
  Mac here fetches them: one kernel per commit. The desktop
  windows at the PR's commit as `build` starts, beside it. The shots go to
  the branch `screens-pr-<number>`, and one comment on the PR, kept up to
  date, has a row for each screen that changed or is new on any phone:
  Android's on main and on the PR, then the iPhone's, then the desktop's,
  so every phone's before and after are side by side. `ios` runs on
  pushes to this repository's branches, and the desktop builds and runs
  the PR's code, so only for a branch of this repository, with a token
  that can only read: a fork's PR gets its Android shots only.

- **When only the screenshots' scripts change** (`.github/screens/`, an
  app's `demo/`, the workflow) on main, every app is taken again: each
  APK as the F-Droid repository's index offers it (`published.sh`), and
  each desktop window as nix builds main, which, as its inputs have not
  moved, is the cache's; each iPhone app whose hash moved (a change to
  Android's scripts moves none). Running the workflow by hand does the
  same, every iPhone app included. Run by hand with a PR's number (`pr`),
  it takes that PR's iPhone shots at its head, of each app whose hash
  moved, and updates its comment: after a Mac's run failed, or to try the
  workflow as a branch has it. Run by hand on a branch with `dry`, it
  takes every app as by hand, with that branch's workflow and scripts and
  its commit's iPhone apps (and its commit's APKs, those `build` made at
  that commit, awaited if it is making them: a build there that is missing,
  cancelled or failed fails the run; only an app whose APK build was
  skipped as already built takes F-Droid's), and publishes nothing (no `screens` branch, no
  cache): its artifacts are what it took. A `workflow_run` always runs
  main's file, so a change to it is tried this way before it is merged.

## The larger screens

Each is a "phone", a directory beside `android/`, `ios/` and `desktop/`
under `apps/<app>/`, laid out, published and compared as they are. The same
build, the same driver, the same shots.json, light and dark.

| Directory | Device | Taken |
| --- | --- | --- |
| `android-tablet/` | Pixel Tablet (`pixel_tablet`), landscape | every Android app |
| `fold-unfolded/` | Pixel Fold (`pixel_fold`, else the emulator's `7.6in Foldable`, its inner screen), open | every Android app |
| `fold-folded/` | the same Fold, folded | every Android app |
| `ipad-portrait/` | iPad Pro 13-inch (else iPad Air 13-inch) | an app whose build takes the iPad |
| `ipad-landscape/` | the same iPad, on its side | the same |
| `mac/` | the runner's own screen | an app whose commit has `<app>-macos` |

- **Android's tablet and Fold**: each app is three Android jobs at once
  (both themes in each: a walk switches them itself, and the driver takes
  them one after the other), all from the same APK: the phone's (the Pixel 7, as before),
  the tablet's and the Fold's. The tablet's boots a Pixel Tablet, held in
  landscape (rotation locked, its natural way up), and takes every shot;
  the Fold's boots the Fold (the Pixel Fold's
  profile where the runner's avdmanager lists it; GitHub's has had only
  the emulator's `7.6in Foldable`, and device.json says which), opened
  (`adb emu unfold`), every shot taken, then folded and every shot taken
  again (a walk from a new install in each; an app driven from outside
  keeps its login). Folded is told three ways, until
  the screen changes size: the emulator's hinge (`adb emu fold`), Android's
  device state (`cmd device_state`, CLOSED, which apps read the posture
  from), and, as GitHub's headless `7.6in Foldable` takes both and still
  draws its one inner screen whole, the Pixel Fold's outer screen's size
  and density given to Android (`wm size 1080x2092`, `wm density 420`).
  The `pixel_fold` profile has two displays, and folded it turns the
  outer one on while `screencap` still draws the first: then the display
  whose picture changed size is the one photographed (SCREENCAP_DISPLAY,
  apps-android.sh), every shot of the folded Fold taken from it.
  So the folded shots are the app laid out for the outer screen, in the
  folded posture; the launcher's taskbar, there since the inner screen,
  can stay at the foot. A Fold whose screen does not change at all is
  said, and its folded shots are not taken. device.json is from what was
  drawn: a tablet's is wider than tall.
- **The iPad**: in the iPhone's job, on the same Mac. Once the apps are
  built, `ios-family.sh` reads each one's `UIDeviceFamily`; when none
  takes the iPad (main's apps today are the iPhone's alone), nothing is
  booted and nothing more is done. Else, once the iPhone's shots are
  taken, the iPhone is shut down and an iPad booted (not under the
  iPhone's shots: in dry run 43052816932 the iPad booted and the Mac
  windows ran beside them on the runner's 7 GB, swap grew past 1.2 GB, the
  iPad took 303 s to boot instead of about one minute, and the iPhone's
  launches timed out), and each app that takes it is taken upright and then on its side
  (the driver's `XCUIDevice` orientation, `APP_ORIENTATION`; a screenshot
  XCUITest gives in the screen's portrait frame is turned a quarter
  counter-clockwise), signed in once. An app whose iPad shots fail keeps
  no hash (`<app>/ios/inputs`), so it is taken whole again.
- **The Mac**: in the same job, after the iPad (beside the iPhone's
  shots, in 43052816932, its windows took 1162 s instead of 218 s and
  came out at the wrong sizes). The commit's Mac packages
  are listed by name (nothing evaluated); an app with `<app>-macos` is
  built by `nix-xcode.sh` as the iPhone's are, opened with `--server
  <URL> --user <name>` (what the desktop window is given; an app that takes
  them opens signed in to the same sample-data server), and its window
  taken with `screencapture -l` in light and dark (System Events' dark
  mode) as one shot, `window`; then each of shots.json's `screens` that is
  `on` "mac", the app opened afresh with `launch.mac` and its arguments. No app has a Mac build yet, so every app is
  passed over in the seconds the listing takes; this is plumbing, not yet
  seen running an app. Steps are not driven yet: a Mac window needs a
  macOS UI-test driver of its own, which is
  the next thing to add when there is a Mac build to drive.
- **What a run costs.** Linux: a tablet's and a Fold's job per app and
  theme, beside the phone's, each its own nix, server and emulator, at
  once with the phone's. Together, one job after the other, they were 5
  (revelation), 12 (money) and 17 to 18 (harken) minutes; apart, the
  longest is the Fold's (two postures). The
  Mac: for apps that take neither the iPad nor the Mac, about five
  seconds (the iPad's check, the listing of the Mac packages); for one
  that takes the iPad, the iPad's boot (about a minute) and its shots
  twice over (money's were eight minutes, both ways up, light and dark).

## The desktop

The desktop windows are iced apps (`harken/iced`, `money/iced`, over
`ui/rust/arkui`), so there is no emulator to run them in and no accessibility
tree to drive them by; what there is, is iced's own headless runtime.
`ui/rust/shots` is that, for any app: the app's `main` hands it the same
`iced::application(...)` it would run, when `ARK_SHOTS` names its
shots.json, and otherwise runs its window as ever. Nothing the window
draws is different, and the hook only captures.

- **The app as iced runs it.** Its boot, its updates, the tasks they return
  and its subscriptions on the app's own executor; the window's events
  through its widgets, then to its subscriptions (so the vim keys and
  Ctrl+K work), its size said as a window opening says it, a redraw when a
  widget asks for one. It is iced_test's `Emulator` taken further: that one
  forgets the widgets' state once it has drawn and tells no subscription of
  an event, so it could neither take light and dark of one screen nor press
  a key the app listens for.
- **Signing in, with no browser.** The window is given `--server` and
  `--user` as a person's would be (the server's `user` in shots.json). With
  nobody remembered it signs in as the desktop does: ark-auth's loopback
  listener, and the server's `/auth/login`, which with dev auth and
  `ARK_DEV_USER` set answers at once with a redirect to that loopback
  carrying the code; `ark_auth::client::login` takes the code from the
  redirect itself, so no browser is opened. The login is remembered in the
  run's own home. Every shot is taken in that one window, in order, each
  shot's steps from where the last left it; a `fresh` one closes it and
  opens another, signed in already, its replica synced from the server.
- **Drawn the same everywhere.** No window and no GPU: wgpu, the renderer a
  person's window draws with, on Mesa's lavapipe (Vulkan on the CPU), and
  DejaVu for the window's default sans-serif in place of the runner's
  fonts, both from the flake's nixpkgs (`app-desktop.sh` sets
  `VK_ICD_FILENAMES` and `ARK_SHOTS_FONTS`). Two runs are the same to the
  pixel. Not tiny-skia, iced's CPU fallback: its 0.14 draws an image whole
  where a widget clips it, so every album cover spills out of its card. A
  shot is taken once nothing has happened for two seconds, at 1440×900 at
  twice the pixels, light and then dark from the same screen.

To take one app's by hand: start its server (`app-server.sh`), build its
window (`nix build .#harken-iced`), and run `app-desktop.sh <out> result
harken http://127.0.0.1:8787` from the repository's root; `ARK_SHOTS_ONLY=<id>`
takes one shot.

## The browser

`app-web.sh <out> <site> <app>` photographs an app's web client (`nix
build .#money-web`: its index.html beside pkg/) in Playwright's Chromium
(`web-shots.js`): the page served on the loopback, DejaVu the only font
fontconfig knows (the desktop's), every one of shots.json's `screens` on
the sizes it is `on` (its launch arguments as the page's address:
`-screen budget -inspector 1` is `?screen=budget&inspector=1`), then the
browser's own `web.shots`, each opened by its address — the launch arguments every shell reads (`screen`,
`inspector`, `pane`) and arkui-web's own (`context`, `menu`, `popover`,
`command`, `keys`) — then its steps in the desktop's vocabulary (`click`,
`key`, `type`, `await`, `wait`), at each of `web.sizes` (`mac`,
`ipad-landscape`, `ipad-portrait`, `ios`) in light and dark, reduced motion so nothing is
caught moving. It writes `<out>/<size>/<theme>/<id>.png` and each its
`<id>.tree.txt`. Every shot's address starts with the app's `web.query`
(money's `demo`: the household in the page, no server to sign in to).

`ARK_WEB_MODE=ssg` takes a static site instead (an app whose app.toml's
`[web] modes` has `ssg`; money's and harken's are the SPA alone), each
screen by its route's address (ui/docs/app.md, "The Browser's Modes"),
the browser's own `web.shots` left out; `ARK_WEB_JS=0` takes it with
script off.

Not yet a job of app-screens.yaml: a job would build `.#money-web`
(cached), `npx playwright install --with-deps chromium`, and run
`app-web.sh screens/web money` from the repository's root, then publish
or compare `screens/web` as the desktop's are.

## The iPhone builds, in nix

`nix build .#money-ios-simulator` (and `harken-ios-simulator`) is the app
in Release for the simulator, the build TestFlight is sent. Xcode is pinned
in `flake.nix` (`xcode`) by its version and build number, 26.3 (17C529),
and used where GitHub's macos-15 image installs it,
`/Applications/Xcode_26.3.app`. It is never put in the store: Apple's
licence keeps it off any cache, and a store path would be pushed to and
fetched from Cachix (the jobs' push filter drops anything named for Xcode
too). The apps themselves are cached. A different Xcode is a different pin
and so a different build. When GitHub's image moves to another Xcode,
`xcode-pinned.sh` (and every build, from the Xcode's `version.plist`) says
so and nothing is built: the pin's `version` and `build` are moved to the
new one's (`xcodebuild -version`).

The kernel is built under nix's strict sandbox, from the pinned Xcode's
toolchain alone: its directory is the one path outside the store the
sandbox lets in (`__impureHostDeps`, allowed by the runner's nix.conf,
`allowed-impure-host-deps`). The app's own `xcodebuild` cannot be: Xcode's file
watcher asks DiskArbitration and other system services about what it
watches, which nix's macOS sandbox lets no build reach (its list of
services is nix's own; sandbox paths only add paths; a derivation's
`__sandboxProfile` is not applied to a sandboxed build), and it falls
when refused. So that one step is `__noChroot`, and the macOS runner's nix
is `sandbox = relaxed`, where every other build is still sandboxed. How
that was found, step by step on GitHub's runners, is the branch
`claude/sleepy-faraday-lcbow5-xcode` (its `xcode-strict.yaml`).

The scripts are always main's: a workflow_run workflow runs with this
repository's token, so it never runs a PR's code. It only reads the PR
run's artifacts.

money's sample household is dated today, so a screen showing dates can
differ from main's for that reason alone.
