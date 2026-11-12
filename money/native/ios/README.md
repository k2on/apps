# money for iOS

SwiftUI over money's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with money's domain linked in, as `MoneyFFI.xcframework` — the same
code the server, the desktop window and the browser run. The kernel keeps
the replica, dials the server, keeps the views and says what became of
every change; the app's code is its screens and its model. Every change
is one of the domain's (`MoneyChange`, `session.send(change:)`), every view
UniFFI's records, and what a row says — `line.tone()`, `line.captions(style:)`,
`ask.said(now:)`, the form's `Draft` and its `problem()` — the records'
own methods: nothing crosses as JSON, and no rule is the app's.

The desktop window (`money/iced`) is the spec: every page it has is here,
drawn the way Apple's own apps draw theirs rather than ported. harken's iOS
app (`harken/ios`) is the template: its session, its model, its first run
and its checks, said again for a budget.

**Built by CI, never run on a device.** It was written on Linux with no
Mac; `.github/workflows/ios.yaml` (its `money` job, after the `kernel` job shared with harken) is where it is compiled, its tests
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
  Reorder Categories, and showing hidden categories. **Reorder Categories**
  is the list's Edit mode, as Reminders has its lists and their groups:
  one list, each group a row (a folder) over its categories, every row with
  the system's grabber. A category dragged under another group joins it
  there (`categories.move_category`, in front of the category it lands on);
  a group dragged takes its categories with it (`categories.move_group`).
  What was let go is drawn where it went until the session agrees. Done
  ends it.
- **Credit cards**, budgeted the way YNAB budgets them
  (`money_core::cards`): the cards' group, Credit Card Payments, comes
  first, its header saying what of the cards' debt is not covered; each
  card's payment category wears the card's mark and a bar of how much of
  what the card owes is set aside, "$X short" in orange or "Covered" in
  green, and holding it offers "Assign $X to Cover the Card" (no target,
  rename or delete: it is the card's). A category that spent more on a card
  than it had is amber, "$X on card" — debt, not the cash overspending that
  is red. A payment category's page is the desktop's card inspector:
  Available for Payment large with **Assign $X** at a tap when it falls
  short, what the card owes at the month's end and what of it is not
  covered, how the money got there, and what went on the card this month.
  Edit keeps a payment with the cards and every other category out of
  their group, as the domain does.
- **A category** (pushed from its row) is the desktop's inspector as a page:
  Available large in its tone with Cover Overspending or Move Money under
  it, how it got there (left over, assigned — tap to change — and
  activity), its target as a capacity ring with the button that meets it,
  Auto-Assign's three amounts as one-tap choices, the month's transactions
  in it, and rename, hide and delete last. Spending on a card beyond what
  it had is its own line, in amber.
- **A category assigned by rule** (`money_core::rules`, set on the
  desktop: a share of matching income from a month on) is read-only in a
  month its rule assigns: its row wears the rule's mark (`function`) by
  what it was assigned and has no swipes, holding it says the rule, and
  its page says "Assigned by rule: 10% of income matching ..." where Move
  Money would be, with no meeting the target and no Auto-Assign amounts.
  The keypad shows the amount and no Assign; Move Money never offers it
  as where money comes from or goes. The phone does not edit rules.
- **Accounts** — net worth large, All Accounts, then the accounts on budget,
  tracking only and closed, each with its kind's mark (a tinted rounded
  square, as Settings draws one), its bank and last digits when Plaid
  feeds it, a count of what waits for a look, and its balance. Holding one
  renames, reconciles, closes or reopens it. **Bank Connections** is
  Settings' Banks section on a page of its own, with Sync Now. **Add Account** is a sheet: name, type (which
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
  system's empty state. All is every account's register, searchable: the
  search field is the first thing under the large title (ArkUI's
  `SearchRow`, the system's field as a row, in the title's `TitleSection`
  with To Review / All), not the bar's drawer.
- **Insights** — the desktop's Reflect in Swift Charts: the month's spending
  as a donut of its groups that a finger selects (the middle then names the
  group and its share), its legend of amounts, the month's income, spent
  and left over, and each group's assigned against spent as paired bars.

Every sheet has the detents its work needs. A transaction is a form the way
Wallet's and Reminders' are: Outflow / Inflow, the amount on the keypad as
Apple Cash takes one, the payee — wrapped, never cut — with the household's
payees offered as it is typed (each bringing its last category) and, for
one the bank brought, its **Bank Description** under it when that says
more (`CHIPOTLE 1234 SAN FRANCISCO CA` for Chipotle), whole and copyable;
the category from a searchable list of the budget with each one's
available amount (Ready to Assign for an inflow, the other accounts for a
transfer), the account, the date, cleared, the memo, "Entered by sam" when
somebody else did. Assigning and setting a target are the keypad with suggestions (target, last month, average spent);
moving money is the amount first and then where. Everything moves on
ArkUI's springs; haptics mark a month paged, an approval, a save, a
selection on the chart. Amounts are the locale's currency format, every
row has an accessibility label and value, and Dynamic Type and dark mode
are the system's.

