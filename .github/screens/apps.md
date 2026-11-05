# The apps' screenshots

Each app's screens, taken from the builds that were published (the APK in
the F-Droid repository, the commit sent to TestFlight), signed in to the
app's own server with sample data, and driven from outside as a person
would: no code in any app knows it is being photographed. The same system
the docs' arkui shots use (`screens.yaml`): a real Android emulator and a
real iOS simulator, light and dark, onto the `screens` branch with a
manifest. On a pull request, the same shots are compared with main's and
the ones that changed are shown on the PR.

`.github/workflows/app-screens.yaml` runs it. Nothing there, or in the
scripts here, names an app.

## An app's part

The apps are the flake's one list (`flake.nix`, `publish`: its `apks` and
its `ios`); an app on it with `<app>/screens/shots.json` is photographed
on each phone it is listed for. Every app's id, its Android applicationId
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
  to `<dir>/pid` and its log to `<dir>/server.log`, and returns. money's
  writes the demo household into a fresh data directory first
  (`money-seed`); harken's makes a media folder of tagged files from the
  demo catalogue (`media.py`) for the server's own scanner to read.

And the builds: on Android the artifact `apk-<app>` of `build.yaml`
(already the case for every APK in the flake's `publish`); on the iPhone
the flake's `<app>-ios-simulator`, which nix builds at the commit (see
below).

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
- **`apps-android.sh`**, **`apps-ios.sh`**: each app in turn, so one that
  fails does not stop the others.
- **`published.sh`**: each APK as the F-Droid repository offers it, for a
  run that builds nothing; **`await-apk.sh`**: an APK from a `build` run
  still going, as soon as it is uploaded.
- **`ios-build.sh`**: each iPhone app as nix builds it at the commit:
  the cache's when its inputs have not moved, with the runner's Xcode put
  in the store only when something must be built.
- **`apps-lay.sh`**: lays a run's captures over a tree. A phone taken only
  in part keeps what the tree had.
- **`manifest.py`**: the docs' manifest, written for each app
  (`apps/<app>/manifest.json`).
- **`compare.py`**: a PR's shots against main's (ImageMagick, more than a
  thousandth of the pixels more than a few percent off), as the PR's
  comment.

## When it runs

Each app is a job of its own on each phone, so the apps are taken at once.
Each job starts its app's server first, in the background (its
`server.sh`, nix's build of the server: the cache's, or built while the
rest goes on), under whatever is slow anyway: the app's build on the
iPhone, the system image and the emulator's boot on Android.

- **On main**: Android once `build` has ended and its deploy has
  succeeded, from that run's APKs: an APK is there only when its inputs
  moved and it went to F-Droid. The iPhone as soon as `ios` has started,
  every app built by nix at that commit, an unchanged one the cache's.
  Each phone taken whole replaces what the `screens` branch had at
  `apps/<app>/<phone>/`.
- **On a pull request**: both phones as soon as their run starts. `build`
  builds the APKs whose inputs differ from what main published, and each
  app's job, its emulator booted and its server up meanwhile, takes its
  APK the moment the run uploads it (`await-apk.sh`), before the run's
  tests have ended; an app with no new APK is passed over. The iPhone apps
  are built by nix at the PR's commit, beside `ios`. The shots go to the
  branch `screens-pr-<number>`, and one comment on the PR, kept up to
  date, has a row for each screen that changed or is new on either phone:
  Android's on main and on the PR, then the iPhone's on main and on the
  PR, so both phones' before and after are side by side. `ios` runs on
  pushes to this repository's branches, so a fork's PR gets its Android
  shots only.

- **When only the screenshots' scripts change** (`.github/screens/`, an
  app's `screens/`, the workflow) on main, every app is taken again: each
  APK as the F-Droid repository's index offers it (`published.sh`), and
  each iPhone app as nix builds main, which, as its inputs have not moved,
  is the cache's. Running the workflow by hand does the same.

## The iPhone builds, in nix

`nix build .#money-ios-simulator` (and `harken-ios-simulator`) is the app
in Release for the simulator, the build TestFlight is sent. Xcode is one of
its inputs, pinned in `flake.nix` by its hash: the Xcode.app of GitHub's
macos-15 image, put in the store as it is installed (`nix-store
--add-fixed --recursive sha256 /Applications/Xcode_26.3.app`; nothing is
downloaded). Apple's licence keeps Xcode off any cache, so the job's
Cachix push filters it out; the apps themselves are cached. A different
Xcode is a different hash and so a different build. When GitHub's image
moves to another Xcode, `ios-build.sh` says so and builds nothing: the
pin is moved to the new Xcode's hash (the same `nix-store --add-fixed`
prints its path, `nix hash path` its hash).

The kernel is built under nix's strict sandbox, from the stored Xcode's
toolchain alone. The app's own `xcodebuild` cannot be: Xcode's file
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
