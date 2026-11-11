# money's library for its phones

> **This crate is being removed.** It is here only while some of money's
> screens are still native (Receipts, Settings, and the phones'
> `Model.swift` / `Book.kt`). The end state generates the phones'
> bindings from `money/app` and `money/core` (nix/ark-app.nix), with no
> committed `money/bridge`. Don't build on it: a new screen or field goes
> in `money/app` as a `#[view]`; a device feature is a port in
> `ark-module` with a module under `modules/ark-*`; domain rules go in
> `money/core`. ui/docs/app.md, "Generated, Not Committed", says what to
> do instead in each case. Anything that truly must stay an export here
> is listed below under "Still Here".

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
- **`moneyDemo(today)`** — the demo's household (`money_core::demo`, the
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

**Changes are typed.** Every mutation the domain has is a case of
`MoneyChange` (`money_core::changes`), its input UniFFI's record of the
domain's own: `session.send(.assign(value: Assign(month:categoryId:amount:)))`
in Swift, `session.send(MoneyChange.Assign(Assign(month, id, 25_000)))` in
Kotlin. It crosses as its bytes, never JSON; `session.check(change)` is the
domain's sentence for its first failed refinement, before it is sent.

**Rows say what they mean.** The records carry their rules as methods
(`money_core::words`, exported on their twins in src/core.rs): a budget line's
`tone()`, `meter()`, `story(style)`, `captions(style)`, `isPayment()`,
`toCover()`; a transaction's `needsCategory()`, `bankSaid()`,
`transferAccountId` and `draft()`; the form's `Draft` with `problem()` and
`toChange()`; an ask's `said(now)`, a bank's `status(now)`, and
`follow(asks, since)`; `accountShelves`, `netWorth`, `accountKinds()`,
`suggestPayees`, `unmatchedReceipts`. A phone decides none of them.

**Views are the core's.** `Session(peer)` holds the views the screens
draw, each named by a `MoneyView` — `accounts`, `groups`, `budget(month)`,
`inbox`, `register(account)`, `payees`, `banks`, `linked`, `asks`,
`photos`, `receipts`, `paid(txn)`, `items(receipt)`, `search(account,
text)` (the core's filter: payee, the bank's words, memo, category, account or amount) — and what a page is
made of is the domain's (`money_core::MoneyView`): a month's budget is
its lines, its summary, its spending and its formulas on every platform.
`session.hold(view)` holds one while a screen shows it (a `Holding`, let
go when dropped; held twice, it stays until both are), the core keeps its
rows spliced on its own thread, `session.setListener(Changed)` is told
which views moved (`moved(views)`, on that thread; the app hops to its
main thread), and `session.version(view)` grows whenever one did. A view is
read by name: `session.budget(month)` (`Budget`: lines, summary, spending),
`session.register(account)`, … — UniFFI's own records
(`uniffi.money_bridge` on Android; the same module on iOS), each the
twin this crate writes of the core's type (`ark::twins!`), no JSON. ArkUI
draws them: `watched.watch(view) { read }` on Android, `.holding(view)` on
iOS; the iced window holds `ark_peer::store::Store<MoneyView>` itself.
`session.send(change)` is `moneySend` with its refusal kept
(`session.refusal()`). Nothing runs on a clock.

**Refusals are values.** A form asks first: `session.check(change)` is
the domain's own sentence for the first refinement the change fails ("an
account needs a name"), or none. `moneySend(peer, change)` sends one and
answers `Sent.intent(id)` or `Sent.refused(why)` rather than throwing.
After it is sent, `peer.standing(id)` is pending, confirmed or
rejected with why; `peer.takeRejections()` is what the server refused since
the last ask; `peer.status()` is the link, what is pending, and `denied`
when the server turned the login away.

Also: `moneyDomain()` (`"<schema hash> <version>"`, what a server's
`/healthz` says on its `domain` line), `moneyToday()`, `moneyAddMonths`,
and every number, day and length
as the apps write them: `ark_format`'s, linked into the library, as
`formatAmount`, `formatMonth` ("October 2026"), `formatDayHeading` …
(`uniffi.ark_format` in Kotlin).

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
cargo test -p money-bridge              # rust/
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

## Still Here

What the phones call through this crate that the generated bindings will
have to carry when it goes: `moneyOpen`, `moneyDemo` and `DemoBank`, the
session (`session.rs`), the hall of servers and logins (`hall.rs`), and
the routes (`routes.rs`). Add to this list only with a reason in the commit.
