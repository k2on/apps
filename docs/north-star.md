# North Star

The design every app, the kernel and the UI are being moved toward. It is
the target, not a record: `plan-v5.md` says what is built today, and the
last section here says how to get from one to the other, step by step.

## Goals

- **One system.** A tap, a database change, a server confirmation and a
  playback position are the same kind of message, through the same loop,
  on every device.
- **One kernel, small enough for a watch.** The same kernel and protocol
  run on a server, a desktop, a phone, a web page and a Pebble watch. It
  has no I/O and needs no standard library, only an allocator. Nothing is
  a "lite" version of anything.
- **Offline first.** Every device works with no connection, keeps its
  changes, and syncs when it reconnects, a watch through its phone.
- **Partial sync.** A device holds only what its views read, from the
  server and from its own disk, as Zero does and further: offline writes,
  old clients that keep working, and loading from an index on disk.
- **Typed end to end.** Permissions, the API and versions are checked by
  the compiler, not by convention.
- **Expo-like apps.** An app is folders and plain functions. What can be
  derived from the code is never written twice.

## Vocabulary

Everything is built from seven things:

| Name | What it is |
|---|---|
| **Entry** | the one message |
| **Space** | where an entry and its data live: `local`, `device`, `session`, `shared` |
| **Node** | one process running the loop: `step(entry) -> entries` |
| **Handler** | a query, a mutation, a reaction, or a view's `&mut self` method |
| **Cx\<Role\>** | what a handler runs with: the database as middleware narrowed it, and the role |
| **Port** | the outside world: disk, network, clock, camera, a bank |
| **Version** | a frozen schema and API, and the lenses between it and the next |

## Crates

```
ark       the kernel: Entry, Space, Node::step, the store trait, the query
          engine, the log, rebase, versions, Cx and roles.
          no_std + alloc. No I/O, no threads. The same build everywhere.

ark-ui    primitives, native containers and controls, and the generators
          for SwiftUI, Compose, iced, the web and the watch. Knows nothing
          about data.

ark-app   the framework that joins them: the one macro that reads src/app/
          (routes, pages, api, middleware), the hosts for each platform,
          and ports.
```

The kernel is everything that must agree across devices. A **host** is
everything that differs by platform, given to the kernel through traits:

- `Store`: rows and indexes. In memory on a watch, SQLite on phones,
  desktop and the server, OPFS on the web.
- `Link`: bytes to and from the other end. A socket, a phone relay, a test
  harness.
- `Clock` and `Rng`: time and ids, so the kernel stays deterministic.
- `Port`s: everything else the app asks for.
- The server's host adds what only an authority has: the master log on
  disk, the record of what each client holds, threads, HTTP.

Because the kernel is no_std and does no I/O, the rule "if it fits on a
watch, it fits everywhere" keeps it honest: every choice the watch forces
(no threads inside, operators that don't hold their inputs, a compact
wire) is also a better choice on a server.

## The Entry

```rust
struct Entry<B> {
    id: EntryId,           // the device's origin and its counter: never renumbered
    space: Space,
    actor: Actor,
    at: Hlc,               // ordered across devices without trusting their clocks
    cause: Option<EntryId>,
    version: Version,      // the build that made it
    seq: Option<Seq>,      // set when the master log orders it
    body: B,
}
```

The envelope is shared; the body is each machine's own typed enum. There is
no single giant message type and no dynamic body.

| Space | Holds | Kept | Travels |
|---|---|---|---|
| `local` | a view's state | in memory | never |
| `device` | settings on this device | on its disk | never |
| `session` | live state: playback position, presence | in memory | to your devices through the server, not stored |
| `shared` | the app's data | the master log | to the server, then to whoever may see it |

High-rate `local` and `session` entries are coalesced by the kernel, one
per frame or at the rate a subscriber asked for, never stored in a log.

## The Loop

```rust
trait Machine {
    type Body;
    fn step(&mut self, e: Entry<Self::Body>) -> Vec<Entry<Self::Body>>;
}
```

`Node::step` finds the entry's handler, runs it in a transaction on its
space, wakes the queries and reactions whose reads it touched, and returns
what must travel. The host routes what comes out:

