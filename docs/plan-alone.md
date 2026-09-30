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

## Landed

**What.** `ark::journal` (`9316419`) is the log on a key/value storage —
the server's snapshot and `{seq, entry, facts}` records, generic over a
`Keys` trait (`load`, `save`, `remove`, and `append`/`truncate` with
defaults that rewrite a record whole) — in two layouts: the server's (one
page appended to, a snapshot compaction) and a peer alone's (a page per
write, merged while the older is no larger than the newer, the snapshot
never moved). The server's `persist.rs` is that in the server layout
(`6a8b4e0`); its files, records, crash rules and messages are unchanged,
and `ark-server/tests/journal.rs` passes as it was.

`ark-client` (`4e079b6`): the `replica` record carries `fork: { log,
cursor }` where it carried `mode`; a `log` record is a peer alone. The
alone log is

```text
log      { t: "log", base: { seq: fork cursor, hash, rows: the store there,
           log: fork log }, entries: [], ids: [] }
log.N    [4-byte length, { seq, entry, facts }]…   one page per mutate,
                                                   merged into fewer
```

written through the peer's own `Storage`, so a browser's `localStorage`
carries it. `mutate` alone writes its page before it returns and the
authority's log is emptied after every append (`Log::take_entries`): a
head and the ids, no entries. `Peer::open` over either use transitions;
`Error::ModeMismatch` is gone. `Peer::join(url, Option<Login>)`:
`persist_log`, then the local history read back and taken out of the
confirmed store by its own facts newest first (`Replica::fork_back`, a
rebase by changes — a view is patched, never `Rebuilt`), every entry
re-queued as pending in order under the joining login (nobody, when
there is none), then **the write order**: the pending snapshot of the
re-queued intents, `who`, the `replica` snapshot at the fork (the
journal's pages after it removed), and only then the alone log removed —
pages newest first, `log` last — and the link. `Peer::leave()`: the link
closed, `log` created at the replica's `(log, cursor)`, the authority
started from the confirmed store, pending sequenced locally. `status()`
says `alone`, `joining` while local intents are pending on an open link,
`joining: N`, and the fork. Reopening alone over a join that stopped
halfway confirms from the history what is still pending, and does not
sequence it twice.

`harken-peer` (`7b9026c`): `--alone` then `--server` over one directory,
`join` and `leave`. The desktop and the browser (`48abacb`): no server
named opens `local` alone as nobody (`Options::alone_as_nobody`), the
status line says `alone`, and the connect entry joins in place,
remembers the server for `local`, and starts that server's sign-in. The
demo still seeds its library in memory as a peer alone, as it did; it
offers no connect.

**Tests, each falsified once.** `ark::journal` (merging, a torn page, a
merge stopped halfway, create over leftovers); `ark-client`'s
`alone_tests` (the history survives a reopen; no entry in memory after
500 appends; a torn page and a store ahead of the history; 51–250 kept
over a fork at 50; alone then a fresh hub; beside another user; leave and
join twice; a join stopped after each of its writes; a server replica
opened alone; 2,000 local intents); `harken-iced`'s
`a_window_alone_joins_a_server_in_place` (no list reset through the join);
`harken-peer`'s `alone_then_a_server_is_one_directory`; fleet scenarios
14–19.

**The fleet's timings** (a debug build, a shared VM):

| scenario | measured |
|---|---:|
| 14. 61 local intents: start with `--server`, and a second device, to converged | 1,397 ms |
| 17. a join of 400 local intents, `join` said to its answer | 426 ms |
| 17. kills at 10, 50, 90 and 99% of that | history, history, history, joined — all converged |
| 18. 300 songs said, killed 120 ms in | 27 answered, 27 kept, all on the server after the join |
| 19. 2,000 local intents: start with `--server` to converged | 6,923 ms |
| 19. …authoring them alone through `harken-peer` | 73,895 ms |

In process, two thousand local intents re-queue in 175 ms and are on a
sans-io hub and confirmed in 405 ms.

**Not verified, and for the coordinator.**

- Authoring alone in a *debug* build grows with the library — 0.5 ms
  a mutate at a hundred songs and 10 at a thousand through `harken-peer`,
  where a peer of a server is flat at 1.8 — and the same binary in release
  is flat at 1.7 ms, the same as with a server. The alone log is not it:
  without the log written, the debug numbers are the same. Most likely it
  is the debug-only checks on the path a peer alone takes on every mutate
  — the view compared with the confirmed store whole once nothing is
  pending, the record run again over the authority's store — though that
  was not profiled; it is why scenario 19 takes a minute.
- No kill in scenario 17 landed between the two writes of a join — the
  window is a few file removals wide. The unit test walks a stop after every
  write of the join; the fleet shows kills before the join and after it
  converge.
- A join rewrites the local entries' author to the joining login. A peer
  alone under one name joining as another person makes its history that
  person's; there is no question asked.
- A `replica` record written alone before the fork existed opens with its
  store as the fork at its cursor and naming no log: it has no history to
  hand a server, and a join sends a `Hello` at that cursor naming none,
  which a named server pages from there. No such directory exists outside
  a demo; untested.
- The browser's alone start, connect and the sign-in redirect back to a
  joined `local` were compiled (`nix build .#harken-web`) and not opened
  in a browser. The desktop's connect was not run in a window; the peer
  underneath it is tested.
- A view is reset on a login change whatever it reads (`ark_client::View`),
  so the sign-in after a join resets the lists as every sign-in has; the
  join itself does not. Adoption keeping the peer's own sequence numbers
  has no wire yet (§1).
- The alone peer's *confirmed* journal still compacts by size and not by
  `ark::retention` (R10's other half, not this round).
