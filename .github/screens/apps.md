# The apps' screenshots

Each app's screens, taken from the builds that were published (the APK in
the F-Droid repository, the commit sent to TestFlight), signed in to the
app's own server with sample data, and driven from outside as a person
would: no code in any app knows it is being photographed. The same system
the docs' arkui shots use (`screens.yaml`): a real Android emulator and a
real iOS simulator, light and dark, onto the `screens` branch with a
manifest. Beside them a third "phone", `desktop`: the app's iced window,
built by nix at the commit and run with no window ([below](#the-desktop)).
On a pull request, the same shots are compared with main's and the ones
that changed are shown on the PR.

`.github/workflows/app-screens.yaml` runs it. Nothing there, or in the
scripts here, names an app.

## An app's part

The apps are the flake's one list (`flake.nix`, `publish`: its `apks`, its
`ios` and its `desktop`); an app on it with `<app>/screens/shots.json` is
photographed on each phone it is listed for, and on the desktop when its
shots.json has `desktop`. Every app's id, its Android applicationId
and its iPhone bundle id, is `bundlePrefix` (`us.koon`) and its name, so
neither driver is told it: Android reads it from the APK, iOS from the app.

All of it is in `<app>/screens/`:

- **`shots.json`**: the app's server, and its shots.

  ```json
  {
    "server": { "port": 8788, "user": "alex" },
    "shots": [
      { "id": "budget", "steps": [{ "tap": "Budget" }] },
      { "id": "insights", "steps": [{ "tap": "Insights" }, { "wait": 2 }] }
    ]
  }
  ```

  `server` (optional): the port its `server.sh`
  listens on, and the name the server's dev sign-in is given. Each shot is
  the app opened afresh, its `steps` done in order, and the screen taken:
  `{"tap": "<label>"}` taps the element whose accessibility label (what it
  says) or identifier is `<label>` (an iOS `accessibilityIdentifier`, an
  Android resource id or exposed Compose `testTag`), and `"role"`
  (`tab`, `button`, `field`, `text`) says which kind when several share a
  label (a tab and a heading both saying "Transactions"); `{"type":
  "<text>"}` types into what has focus; `{"wait": <seconds>}` waits.
  `"where": "bottom"` on a tap takes the lowest on the screen of all that
  match, and `"unless": "<label>"` skips a step when that label is on the
  screen already; `{"await": "<label>"}` waits (up to 20 s) until it is (`a|b`: either).
  `{"relaunch": true}` stops and opens the app again. harken's `player`
  shot: the lowest "Für Elise" (the mini player, if one is there; else
  Recently added's, which plays it), until something plays ("Pause") or
  the player is open ("Close the player"); then the app opened again, so
  that the mini player is there from the start (on Android one that has
  only just come does not open on a tap), the mini player tapped, and the
  shot taken once the player is open.

  **Write steps from the accessibility tree, not by guessing.** Every shot
  is saved with the tree of the screen it was taken from, beside its
  picture: `<id>.tree.txt` on the `screens` branch (`apps/<app>/<phone>/
  <theme>/`) and in the run's artifacts. On Android it is `ui.py --tree`'s
  outline of uiautomator's dump (what each node says, its `#id`, its
  `[role]`, whether it can be pressed); on iOS, XCUITest's own
  description of the app's hierarchy. To add a shot, open the tree of the
  screen it starts from and name what to tap as it is written there. A
  step that finds nothing leaves `stuck.png` and `stuck.tree.txt`: what
  was on the screen instead.

- **`server.sh`** (only for an app with a server):
  `server.sh <dir> <listen address>`, run from the repository's root,
  starts the app's server with dev auth and something in it, writes its pid
  to `<dir>/pid` and its log to `<dir>/server.log`, and returns. Each
  seeds a fresh data directory first with the app's seed, which lives
  beside the app, not in it (`docs/seed.md`): `nix build .#<app>-seed`, then
  `<app>-seed <dir>/data`, then the server from `nix build .#<app>-server`
  on it. money's writes the demo household as the log the server boots on
  (`money-seed`); harken's writes the demo catalogue as a media root of
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
  the phones' taps have them); `{"hover": "<text>"}` moves the pointer over
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
- **Signing in**: every app here signs in through arkui's onboarding, so
  the drivers do it the same way for all of them. They type the server's
  address into "Server address", tap Continue (and "Sign in" if it is
  there), give the server's dev sign-in page the name, and allow a
  permission the app asks for. The login is kept, so each shot after opens
  signed in.
- **`app-android.sh`** (with `ui.py`, which finds a label's place in
  uiautomator's dump): installs the APK, signs in through Chrome, and does
  each shot's steps with `adb input`. The emulator reaches the server on
  the host's loopback as `10.0.2.2`.
- **`app-ios.sh`** and the UI test in **`drive-ios/`**: an XCUITest bundle
  of its own, which drives the installed app by its bundle id (the apps'
  projects are untouched). It signs in through `ASWebAuthenticationSession`
  (the system's "wants to use … to sign in", then the page) and writes each
  shot's PNG.
- **`app-desktop.sh`**: runs the window nix built with `ARK_SHOTS` set,
  in a home of its own, given `--server` and `--user`; it signs in, does
  each shot's steps and writes its PNGs ([the desktop](#the-desktop)).
- **`apps-android.sh`**, **`apps-ios.sh`**, **`apps-desktop.sh`**: each app
  in turn, so one that fails does not stop the others.
- **`ios-inputs.sh`**: the hash of what an iPhone app's shots are taken
  from, said on Linux ([when it runs](#when-it-runs)).
- **`published.sh`**: each APK as the F-Droid repository offers it, for a
  run that builds nothing; **`await-apk.sh`**: an APK from a `build` run
  still going, as soon as it is uploaded.
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
  columns are its list of them (`PHONES`: Android, iPhone, Desktop).

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
from), the packages its `server.sh` builds, its `screens/`, its sign-in's
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
  app's `screens/`, the workflow) on main, every app is taken again: each
  APK as the F-Droid repository's index offers it (`published.sh`), and
  each desktop window as nix builds main, which, as its inputs have not
  moved, is the cache's; each iPhone app whose hash moved (a change to
  Android's scripts moves none). Running the workflow by hand does the
  same, every iPhone app included. Run by hand with a PR's number (`pr`),
  it takes that PR's iPhone shots at its head, of each app whose hash
  moved, and updates its comment: after a Mac's run failed, or to try the
  workflow as a branch has it.

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
  run's own home, so each shot after opens signed in, its replica synced
  from the server.
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