| Space or target | The host |
|---|---|
| `local` | feeds it back in |
| `session` | sends it, never stores it |
| `shared` | keeps it as pending, sends it; it comes back with `seq`, or refused |
| a port | hands it to the driver; the answer returns as an entry with `cause` set |

This one function replaces the peer's `Input`/`Output` machine, the UI
runtime's `update` and `Command`s, and the server's handling of calls.

### Authority and Rebase

There is one master log, on the server. The server is the only node that
appends to it: it runs middleware, checks the role, runs the mutation on
confirmed state, assigns `seq`, and keeps the rows it changed as **facts**.

A client:

- runs its own mutation at once, on an overlay of pending entries over
  confirmed state;
- applies the server's confirmed facts, never re-running anyone else's code;
- re-runs only its own pending entries on top, with its own version's code
  (the rebase);
- drops a refused entry, and shows the refusal.

A device with no server is a replica that isn't connected, nothing more:
there is no separate offline mode. Pending entries are kept as records, so
they survive a restart and are put back, not run again.

A refused entry never blocks the entries queued after it.

## Data

Tables are structs. Where they live is an argument:

```rust
#[model(shared)]
pub struct Txn {
    #[key] pub id: Id<Txn>,
    #[index] pub account: Id<Account>,
    pub payee: String,
    pub amount: Cents,
    #[index] pub date: Date,
    pub approved: bool,
}
```

A struct with a `#[key]` is a table; one without is a single value
(`#[model(device)] struct Appearance`). Indexes are declared, so queries
can read ranges of them on a server, a phone and a watch alike.

## The API

Each file in `src/app/api/` is a namespace, as in Convex. Its functions
are the API, with no attributes and no registration:

```rust
// src/app/api/txs.rs  →  api.txs.*
pub fn list(cx: &Cx<Member>, account: Id<Account>) -> Plan<Txn> {
    cx.db.scan::<Txn>()
        .by(Txn::account, account)
        .sort(|t| Desc(t.date))
        .take(200)
}

pub fn approve(cx: &mut Cx<Member>, id: Id<Txn>) -> Result<()> {
    cx.db.update::<Txn>(id, |t| t.approved = true)
}

pub fn purge(cx: &mut Cx<Owner>, before: Date) -> Result<()> { … }
```

The routes macro reads each signature:

- `&Cx` is a query and `&mut Cx` a mutation;
- `Cx`'s parameter is the role the caller needs;
- the rest is the input, and the return type the output.

The path is the name: `api/txs.rs` and `list` are `api.txs.list` on every
client and `"txs.list"` in the log. Each procedure is also an HTTP
endpoint (`POST /api/txs/approve`) through the same middleware.

Mutations must be deterministic: time from `cx.now()`, ids from
`cx.new_id()`, no I/O. A mutation may run on a device that doesn't hold
every row it reads; it must handle a missing row (refuse, or skip its
optimistic part), and the server's run decides.

## Middleware and Roles

```rust
// src/app/api/_middleware.rs
pub fn member(cx: Cx<Signed>, next: Next<Member>) -> Result<Response> {
    let me = cx.actor();
    let home = cx.db.get::<Membership>(me)?.household;     // read once, here
    next(cx.narrow(Member { me, home }, |db| db
        .filter::<Account>(move |a| a.household == home)
        .filter::<Paycheck>(move |p| p.owner == me)
        .mask::<Account>(move |a| a.owner != me, |a| a.number = None)))
}
```

- The body runs once per connection. Its reads are tracked, so it runs
  again when they change: removing someone from a household rebuilds their
  feed.
- The closures passed to `filter` and `mask` are pure, per row. They are
  added to every query as an operator, so rows a user can't see never
  reach a result, on the server or on a device's own disk.
- A table no middleware names is fed to nobody.
- A `_middleware.rs` in a folder narrows again for what is in that folder.
- The server checks every entry again; a client's types prevent mistakes,
  not attacks.

The role is a type. `Owner: Has<Member>`, and a procedure needing `Owner`
exists only on `Api<R>` where `R: Has<Owner>`.

## Queries and Partial Sync

