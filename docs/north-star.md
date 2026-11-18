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
  changes, and syncs when it reconnects, a watch with its phone.
- **Partial sync.** A device holds only what its views read, from the
  server and from its own disk, as Zero does and further: offline writes,
  old clients that keep working, and loading from an index on disk.
- **Typed end to end.** Permissions, the API and versions are checked by
  the compiler, not by convention.
- **Expo-like apps.** An app is folders and plain functions. What can be
  derived from the code is never written twice.

## Vocabulary

Everything is built from eleven things:

| Name | What it is |
|---|---|
| **Entry** | the one message |
| **Space** | where an entry and its data live: `local`, `device`, `session`, `shared` |
| **Node** | one process running the loop: `step(entry) -> entries`, then a settle |
| **Handler** | a query, a mutation, a reaction, or a local entry's: a view's `&mut self` method is one like any other |
| **Cx\<Role\>** | what a handler runs with: the database as middleware narrowed it, the cells, and the role |
| **Cell** | a single value that never travels: a view's state, a selection, a draft. Typed (`Cell<T>`) at the app's edge; in the kernel a type-erased slot (a pointer, a vtable to compare and drop it, and a version). A write is a pointer swap, cut off when the value is equal; it is never encoded. The `local` space is cells |
| **Plan** | a query of incremental operators over rows, kept live by the node and pulled by `fetch`, also one row (`Plan::get`): a persistent node of the graph, kept by the function that made it and its arguments |
| **Sink** | what is told that something it read changed: a view's leaf, the disk's flush, an upstream's record of what a downstream node holds. It pulls what it needs when it next draws or writes |
| **Binding** | a leaf of a view joined to what it shows: a tracked thunk, its every read of a cell, a row or a plan's answer, through whatever functions it calls, recorded against it. It runs again only when one of those changes, and is cut off by equality. Written by the build as data, never as code per screen |
| **Port** | the outside world: disk, network, clock, camera, a bank |
| **Version** | a frozen schema and API, and the lenses between it and the next |

