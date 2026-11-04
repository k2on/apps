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

All of it is in `<app>/screens/`:

- **`shots.json`**: the app's package ids, its server, and its shots.

  ```json
  {
    "android": "us.koon.money",
    "ios": "us.koon.money",
    "server": { "port": 8788, "user": "alex" },
    "shots": [
      { "id": "budget", "steps": [{ "tap": "Budget" }] },
      { "id": "insights", "steps": [{ "tap": "Insights" }, { "wait": 2 }] }
    ]
  }
  ```

  `android` / `ios`: the package and bundle id; an app missing one is not
  taken on that phone. `server` (optional): the port its `server.sh`
  listens on, and the name the server's dev sign-in is given. Each shot is
  the app opened afresh, its `steps` done in order, and the screen taken:
  `{"tap": "<label>"}` taps what shows that text (a tab, a button),
  `{"type": "<text>"}` types into what has focus, `{"wait": <seconds>}`
  waits.

- **`server.sh`** (only for an app with a server):
  `server.sh <dir> <listen address>`, run from the repository's root,
  starts the app's server with dev auth and something in it, writes its pid
  to `<dir>/pid` and its log to `<dir>/server.log`, and returns. money's
  writes the demo household into a fresh data directory first
  (`money-seed`); harken's makes a media folder of tagged files from the
  demo catalogue (`media.py`) for the server's own scanner to read.

And the builds, as artifacts of the workflows that publish them:
`apk-<app>` from `build.yaml` (already the case for every APK in the
flake's `publish`) and `simulator-<app>` from `ios.yaml`
(`../ios-simulator.sh <App>` in the app's job).

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
- **`published.sh`**: each app's last published build, for a run that
  builds nothing.
- **`apps-lay.sh`**: lays a run's captures over a tree. A phone taken only
  in part keeps what the tree had.
- **`manifest.py`**: the docs' manifest, written for each app
  (`apps/<app>/manifest.json`).
- **`compare.py`**: a PR's shots against main's (ImageMagick, more than a
  thousandth of the pixels more than a few percent off), as the PR's
  comment.

## When it runs

After `build` or `ios` finishes, from that run's artifacts:

- **On main**: an APK is there only when its inputs moved and it went to
  F-Droid, and only once the deploy has succeeded. A simulator build is
  there only when that app went to TestFlight. Each phone taken whole
  replaces what the `screens` branch had at `apps/<app>/<phone>/`.
- **On a pull request**: `build` builds the APKs whose inputs differ from
  what main published, and `ios` builds the simulator apps for any branch
  push. The shots go to the branch `screens-pr-<number>`, and one comment on
  the PR, kept up to date, shows each changed or new shot beside main's.
  `ios` runs on pushes to this repository's branches, so a fork's PR gets
  its Android shots only.

- **When only the screenshots' scripts change** (`.github/screens/`, an
  app's `screens/`, the workflow) on main, nothing is built. Every app is
  taken again from what was last published (`published.sh`): each APK as
  the F-Droid repository's index offers it, and each iPhone build as main's
  newest `simulator-<app>` artifact. Main makes that artifact only when it
  sends the app to TestFlight, and keeps it 90 days. Running the workflow by
  hand does the same.

The scripts are always main's: a workflow_run workflow runs with this
repository's token, so it never runs a PR's code. It only reads the PR
run's artifacts.

money's sample household is dated today, so a screen showing dates can
differ from main's for that reason alone.
