# money for iOS

SwiftUI over money's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with money's domain linked in, as `MoneyFFI.xcframework` — the same
code the server, the desktop window and the browser run. The kernel keeps
the replica, dials the server, keeps the views and says what became of
every change; the app's code is its screens and its model. It calls the
domain by name through the Swift `arkdb gen swift` writes
(`Money/Gen/Money.swift`, never edited: `ARK_WRITE_GEN=1 cargo test -p
money-ffi --test gen` writes it) over UniFFI's Swift and the bridge's typed
half (`rust/ark-ffi/swift/Typed.swift`).

The desktop window (`money/iced`) is the spec: every page it has is here,
drawn the way Apple's own apps draw theirs rather than ported. harken's iOS
app (`harken/ios`) is the template: its session, its model, its first run
and its checks, said again for a budget.

**Built by CI, never run on a device.** It was written on Linux with no
Mac; `.github/workflows/money-ios.yaml` is where it is compiled, its tests
run on a simulator, and it is archived.

## The app

Four tabs in the system's Liquid Glass bar, which shrinks as a list scrolls:

- **Budget** — the month paged under the large title (ArkUI's `Pager`, with
  Today when it is elsewhere), and **Ready to Assign** as the screen's one
  large figure on glass, the way Apple Card's balance is: green while there
  is money to give a job, red when more was assigned than there is; tapping
  it assigns it. Under it, pills for what is overspent and underfunded (the
  second assigns every target at a tap). Then the groups, inset-grouped and
  collapsible, each category a row with its available amount in a pill of
  its tone (funded, short of its target, overspent, nothing) and, with a
  target, a bar of how far the month is toward it. Swipe right to assign,
  left to move money (Cover, in red, when it is overspent); hold for
  everything else — assign to meet the target, set the target, hide,
  delete. The ••• menu has Auto-Assign's three, a new category or group,
  and showing hidden categories.
- **A category** (pushed from its row) is the desktop's inspector as a page:
  Available large in its tone with Cover Overspending or Move Money under
  it, how it got there (left over, assigned — tap to change — and
  activity), its target as a capacity ring with the button that meets it,
  Auto-Assign's three amounts as one-tap choices, the month's transactions
  in it, and rename, hide and delete last.
- **Accounts** — net worth large, All Accounts, then the accounts on budget,
  tracking only and closed, each with its kind's mark (a tinted rounded
  square, as Settings draws one), its bank and last digits when Plaid
  feeds it, a count of what waits for a look, and its balance. Holding one
  renames, reconciles, closes or reopens it. **Bank Connections** is a page
  of the banks, when each last synced and what that brought, the accounts
  each feeds, and Sync Now. **Add Account** is a sheet: name, type (which
  sets on budget or tracking as the desktop does), the balance on the
  keypad, and the toggle with a sentence on what it means.
- **A register** — the working balance large with cleared and uncleared
  under it, and what the bank last said; then the transactions a day at a
  time (Today, Yesterday, then the weekday), each with its payee, its
  category (orange when it needs one), its memo, its amount (an inflow
  green and signed), the cleared mark, and a dot while it waits for a look.
  Swipe right to clear or approve, left to delete; tap to edit; hold for
  categorize, approve, clear, delete. Searchable. Its ••• menu reconciles
  (a sheet that asks "Is your cleared balance $…?" — yes locks it, no takes
  the bank's figure on the keypad and enters the adjustment), approves all,
  and closes the account.
- **Transactions** — a segmented Review / All. Review is the inbox: swipe
  right to approve (a full swipe, a success tap), left to categorize, and
  one that needs a category asks for one and is approved with it; Approve
  All takes what already has one. Nothing waiting is "All Caught Up", the
  system's empty state. All is every account's register, searchable.
- **Insights** — the desktop's Reflect in Swift Charts: the month's spending
  as a donut of its groups that a finger selects (the middle then names the
  group and its share), its legend of amounts, the month's income, spent
  and left over, and each group's assigned against spent as paired bars.

Every sheet has the detents its work needs. A transaction is a form the way
Wallet's and Reminders' are: Outflow / Inflow, the amount on the keypad as
Apple Cash takes one, the payee with the household's payees offered as it
is typed (each bringing its last category), the category from a searchable
list of the budget with each one's available amount (Ready to Assign for an
inflow, the other accounts for a transfer), the account, the date, cleared,
the memo, "Entered by sam" when somebody else did. Assigning and setting a
target are the keypad with suggestions (target, last month, average spent);
moving money is the amount first and then where. Everything moves on
ArkUI's springs; haptics mark a month paged, an approval, a save, a
selection on the chart. Amounts are the locale's currency format, every
row has an accessibility label and value, and Dynamic Type and dark mode
are the system's.

**Pull to refresh** on any list asks the bank for anything new: on a server,
`banks.sync` (any household member may ask the importer to sync now) with
the link dialled; in the demo, the demo's bank (`DemoBank`), whose first
two syncs bring a pending coffee and a fill-up and then post the coffee.
What it brought is said on glass at the top.

