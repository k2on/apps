# A peer without a server is a peer, not a mode

`docs/arkdb.md` §3.20 says it in one line — "a peer with no server is a
database with one replica, not a client in a mode" — and the code does not
yet keep the promise. Today `Options::alone` is a mode written into the
replica file, and `Peer::open` refuses to open that file the other way
(`Error::ModeMismatch`); the authority a peer alone runs keeps no entries,
only its store, so what was done alone can never be handed to a server;
the desktop opens alone only under the demo feature; and the fleet has no
scenario that starts alone. This is the design that makes the line true.

**What "first class" means here, concretely:**

1. A peer used with no server for a year loses nothing by then being
   pointed at one: everything it sequenced alone lands on the server, in
   order, on top of whatever the server has, and every other peer sees it.
2. A peer that had a server can leave it and go on alone, and come back.
3. Alone is as durable, as bounded in memory and as fast as connected: the
   same journals, the same retention rule, the same benchmarks.
4. The desktop and the browser start alone when given no server, for real
   and not as a demo, and offer "connect" as a transition, not a reinstall.
5. The fleet exercises every one of these, including a kill in the middle
   of a transition.

## 1. The fork point, and local history

A replica remembers the **fork**: the `(log_id, cursor)` it last shared
with a server — `(None, 0)` for a peer that never had one. Everything it
sequences alone after the fork is **local history**: the alone authority's
log, from the fork's snapshot up. The two facts that make the transitions
mechanical:

- *alone → server*: local history becomes pending. The replica's confirmed
  store is rolled back to the fork's snapshot, its cursor and log id to the
  fork's, and every local entry since the fork is re-queued as a pending
  intent in sequence order (its id, actor, session, function hash, autos
  and args are all in the entry; nothing is re-drawn). Then the peer
  connects as any peer does: `Hello` names the fork's log, the server pages
  it or sends its snapshot, and pending is pushed and rebased on top. A
  server that has never seen these ids sequences them after its own; a
  local intent that now refuses is dropped with its reason, as any rebase
  does. Cost: one replay of the local history at the transition, which is
  what those intents cost the first time.
- *server → alone*: the fork is recorded as the current `(log_id, cursor)`,
  the alone authority starts from the confirmed store as its snapshot at
  that cursor, and pending is sequenced locally at once (what `open` alone
  already does). Coming back is the first transition.

The mode is therefore **not in the replica file**. `Stored` carries the
fork, the alone log's presence says which way the peer was last used, and
`Peer::open` performs a transition when `opts` says the other way rather
than refusing. `Error::ModeMismatch` goes.

**Not in this round:** adoption of the whole local log by a server that has
none of its own (`Authority::adopt` exists in-process; there is no wire for
it). Re-queuing as pending reaches the same state with the server's own
sequence numbers; adoption would preserve the peer's. It is the next step
if a household ever moves a year of history onto a new server and wants
its sequence numbers kept.

## 2. The alone log is a journal through `Storage`

The alone authority's entries are kept **on disk and not in memory**: after
each local append the entry and its facts are written to the log journal
and pruned from the in-memory `Log` (the authority keeps its store and its
id set for dedupe; it never pages anyone). The journal is the server's
shape (`log` snapshot + `log.N` pages, a page per batch of appends, the
snapshot rewritten by compaction) written through the `Storage` trait so
the browser's `Local` carries it too. Move the journal logic out of
`rust/ark-server/src/persist.rs` into `ark` (`ark::journal`, generic over a
small read/write/remove-by-key trait) and have the server's file persistence
and the client's alone log both use it; the server's on-disk format does not
change.

The alone log's **snapshot base is the fork** and never moves past it while
the peer is alone: compaction here rewrites pages into fewer pages, never
into a snapshot above the fork, because the entries are the local history a
later transition needs. Memory is bounded by the store; disk grows with the
local history, as a server's log does.

## 3. Retention, one rule for both

The server keeps its whole log in memory today and nothing advances the
horizon. The rule, in `ark` (`ark::retention`), used by the server and by
the replica's own confirmed journal:

- keep every entry above the lowest cursor of any peer heard from within
  `RETAIN_DAYS` (30) — the server records a cursor per session id at every
  `Hello` and every ack, in its state file;
- and never fewer than `RETAIN_ENTRIES` (10,000) above the head;
- compact (move the horizon: `Authority::compact`, which writes a snapshot)
  when the retained count exceeds that by half again.

A peer whose cursor falls below the horizon is served the snapshot and
rebases its pending onto it, which is the designed answer and the fleet's
scenario 5 already walks. Both constants are environment variables on the
server (`HARKEN_RETAIN_DAYS`, `HARKEN_RETAIN_ENTRIES`). The alone peer's
*confirmed* journal already compacts by size (`2a5e91e`); its authority
holds no entries; the alone *log* is §2.

## 4. What the programs do

- **`ark_client::Peer`**: `open` with either `Options`, transitioning when
  the storage was last used the other way; `Peer::join(url, login)` (alone →
  server, then connect) and `Peer::leave()` (server → alone) as the
  explicit transitions an app calls; `status` says `alone` and, after a
  join, how many local intents are still pending.
- **`harken-peer`**: `--alone` then later `--server URL` on the same
  directory is the transition; `join`/`leave` as commands.
- **The desktop and the browser**: no `--server` and no `?server=` opens
  alone in a fixed directory (`local`), for real; the status line says so; a
  "connect" entry (a URL, then the sign-in that server needs) calls `join`
  in place, and the sidebar and views carry on — the library does not blink.
  The demo feature keeps seeding, over the same alone peer. "Leave" is an
  API and not a control this round.
- **The fleet**: alone for N intents then a fresh server — the server ends
  with N entries, every hash equal; alone for N while two others have used
  the server — local history lands after theirs, all converge, and its
  playlist items sit after theirs; server → alone (`leave`), more intents,
  → server again; `kill -9` during a join (the transition is ordered: write
  the re-queued pending and the fork's replica, then remove the alone log —
  a peer killed between the two reopens with both and finishes the join);
  an alone peer killed mid-append reopens to the last whole entry; a
  2,000-intent alone history joining a server, its time reported; and the
  desktop's alone start covered by its own tests (open with no server,
  mutate, join an in-process hub, views patched not reset).

## 5. Rules

The commit rules of `docs/plan-v4.md` Part 2 apply. The transitions are
tested with the counting and kill-simulation patterns the persistence work
established; every new test is falsified once. The `rebase/` and
`protocol/` vectors are untouched: nothing here changes a frame. What a
peer alone *is* — an authority in-process — is unchanged; what changes is
that it remembers where it forked and keeps what it did.