A query is a plan of incremental operators. The kernel's engine
(`rust/ark/src/plan.rs`) already has `scan`, `filter`, `map`, `sort`,
`take`, `group` and `join`, each maintained incrementally, with the reads
its closures make recorded. The target adds what lets it run partially:

- **Pull as well as push.** Every operator answers an ordered, resumable
  `fetch(from, reverse, limit)`, answered at the bottom by an index. A
  `take` over an index-ordered input keeps only its size and its last row,
  and fetches neighbours when a row leaves. A sort by a closure no index
  provides keeps today's behaviour.
- **`related`**: a parent with its children, held as an index from parent
  key to children, with per-parent limits.
- **Aggregates as running state.** Count, sum and average update in
  constant time; min and max refetch only when the extreme leaves.

### What Syncs

- A device subscribes to queries by name and arguments, never by sending
  a query. The server runs the same Rust.
- The server sends the **base rows** each plan read, whole, never a plan's
  mapped output, so two queries share rows and counts stay simple.
- An aggregate syncs as its result, in its parts (sum and count, not the
  average), so a device can apply `count` and `sum` changes optimistically.
  `min` and `max` wait for the server.
- The server keeps, per client: the queries it asked for and the ones it
  has, the rows it holds with how many queries need each, and the `seq` it
  is at. Never the rows' contents.
- Every batch of changes is tagged with one `seq` and applied at once, so
  views never show different moments.
- A row that stops matching is sent as a removal ("move-out").
- Each subscribed group of rows carries a checksum, so a device can prove
  it holds what the server thinks it holds.
- A query stays subscribed for a TTL after its view closes; `preload()`
  subscribes ahead.
- A result knows whether it is complete. A device records which index
  ranges it has fully fetched, so it can tell from its own disk, not only
  from the server.

### Loading From Disk

The same plans decide what a device loads from its own disk. A desktop
holding gigabytes keeps in memory only what open queries read.

- The store is indexed on disk (SQLite behind the `Store` trait).
- Operator state is partial, per key, as in Noria: a key is either fully
  loaded and kept current, or evicted. Reading an evicted key fetches it
  from the operator above, down to the disk ("upquery"). An update for an
  evicted key is dropped.
- Rows touched by pending entries stay loaded until the rebase.
- Evicting locally never changes what the device is subscribed to.

Noria's four correctness invariants are the specification the engine's
tests are written against, alongside the rule that every incremental
result equals a fresh run.

## Views

A page is a struct in its route file. Its plain fields are its inputs,
from the address or its parent; its `#[state]` fields are its state:

```rust
// src/app/(household)/accounts/[id].rs
#[view(role = Member)]
pub struct Register {
    id: Id<Account>,
    #[state] selected: Option<Id<Txn>>,
}

#[view]
impl Register {
    fn select(&mut self, t: Id<Txn>) { self.selected = Some(t); }

    fn body(&self) -> View {
        let txs = api.txs.list(self.id);                       // live
        list(txs.iter().map(|t| row(t)
            .selected(self.selected == Some(t.id))
            .on_tap(self.select(t.id))                         // a local entry
            .on_swipe("Approve", api.txs.approve(t.id))))      // a shared entry
    }
}
```

- `&mut self` methods are the page's messages. Called from `body`, they
  return the message; the runtime runs the method when it arrives. Every
  tap is a recorded `local` entry, as in Elm, with no `Msg` or `update`
  written.
- `api` is added by `#[view]`, typed `Api<Member>` here, so `api.txs.purge`
  doesn't compile in this page. The page's role is checked against the
  middleware its folder's layouts run.
- A `_layout.rs` only frames its children: a shell, tabs, a stack, sheets.
- App-wide state that isn't one page's is a `#[model(device)]` or
  `#[model(session)]` value, read with `Player::get()`.
- Shared components are `#[view]` structs in `src/components/`.

The same view code renders through arkui on every target, the watch
included.

## Reactions

Side effects are reactions to row changes. Where they run is their folder:

```rust
// src/app/on/server/receipts.rs
#[on(insert Photo)]
async fn read(p: &Photo, ocr: &Ocr, db: &Db) -> Effects<Server> {
    db.upsert(Receipt::from_lines(p.id, ocr.read(&p.file).await?));
    Ok(())
}

#[on(every = "1h")]
async fn import(db: &Db, bank: &Bank) -> Effects<Server> { … }
```