**Receipts** are in a transaction's form, written in Rust
(money/app/src/receipts, generated into the sheets' SwiftUI): one that
paid for it is drawn as the paper is (ArkUI's `Paper`, black monospace on
a white slip in light and dark alike), its photo a tap away (ArkUI's
`Photo`, pinched to zoom), with "Check This" when its reading did not add
up; it can be put right or taken off. With none, it can be scanned
(ark-scanner over ark-camera: the page's edges found live in Rust, taken
when steady, squared up), chosen from the photo library (PHPicker,
ark-scanner's `PickPhoto`), typed, or matched to one of the receipts
nobody has, the nearest in amount and then in days first. A photo is kept
by the bridge on the phone and sent beside the rows when there is a
connection (`docs/plan-files.md`); the server reads it and the section
says it is reading until then. A register row whose transaction has a
receipt, or a photo of one, carries the receipt mark.

**Pull to refresh** on any list asks the bank for anything new: on a server,
`banks.sync` (any household member may ask the importer to sync now) with
the link dialled; in the demo, the demo's bank (money's host's, `money_app::host`), whose first
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

The **account**: the person's face at the end of each tab's large title,
on its own line (ArkUI's `LargeTitleRow`, the one title row every tab of
both apps has, and `AccountFace`) — beside Budget's ••• menu and
Transactions' + for a new transaction, with no other + there: Add Account
is the Accounts list's last row, and Budget's new transaction is its •••
menu's first item — marked when a bank wants somebody, raises the account
sheet (ArkUI's `AccountSheet`): who and where, the banks that want a look,
Settings, and Sign Out or Leave the Demo.

**Settings** (from the account sheet) has
the **Banks** (`money_core::banks`, money/README.md's banks): each
connected bank with how it is doing and when it last synced, and
**Connect a Bank**, which asks the server (`banks.connect`) and opens
Plaid's page in Safari's view (ArkUI's `SafariView`,
`SFSafariViewController`) as soon as the server answers the ask with it.
The page does not come back to the app — the server finishes the
connection by asking Plaid — so the app only watches the ask
(`banks.asks`): its row says asking, the page and how long it stays open
(Open Plaid's Page again), done, failed with the server's sentence, or
expired, and the page closes itself once the ask is over.
`ASWebAuthenticationSession` is the sign-in's, not this: it waits for a
redirect that never comes. A bank's page has Reconnect (`banks.reconnect`,
prominent when its login ran out), Disconnect behind a confirmation
(`banks.disconnect`), and each of its accounts with what it feeds — or,
while it waits, a menu: a new account of its kind (`banks.open`), an
existing account (`banks.link`), or Don't Import It (`banks.ignore`). The
demo has no server: its bank is shown, and connecting says so.

A transaction already there is made a **transfer** from its form's
category list (Transfer To, `EditTransaction.transfer_to`): the domain pairs
it with the matching transaction in the other account or adds its other
half. A transfer's form knows its account from its payee, and offers only
accounts. A card's payment category is never offered as a transaction's.

## Building

The project is generated, never committed (ui/docs/app.md, "Generated,
Not Committed"): from money/app.toml, the modules' module.toml and the
Rust views, which this directory's Swift is compiled beside.

```
nix run .#money-prebuild          # on a Mac with Xcode: money/.native/{ios,android}, the kernel linked in
cd money/.native/ios
xcodegen generate                 # nix build .#xcodegen, or Homebrew's
xcodebuild test -scheme Money -destination 'platform=iOS Simulator,name=iPhone 17'
xcodebuild archive -scheme Money -configuration Release -destination 'generic/platform=iOS'
```

The generated `project.yml` is the project: one multiplatform target for
the iPhone, the iPad and the Mac (iOS and macOS 26.0, Swift 5 language
mode for harken's reasons, the `money` URL scheme, the app.toml's
Info.plist keys and its modules', the asset catalog with the accent from
the theme and the icon rendered from money/icon.svg, automatic signing,
bundle id `us.koon.money`) over `../ffi/ios/MoneyFFI.xcframework` (the
generated `money-ffi` shim over money/app) and `../ffi/ios/swift`, this
directory's `Money/` beside the generated views (`Generated/`), and the
`MoneyTests` unit tests, in the one scheme `Money`.

CI (`.github/workflows/ios.yaml`) does the same on `macos-15` for
every push touching `money/native/ios`, `money/app.toml`, `money/app`,
`money/core`, `rust/`, `ui/prebuild.py` or `ui/swift`, and on demand: the kernel for the phone, the simulator and
the Mac, the bindings, the xcframework, the project, the archive, then the
tests on the newest iOS 26 iPhone simulator. The upload to TestFlight runs
only on `main` and only when the repository variable `MONEY_IOS_UPLOAD` is
`true`: set it once the App Store Connect record for `us.koon.money`
exists.

## How the pieces meet

The first run, the session and the model are Rust's: the generated
`@main` (ui/prebuild.py, from money/app.toml) holds money's host
(`MoneyHost`, its generated library's over money_app::host, under ark-ffi's `ArkHost`) through
the app library every Ark app shares (ui/swift's `ArkApp`: `ArkApp`,
`ArkRoot`, `ArkScreen`) — the first run every app shares, the sign-in in
`ASWebAuthenticationSession`'s shared session, what is remembered (the
login in the keychain, the server and the demo in Application Support),
the session's link watched, and every screen's runtime over the session,
what one asks of another carried out there. Its tests are Rust's
(money/app/tests/phone.rs).

Foundation only — type-checked on Linux:

- **`Budget`** — the rows as SwiftUI lists them, and money written in the
  phone's locale (`Numbers`); every rule is the core's.

Apple's — compiled by CI: `MoneyApp` (money's scenes and `MoneyScreens`,
the holders of the host's runtimes), `Shell` (the shell's host: routes,
the generated screens hosted, a register's own runtime per register shown,
the generated sheets over it), `InsightsView`, the receipts' views,
`Theme`. Settings (its General, Household, Connected Banks and Appearance
panes, a bank's page) is the shell's, generated from money/app. Budget, Accounts, Transactions, a register and
the sheets (a transaction's form, Categorize, Reconcile) are generated from
money/app.

**From ArkUI** (`ui/swift`, any Ark app's, harken's use of it unchanged):
in ArkUICore, `Cents` (amounts as typed, the keypad's digits), `Order`
(rows moved as Edit mode hands them over) and the
first run's server half — `Address`, `Probe`, `Health`, `Shelf` — which
harken's onboarding still keeps its own copy of; in ArkUI, `CentsText`,
`MarkedCents`, `Pill` and `Tint`, `HeroFigure`, `AmountEntry`, `Pager`, `WelcomePage`,
`SafariView` (and `.safari`) and `.toast`.

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
  bash money/native/ios/typecheck.sh <uniffi swift bindings>
```

The tests (`MoneyTests`) hold the first run's machine, the form's inputs,
amounts and days with no session, and over the demo's household for
2026-10-26: the month under its groups, covering Dining Out's overspending
from Groceries, assigning against Ready to Assign, Auto-Assign meeting
every target, the month paged, the accounts shelved, a register held,
spliced with a new transaction and let go, the inbox emptied, the demo
bank's three syncs and what it brought opened with its bank description,
a refusal said once in the domain's words, and Edit's drops: a category
dragged under another group joining it, either end of the list, a group
taking its categories past others, a drop that moves nothing asking
nothing, and each drawn ahead as the budget comes back — the cards' group
first and its payment kept there. The cards (`CardTests`): credit
overspending told from cash, the demo's card payment short until one tap
covers it out of Ready to Assign, payments kept out of a transaction's
categories, and a typed payment made a transfer to the card from its
form. The banks (`BankTests`, rows made by hand): an ask followed until
its page arrives and only its own, what asks and banks say, and what a
bank's account may feed.
The rules (`RuleTests`): only a month by hand is editable, Move Money
passes a ruled category by, and the demo with a rule set on Gifts
assigned 10% of Acme's paydays, read-only, its hand assignment refused
in the domain's words, Auto-Assign passing it by, and the month before
the rule still by hand.

## Not built

- Splits, scheduled transactions and goals with dates: the domain has
  none of them.

## Not verified

Everything that needs a device or a real server: the screens and their
Liquid Glass as drawn, the motion and haptics, VoiceOver's reading of
them, the receipt scanner and the photo picker, the sign-in sheet and the `money://auth` redirect against
money.koon.us, pull to refresh against a server's importer, connecting,
reconnecting and disconnecting a bank against a server with Plaid's keys
(Plaid's page in Safari's view, and its closing when the ask is over),
the app icon in App Store Connect, and the TestFlight upload (gated off
until the app record exists). `checks.money-ios-typecheck` evaluates but
has not been built here (it needs the FFI's check and nixpkgs' Swift, and
the disk was full).