**The first run** has the household's server already typed
(`money.koon.us`) and **Try the Demo** under it: the demo's household
(`moneyDemo`), on the phone alone, which needs neither a server nor a
sign-in. A server is found by its `/healthz` (`domain <hash> …` naming
money's), and the sign-in goes by itself, at most once a launch, in the
system's browser sheet (`ASWebAuthenticationSession`) to
`/auth/login?redirect=money://auth`, the code exchanged at
`/auth/exchange`, and the replica opened and dialled at `/sync`. Settings
signs out, or leaves the demo.

## Building

```
nix build .#money-ios-xcframework -o result-ios      # on a Mac with Xcode
rm -rf ../ffi/ios && cp -RL result-ios ../ffi/ios     # MoneyFFI.xcframework, swift/
brew install xcodegen                                  # once
cd money/ios
xcodegen generate
xcodebuild test -scheme Money -destination 'platform=iOS Simulator,name=iPhone 17'
xcodebuild archive -scheme Money -configuration Release -destination 'generic/platform=iOS'
```

`project.yml` is the project: the app (iOS 26.0, Swift 5 language mode for
harken's reasons, the `money` URL scheme, `ITSAppUsesNonExemptEncryption:
false`, the icon in `Money/Assets.xcassets`, version `1.0 (1)` which CI
overrides, automatic signing, bundle id `us.koon.money`) over
`../ffi/ios/MoneyFFI.xcframework` and `../ffi/ios/swift`, and the
`MoneyTests` unit tests, in the one scheme `Money`. `Money.xcodeproj` and
`Money/Info.plist` are outputs; regenerate rather than edit. The icon is
`tools/icon.svg` (three bars, a month's plan, white on the accent's green),
made opaque at 1024 by `tools/icon.sh` with Inkscape.

CI (`.github/workflows/money-ios.yaml`) does the same on `macos-15` for
every push touching `money/ios`, `money/ffi`, `money/domain`, `rust/` or
`swift/ArkUI`, and on demand: the kernel for the phone, the simulator and
the Mac, the bindings, the xcframework, the project, the archive, then the
tests on the newest iOS 26 iPhone simulator. The upload to TestFlight runs
only on `main` and only when the repository variable `MONEY_IOS_UPLOAD` is
`true`: set it once the App Store Connect record for `us.koon.money`
exists.

## How the pieces meet

Foundation only — type-checked on Linux, tested there and on the simulator:

- **`Bridge`** — the session: the peer opened over the replica
  (`Application Support/money/server-…/replica`, one per server) and
  dialled, or the demo's household in memory; drained on one queue
  whenever it says anything moved (`Listener`), its views spliced
  (accounts, groups, the month's lines, summary and spending, the inbox,
  payees, banks; a register while a screen holds it), a rejection or a
  denial said once, and a `Snapshot` sent on. The month's three views are
  opened again when the month changes. Every write is a domain function;
  a refusal is said in the domain's words.
- **`Model`** — the main actor's: the first run's machine run, the session
  it opens, the month on screen, and every button.
- **`Onboarding`** — the first run as a machine with no network, browser or
  clock, the demo one of its steps.
- **`Budget`** — the arithmetic the screens draw: the month under its
  groups, a figure's tone, which way Move Money goes (from the category
  chosen into an overspent one, out of any other), the accounts' sections
  and net worth, the transaction form (`Draft`) and its inputs, payees
  offered, registers a day at a time, and days as the domain writes them
  (noon of the day in the phone's calendar, for a picker and a heading).

Apple's — compiled by CI: `MoneyApp`, `Shell` (tabs, routes, sheets),
`BudgetView`, `CategoryView`, `AccountsView` (and Bank Connections),
`RegisterView`, `TransactionsView`, `InsightsView`, `Sheets`,
`SettingsView`, `OnboardingViews`, `Theme`.

**From ArkUI** (`swift/ArkUI`, any Ark app's, harken's use of it unchanged):
in ArkUICore, `Cents` (amounts as typed, the keypad's digits) and the
first run's server half — `Address`, `Probe`, `Health`, `Shelf` — which
harken's onboarding still keeps its own copy of; in ArkUI, `CentsText`,
`Pill` and `Tint`, `HeroFigure`, `AmountEntry`, `Pager`, `WelcomePage` and
`.toast`.

## Checks

`checks.money-ios-typecheck` (`tools/typecheck.sh`, harken's for money):
every file under `Money/` whose imports are Foundation and Combine only,
with the generated types, against UniFFI's Swift for money's library, its
headers, ArkUICore and a stand-in for Combine; then MoneyTests type-check
against a stand-in for XCTest's assertions, and, handed the library built
for this machine, run:

```
cargo build -p money-ffi --lib          # rust/
MONEY_FFI_LIB=$PWD/rust/target/debug/libmoney_ffi.so \
  bash money/ios/tools/typecheck.sh <uniffi swift bindings>
```

The tests (`MoneyTests`) hold the first run's machine, the form's inputs,
amounts and days with no session, and over the demo's household for
2026-10-26: the month under its groups, covering Dining Out's overspending
from Groceries, assigning against Ready to Assign, Auto-Assign meeting
every target, the month paged, the accounts shelved, a register held,
spliced with a new transaction and let go, the inbox emptied, the demo
bank's three syncs, and a refusal said once in the domain's words.

## Not built

- **Linking and ignoring a bank's accounts.** `banks.link` and
  `banks.ignore` take a Plaid account id, and no view hands one to a phone:
  the `linked` table is only read inside the domain. A `banks.linked` view
  (each followed or ignored account, its bank, mask and the account it
  feeds) would make Bank Connections' rows into Link… and Ignore. The
  desktop has no such page either.
- Splits, scheduled transactions, credit-card payment categories, goals
  with dates and moving categories by drag: the domain has none of them.

## Not verified

Everything that needs a device or a real server: the screens and their
Liquid Glass as drawn, the motion and haptics, VoiceOver's reading of
them, the sign-in sheet and the `money://auth` redirect against
money.koon.us, pull to refresh against a server's importer, the app icon
in App Store Connect, and the TestFlight upload (gated off until the app
record exists). `checks.money-ios-typecheck` evaluates but has not been
built here (it needs the FFI's check and nixpkgs' Swift, and the disk was
full).
