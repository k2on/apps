# money for Android

Compose over money's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with money's domain linked in, `libmoney_ffi.so`, the same code the
server, the desktop and the browser run. The phone is a **client** of it:
the kernel keeps the household's replica on the phone's disk, dials the
server, rebases, and keeps the views; the app's own code is its screens,
its model and its way in. It calls the domain through the Kotlin `arkdb
gen` writes from the registered domain (`app/src/main/kotlin/dev/money/
android/gen/Money.kt`, never edited: `ARK_WRITE_GEN=1 cargo test -p
money-ffi --test gen` writes it), and every view a budget is drawn from
hands over UniFFI's own records (`uniffi.money_domain`), no JSON per row.
The look it shares with harken is `kotlin/arkui` (package `dev.arkui`),
compiled in from its sources.

The installed app is `us.koon.money`; its sign-in comes back on
`money://auth`; its server is `https://money.koon.us` unless somebody types
another; and with no server at all it runs the demo's household
(`moneyDemo`) on the phone alone.

**Assembled and tested on the JVM, not run.** `nix build .#money-apk`
compiles every Kotlin file here and arkui's, runs the app's JVM tests, and
assembles the release APK with the kernel for `arm64-v8a`, `armeabi-v7a`
and `x86_64`. The tests are the app's own over the **real kernel**: the
library built for the machine they run on (`money-android-lib`'s
`host/`), loaded through the same UniFFI Kotlin and JNA the phone uses —
the demo's household, seeded to a fixed day, driven by the screens' model
as the screens drive it, and the way in walked with a server that answers
as one would. No phone has run this APK: the Custom Tab's redirect, JNA's
loading on a device, dynamic colour and the back gesture's preview are the
parts only a phone can say anything about.

## Building

From the repository root, with nothing but nix:

    nix build .#money-apk               # result/money.apk
    adb install result/money.apk
    nix build .#money-android-screens   # result/index.html and the PNGs

`money-apk` is harken's derivation (`androidApp` in `flake.nix`) over
`money-android-lib`, with `kotlin/arkui` added to its source. **The Maven
graph is harken's**, replayed as it is: `deps.json` is a copy of
`harken/android/deps.json`, and `app/build.gradle.kts` asks for exactly
harken's libraries — the player's included, unused — so that Gradle
settles every shared library at the version that graph fetched. A graph of
money's own is

    $(nix build .#money-apk-deps --print-out-paths)   # needs the network

after which Media3 can leave the build file. `money-android-lib` is the
kernel per ABI, built with the NDK on an x86_64 or macOS builder; on an
ARM laptop the APK is CI's (`.github/workflows/build.yaml`, whose `apk`
matrix has money beside harken and Revelation).

On a laptop, with Android Studio or a command-line SDK with platform 35
and build-tools 35.0.0 plus JDK 21, the kernel built once and gradle
pointed at it:

    nix build .#money-android-lib -o money/ffi/android   # what -PmoneyLib defaults to
    cd money/android
    gradle assembleDebug
    gradle :app:testDebugUnitTest
    adb install app/build/outputs/apk/debug/app-debug.apk

**One key, committed, and public on purpose.** `debug.keystore` is the key
every build signs with, debug and release, so one APK installs over the
last, for the reasons harken's README gives; its certificate's SHA-256 is
`125e43a7…646a6afa`. It is a PKCS#12 file made with openssl (there was no
JDK to run keytool), which `storeType = "pkcs12"` says. Anyone can sign an
APK as this app with it. F-Droid offers money from the same repository as
harken and Revelation (`nix/fdroid.nix`; `fdroid/` here is its metadata
and icon).

## What it does

**The way in** (`Start.kt`). The welcome asks for the server, with
`money.koon.us` already typed, and **Try the demo** under it. Continue asks
the address's `/healthz` whether it runs money's domain (arkui's
`Probe`, `Health` and `Address`, which harken's onboarding uses too); then
**Sign in** opens the server's own page in a Custom Tab
(`loginUrl(server, "money://auth")`), the code that comes back is
exchanged (`exchange`), and the replica opens (`moneyOpen`, one directory
per server) and dials (`socketUrl`). The server, the login and whether
this phone runs the demo are remembered, so a launch opens straight onto
the budget. Signing out and a server's denial come back to Sign in with
the server and the replica kept; leaving the demo comes back to the
welcome. The machine (`Starter`) is plain Kotlin over its effects, so
`StartTests` walks it with no network.

**The model** (`Book.kt`). One thread holds the views — accounts, groups,
the month's lines, its summary and its spending, the inbox, the payees,
the banks, the banks' accounts, and each register a screen holds —
reads each once, and after every change the peer reports asks each what it
did and splices the patches in. Then it publishes one `Ledger`, an
immutable snapshot every screen draws from (a `StateFlow`). Every press is
one of the domain's functions, typed; a refusal, here or from the server
later, is the domain's own sentence as a snackbar ("Couldn't assign it:
money is assigned from Ready to Assign, not to it"); a form asks first
(`peer.check`) and says the sentence under its field. Nothing runs on a
clock.

**The screens**, laid out as Google lays out its own apps: Material 3 in
the wallpaper's colours (dynamic colour on Android 12 and later; money's
own green scheme before that and in every screenshot), the expressive
shapes and type, edge to edge, a navigation bar, and large top app bars
that collapse as a page scrolls. Pulling any page down asks the bank for
anything new: the demo's bank answers at once (`DemoBank`), a server's
importer is asked (`banks.sync`) and its import arrives as any change
does.

