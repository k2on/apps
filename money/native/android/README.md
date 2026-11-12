# money for Android

Compose over money's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with money's domain linked in, `libmoney_ffi.so`, the same code the
server, the desktop and the browser run. The phone is a **client** of it:
the kernel keeps the household's replica on the phone's disk, dials the
server, rebases, and keeps the views; the app's own code is its screens,
its model and its way in. Every change it makes is one of the domain's
(`MoneyChange`, sent with `session.send`), every view hands over UniFFI's
own records (`uniffi.money_core`), and what a row says — a line's tone,
story and captions, an ask's sentence, the form's rules (`Draft`) — is the
records' own methods: nothing crosses as JSON, and no rule is the app's.
The look it shares with harken is `ui/kotlin` (package `dev.arkui`),
compiled in from its sources.

The installed app is `us.koon.money`; its sign-in comes back on
`money://auth`; its server is `https://money.koon.us` unless somebody types
another; and with no server at all it runs the demo's household
(`moneyDemo`) on the phone alone.

**Assembled, not run.** `nix build .#money-apk` compiles every Kotlin file
here and arkui's and assembles the release APK with the kernel for
`arm64-v8a`, `armeabi-v7a` and `x86_64`. What its model and its way in
do is money's host's, tested in Rust (money/app/tests/phone.rs). No phone has run this APK: the Custom Tab's redirect, JNA's
loading on a device, dynamic colour and the back gesture's preview are the
parts only a phone can say anything about.

## Building

From the repository root, with nothing but nix:

    nix build .#money-apk               # result/money.apk
    adb install result/money.apk

`money-apk` is harken's derivation (`androidApp` in `flake.nix`) over
`money-android-lib`, with `ui/kotlin` added to its source. **The Maven
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

    nix run .#money-prebuild    # money/.native/android, the kernel linked in at .native/ffi/android
    cd money/.native/android
    gradle assembleDebug
    adb install app/build/outputs/apk/debug/app-debug.apk

**One key, committed, and public on purpose.** `debug.keystore` is the key
every build signs with, debug and release, so one APK installs over the
last, for the reasons harken's README gives; its certificate's SHA-256 is
`125e43a7…646a6afa`. It is a PKCS#12 file made with openssl (there was no
JDK to run keytool), which `storeType = "pkcs12"` says. Anyone can sign an
APK as this app with it. F-Droid offers money from the same repository as
harken and Revelation (`nix/fdroid.nix`; its metadata is money/app.toml's `[store]`, written out by `ui/prebuild.py fdroid`,
and its icon money/icon.svg).

## What it does