State is **rows** (collections that sync, so they are encoded) and
**cells** (single values that don't). Change is one **settle** per step.
A view is a plan over those whose leaves are sinks that pull.

## Crates

```
ark           everything a device runs and must agree on: Entry, Space,
              Node::step and its settle, the Store trait, sync, plans
              (one-row plans included), cells, read sets, the
              Sink trait and the interpreter of binding tables.
              no_std + alloc. No I/O, no threads. The same build everywhere.

ark-upstream  serving other nodes: the Authority, Serve and Relay, and the
              per-client records. Linked by a server, a desktop or a phone;
              never by a watch.

ark-gen       build time only: the #[view] recorder, the routes macro,
              schema freezing, and the emitters (SwiftUI, Compose, Pebble's
              binding tables). Never linked on a device.

ark-host-*    one per platform: its stores, its links, its ports, and each
              toolkit's sinks (the Swift and Kotlin bridges, iced, the web,
              Pebble's layers).
```

The line between them is **when and where code runs**, not data versus
UI. Change tracking, view state and joining a leaf to the value it shows
are one mechanism, so splitting "the data" from "the UI" puts glue
between them on every screen: a host rebuilds a record from rows and a
toolkit diffs it a second time. What every device must agree on is one
crate, what only an upstream runs is another, what runs only at build
time is a third, and what differs by platform is a host.

arkui's rule stands inside this: it holds primitives, native containers
and native controls, nothing else, and every value is a token. Its
description of a view (primitives, parts, bindings) is what the emitters
and the sinks agree on; the recorder that makes it is `ark-gen`'s, and
the toolkits that draw it are the hosts'.

**The database works without the UI, and the UI without the database.**

- A server, a CLI tool, a test or a reaction links `ark` (and
  `ark-upstream`) and nothing of a view. The Sink trait is the one seam:
  no layer, toolkit or binding is needed to use it, and a server's sinks
  are its disk's flush and its records of what each client holds.
- A program that is only views, as the docs site is, runs on `ark`'s
  local cells, its constants and the settle alone: no store on disk, no
  link, no shared space and no upstream, and none of the sync code is
  linked.

A **host** is everything that differs by platform, given to the kernel
through traits:

- `Store`: rows and indexes. In memory on a watch, SQLite on phones,
  desktop and the server, OPFS on the web.
- `Link`: bytes to and from the node's upstream. A socket, Bluetooth to a
  phone, a test harness.
- `Sink`: what is told after a settle: a toolkit's views, the disk's
  flush.
- `Port`s: everything else the app asks for.

Time comes in as an input and ids are drawn deterministically from the
entry, so the kernel needs no clock or random-number trait.
- The server's host adds what only an authority has: the master log on
  disk, the record of what each client holds, threads, HTTP.

Because the kernel is no_std and does no I/O, the rule "if it fits on a
watch, it fits everywhere" keeps it honest: every choice the watch forces
(no threads inside, operators that don't hold their inputs, a compact
wire) is also a better choice on a server.

### The Kernel's Budget

A Pebble Time 2 or Round 2 app gets 128 KB of RAM for its code and heap
together (Pebble 2 Duo: 64 KB, a later target). The kernel's budget is
**48 KB of code and 24 KB of heap**, leaving room for an app's domain, its
views and the renderer. CI gates it: a build of the kernel for
`thumbv7m-none-eabi` at `opt-level = "z"` that fails when it grows past
the budget. What it builds is a watch and nothing else: the watch's slice
run over the frames its upstream sends, recorded at build time from a
real authority and phone node, so no server's or phone's code is counted
as the kernel's.

Measured today, linking the real `Peer::step` at those settings: about
**133 KB** of Thumb-2 code with its dependencies (ark 79 KB, postcard
9 KB, serde_json 9 KB, BTreeMap 7 KB, `core::fmt` 6 KB, sha2 2 KB, glue
20 KB). The kernel has to shrink about 3×. It doesn't build for the watch
at all yet: serde and `thiserror` 1 need std, and ark itself uses a
`Mutex` (`json.rs`), a `thread_local!` (`db.rs`) and `io::Read`
(`file.rs`).

What leaves the kernel for the hosts, and the choices that brings, all of
them good on every platform:

- **Integer ids on the wire**, per version, in place of function, table,
  role and actor names as strings.
- **Its own compact codec** with a decoder that reads frames in parts, in
  place of serde, postcard and serde_json.
- **Refusals as codes**, worded by the host, not formatted text.
- **JSON, dynamic rows and files** move to host crates (`ark-json`,
  `ark-files`). Disk and threads stay in the drivers.
- **The authority's sequencing** is its own type, which a device never
  links. Per-connection feeds and fan-out live in the server host.
- **Rate rules are the session space's**, with the rate a subscriber wants
  stated when it subscribes. A playback position is an anchor each device
  extrapolates from, not a stream.

Two choices would cost the server, so they are host choices behind the
same traits and wire, never different semantics: an arena store (the
server keeps its B-tree), and operators that don't keep their inputs (a
host may let a view cache them).

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
| `local` | cells: a view's state, never encoded | in memory | never |
| `device` | settings on this device | on its disk | never |
| `session` | live state: playback position, presence | in memory | to your devices through the server, not stored |
| `shared` | the app's data | the master log | to the server, then to whoever may see it |

Only rows are encoded, because only rows cross the wire, the disk and the
shared log. A local entry is encoded only if a bug report records it.

## The Loop

```rust
impl Node {
    fn step(&mut self, e: Entry<B>) -> Vec<Entry<B>>;
    fn settle(&mut self, sink: &mut dyn Sink);
}
```

`Node::step` finds the entry's handler, runs it in a transaction on its
space, and returns what must travel. The host routes what comes out:

| Space or target | The host |
|---|---|
| `local` | feeds it back in |
| `session` | steps it here and sends it, never logs it |
| `shared` | keeps it as pending, sends it; it comes back with `seq`, or refused |
| a port | hands it to the driver; the answer returns as an entry with `cause` set |

Every step, and everything received from upstream, ends in one
**settle**, the node's and not the host's or the app's:

1. **The handler's writes.** Its row writes go to its space's overlay; its
   cell writes are journaled beside them. A refusal drops both; otherwise
   both are kept. An equal cell write marks nothing.
2. **Rows.** Each space's changes go through the node's watched plans, in
   the order they were watched, each answering its patches.
3. **Marks.** The readers of every changed cell, row and plan answer are
   marked. Only marks move: values are worked out when they are read,
   after the settle, so no reader sees half a step. A reader whose value
   comes out equal to what it shows stops there.
4. **Sinks.** Each sink is told once: which of its readers are dirty, and
   the patches of the plans it holds. The disk's sink gets the rows it
   keeps.