- Each reaction is a consumer of the master log with its own cursor.
  Delivery is at least once; effects are keyed so a repeat does nothing.
  The log is the outbox.
- A failing reaction retries with backoff, then records the failure on the
  row it was working on and moves on. One bad row never blocks the rest.
- A new reaction may start its cursor at zero and work through history.
- Its ports decide where it can live: one asking for `&Bank` only compiles
  in `on/server/`, one asking for `&Notifications` only in `on/device/`.

## Versions

Each release freezes its schema and its API (generated, committed, never
edited). The build fails when they change without a step to the previous
release.

- **Rule 0: additive changes need nothing.** A new optional field, or one
  with a default, a new table, a new procedure: the kernel fits old and
  new to each other.
- **Anything else is a lens**, written once, with `up`, `down` and `put`:

  ```rust
  impl Lens<v1::Txn> for Txn {
      fn up(old: v1::Txn) -> Txn { … }
      fn down(new: &Txn) -> v1::Txn { … }
      fn put(old: v1::Txn, current: &Txn) -> Txn { … }   // keeps what v1 can't see
  }
  ```

  The steps a lens may take, as in Jazz: add with a default, drop (with a
  default for old clients), rename, create or drop a table. Changing a
  type or moving data between tables is a new field or table plus a
  reaction that fills it.
- Every entry carries the version that wrote it. Code runs at the current
  version only; old entries, pending mutations and query arguments are
  translated up, rows are translated down per client.
- The server updates first. It serves a window of releases; a client
  outside it is told to update at connect, and a client newer than the
  server stays offline until it catches up.
- A behaviour change no lens can express is flagged, as Stripe flags
  them.

## Hosts

| Host | Store | Link | Also |
|---|---|---|---|
| server | SQLite, the master log | sockets, HTTP | per-client sync records, reactions, threads |
| desktop | SQLite | socket | iced windows |
| phone | SQLite | socket | SwiftUI or Compose |
| web | OPFS | socket | the generated page |
| watch | in memory, persisted to the watch's storage | the phone, as a pipe | Pebble UI |

The watch runs a full node: its queries, its pending entries, its views,
offline. When the phone is in reach it is only the watch's link to the
server; the watch syncs end to end itself. Live entries (playback position)
arrive through the phone into the watch's own loop like any other.

## An App

```
money/
  app.toml  icon.svg  Cargo.toml
  src/
    lib.rs
    models/                  #[model] tables and values
    app/
      _layout.rs             the shell
      (household)/
        _layout.rs           runs `member`
        budget.rs            a page
        accounts/
          index.rs
          [id].rs            a page with its input
      (modals)/
        _layout.rs
        add-transaction.rs
      api/
        _middleware.rs
        txs.rs               api.txs.*
        budget.rs            api.budget.*
      on/
        server/              reactions on the server
        device/              reactions on devices
      +not-found.rs
    components/              shared views
    demo/                    its demo household
  versions/                  frozen schemas and APIs, generated
```

There is no `server/` crate: an app's server is the generic server host
running its `api/`, `_middleware.rs` and `on/server/`.

## Testing

One harness for everything: feed entries into a `Node`, check its rows and
what comes out. A page, a sync bug and a server rule are tested the same
way, with ports answered by the test.

- **The engine:** every incremental result equals a fresh run, and
  Noria's invariants hold, under the fuzzer's random changes and
  evictions.
- **The kernel:** the fuzzer, run at depth nightly (1,000 sessions and
  more), checking convergence.
- **Permissions:** every table against a row of the reader's and a row of
  someone else's, and a new table fed to nobody.
- **Versions:** an old and new client against an old and new server, every
  pair.

A bug report is a list of entries; replaying it gives the same screen,
because nothing in the loop does I/O.

## From Today