- **Budget** — the month as the title, stepped by the arrows beside it;
  Ready to Assign large in a card tinted by what it says (money waiting,
  every dollar given a job, or assigned more than there is), with the
  month's income, assigned and activity under it and a button that funds
  what is underfunded; chips for what needs a look; then each group as a
  card that folds, every category a row with what is happening to it in
  words ("Spent $82.40 of $150.00", "$40.00 more needed this month",
  "Overspent by $12.10"), a meter, and what it has in a pill —
  the primary's container where there is money, the tertiary's where a
  target is not met, the error's where it is overspent. The menu assigns
  the month (underfunded, as last month, or reset), adds a group, and
  shows the hidden categories. A category opens **its sheet**: what it has,
  carried, assigned and spent; what it is assigned, typed or from chips
  (its target, last month's, its average); its target with a meter; and
  **Move money** or **Cover overspending**, a category chosen on the
  sheet's second page. Rename, hide and delete are in its menu.
- **Accounts** — net worth large on top, then the budget's accounts and
  the tracked ones, each with its kind, its bank and mask, how many of its
  transactions want a look, and its balance. **Add account** is a sheet:
  a name the domain checks as it is typed, the kind as chips (which sets
  whether it is in the budget, as the domain's rule does), and what is in
  it or owed. **Bank connections** lists each bank's accounts with the one
  each feeds: tapped, it is pointed at another (`banks.link`); its switch
  follows it or not (`banks.ignore`). The avatar opens who is signed in,
  where, whether changes are waiting to be sent, and Sign out.
- **An account's register** — working, cleared and uncleared balances,
  what the bank says, then the transactions by day under sticky headings.
  The mark at a row's front clears it or not; a lock is reconciled.
  **Reconcile** asks whether the cleared balance is the bank's: yes locks
  it, no asks what the bank says and adjusts.
- **Transactions** — **To review** first: what the bank brought and
  nobody approved, or what has no category. A row swipes to approve (or
  says Approve to TalkBack), its category is a chip that opens the
  chooser, and **Approve all** is in the top bar; nothing to review says
  so. **All** is every account's transactions by day.
- **Insights** — the month's spending by group as a ring with the total in
  its middle and the same groups as bars; what came in, was assigned and
  was spent, and how much of what came in was kept; and the categories
  that spent the most.
- **A transaction** — a full-height sheet from the FAB (every tab but
  Insights) or a row's tap: Money out or Money in, the amount large, the
  payee with the ones paid before as chips (each bringing the category it
  went to last time), the category or a transfer to another account, the
  account, the day (Material's date picker), a memo, and Cleared; a
  correction says who entered it and can delete it.

**Motion.** Between tabs, Material's fade through; into a register or the
bank's page and back, the shared X axis, which the system's back gesture
drives under the finger (Android 14 and later; `enableOnBackInvokedCallback`
in the manifest); sheets shrink as the back gesture takes them (Material's
own). Meters and charts spring to their values; the swipe's glyph grows
and the phone ticks where letting go approves.

**For TalkBack.** Every category row and transaction row is one element
read as a sentence ("Groceries, $82.40 available. Spent $67.60 of
$150.00"); a chart is a sentence of its parts; headings are headings; the
cleared mark says its state; a swipe is also a custom action; amounts are
written, and read, in the phone's locale (arkui's `Amounts`: its symbol,
grouping and decimal mark, in the locale's currency where that counts in
hundredths).

## Tests

    gradle :app:testDebugUnitTest      # what money-apk runs
    gradle :app:recordPaparazziDebug   # the screens too (money-android-screens)

- `BookTests` — the model over the demo's household on 2026-10-26:
  assigning, moving and covering, targets and auto-assign, a refusal in
  the domain's words, the inbox approved and filed, the demo bank's two
  syncs and then nothing, a transaction added, corrected, cleared and
  deleted, a transfer, reconciling (and an adjustment), an account opened
  after its name is checked, a bank's account followed and not, the month
  stepped and back, categories added, hidden and deleted.
- `StartTests` — the way in: the welcome, the demo remembered and left, an
  address that is not a money server, the sign-in's tab and its code, a
  code that does not exchange, signing out; and days written in the
  locale.
- `AmountsTests` — arkui's amounts in the US and in Germany, typed and
  written.
- `Screens` — every page, light and dark, from the demo (Paparazzi). Only
  `recordPaparazziDebug` runs it, so a page layoutlib cannot draw holds
  back the screenshots and not the APK.

## Not verified

No phone has run it, and no emulator: everything above is the JVM's.
Signing in to `money.koon.us` has not been tried (the server must allow
`money://` as a redirect, as harken's allows `harken://`), nor has a
server's importer answering `banks.sync`. Dynamic colour, the predictive
back gesture, the Custom Tab's return and JNA's loading on a device are
the phone's to say. Splits, scheduled transactions and YNAB's
credit-card payment categories are not in the domain, so not here either.
