# ark-client

A Rust peer on ArkDB, for any app's module — the desktop and the browser
(`wasm32-unknown-unknown`). What `petros::Client` plus
`petros::transport::{ws, web}` were for harken's iced client, over the
`ark` engine instead of SQLite.

```text
Peer ─ the Replica of the log, persisted (a directory | localStorage | memory)
     ─ ark::protocol::Client ── Link ── Transport (a tungstenite thread | the browser's WebSocket)
     ─ an Authority, when alone (no server: the demo, a peer working by itself)
```

A peer is `replay(confirmed) then replay(pending)`: a mutation applies at
once to the optimistic store, is kept durable as a pending intent, and is
pushed when the socket is up; confirmed entries only move forward, and the
only thing ever undone is this peer's own pending, replayed on top. That is
the rebase, and a screen hears about it as changes like any other: the
transitions it made — the inverse of what it undid, what landed, what it
re-applied — so a view patches through it (`docs/plan-perf.md` R2).
`Changes::Rebuilt` is for a store replaced whole, as opening over a
snapshot is.

## The API

```rust
use ark_client::{args, Domain, Options, Peer, Standing, Update, View};
use ark::value::Value;

// The app's module, every procedure native. (`ark_client::demo` is the
// demo of spec/AUTHORING.md Appendix B, which the tests use.)
let domain = Domain::new(&my_domain::module());

// Who authors: the login from ark-auth — or `Options::signed_out()` before
// anybody has signed in, `Options::dev(name)` against a dev-auth server, or
// `Options::alone(name)` with no server at all.
let opts = Options::server(login.user.id, login.session, Some(login.token));
let mut peer = Peer::open_path(domain, data_dir, opts)?;       // natively
// let mut peer = Peer::open_local(domain, "myapp", opts)?;    // in a browser
// let mut peer = Peer::open_memory(domain, opts)?;            // a test, the demo

peer.connect("wss://app.example/sync");                        // reconnects on its own

let id = peer.mutate("create_playlist", args([("name", Value::text("Road trip"))]))?;
let rows = peer.query("items", &args([("playlist_id", Value::Id(list))]))?;
let checked = peer.check("create_playlist", &args([("name", Value::text(" "))]))?; // the form validator
let mut items: View = peer.view("items", args([("playlist_id", Value::Id(list))]))?;

// on every tick (50 ms):
let pumped = peer.pump();              // dial, frames in and out, persist
let changes = peer.take_changes();     // Applied(changes) | Rebuilt
match items.update(&peer, &changes)? {
    Update::Unchanged => {}
    Update::Patched(patches) => ark_client::splice(&mut my_rows, &patches),
    Update::Reset => my_rows = items.rows().to_vec(),
}
for r in peer.take_rejections() { /* the server refused r.id: r.reason */ }
if let Standing::Rejected(why) = peer.standing(&id) { /* show why beside the item */ }

// the live channel, on the same socket
peer.say(frame_bytes);                 // dropped, not queued, while unlinked
for f in peer.heard() { /* … */ }
if peer.epoch() != introduced_on { /* a new connection: say who you are again */ }
```

- **Autos** are drawn at the origin, once, and frozen in the entry: a
  version-4 id per `NewId` (the OS's randomness; `getrandom` with `js` in a
  browser) and `Date.now()`/`SystemTime` per `Now`. `Autos::seeded(n)` makes
  them reproducible for tests.