Everything that reads records what it read, in one read set: a plan's
closures, a view's leaf as it pulls. Cells by slot, rows by table and key
range, a plan's answer by its watch. Nothing is computed when it is
written.

A value derived from cells and rows is a plain function, with no
annotation and nothing kept per call. What it reads is recorded against
the binding that called it. A scalar function's result is shared within
one settle, so two bindings that call it with the same arguments run it
once; that cache is cleared when the settle ends. A function that
returns a `List<T>` or a `Plan<T>` is different: the macro sees the
return type and keeps it as a persistent plan in the graph, keyed by the
function and its arguments, so every view that asks for it shares one.
An explicit memo, kept across settles, is a possible later optimisation,
for a binding that mixes a fast input (the time) with an expensive
scalar that is not a plan; it is deferred until a screen needs it.

A tap is a local entry whose handler writes cells; a sync is a shared or
session entry whose handlers write rows. Both end in the same settle, and
a view finds out the same way. High-rate entries coalesce by
construction: a sink pulls the latest value once per frame, however many
writes there were, and an upstream sends session rows at the rate a
subscriber asked for, never logging them.

This one loop replaces the peer's `Input`/`Output` machine, the UI
runtime's `update` and `Command`s, the record a host rebuilt for a screen
from rows, and the server's handling of calls.

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
  `take` over an index-ordered input holds its window, never more than its
  limit: changes up to its edge go into it, and it fetches past the edge
  only when too few are left. A sort by a closure no index
  provides keeps today's behaviour.
- **`related`**: a parent with its children, held as an index from parent
  key to children, with per-parent limits. Its answer is one ordered list
  of nested places, a row's parent the nearest row before it whose place
  begins its own, so a view shows it as a nested list from the same
  positional patches; `join` is the same without the parents' rows.
- **Aggregates as running state.** Count, sum and average update in
  constant time; min and max are folded again over the group's numbers,
  which it holds, only when the extreme leaves. `filter`, `map`, `sort`
  and `group` are one operator, a closure per row with, for a sort or a
  group, a key it holds the row under.

### What Syncs

- A device subscribes to queries by name and arguments, never by sending
  a query. The server runs the same Rust.
- The server sends the **base rows** each plan read, whole, never a plan's
  mapped output, so two queries share rows and counts stay simple. A
  closure's recorded reads are base rows too: the device re-runs the plan
  over what it holds and gets the server's answer.
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
from the address or its parent; its `#[state]` fields are its state, each
a cell:

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
        let txs = api.txs.list(self.id);                       // a List<Txn>
        list(for t in txs {
            row(t)
                .selected(self.selected == Some(t.id))         // a function
                .on_tap(self.select(t.id))                     // a local entry
                .on_swipe("Approve", api.txs.approve(t.id))    // a shared entry
        })
    }
}
```

- `&mut self` methods are the page's messages. Called from `body`, they
  return the message; the runtime runs the method when it arrives. Every
  tap is a recorded `local` entry, as in Elm, with no `Msg` or `update`
  written, and its handler writes the page's cells.
- `api` is added by `#[view]`, typed `Api<Member>` here, so `api.txs.purge`
  doesn't compile in this page. The page's role is checked against the
  middleware its folder's layouts run.
- A `_layout.rs` only frames its children: a shell, tabs, a stack, sheets.
- App-wide state that isn't one page's is a `#[model(device)]` or
  `#[model(session)]` value, read with `Player::get()`.
- Shared components are `#[view]` structs in `src/components/`.

### Sources, Bindings and Sinks

A view reads **sources**, never a record. What `api` answers is one:

- `api.txs.list(…)` is a `List<T>`, a watched plan's answer;
- a one-row query is a `Row<T>`, cut off by its bytes and decoded when
  read;
- a `#[state]` field, or a value the host keeps (the link, the time), is a
  `Cell<T>`;
- a call on the way (`clock(p.at(now))`, `self.selected == Some(t.id)`) is
  a plain function, its reads the binding's.

No source is passed in as a parameter: a page's inputs are plain values,
and what it reads it asks `api` for.

The recorder keeps each leaf as a **binding**: a path rooted at a source,
through the function calls on the way. A `for` over a list is a list bound to
a `List`; an `if` is a leaf's visibility bound to a `bool`. The bindings
are written as tables, data in rodata, not as draw code per screen, so a
second screen costs its table and not another copy of the code.

