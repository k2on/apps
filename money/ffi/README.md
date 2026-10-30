# money's library for its phones

The Rust kernel with money's domain linked in (`docs/plan-v5.md` §1.16,
N3): `ark-ffi` through UniFFI, one shared library the iOS and Android apps
load — `MoneyFFI.xcframework` on the phone, `libmoney_ffi.so` on Android —
and the same code the demo window, the browser and the server run. The
kernel keeps the replica, dials the server, keeps the views and says what
became of every change; the apps' code is their screens and their model.

## What the apps call

Three ways in, each an `ark_ffi::Peer`:

- **`moneyOpen(dir, login)`** — the household's replica on the phone's
  disk, one directory per server, as whoever last signed in there or as
  `login`. Then `peer.connect(socketUrl(server))`: it dials whenever
  signed in, and redials by itself.
- **`moneyDemo(today)`** — the demo's household (`money_domain::seed`, the
  demo window's to the cent) in memory, its own authority, signed in as
  Alex with the importer's role: what the apps run with no server, their
  previews draw, and the screenshots are taken of. A fixed `today`
  (`"2026-10-26"`) is the same household every time; none is today.
  **`DemoBank().sync(peer, today)`** stands in for Plaid behind Sync, as
  the window's does: a pending coffee and a fill-up, then the coffee posted
  with its tip and an order, then nothing new. Its `note` is what the
  demo says of it.
- **`Hall()`** — a money server in the process, over memory:
  `hall.device(dir, user, session, roles)` is a phone dialling it,
  `hall.carry()` delivers everything said. What the apps' tests sync two
  phones through (`roles: ["bank"]` is the importer).

Over a peer, the domain is called through the typed Swift and Kotlin
`arkdb gen` writes (`Money(peer:)` / `Money(peer)`), never edited:

```
money/ios/Money/Gen/Money.swift
money/android/app/src/main/kotlin/dev/money/android/gen/Money.kt
```

`ARK_WRITE_GEN=1 cargo test -p money-ffi --test gen` writes them, and the
same test fails when they are not what the domain generates. Every
record a function takes is a struct or data class there, every function
a method:

```swift
let m = Money(peer: peer)
let lines = try m.month(Month(month: "2026-10"))       // LiveView<BudgetLine>
try m.assign(Assign(month: "2026-10", categoryId: id, amount: 25_000))   // its intent's id
```

**Views are typed and kept.** The views a budget is drawn from hand over
UniFFI's own records (`uniffi.money_domain` on Android; the same module
on iOS), no JSON per row: `accounts` (`AccountLine`), `groups`
(`GroupLine`), `month` (`BudgetLine`), `summary` (`Summary`, one row),
`spending` (`SpendingLine`), `register` and `inbox` (`TxnLine`),
`payees` (`PayeeLine`), `banks` (`BankLine`), `linked` (`LinkedLine`, each of a
bank's accounts and the one it feeds, what `link` and `ignore` act on). A `LiveView<T>` is read
once (`rows()`), then asked what it did (`take()`: unchanged, its patches,
or a reset with every row) and spliced into the list the screen holds.
`peer.setListener` says when anything moved, on the kernel's thread; the
app takes its views then and publishes — an `@Observable` model on iOS,
a `StateFlow` on Android. Nothing runs on a clock.

**Refusals are values.** A form asks first: `peer.check(name, json)` is
the domain's own sentence for the first refinement the input fails ("an
account needs a name"), or none. `moneySend(peer, name, json)` sends a
mutation and answers `Sent.intent(id)` or `Sent.refused(why)` rather than
throwing (the generated methods throw `Refusal`, whose `why` is the same
sentence). After it is sent, `peer.standing(id)` is pending, confirmed or
rejected with why; `peer.takeRejections()` is what the server refused since
the last ask; `peer.status()` is the link, what is pending, and `denied`
when the server turned the login away.

Also: `moneyDomain()` (`"<schema hash> <version>"`, what a server's
`/healthz` says on its `domain` line), `moneyToday()`, `moneyAddMonths`,
`moneyMonthName` ("October 2026").

## Signing in

As harken's phones do, through the bridge's own functions — nothing of it
is money's but the scheme:

1. Ask `<server>/healthz`; it is a money server when its `domain <hash>
   <version> current` line has `moneyDomain()`'s hash.
2. Open `loginUrl(server, "money://auth")` — `<server>/auth/login?redirect=…`
   — in `ASWebAuthenticationSession` (callback scheme `money`) or a Custom
   Tab (the manifest taking `money://auth`). The server signs in with its
   OpenID provider and sends the code to `money://auth?code=…`.
3. `exchange(server, code)` posts it to `<server>/auth/exchange` and
   answers the `Login` (token, session, who, roles, picture).
4. `moneyOpen(dir, login)` — or `peer.login(login)` on a replica already
   open — and `peer.connect(socketUrl(server))`, which is `ws(s)://…/sync`
   with the token. `peer.signedIn()` is who it is once the session row has
   synced; `peer.logout()` revokes it and keeps what is pending.

A server must allow `money://` as a redirect, as harken's allows
`harken://`.

## Building

```
nix build .#money-ios-xcframework       # on a Mac with Xcode: MoneyFFI.xcframework, swift/
nix build .#money-android-lib           # jniLibs/<abi>/, kotlin/, host/
cargo test -p money-ffi                 # rust/
```

`money-android-lib` is harken's derivation for money's crate:
`arm64-v8a`, `armeabi-v7a` and `x86_64` against API 26 with the NDK
nixpkgs packages (an x86_64 or macOS builder), UniFFI's Kotlin and the
typed half (`dev.arkdb.ffi`) under `kotlin/`, and the library for the
build machine under `host/` for the JVM tests. `money-ios-xcframework`
needs Apple's SDK and evaluates elsewhere. `checks.ffi` runs this crate's
tests and has UniFFI write its Kotlin and Swift.

## Not verified

Neither framework has been built (no Mac; no x86_64 builder for the NDK
here), and no phone has loaded either. The sign-in is the bridge's and
harken's, untried against money's server.