| Today | Becomes |
|---|---|
| `ark::peer` (`Input`/`Output`), ark_app's `Runtime` | `Node::step` |
| `Frame`, UI `Msg` and `Command` | `Entry` |
| `#[mutator]`, `Domain` registration | functions in `src/app/api/` |
| `MoneyView`, `ViewKey`, holds, page records, twins | queries |
| `Scope`, `Feed`, ark-auth's roles | middleware and `Cx<Role>` |
| effect jobs (`Job<T>`) | reactions |
| `App` with `update`; screens in `_layout.rs` | `#[view]` pages in their route files |
| `ark-module` | ports |
| the volatile space | `session` |
| JSON migrations | rule 0 and lenses |
| each app's `server/` | `api/`, `on/server/` and the server host |
| ark, ark-peer, ark-auth | `ark` |
| arkui, arkui-web, arkui-macros, glyphs, gen | `ark-ui` |
| ark_app, ark_router, ark-ffi, ark-module | `ark-app` |

Kept as they are: the master log with its facts and hashes, the incremental
operators, file routing, arkui's primitives and generators.

## Steps

Each step lands on main with both apps working. Nothing is kept for
compatibility.

1. **This document.**
2. **The kernel's shape.** `Entry` and `Node::step`; the peer's inputs and
   the wire become entries. From PR #31: no alone or join modes (an
   unconnected replica), pending entries kept as records, facts not
   states, entry ids never renumbered, a journal of refusals only, the
   machine emitting its own records, `tracing` in place of recording.
   no_std + alloc, with `Store`, `Link`, `Clock` and `Rng` as traits.
3. **Partial sync**, the largest step:
   - **3a** pull-based `fetch` with index-backed scans, bounded `take`,
     `related`, running aggregates;
   - **3b** subscriptions by query, per-client sync records, base rows and
     aggregate results, atomic batches, move-outs, checksums; whole-table
     feeds go;
   - **3c** the indexed store on disk and partial loading with eviction;
   - **3d** completeness known locally, TTLs, preload.
4. **The API and middleware.** The routes macro reads `src/app/api/`;
   mutators move there with `Cx<Role>`; middleware replaces `Scope`;
   calls go by full path.
5. **Pages.** `#[view]` structs with `#[state]` in their route files;
   `App`, `Msg` and `update` go.
6. **Reactions and the server host.** Jobs become reactions; each app's
   `server/` folds into `api/` and `on/server/`; the server host bounds
   its threads.
7. **Versions.** Frozen schemas and APIs, rule 0, lenses, the connect
   handshake that says a build is too old.
8. **The session space.** harken's listening session moves onto it,
   through middleware, so one person's listening reaches only their own
   devices.
9. **Crates.** Merge and rename into `ark`, `ark-ui` and `ark-app`.
10. **The watch.** A Pebble host and an arkui generator for it.

Three bugs on main today are fixed by these steps rather than separately:
live rows fanned out to every account (step 8), unbounded call threads
(step 6), and calls matched by the last part of their name (step 4).

## Prior Art

| System | Taken | Not taken |
|---|---|---|
| Zero | subscriptions by query name, base rows, per-client records, desired and got sets, aggregates as parts, pull-based operators, permissions as code | its query language and planner; a whole local store in memory; refusing offline writes; reloading on a schema change |
| Replicache | mutations by name, rebase, a per-client mutation counter | the browser-specific storage layers |
| Noria | partial state per key, upqueries, eviction, its invariants | |
| Linear | knowing locally which index ranges are fetched | |
| Convex | one log, read sets, plain-function API by file | re-running whole queries and resending whole results |
| PowerSync | checksums, TTL eviction | writes through a queue a refusal can block |
| Jazz, Cambria, Stripe | lenses over changes, entries tagged by version, a small set of lens steps | encryption as permissions |
| Electric | position-addressed responses a cache can serve | syncing reads only |
| Elm, iced, GPUI, Leptos | messages from views, views as structs, context by type, server functions | hooks with call-order rules |

## Open Questions

- What an optimistic mutation does when it reads a row the device has
  never loaded: refuse locally, or run against what it has and let the
  server decide. Each mutation may need to say.
- Server state per client means a client must keep reaching the server
  that holds it, and old records must be collected.
- How much a watch can store and how often its flash can be written, which
  bound the watch's replica and pending queue.
- Sorts by a closure no index provides stay unbounded; how often the apps
  need them decides whether that matters.