A view's text is `Text`, never `String`: a literal converts, and `text!`
builds a template whose holes are bindings of their own, so a sink can
tell the words from what fills them. There is no `From<String>`; logging
keeps `format!`. The rule is every view's, on every toolkit.

Each toolkit draws a binding as a **sink** that pulls:

| Toolkit | A text leaf | A list | Reads known by |
|---|---|---|---|
| Pebble | a layer whose `update_proc` pulls its value at draw | a `MenuLayer` of `count` rows, each pulled | the kernel: each binding's reader records what its source read as it ran |
| SwiftUI | an `@Observable` screen whose getters call into Rust; the settle's dirty readers fire its registrar per property | `List`/`ForEach` over `count` and a row pulled through `fetch`, ids from each row's identity, patches as inserts, removes and moves | Observation's own tracking |
| Compose | a snapshot `State` per source, bumped by the settle, read through a getter | `LazyColumn` over `fetch`, keys from each row's identity | the snapshot system |
| web | a signal per source, fine-grained DOM updates | keyed by identity, patched | signals |
| iced | `view()` reads cells and plans through the node | `fetch` of the visible window | the dirty readers decide whether to ask for a new `view()` |

Observation, snapshots and signals are the platform's half of the same
push and pull. So no record exists between a plan and the screen: there
is none to build in Rust, none to cross UniFFI on every change, and no
second diff in SwiftUI or Compose, since each property invalidates only
its readers. A list is pulled lazily, a window at a time, from `fetch`.
A patch carries a row's depth in a nested list, a move when the same row
changes its place (so a toolkit can animate it, as ui/docs/motion.md
asks), and the row's identity, its base key, so a toolkit keys its cells
by the row and not its position.

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
| watch | in memory, persisted to the watch's storage | its phone, over Bluetooth | Pebble UI |

The watch runs a full node: its queries, its pending entries, its views,
offline. Its upstream is the phone (see "Upstreams"), so it needs no
internet, server address or token of its own.

## Upstreams

Every node has one upstream, and every link speaks the same protocol:

```
server  ←  phone  ←  watch
   ↑
 laptop
```

A node that serves another answers it exactly as the server answers it:

- **Subscriptions.** A downstream node subscribes to its queries at its
  upstream, which runs them on its own replica, sends the rows, and keeps
  the record of what the downstream holds.
- **Pending entries.** A downstream node's entries reach its upstream as
  pending, keep their origin, and travel on to the server with the
  upstream's own. Only the server confirms; confirmations come back down
  the same path, so "confirmed" still means final everywhere.
- **Its view of the world** is its upstream's: confirmed facts and the
  upstream's pending entries, so a watch and its phone always agree, even
  when neither reaches the server.
- **Session state belongs to the node that owns it.** A phone's player is
  the authority for its own playback, so a pause on its watch reaches it
  over Bluetooth with no server. A laptop controlling the same player goes
  through the server, which forwards to the phone. A row's key begins
  with its owner's origin, so every node knows whose it is, and a phone
  answers for its watch's rows as for its own.
- **Live controls don't queue.** A session entry is delivered only while
  connected: offline, a control of another node's row is refused where it
  is made, so the control shows that it can't reach the player and
  nothing fires later. An owner's own rows are always applied. Every
  hello is answered with the latest of every session row the node may
  see, its own included: a row it doesn't own takes its upstream's, and
  one it owns takes nothing from the answer and is said again, newer than
  anything kept upstream while it was away. Any divergence heals on
  reconnect.
- **The tree's clock.** A hello carries the node's time and its answer the
  upstream's estimate of the root's, so each node measures its offset
  NTP's way (the shortest round trip of its last few hellos), composed
  down the tree. Anchors are written and extrapolated in that time, the
  owner's whatever a watch's clock says.
- **Versions.** An upstream translates for its downstream through the
  version chain, as the server does, so an upstream must be at least as
  new as what it serves.