- **Persistence** is what `Replica::open` takes and nothing optimistic, in
  canonical-CBOR records (the layout is in `storage.rs`'s module docs): a
  snapshot, `replica` — the confirmed store at a cursor — and after it a
  journal of pages, `facts.1`, `facts.2`, …, each the changes the confirmed
  store moved by for a contiguous run of sequences, in the form a
  `FactsFor` frame carries them; `pending` — the intents not yet answered —
  the same shape, a snapshot and after it pages `pending.1`, `pending.2`,
  …, each the ids that left and the intents that joined since the one
  before (`docs/plan-perf.md` §R3); and `who`, the login last authored as,
  written when it does, so a sign-in writes neither the store nor a page.
  So `mutate` writes one page holding its intent — before it returns,
  alone, which is the durability of a local write — and not the backlog
  nor the store, and a `pump` after it writes one page of what its answers
  moved and one page of what the confirmed store moved by — a mutation's
  changes, not the library, which a peer alone used to re-encode whole
  after every tap, and not every intent still pending, which a peer
  offline used to (21 ms a tap at eight thousand pending). The pending
  pages are compacted on a pump, never inside `mutate`: once they outgrow
  their snapshot, and once nothing is pending, when the snapshot is a few
  bytes and replaces the page. Each pending snapshot carries a
  generation and each page the generation it extends, so a stop between a
  compaction's snapshot and its removing the pages leaves pages `open`
  skips rather than replays. A snapshot is written when the confirmed store was
  replaced (a snapshot from below the server's horizon), and to compact:
  once the pages' total size exceeds the snapshot's, a fresh one at the
  cursor, then the pages removed, newest first — so the bytes written stay
  within twice the journal's whatever the store's size, and a storage holds
  up to about twice a snapshot. The order is the pending record first, so a
  stop between the two leaves an intent to be sent again rather than one
  applied twice; `open` reads the pages in order, skips any a snapshot
  already holds, drops a torn or out-of-order tail rather than apply it,
  and compacts away whatever it could not use. A peer alone moves its
  cursor on every mutate; it writes the page on its next `pump` (the
  clients call one every fifty milliseconds) and when it is dropped, so
  the window in which a crash loses the last change is one tick. A
  directory writes each record to a temporary name, syncs it, renames it
  and syncs the directory. A storage written before the journal — a
  `replica` record and nothing after it — opens as it did, and so does a
  `pending` record written before the pages (it has no generation, which
  reads as 0). A torn pending page is dropped with every page after it:
  it held the one intent whose `mutate` had not returned. A directory
  keeps the mode it was opened with — alone or with a server — and refuses
  the other (`Error::ModeMismatch`), because the sequences mean different
  things. Signed out and signed in are the same mode: one directory is
  opened either way.
  In a browser it is `localStorage`, base64 under `ark:<name>:replica`,
  `ark:<name>:facts.<n>`, `ark:<name>:pending`, `ark:<name>:pending.<n>`
  and `ark:<name>:who`:
  synchronous, which iced's `boot` needs, and limited to the origin's quota
  of about five megabytes — which, with room for the journal beside the
  snapshot, is a snapshot of about half that: a library of a few thousand
  rows fits. An app that outgrows it implements `storage::Storage` over
  IndexedDB, loaded before `open`.
- **Views**: every query is maintained (`docs/plan-v4.md` §1.5). A query
  is a plan, and `peer.view(name, args)` runs its middleware — input
  checks, guards, provides — over the peer's store, hydrates the plan in the
  scope that gives, and keeps `ark::view`'s entries: each candidate row or
  group, what its subtree looked up and joined on, and whether its having
  admitted it. `update` hands the changes of one settle to
  `ark::view::push_all`, which rebuilds the entries they touch — once each,
  against the store as it now is, at any depth of lookups and related
  lists — and reports patches that splice the old list into the new; a
  limit's window refills from the entries rather than the store. A change
  costs the entries it touches, whatever the size of the list; a change
  nothing depends on costs two index probes. The tables the middleware
  reads (`ark::ir::reads`) are kept too: a change to one of them, or another
  user signed in, runs the middleware again, and a different outcome — a
  playlist renamed under an open page, or deleted so `owned` refuses — is a
  re-hydrate (an empty list on a refusal) and `Update::Reset`, as a store
  replaced whole (`Rebuilt`) is. A rebase is not: it is patches.
- **The link**: `connect(url)` dials on the next `pump`; a drop dials again
  after half a second, doubling to thirty (`Timing`). A denial (`Denied`, a
  token the server does not accept) stops the link: `set_token` and
  `reconnect` are the way back, and nothing pending is lost. `disconnect`
  goes offline on purpose. The server pings every twenty seconds and a
  browser answers on its own; the native transport pings too and gives up a
  socket that has said nothing for three of its intervals.
- **Rejections** carry the sentence every replica reaches
  (`ark::protocol::refusal_text`: a mutator's own `refuse` word for word, a
  constraint named). `take_rejections()` drains them as they arrive;
  `standing(&id)` answers, for any intent this peer authored, whether it is
  pending, confirmed, or rejected and why — what a screen shows beside the
  item that did not happen.
- **Signed out**, nobody has signed in yet (`Options::signed_out()`): every
  intent is authored as nobody (`Ctx::nobody`), stays pending and durable
  across restarts — it is not committed, which is what makes it different
  from alone — and nothing is dialled; `connect(url)` keeps the link idle.
  `peer.sign_in(user, session, token)` makes every intent nobody authored
  the signer's under that login, replays the view (rows say who they now
  say before a byte is sent), writes it down, and dials; the ids `mutate`
  returned are the same ids, so `standing(&id)` follows each to confirmed
  or to the sentence the server refused it with. Opening the directory with
  a login instead of calling `sign_in` does the same.

  `peer.sign_out()` forgets the token and stops the link and **keeps the
  login**: a peer somebody has used goes on authoring as them, offline, and
  a reopen with `Options::signed_out()` does too. What they author then is
  theirs — accepted when they sign in again under any login of theirs
  (`Auth::owns`), refused as `not yours` if somebody else does. Only work
  done before anyone ever signed in on a peer is given to whoever signs in,
  because only that was never anybody's. (petros did the same for a denied
  peer: its pending edits waited for the next login as the same person.)
- **Alone**, the peer is its own authority: every intent is sequenced at
  once, nothing stays pending, and `verify` answers immediately.
