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
the rebase, and `Changes::Rebuilt` is how a screen hears about it.

## The API

```rust
use ark_client::{args, Domain, Options, Peer, Standing, Update, View};
use ark::value::Value;

// The app's module, every procedure native. (`ark_client::demo` is the
// demo of spec/AUTHORING.md Appendix B, which the tests use.)
let domain = Domain::new(&my_domain::module());

// Who authors: the login from ark-auth — or `Options::dev(name)` against a
// dev-auth server, or `Options::alone(name)` with no server at all.
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
- **Persistence** is what `Replica::open` takes and nothing optimistic: the
  confirmed store, the cursor and the pending intents, one canonical-CBOR
  record (the Swift client's `ReplicaFile` shape), written whole after every
  mutate and every pump that moved it. A directory keeps the mode it was
  opened with — alone or with a server — and refuses the other
  (`Error::ModeMismatch`), because the sequences mean different things.
  In a browser it is `localStorage`, base64 under `ark:<name>:replica`:
  synchronous, which iced's `boot` needs, and limited to the origin's quota
  of about five megabytes — a library of a few thousand rows fits. An app
  that outgrows it implements `storage::Storage` over IndexedDB, loaded
  before `open`.
- **Views**: a query that is one `select` with no middleware
  (`db.t.filter(..).order_by(..).all()`) is maintained through `ark::view`:
  a change costs the rows it moved. Several changes at once are pushed each
  against the store as it stood just after that change (the current store
  rolled back through the later ones). Any other query is re-run on a change
  and diffed; either way the patches splice the old list into the new. A
  plan the reading got wrong is caught at hydrate — the view is held to the
  query's own answer — and falls back to re-running.
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
| the demo: a server-less peer whose seeded library stays pending | `Options::alone(name)`: its own authority, nothing pending, and a directory that says it was opened alone |

## Tested, and not

`cargo test -p ark-client` tests the link's backoff and its reset on open,
a refused native connection reported closed, the URL authority, base64, and
the diff a re-run view reports. The peer against a real server is tested in
ark-server (`tests/sync.rs`, `tests/live.rs`, `tests/auth.rs`): two peers
syncing over sockets, an offline edit rebased on reconnect (a `Rebuilt` and
a `Reset` view), a view following the log patch by patch, pending intents
and the confirmed store across a reopen of the directory, a mode mismatch
refused, a peer alone, live frames relayed within an account, the sign-in
token on the socket and a denial.

Not verified: the wasm build in a browser. It compiles for
`wasm32-unknown-unknown` (clippy clean) and nobody has opened a page with
it — the `WebSocket` transport and `localStorage` have run nowhere. Nor has
`wss://` from the native transport been tried against a TLS server.
