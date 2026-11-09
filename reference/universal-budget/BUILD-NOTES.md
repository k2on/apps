# Build notes

What it took to get the reference compiling and screenshotted on GitHub's
macOS runner (`.github/workflows/reference-shots.yaml`, Xcode 26.3,
macos-15 image). Every change is the smallest one that builds; none changes
the design.

## Build setup (CI only, not the project)

- Built with `ENABLE_DEBUG_DYLIB=NO ENABLE_PREVIEWS=NO` (one plain binary).
- iOS Simulator: `CODE_SIGNING_ALLOWED=NO`.
- macOS: ad hoc (`CODE_SIGN_IDENTITY=-`), `ENABLE_HARDENED_RUNTIME=NO`, so
  the app launches unsigned by a team.
- Screens are opened with SCREENS.md's launch arguments; the iPad's
  landscape shots go through a small UI test (`ci/landscape/`) because
  `simctl` cannot rotate a simulator.

## Compile fixes

1. **`defaultAdaptableTabBarPlacement` is iOS-only.** The iOS build
   succeeded first time; the macOS build failed with
   `'defaultAdaptableTabBarPlacement' is unavailable in macOS` and
   `'sidebar' is unavailable in macOS` (ContentView.swift). Wrapped the one
   modifier in `#if os(iOS)`. The Mac always shows the sidebar anyway, so
   `-columns all` meant nothing there.

## Fixes after the first screenshots

No other compile errors: after fix 1 both platforms built. The first
screenshots (run 42710930719) showed these, each fixed minimally:

2. **"3 to reviews".** The register's review link used
   `^[\(n) to review](inflect: true)`, and automatic grammar agreement
   inflected the word "review" as a plural noun. Now plain
   `"\(n) to review"` (AccountRegisterView.swift).
3. **Mac: `-screen account:checking` showed the Accounts list.** A path set
   on the Accounts tab at launch was not pushed on the Mac (it was on iPhone
   and iPad). On the Mac the account's own sidebar item is selected instead,
   which is the Mac navigation DESIGN.md describes (accounts as sidebar
   items, like mailboxes); also for `reconcile:` (SceneModel.swift,
   `#if os(macOS)`).
4. **Mac: field labels drawn twice.** In grouped forms on the Mac a
   `TextField`'s title is drawn beside it, so "Assigned $150.00 Assigned"
   and "Target Target $3,000.00". The amount fields that sit inside a
   `LabeledContent` now have `.labelsHidden()` (AssignedField, the goal
   target, Add Account's balance, Reconcile's statement balance).

## Capture notes (CI, not the app)

- The macos-15 runner's display starts at 1024x768, which clamped the
  1440x900 window to 1024x681; `ci/display.swift` switches it to the largest
  mode first. The app is launched through `open` so its window is key (run
  from the shell it stayed inactive and grey: sidebar icons and progress
  bars lost their tint).
- iPadOS restored the previous launch's sidebar and inspector (shots without
  `-columns all` / `-inspector 1` showed them), so the app's saved state is
  wiped before each launch and each landscape shot is its own test run.

## Seen in the final shots (run 42715731248) and left as they are

Not fixed, as fixing them would mean redesigning rather than a small change:

- Built with the iOS 26 / macOS 26 SDK, so the system draws Liquid Glass:
  floating glass toolbar capsules and a floating tab bar on iPhone.
- iPhone: the tab bar has a fifth **More** tab. The account tabs in the
  `TabSection`s, though `.defaultVisibility(.hidden, for: .tabBar)`, still
  count towards the tab bar's limit.
- iPhone and iPad: `-screen review` filters the list, but the search field
  with its "Needs Review" token is not on screen (on iPhone the search field
  is not shown at all in Transactions; on iPad it is a toolbar search
  button). On the Mac the token is in the toolbar search field.
- iPhone: an account register (`account:visa`) has no visible title; the
  search field and balances bar sit under the toolbar buttons.
- iPhone: the editor sheet shows no number pad (the simulator has a hardware
  keyboard attached).
- iPad landscape (13-inch): the sidebar shows on every screen, with or
  without `-columns all`, even from a wiped state.
- Spending by Category: the category names sit above the bars (the y-axis
  labels), not beside them.
- Net Worth: the household's net worth is negative (the mortgage), so the
  area under the line fills from the line up to $0, a large tinted block.
- Mac: the "Needs Category" menu in the table is not orange (the menu's
  foreground style is not applied); the Kept tile is taller than Income and
  Spending (it has a detail line), so the three tiles' tops do not align.
- Sample data: Electricity is overspent by $4.08 as well as Dining Out
  (by $21.02, not the $44.10 DESIGN.md gives as an example); the summary
  still reads "2 overspent · 2 underfunded" and Ready to Assign is $412.37
  as designed.