- **Sans-io**, for a transport of your own: `connected`, `disconnected`,
  `recv`/`recv_frame`, `take_outgoing`/`take_outgoing_frames`, `persist`;
  `connect_with(url, dial)` takes any `link::Transport` (ark-server's
  `HubHandle::dial` is one: a peer in the server's own process, no socket).

## From petros, call by call

What harken's iced client called on petros and petros-auth, and what it is
here.

| petros | ark-client |
|---|---|
| `petros::open_path(p)` / `open_memory()` + `Client::<App>::open(conn, actor, AutoCtx::system())` | `Peer::open_path(domain, dir, opts)` / `open_memory(domain, opts)` / `open_local(domain, name, opts)` (browser) |
| `AutoCtx::system()` / `seeded(n)` | `Autos::system()` / `Autos::seeded(n)`, in `Options::with_autos` |
| `client.set_session(Some(s))`, `set_token(Some(t))` | `Options::server(user, session, token)`, or `peer.set_session(s)`, `peer.set_token(Some(t))` then `reconnect()` |
| `client.mutate(mutators::f(args))` → `Id` | `peer.mutate("f", args([...]))` → `Id`; a refusal is `Err(Error::Refused(_))`, and `Display` is the reason |
| `harken::q(&mut client.store(), ..)` | `peer.query("q", &args)` → `Value` (or the domain's typed wrapper over it); `peer.store()` is the optimistic `MemoryStore` for a direct read |
| `client.take_changes()` → `Changes::{Applied, Rebuilt}` | `peer.take_changes()` → the engine's `Changes::{Applied, Rebuilt}` |
| `petros::ivm::View` + `hydrate` / `apply(store, &changes)` → patches | `peer.view("q", args)` / `view.update(&peer, &changes)` → `Update::{Unchanged, Patched, Reset}`; `splice` |
| `client.pending_len()`, `cursor()` | `peer.pending_len()`, `peer.cursor()` |
| `client.take_rejections()` → `Rejection { id, reason }` | `peer.take_rejections()`, the same shape; and `peer.standing(&id)` for one item |
| `client.take_denial()` | `pump().denied`, or `peer.denied()` until the next `reconnect` |
| `Link::connect(&socket_url(server))` + `client.connected()` | `peer.connect(&ark_auth::socket_url(server))` — the link says `connected` itself, and reconnects |
| the pump: `take_outgoing` → `link.send`, `link.try_recv` → `client.recv`, `link.is_alive()` | `peer.pump()` → `Pumped { moved, opened, dropped, denied, rejected, note }` |
| `link = None` to go offline; a new `Link` to come back | `peer.disconnect()` / `peer.reconnect()` |
| `client.linked()`, `client.epoch()` | `peer.linked()`, `peer.epoch()` |
| `client.say(&say)` (serde) / `client.heard::<Hear>()` | `peer.say(bytes)` / `peer.heard()` — or `say_value(&Value)` / `heard_values()`, canonical CBOR of an ArkDB value |
| `ServerMsg::Heard` counted by hand | `status().heard_frames`, `status().bad_frames` |
| `petros::encode` / `decode` | `ark::canon::encode(&value)` / `decode` |
| `transport::web::Link` vs `transport::ws::Link` by `cfg` | one `Peer::connect`; the platform's transport is chosen inside |
| (nothing: a petros peer needed a login to author) | `Options::signed_out()`, then `peer.sign_in(user, session, token)`; `peer.sign_out()` |
| the demo: a server-less peer whose seeded library stays pending | `Options::alone(name)`: its own authority, nothing pending, and a directory that says it was opened alone |

## Tested, and not

`cargo test -p ark-client` tests the link's backoff and its reset on open,
a refused native connection reported closed, the URL authority, base64,
the demo's `items` patch by patch, under seeded churn and through four
rebases against a sans-io server (patches every time, never a reset), and a view reset by
its middleware (a playlist renamed and deleted under it) and by another user
signing in. The peer against a real server is tested in
ark-server (`tests/sync.rs`, `tests/live.rs`, `tests/auth.rs`): two peers
syncing over sockets, an offline edit rebased on reconnect (the view
patched through it), a view following the log patch by patch, pending intents
and the confirmed store across a reopen of the directory, a mode mismatch
refused, a peer alone, live frames relayed within an account, the sign-in
token on the socket and a denial. Signing in late: fifty-one entries
authored signed out, across a restart, all confirmed as the signer's with
the playlist hers on the server, the directory then opened signed in, and
work done after `sign_out` still hers across a restart and a new login;
and the one entry the server's copy refused (its playlist was never made
there) standing `Rejected("playlist_id: no such playlist")` while the rest
are confirmed. An intent refused both by the peer's own rebase and by the
authority it had been pushed to is reported once.

Not verified: the wasm build in a browser. It compiles for
`wasm32-unknown-unknown` (clippy clean) and nobody has opened a page with
it — the `WebSocket` transport and `localStorage` have run nowhere. Nor has
`wss://` from the native transport been tried against a TLS server.