Serving a downstream node (running its subscriptions, keeping its sync
record) is host code that phones, desktops and the server run and a watch
never does, so it doesn't count against the kernel's budget. It is
`ark-upstream`'s (today the kernel's `upstream` feature, which a watch's
build never enables): `Serve` (the server's authority runs it) and a phone's
`Relay`, which answers its watch from the phone's view, holds the watch's
pushes until the server judges them, and passes the verdicts back to the
connection that pushed each.

On an Android phone, the app itself is the watch's upstream, talking to
the watch through PebbleKit 2; iOS has no such path (see "Open
Questions"). When it isn't installed or running, a
stateless relay script in the Pebble phone app can connect the watch
straight to the server instead. The protocol is the same, so the watch
doesn't care which; it only loses working with its phone while the phone
is offline.

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
- **The settle:** after any steps, every watched answer equals a fresh
  run's, every reader whose reads changed was told, none twice, and
  nothing whose reads did not change was run again.
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
| a screen's record, its `draw`, UniFFI's crossing of it | sources, bindings and sinks |
| the `local` space's rows, a view's fields | cells |
| ark, ark-peer, ark-auth, the kernel's `upstream` feature | `ark` and `ark-upstream` |
| arkui-macros, gen, ark_router's macro | `ark-gen` |
| arkui, arkui-web, glyphs, ark_app, ark-ffi, ark-module | `ark` (the view's description, the sinks' interpreter) and `ark-host-*` |

Kept as they are: the master log with its facts and hashes, the incremental
operators, file routing, arkui's primitives and generators.

## Steps

The watch comes first, not last. The new kernel is built to fit a watch
from its first line, so it never has to shrink, and the first app on it is
the smallest screen that needs every part of the design. Each step lands
on main with the apps working. Nothing is kept for compatibility.

1. **This document.** Done.
2. **The watch's facts, and the watch in CI.**
   - On a real Pebble Time 2: a Rust watchapp linked against the SDK, with
     an allocator in the app's heap, that measures how much an app can
     store, the largest stored value, whether a native app can reach a
     file API, the AppMessage sizes and throughput, and whether the
     phone's script can hold a WebSocket open. If storage is tiny, the
     design changes here, before anything is built on it.
   - In CI: the kernel built for `thumbv7m-none-eabi` at
     `opt-level = "z"` and failed past its budget; its tests run under
     QEMU for the Cortex-M3; the `.pbw` built in nix. Later, screenshot
     walks in the Pebble emulator, as app-screens does for phones.
   - **Done** in the Time 2's emulator and in CI; the real watch's
     numbers are still to measure (see "Open Questions").
3. **The new kernel, fresh and within budget.** A new crate, no_std +
   alloc: `Entry`, `Node::step`, its own codec with integer ids, the
   `Store` and `Link` traits, the incremental operators ported from
   `plan.rs`, the protocol from `peer.rs`, held to the protocol vectors
   and the fuzzer. From PR #31: an unconnected replica in place of alone
   and join modes, pending entries kept as records, facts not states,
   entry ids never renumbered, a journal of refusals only, `tracing` in
   place of recording. The old kernel runs the apps until each moves,
   then it is deleted. **Done.**
4. **Partial sync, enough for the slice:** pull-based `fetch` with
   index-backed scans and a bounded `take`; subscriptions by query, with
   base rows, per-client sync records, atomic batches and move-outs; the
   session space with its rate rules. **Done.**
5. **The watch slice: harken's now playing.** The track, its position,
   play, pause and skip, and the next few in the queue, offline and with
   the phone as its upstream. A Pebble host (its storage, AppMessage, the
   event loop driving `Node::step`), harken's phone app serving the watch
   (with the relay script as the fallback), an
   arkui renderer for Pebble's layers (text, a stack, a menu, a glyph, the
   action bar), and the `(glance)` route group. When this works offline
   and reconnecting, the kernel is proven. **Done**, on the demo
   watchapp, with `related` and its flat `join` from step 16 and the
   recorded reads of a plan's closures.
6. **Cells, the read set, watches, the settle and sinks, in the kernel.**
   A type-erased slot arena with typed `Cell<T>` handles, `Cx::get` and
   `Cx::set` journaled with the step's rows so a refusal drops both, and
   no `local` store; one read set for plan closures, cells, one-row gets
   and readers; watches the node owns, one per query and arguments
   (`Node::watch`); `Node::settle(&mut dyn Sink)`; `Plan::get`; patches
   with a depth, a move and each row's identity. Every incremental answer
   equals a fresh run's, no sink is told twice in a settle, and the walk
   visits nothing whose reads did not change. The unsafe core is one
   module, run under Miri on the host. **Done**, but for Miri, which the
   machine it was built on did not have.
7. **The watch on bindings.** The emitter writes binding tables from the
   view's paths in place of `draw(record)`; the Pebble host is the sink,
   its layers pulling at draw; the demo loses its `Face` record. Within a
   few hundred bytes of the 58.5 KB measured for it, and pixel-identical
   in the emulator. **Done.** What a layer reads is recorded by its
   reader as its source runs, not a mask the emitter writes: a source is
   a function of the node (`p.title` is `sources::title`), and only
   running it says what it reads. The view's `Sources` struct stays as
   its declaration, which the recorder walks for the snapshots and the
   phones until step 10.
8. **`#[view]` records bindings and sources.** ark_app's recorder keeps
   paths rooted at sources, and `Real` reads them; functions returning a
   plan become keyed watches, and scalar functions share one result per
   settle; the Swift, Kotlin and
   Pebble emitters and their snapshots, all from the same `#[view]`.
   **Done**, on the demo watchapp, ahead of `api` (step 12): a view's
   sources are its `let`s (`cell(C)`, `row(f(…))`, `list(f(…))`), and
   what it works out of them is a `let` over its bindings calling plain
   functions, its `text!`s' holes kept apart. The recorder keeps them
   beside the view (`Recording`); `Real` reads them through a node's
   reader (`ark_kernel::source`); the Pebble emitter writes each layer's
   binding from them, sharing what more than one layer reads within a
   settle. The phones are written from the same recording, reading the
   lets as a record's fields until step 10; harken and money keep their
   records until their screens move (steps 9 and 13).
9. **Every other host onto the new kernel:** phones, desktop, web and the
   server. The old kernel is deleted.
10. **The phones' observable bridges.** Swift Observation and Compose
    state over per-leaf getters, and lazy lists over `fetch`; screen
    records and their UniFFI encoding go.
11. **iced and the web as sinks**, on the same bindings.
12. **The API and middleware.** The routes macro reads `src/app/api/`;
    mutators move there with `Cx<Role>`; middleware replaces `Scope`;
    calls go by full path, and `api.*` answers sources.
13. **Pages.** `#[view]` structs in their route files, `#[state]` fields
    as cells and `&mut self` methods as local entries; `App`, `Msg` and
    `update` go.
14. **The codec's derive and generated ids.** `#[derive(Encode, Decode)]`
    in place of hand-written impls; the routes macro numbers tables,
    queries, handlers, cells, views and layers from the code, one id
    space, and the kernel takes only their newtypes.
15. **`ark-format` over a byte sink**, no_std: one function for the watch
    and the phones, and its FFI crossing a getter like any other.
16. **The rest of partial sync:** the indexed store on disk with partial
    loading and eviction, running aggregates synced in parts,
    completeness known locally, TTLs, preload, checksums (`related` is
    done).
17. **Reactions and the server host.** Jobs become reactions; each app's
    `server/` folds into `api/` and `on/server/`; the server host bounds
    its threads.
18. **Versions.** Frozen schemas and APIs, rule 0, lenses, the connect
    handshake that says a build is too old.
19. **Crates.** Merge and rename into `ark`, `ark-upstream`, `ark-gen` and
    the `ark-host-*` crates.
20. **money on the watch.**

An app reaches a watch through a route group for the smallest windows,
not by naming a platform:

```
src/app/
  (tabs)/          phones, tablets, desktop
  (glance)/        the smallest windows: a watch
    _layout.rs
    now-playing.rs
```

An app with no `(glance)` group has no watch app.

Three bugs on main today are fixed by these steps rather than separately:
live rows fanned out to every account (steps 4 and 12, the session space
through middleware), unbounded call threads (step 17), and calls matched
by the last part of their name (step 3, the new kernel's integer ids).

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
- A watch's storage, measured in the Time 2's emulator: 1 MiB per app in
  all, but 256 bytes per stored value, and no file API for native apps.
  The watch's store has to be paged over 256-byte values, and its writes
  (about 13 ms each in the emulator) batched. Still to measure on the real
  watch.
- Whether the kernel really shrinks 3×, and Rust on the Pebble SDK, which
  is community-grade today.
- The phone can see the watch's bytes until frames are encrypted end to
  end.
- A phone app can be a watch's upstream on Android only. Core's phone app
  supports native companions there (PebbleKit 2: the watchapp names its
  phone apps, and Core's app keeps a companion's service bound while the
  watchapp runs), but not on iOS, where PebbleKit iOS is discontinued. On
  an iPhone the watch's upstream is the relay script to the server, so it
  works offline alone but not with an offline phone.
- Sorts by a closure no index provides stay unbounded; how often the apps
  need them decides whether that matters.