**The way in, and the model, are Rust's.** The app's `Application`, its
activity and its screens' holders are generated (ui/prebuild.py, from
money/app.toml) over the app library every Ark app shares (ui/kotlin/app,
`dev.arkui.app`: `ArkApplication`, `ArkActivity`, `ArkScreen`), and what
they hold is money's host (`MoneyHost`, its generated library's over money_app::host, under
ark-ffi's `ArkHost`): the first run every app shares — the server's
`/healthz` asked, the server's own page opened in a Custom Tab
(`loginUrl(server, "money://auth")`), the code that comes back exchanged,
the replica opened over the process's one runtime (one directory per
server) and dialled — what is remembered (the login in the secure store,
the server and the demo in the app's files directory), the session's
link watched (a denial, the server's refusals, the first sync), and every
screen's runtime over the session, what one asks of another carried out
there (the month, the bank asked, a category or a transaction a launch
names, the session's facts in Settings). What is left here is what this
app draws itself: `MoneyRoot` and the shell's host (`MoneyApp.kt`: the
sections' screens, a register's own runtime, the pages pushed, the
account sheet), its theme and its glyphs. A refusal, or what the bank
brought, is a snackbar. Nothing runs on a clock.

**The screens**, laid out as Google lays out its own apps: Material 3 in
the wallpaper's colours (dynamic colour on Android 12 and later; money's
own green scheme before that and in every screenshot), the expressive
shapes and type, edge to edge, a navigation bar, and large top app bars
that collapse as a page scrolls. Pulling any page down asks the bank for
anything new: the demo's bank answers at once (money's host's, `money_app::host`), a server's
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
- **Credit cards**, as YNAB budgets them (`domain/src/cards.rs`): the
  domain's **Credit Card Payments** group comes first, a payment category
  per card with the card's glyph, saying what of the card's debt is covered
  ("Covered: $412.00 owed") or short ("$80.00 short of the $412.00 owed",
  amber), and the group's header adds up what is not covered. Spending more
  on a card than a category had is debt, not next month's money: that
  category's pill is amber ("$30.00 on a card, not covered"), where cash
  overspent is red. A payment category's sheet has the card's balance in
  place of a target — owed, set aside, not covered — and **Assign $X to
  cover the debt**; it is not renamed or deleted, the cards' group takes no
  new category, a payment category is dragged only among the cards (arkui's
  Reorder, `fits`), and no transaction is filed under one.
- **Accounts** — net worth large on top, then the budget's accounts and
  the tracked ones, each with its kind, its bank and mask, how many of its
  transactions want a look, and its balance. **Add account** is a sheet:
  a name the domain checks as it is typed, the kind as chips (which sets
  whether it is in the budget, as the domain's rule does), and what is in
  it or owed. The face in the top bar (badged while a bank wants a look)
  opens **Settings**, as do **Banks** under the accounts and the budget's
  menu.
- **Settings** — **Banks**: each connection with how its last import went
  (a login run out, or a failure, in the error's colour, with Reconnect),
  Reconnect and Disconnect (asked first) in its menu, and each of its
  accounts with the one it feeds: tapped, it is pointed at another
  (`banks.link`); its switch follows it or not (`banks.ignore`); one waiting
  for a choice offers a new account of its kind (`banks.open`), an existing
  one, or Ignore. **Connect a bank** authors `banks.connect`; when the
  server answers with Plaid's page (`banks.asks`) it opens in a Custom Tab,
  and a card follows the ask — the server finishes the connection by
  itself — until it is done, failed or expired (`Banks.kt`). **Account**:
  who is signed in, where, whether changes are waiting to be sent, and Sign
  out.
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
  correction says who entered it, can delete it, and can make it a
  transfer (`transfer_to`: paired with the matching one in the other
  account, or given its other half there).
- **Its receipt** — in Rust (money/app/src/receipts, generated into the
  sheets' Compose), at the foot of a correction's sheet, the iPhone's
  section: what paid for it drawn as the paper (ArkUI's `Paper`,
  monospace on white in light and dark), with "Check This" when the
  server's reading of its photo doesn't add up, its photo a tap away
  (ArkUI's `Photo`, pinched to zoom); **Scan Receipt**, **Choose Photo**
  (the system's Photo Picker, ark-scanner's `PickPhoto` over ark-camera),
  **Type Receipt**, and **Match a Receipt** (one nothing pays for yet, the
  nearest in amount first); a receipt is edited or taken off. A photo is
  kept on the phone by the bridge at once (`putFile`, docs/plan-files.md)
  and read by the server when it gets there.
- **The scanner** — the sheets ask the scanner module (modules/ark-scanner)
  for a page. The camera's page and the photo picker are ark-camera's
  (modules/ark-camera/kotlin, `CameraPage` at the app's root: Camera2, no
  library); the eyes — DocAligner's corner model on tract, the bright
  region, the sharpness, held still for a second — and the warp are Rust's,
  in ark-scanner, tested there on drawn frames.

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

## Not verified

No phone has run it, and no emulator. Signing in to `money.koon.us` has
not been tried (the server must allow `money://` as a redirect, as
harken's allows `harken://`), nor has a server's importer answering
`banks.sync` or a connection.
Dynamic colour, the predictive back gesture, the Custom Tabs (the
sign-in's return, Plaid's page) and JNA's loading on a device are the
phone's to say. Splits and scheduled transactions are not in the domain,
so not here either.
