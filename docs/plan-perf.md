# The performance pass: what grows, and what is decided about each

`rust/ark/tests/perf.rs`, `rust/ark-server/tests/perf.rs` and
`harken/domain/tests/perf.rs` (commit `612bc71`) measure every path at two
or three sizes. This document is the ranking of what they found and the
design of each fix; the code is written from it. A finding is *real growth*
when the per-operation cost rises with the data where the operation touches
a bounded amount of it, and a *constant factor* otherwise. The pass ends
when only constant factors remain and each is stated with its number.

## Round 2 — the real growth

### R1. A bounded read that fetches everything

`add_to_playlist`'s `MAX(pos) + 1` is `order_by(pos desc).limit(1)` over one
playlist's items: `view::read` fetches every candidate row through the
index on `playlist_id` and selects one — 7,999 rows examined for an eight
thousand item playlist, on each of the three applies. `add_song` does the
same twice per file over the whole `media` table (`filter(file eq).first()`
with no index on `file`; `order_by(pos desc).first()` unfiltered), and
`create_playlist` scans every playlist for `MAX(pos)` and every playlist of
every user for one user's names.

**Design.** The store serves an ordered, bounded read from an index when one
covers it. A `Secondary` is already a `BTreeMap` over the index's column
values, so its range under an equality prefix is the rows in the order of
the remaining columns. New on `Store`:

```
fn scan_ordered(&self, table, eq: &[(&str, &Value)], order: &[(&str, Dir)],
                keep: &dyn Fn(&Row) -> bool, limit: usize) -> Option<Vec<Row>>
```

`Some` when an index exists whose columns are the equality columns (any
order) followed by the order columns in sequence, all ascending or all
descending together (a descending walk is the range reversed); it walks the
range forward or backward, applies `keep`, and stops at `limit` rows, in
order. `None` when no index serves it, and the caller does what it does
today. `Overlay` merges the base's ordered answer with its own writes for
that table (the writes are few; merge, re-sort the union of `limit + writes`
rows, cut). `view::read` calls it for a bare plan whose order is by columns.

The domain declares the indexes the reads need — `playlist_item`
`(playlist_id, pos)`, `media` `(file)` and `(pos)`, `playlist` `(pos)` and
`(user_id, pos)` — in `harken/domain/src/schema.rs`. A non-unique index is
additive under `compat` and changes no row; the mutators' closure hashes do
not move (pinned by `every_mutator_hashes_as_it_did_at_spec_v3`); the module
hash does, and `harken.ark` is regenerated. Loading an older snapshot
populates the new indexes as rows are put, which is how `MemoryStore` builds
every index already.

Guard: the counting store shows `add_to_playlist` onto a playlist of 7,999
examining a bounded number of rows (say the number), `add_song` into a
library of 8,000 likewise.

Beside it, three small domain costs found in the same reads: `slug` is
quadratic in the name's length (a `concat` per character) — one pass, one
`concat`; `key_part` computes `slug` twice; `free_number` runs even when
the name is free because Native `pick` is eager — branch with `if_else`
(lazy in both modes) so a fresh name costs a `contains`, and keep the
quadratic search only for a taken name (it is a person's own playlists;
state its cost at a thousand).

### R2. Three applies per intent, and a copy per rebase

A peer alone applies each intent optimistically, again when its in-process
authority sequences it, and a third time when the acknowledgement confirms
it and is compared to the facts; a server peer applies its own twice. A
rebase (`replay`) copies the whole confirmed store and re-applies pending,
once per arriving entry while anything is pending — 124 ms per entry with a
hundred pending over eight thousand rows — and then reports `Rebuilt`, so
every open view re-hydrates: the client pays the library per entry.

**Design.** `Replica` records, beside each pending intent, the changes its
optimistic apply produced (`pending: Vec<(Entry, Vec<Change>)>`; memory
O(pending's writes)). Then:

- **Confirming an own intent without re-running it.** In `advance`, when
  the next inbox entry is the first pending intent (the whole entry, as
  `5e77a20` already requires) and nothing but own intents in order has
  landed since the view was last equal to confirmed, the recorded changes
  *are* what re-running over `confirmed` would produce, by determinism. So:
  with facts present, compare the facts to the recorded changes; equal is
  confirmed (apply the facts to `confirmed`, no run), different is
  divergence exactly as today. Without facts (a peer alone), the recorded
  changes are the facts. `debug_assert!` re-runs the closure and compares,
  so a debug build keeps the old guard on every confirm.
- **A peer alone sequences by its own record.** `local_commit` hands the
  authority the entry *and* the recorded changes; a new
  `Authority::append_as(e, facts)` appends them without running the closure
  (the authority and the replica are one process over one store — the
  second run was the first run again). `sequence_entry` is unchanged for a
  server, which must judge what it is sent. Under `debug_assert!`, the
  alone authority still runs and compares.
- **A rebase moves by changes, not by a copy, and reports them.** The view
  is a function of `confirmed` and the pending intents in order. When
  entries land while anything is pending, instead of `view = confirmed.
  clone(); re-apply pending`:
  1. undo the pending intents' recorded changes over the view in reverse
     order (each `Change` inverts exactly: `Add` → delete the key, `Remove`
     → put the row back, `Edit(old, new)` → put `old`) — the view is now
     the old confirmed store;
  2. apply the landed facts (`acc`) — the view is now the new confirmed
     store;
  3. re-run the surviving pending intents over the view through an overlay
     in order, recording each one's new changes (one that now refuses is
     dropped with its reason, as today).
  What views are told is the concatenation: the inverses, then `acc`, then
  the new changes — every one a real transition the view store made, in the
  order it made them, so `push_all` (which reconciles each touched key once
  against the final store) costs the keys touched and no view re-hydrates.
  `Changes::Rebuilt` remains for a wholesale replacement of `confirmed` (a
  snapshot adopted from the server, a schema rebuild). `sign_in` is a
  rebase of this kind too.

This changes what the specification says about a rollback. `spec/README.md`
("Requirements that are not functions"), `docs/arkdb.md` §3.6 and §3.13 and
`docs/plan-v4.md` §1.6 say a rebase tells every view `Rebuilt` because "a
rollback reports nothing" — which was true of a savepoint in SQLite and is
not true of changes a replica recorded itself. The new sentence: **a rebase
reports the transitions it made — the inverse of what it undid, what
landed, what it re-applied — and `Rebuilt` is reserved for a store replaced
whole.** The contract that a view equals a fresh hydrate is unchanged and is
what the churn tests hold; the `rebase/` vectors check state hashes at
settle and are unaffected.

Guard: a counting `MemoryStore::clone` shows zero copies across a rebase
with pending; `Peer::take_changes` after a rebase is `Applied` and the
views' contract holds through it (`rust/ark-client` and
`harken/domain/tests/views.rs` already churn through rebases — they must
now never see `Rebuilt` from a rebase, and a test asserts it); the alone
peer applies once per intent (count applies).

### R3. The server rewrites its log per push; a peer rewrites its pending per mutate

`rust/ark-server/src/persist.rs` encodes the whole log after every head
move (12.8 ms per push at two thousand entries, quadratic), and writes it
in place — a kill mid-write is a torn file the server refuses on restart.
`persist_pending` encodes every pending intent on every `mutate` (21 ms
each at eight thousand pending; the scanner authors a whole directory
before its next pump).

**Design.** Both become a snapshot and a journal, the shape `2a5e91e` gave
the client's confirmed store:

- **Server**: `log.ark-log` is the snapshot (base, entries to some sequence,
  ids), written to a temporary name and renamed; `log.ark-journal` is
  append-only, one length-prefixed canonical-CBOR record `{seq, entry,
  facts}` per appended entry, `fsync`ed once per batch of appends. Load is
  snapshot then journal in order, stopping at the first record that is
  short, does not decode, or does not carry the next sequence (a torn tail
  is dropped, and since the entry it held was never acknowledged as durable
  — the ack follows the fsync — nothing a peer was told is lost; state that
  ordering in `hub.rs` and hold it). Compact — rewrite the snapshot, truncate
  the journal — once the journal is larger than the snapshot. Retirement
  below the horizon (`compact_to`) writes a snapshot.
- **Client pending**: the `pending` record is the snapshot; `pending.N`
  pages are append-only ops, `{"t":"pending-ops","add":[entries],"drop":
  [ids]}`, one per `mutate` (an `add`) or per pump that acknowledged or
  rejected (a `drop`). Open is snapshot then pages; compaction when the
  pages outgrow the snapshot or the live set is small. The durability rule
  is unchanged: the `add` page is written before `mutate` returns, alone.

Guards: bytes written per push (server) and per mutate (client) do not grow
with the log or with pending (counting storage / a counting writer); a kill
between a journal write and the rename of a snapshot reopens to the last
whole record; the fleet's scenario 3 stops being a witness.

### R4. Small real growth, one line each

- `Log::entries_after` clones every entry after the cursor before taking a
  page: take `limit + 1` from the range, then clone.
- `Verify` at the head replays the whole log: the authority's store *is*
  the state at the head; hash it directly, and replay only for a sequence
  below the head.
- The scanner's `known_files` scans `media` once per look: with the `file`
  index, ask per candidate (`scan_where_eq("media", [("file", f)])`) and
  drop the set.

## Round 3 — constant factors, decided after round 2 lands

- **The interpreter clones a whole value to read one field, and every
  local on every bind** (`eval.rs` `Arg`/`Var` then `Field`; `Env::bind`).
  Hydrating `library` is 18 µs and 286 allocations per entry, and an
  interpreted `map`/`filter` over a large local is quadratic in it. Design:
  evaluate `Field(Var|Arg|Provided, name)` by borrowing into the bound
  value and cloning the field alone; keep locals in a `Vec<(Sym, Value)>`
  pushed and truncated by scope rather than cloned per bind. Guard: an
  allocation count per hydrated entry.
- **`Row` as `BTreeMap<String, Value>`**: two thirds of a row clone is its
  keys and map nodes. The design candidate is a positional row —
  `Rc<[Value]>` in the table's column order with the names held once on the
  `Table` — behind the same `Row` API; it is a large change that touches
  every store, the overlay, the indexes, the changes and the wire, and it is
  decided only with round 2's numbers in hand.
- **Grouped and nested views rebuild the whole group per member change**
  (`composers` 6.7 ms per Bach song at two thousand songs). Accepted in
  `docs/plan-v4.md` §1.13; a maintained aggregate (a count moved by ±1
  rather than recounted) is the design if it is ever not, and its shape is
  a projection whose every use of `members` is `len`/`fold` with an
  invertible step.
- Native `pick` is eager (a cost only where an arm is expensive; R1 removes
  the one that was); `Peer::mutate` clones the IR `Function` per call;
  writes clone the table schema; fan-out encodes one message per
  connection. Each a line or two, folded into whichever round touches the
  file.

## The record

Every fix lands with its regression guard as a countable property, its
before-and-after row from the harness, and one falsification. The
harness's tables are re-run at the end of each round and appended here.

### Round 2, R2 — an own intent run once, and a rebase by changes

**What landed.** `Replica::recorded` holds, by id, what each pending
intent's run did to the optimistic store — every transition, in order —
and is never written: an intent opened from disk is run once by the open,
which records it. In `advance`, an entry that is the first pending intent
(the whole entry) with nothing but this peer's own intents landed before it
in that advance is confirmed by its record: facts, when they came, are
compared to it, and a difference is a divergence exactly as a run's was;
without facts the record is the facts. A debug build runs the intent anyway
and asserts it produces the record. `Authority::append_as(e, facts)`
appends without running (deduped by id, never a verdict; a debug build
runs and compares), and `local_commit` uses it when the authority's head is
the replica's cursor and the intent is the first pending — so alone, an
intent is run once, when it is authored. `sequence_entry` is unchanged. A
rebase — entries landing under pending intents, a `reject`, a `sign_in` —
undoes the records newest first (`Add` → delete the key, `Remove` → put the
row, `Edit(old, new)` → put `old`, applied raw), applies what landed, and
re-runs the surviving intents through an overlay, recording them anew; a
verdict or a sign-in undoes only from the intent it touches. The view is
told the concatenation as `Changes::Applied`; `Rebuilt` is `Replica::open`
alone (and so a snapshot adopted from the server, which opens a replica),
plus the fallback when a record is missing, which only a `pending` cut from
outside produces. `spec/README.md`, `docs/arkdb.md` §3.6/§3.13,
`docs/plan-v4.md` §1.6 and `rust/ark-client/README.md` say so.

**Guards, each falsified once** (`ark/src/peer.rs` tests, and one in
`ark-client/src/view.rs`). Runs are counted per thread, as clones are,
outside the debug guards: alone, 41 intents are 41 runs, native and
interpreted (sequencing with `sequence_entry`: 82; confirming by a run: 82);
against a server, 21 own intents confirmed with and without facts run
nothing (confirming by a run: 21); three intents opened from disk run three
times and confirm with none (not recording in the open's replay: nothing
recorded, and without that assertion 3 runs to confirm). Facts that differ
from the record set `diverged`, the authority's row wins, and the pending
one behind it runs again over it (taking the record regardless: `diverged`
empty). The step test now also follows what a view is told: every change
applied to a copy of the view as it last asked must be a transition from
the row there, and the copy must reach the view; no store is copied across
a rebase (reporting only what landed and what re-ran: "theirs landed under
ours" is told an `Add` of "c" onto the key where "c" already is; rebasing
by the old replay: one copy). A verdict on the second of five intents
drops it and the item on its playlist with its reason, and a sign-in
rewrites two of three, each with no copy and told as transitions (undoing
only the refused intent's record: the view disagrees with a replay).
`ark-client`: a view of the demo's `items` through four rounds of bob
offline with one to four pending while alice's land, then reconnecting to
a sans-io server — `Patched` every time, never `Reset`, the query, a fresh
hydrate (`view::contract`) and its own splice after each (reporting a
rebase as `Rebuilt`: `Reset` at the first reconnect). The `rebase/`
vector files are unchanged and `cargo test -p ark --test vectors` is
green; its claim, and the generator's, that the three-peer rebase "is
reported as a rebuild" now says it is six transitions.

**Before and after**, `cargo test -p ark --release --test perf --
--ignored` on the shared VM, mean µs per operation; before is `peer.rs` at
`3197e8a` and after at `a32be8c`, over the same tree otherwise (R1 landed,
other agents' edits in progress), run back to back:

| row | n | before | after |
|---|---|---|---|
| (a) alone, one playlist: whole | 500 | 55.17 | 37.62 |
| (a) … `local_commit` | 500 | 39.93 | 20.77 |
| (a) alone, one playlist: whole | 2,000 | 53.05 | 30.36 |
| (a) … `local_commit` | 2,000 | 38.05 | 13.78 |
| (a) alone, one playlist: whole | 8,000 | 57.08 | 34.16 |
| (a) … `Replica::mutate` | 8,000 | 16.20 | 20.95 |
| (a) … `local_commit` | 8,000 | 40.88 | 13.21 |
| (a) alone, playlists of 10: whole | 8,000 | 57.09 | 39.31 |
| (a) … `local_commit` | 8,000 | 39.43 | 15.77 |
| (a) offline, all pending | 8,000 | 16.31 | 18.28 |
| (d) receive by intent (native) | 8,010 | 25.96 | 27.01 |
| (d) intent + facts, compared | 8,010 | 25.06 | 26.00 |
| (d) by facts alone | 8,010 | 8.87 | 10.46 |
| (e) K=10, M=256 one at a time, per entry | 500 | 926.1 | 207.4 |
| (e) K=10 one at a time | 2,000 | 3,346.1 | 209.3 |
| (e) K=10 one at a time | 8,000 | 22,260.7 | 229.1 |
| (e) K=10 one page, per entry | 8,000 | 205.8 | 23.5 |
| (e) K=100 one at a time | 500 | 2,200.4 | 1,832.6 |
| (e) K=100 one at a time | 2,000 | 5,197.3 | 1,963.3 |
| (e) K=100 one at a time | 8,000 | 23,233.5 | 2,543.7 |
| (e) K=100 one page, per entry | 8,000 | 180.7 | 42.6 |
| (e) ack K=1000 own, one at a time, per ack | 8,000 | 31.1 | 7.6 |
| (e) the harness's own `MemoryStore::clone` of that store, for scale | 8,000 | 19,999 | 14,709 (no rebase makes one now) |

What is left is constant in the store's size. Alone, the commit is the
journal, the comparison of facts to the record and the two stores moving
by them — a third of what it was; `mutate` gained the record's copy, a
few µs and within this VM's noise. A rebase costs what is pending, not
what is confirmed: one entry landing under K pending undoes and re-runs K
intents, about 20–25 µs each here, flat from 500 rows to 8,000 where it
was a 20 ms copy per entry; a page pays it once. (d) is not on R2's path —
nothing is pending during an initial sync — and did not move.

**Decided in passing.** `MemoryStore`'s `==` tells a table never written
from one whose rows were all removed, which undoing an intent that wrote a
table's first row produces; no read and no hash can see it, so the
replica's own guard compares rows (`same_rows`), and so do the tests. It
is `store.rs`'s to change if it should.

**Not verified.** `ark-server/tests/sync.rs`'s
`two_peers_sync_and_an_offline_edit_rebases_on_top` still asserts the old
`Rebuilt` and `Reset` and fails; it is `rust/ark-server`'s file, and its
new assertions are `Changes::Applied` and `Update::Patched` (the
`ark-client` test above is the same scenario sans-io). The kotlin and
swift runtimes, frozen at spec v3, still report `Rebuilt`. `harken/iced`'s
`peer.rs` comment that a rebase is `Rebuilt` is not this round's file.
Harken's own views through the domain's mutations were run by the
workspace suite and pass, but no test there asserts `Patched` through a
rebase.

### Round 2, R3 — the server's log and a peer's pending, as snapshots and journals

**What landed.** The server: `log.ark-log` is the snapshot (the file as it
always was — base, entries, ids — written to `.log.ark-log.tmp`, synced,
renamed, the directory synced); `log.ark-journal` is append-only, each
record a 4-byte big-endian length and the canonical CBOR of
`{seq, entry, facts}` (one item of the snapshot's `entries`), synced once
per batch. `persist::LogFile` is what the hub holds: `write` appends what
moved since the last write, or writes a snapshot where the horizon moved
(`compact_to`) or an append failed part-way; it compacts once the journal
is larger than the snapshot. `persist::load` reads snapshot then journal
and never writes; `LogFile::open` — the server starting — truncates a torn
tail and compacts if a stopped compaction left records the snapshot holds.
A peer: `pending` is the snapshot, now `{t: "pending", gen, entries}`, and
`pending.1`, `pending.2`, … are pages `{t: "pending-ops", gen, add, drop}`
— one per `mutate`, written before it returns and alone, and one per pump
whose answers moved the list. Compaction on a pump, never inside `mutate`:
once the pages outgrow the snapshot, and once nothing is pending (then the
empty snapshot replaces the page). `gen` is the one addition to the shape
above: pages are not idempotent — replaying an old `add` after a snapshot
that no longer holds the intent would resurrect an answered one — so a
page carries the generation of the snapshot it extends and `open` skips
older ones, which is what a compaction stopped before it removed its
pages leaves. `who` and the confirmed store's `replica`/`facts.<n>` are
untouched; a `pending` record with no `gen` reads as generation 0.

**The ordering.** `Hub::after` writes, then delivers: nothing the machine
queued in answer to a message — the `Ack`, the `Batch` fan-out, anything
else on that connection — is sent until the journal holding the entries is
synced, and a failed write holds the whole queue, in order, until a write
succeeds. It is stated in `hub.rs`'s module docs and held by
`hub::tests::an_entry_is_acknowledged_only_once_the_disk_holds_it` (a
directory where the journal goes makes the write fail; no `Ack` until it is
removed). The fleet's scenario 3b (`a_server_killed_mid_stream_loses_nothing`,
a server killed with `-9` eight times over a 1,500-entry log) passes with
`--include-ignored`; its `#[ignore = "witness: …"]` is the fleet's to lift.

**Guards, each falsified once.** Server (`ark-server/tests/journal.rs`):
bytes per append equal at 300 and 2,400 entries, and a run of appends
within five times its records (always writing a snapshot: 11,530,570 bytes
for 69,022 bytes of records at 300); the journal cut at every byte offset
of its last record reopens to the entry before, truncated there, and an
append after follows on — 230 cuts of a 230-byte record, every one whole (not truncating: 921
bytes where the good records end at 920); a temporary snapshot half
written beside the old one, a renamed snapshot beside an unemptied
journal, and that journal appended on — each reopens whole (not skipping
held records: 30, not 32); compaction and `compact_to` reopen identically
(never compacting: the journal outgrows the snapshot); an old directory
opens writing nothing (compacting on open: 4,663 bytes); a real hub
restarted over its directory has every entry (ignoring the journal: the
head is behind). Peer (`ark-client/src/persistence_tests.rs`): every
`mutate` writes exactly one key, `pending.<n>`, the same bytes at 300 and
at 2,400 pending (writing the record: the key is `pending`); reopened after
every mutate and every pump — acknowledged, refused, compacted, emptied —
the pending list is the live one (not reading pages: the first reopen is
empty); a page torn at 4 of 4 and at 2 of 4 reopens to 13 and 11 of 14,
compacted, and a stopped compaction's older-generation pages are skipped
(applying them: `[b, a]` where the live list is `[b]`); an old `pending`
record opens writing nothing (406 bytes, treated as unclean); `Dir`
round-trips, pages as files, folded on a pump (never compacting: they
stay).

**Before and after**, `cargo test -p ark-server --release --test perf --
--ignored` on the shared VM (mean µs per operation; first and last hundred;
before at `a174981` for the server and client files, the rest of the tree
as other agents had it at the time; the disk is shared with their runs, so
the directory rows are noisy and mostly the `fsync`):

| row | n | before mean | first100 → last100 | after mean | first100 → last100 |
|---|---|---|---|---|---|
| (b) one playlist, log on disk: round trip | 500 | 3,541 | 945 → 5,745 | 3,080 | 5,238 → 5,720 |
| (b) playlists of 10, log on disk | 500 | 3,437 | 1,487 → 5,824 | 4,484 | 4,434 → 4,945 |
| (b) one playlist, log on disk | 2,000 | 10,060 | 1,055 → 12,993 | 3,520 | 6,157 → 4,139 |
| (b) playlists of 10, log on disk | 2,000 | 16,651 | 2,065 → 42,080 | 4,153 | 326 → 4,285 |
| (c) offline mutate, Memory | 500 | 1,025 | 830 → 1,679 | 66 | 23 → 105 |
| (c) offline mutate, Memory | 2,000 | 3,120 | 588 → 6,218 | 24 | 20 → 27 |
| (c) offline mutate, Memory | 8,000 | 12,953 | 2,712 → 27,273 | 35 | 21 → 43 |
| (c) offline mutate, Dir | 500 | 11,848 | 11,399 → 12,240 | 7,897 | 7,839 → 7,914 |
| (c) offline mutate, Dir | 2,000 | 16,162 | 25,358 → 16,120 | 3,230 | 8,197 → 574 |
| (c) offline mutate, Dir | 8,000 | 35,648 | 13,868 → 25,288 | 2,165 | 7,746 → 6,768 |

What is left is constant: a push with the log on disk is one journal
`fsync` (and, once in a doubling, a snapshot), 3–5 ms on this disk and
flat in the log's length where it was 12.8 ms and climbing at 2,000; an
offline `mutate` on a directory is one small file written, synced, renamed
and its directory synced, whatever is pending. The Memory row is the
encoding that went: 35 µs at 8,000 pending against 13 ms.

**Not verified.** A power cut (every kill here is of a process, whose
written pages the kernel keeps; the `fsync`s are what a power cut needs,
and none was pulled); a disk that fills (a failed append holds the acks
and the next write is a snapshot, tested only by the directory-in-the-way
failure); the journal on a filesystem where `O_APPEND` after `set_len(0)`
misbehaves; a peer's pages in a browser's `localStorage` (the code is
shared, the wasm build was not run); many thousands of `pending.<n>`
files in one directory between pumps, which is what a scanner authoring
eight thousand files offline before its first pump leaves until that pump.
The server's `README.md` still says `.data(&data) // log.ark-log,
live.cbor` and `persist.rs`: one log, `log.ark-log`; it is not this
round's file to edit.

### Round 2, R1 and R4 — reads that stop at what they need

**What landed.** `Store::scan_ordered(table, eq, order, keep, limit)`,
a default method answering `None`. `MemoryStore` serves it from the first
`Secondary` whose leading columns are exactly the columns `eq` holds (in
any order), whose remaining columns are the order's next ones in
sequence under one direction (a column `eq` holds is skipped wherever the
order names it), and after which the order says only the key's remaining
columns ascending — the order a bucket's keys are in, and what §9.4
completes every order with. It walks the range under the held prefix
(from the prefix to the prefix followed by a struct, which outranks every
column value) forwards, or backwards bucket by bucket with each bucket's
keys still forwards, and stops at the `limit`-th row `keep` admits.
`Overlay` asks the base with every key it has written left out of `keep`,
adds its own writes that `keep` admits, sorts by `store::compare_rows` and
cuts: exact, at the price of the table's writes once per read, which
`scan_where_eq` already paid there. `view::read` asks it first for every
bare plan and falls back as before. `scan_where_eq` also reads through an
index whose leading columns the equalities hold (a range, the longest
such prefix, put back in key order), which is what serves one person's
playlists by `user_id`. The domain declares `playlist_item (playlist_id,
pos)`, `media (file)`, `media (pos)`, `playlist (pos)` and `playlist
(user_id, pos)`; module hash `8c678d5f…` → `536c3c13…` with no function's
hash moved. `slug` is one fold, last to first, over the characters and
one pass to spell the separators; `key_part` takes the slug once, in an
option; `playlist_name` branches with an option's `filter`/`map_or`
(`EMatch`, lazy both ways — `if_else` is lazy too but a statement, and a
helper is an expression), so a free name costs one `contains`. That moved
`add_song` and `create_playlist`'s closure hashes (module `1a2119a2…`):
`compat::check` and `check_retained` of the previous module against this
one found nothing, and a log's entries naming the old hashes keep running
the old closures. R4: `Log::entries_after` clones the page and asks the
range for one more; `Log::hash_at(n, head)` answers the head with the
held store's hash, which the server's `Verify` and a peer alone's
`verify` use; the scanner's `known_file` asks the view per file through
`media (file)` and the set is gone. And, at R2's request, `MemoryStore`
drops a table from `tables` when its last row goes, so a store that wrote
and undid a table's first row equals one that never wrote it.

**Guards, each falsified once.** `store.rs`: both walk directions with
ties on the position falling to the key, the prefix, `keep` refusing rows
before the limit, the rows examined counted (4, 1, 3 and 13 for limits
4, 1, 3 and all — walking each bucket backwards, dropping the stop, or an
unbounded range each fail it), orders no index holds answering `None`
(dropping the tail's direction check fails it), an overlay write inside
the window and two that take rows out of it (asking the base without
leaving out written keys brings the removed row back), a prefix read in
key order (without the sort it comes back by position), and a table
emptied equal to one never written. `harken/domain/tests/perf.rs`
`a_mutation_examines_the_rows_it_needs_not_the_library`, with the suite:
at 8,000, `add_song` of a new file examines 1 row and 14 gets (16,000
rows before), `add_to_playlist` onto a playlist of 7,999 1 row and 7 gets
(7,999 before), `create_playlist` 2 and 2; the same at 500. Taking the
`playlist_item` index off is 7,999 again; taking `media (file)` off is
8,001. `keys::tests` holds `slug`, `key_part` and `playlist_name` to their
old bodies over names of every shape (not collapsing a run: `a  b` is
`a--b`; numbering a free name: `Favorites (1)`). `log.rs`: a page clones
exactly itself at every cursor (the old body: 1,000 for a page of ten),
and the hash at the head copies no store (replaying: one).

**Before and after**, release builds on the shared VM, mean µs per
operation (last hundred where the row has one). The harken rows' before is
`a174981` (with other agents' work in progress in the tree) and after is
`94176d2`, which also has R2 and R3 — so a
`local_commit` column is theirs as much as this round's, and the clean
measure of R1 is `Replica::mutate` (one optimistic apply, untouched by
R2) and the rows counted. The ark and ark-server rows are one tree
(`94176d2`) built twice, before with `view::read` skipping `scan_ordered`,
`entries_after` cloning every entry and `hash_at` always replaying, run
back to back.

| row | n | before | after |
|---|---|---|---|
| add_song: whole, last 100 | 8,000 | 99,216 | 1,042 |
| … `Replica::mutate`, last 100 | 8,000 | 33,394 | 956 |
| … `local_commit`, last 100 (with R2) | 8,000 | 65,822 | 86 |
| add_song: whole, l/f | 8,000 | 21.56 | 1.23 |
| add_to_playlist, one playlist: whole, last 100 | 8,000 | 38,250 | 55 |
| … `Replica::mutate`, last 100 | 8,000 | 12,760 | 39 |
| add_to_playlist, one playlist: whole, l/f | 8,000 | 126.4 | 1.22 |
| create_playlist, distinct names, last 100 | 800 | 486,016 | 1,245 |
| create_playlist, all "Favorites", last 100 | 800 | 509,550 | 186,912 |
| `playlist_name`, a free name among 1,000 | — | ≈ a taken name's (`free_number` ran) | 195 |
| `playlist_name`, a taken name among 1,000 | — | the same path | 332,400 |
| scanner: one new file, ms | 8,000 | 15.38 (the set) | 0.0035 |
| scanner: full rescan, ms | 8,000 | 16.13 (the set) | 14.79 |
| (d) `entries_after(0)` of | 8,000 | 29,425 | 1,237 |
| (d) `entries_after(4000)` of | 8,000 | 15,299 | 660 |
| (d) the server paging a fresh connection, total ms | 8,010 | 489.6 | 38.7 |
| (d) receive by intent (native), last page | 8,010 | 1,260 | 29 |
| (g) `sequence_entry`, one playlist, last 100 | 8,000 | 7,083 | 21.8 |
| (g) fan-out, C=10, recv | 8,000 | 2,045 | 51 |
| (g) `Verify` at the head, as served | 8,000 | 52,858 | 15,054 |
| (b) one playlist, no log file: round trip, last 100 | 2,000 | 4,029 | 134 |
| (b) … `Peer::mutate` alone, last 100 | 2,000 | 1,840 | 43 |
| (b) one playlist, log on disk: round trip, last 100 | 2,000 | 4,409 | 342 |

What is left is constant, or is the answer's own size. `add_song` is a
flat millisecond, which is its dozen upserts and the interpreter (round
3). `create_playlist` still grows with one person's playlists — it reads
them all, which its `playlist_name` needs — and a taken name is
`free_number`'s quadratic, 330 ms a call at a thousand, as decided. A
`Verify` at the head is now `state_hash` of the store, 15 ms at 8,000
rows, which is the answer and not a replay. (d)'s receive rows moved
because the demo's `add_to_playlist` (whose `item` has a unique
`(playlist_id, pos)`) now reads one row per apply; the page's own cost is
flat. A full rescan is one probe per file, the same order as the set it
replaced, and a look at one new file no longer reads the table. (c) is not
on these paths (playlists of ten, and R3's persistence) and is left out;
its `Dir` rows moved between the two runs by more than anything here could
explain, which is this VM's disk.

**Not verified.** The `harken-iced` and phone clients were not run; the
module they load is the regenerated `harken.ark`, and the equivalence
tests are native (the interpreter's agreement with the new helper bodies
is `every_procedure_agrees_with_the_interpreter`). An existing installation
whose log holds `add_song` or `create_playlist` entries under the old
hashes was not replayed here: that the authority keeps and runs those
closures is §8.3's existing behaviour, and `check_retained` is the only
check made. The timings share the VM with other agents' builds and tests;
the counted rows and clones are what the guards hold.

### Round 3, R5 — the interpreter borrows, and binds on its own stack

**What landed.** `eval.rs` has a borrowing path, `eval_ref(&Expr) ->
Cow<Value>`: a literal, an argument, an auto, a local, a provided value
and a field of any of these, however deep, are answered by reference, and
`if`/`some` pass through to the arm they lead to; everything else is
computed and owned. `eval` takes that path for those six and copies only
at the end, so a value is copied where it is kept — a struct's field, a
list's element, a helper's result, a row written. `Cmp`, `match`, `let`,
`for`, the list functions' lists and a call's arguments take it too.
Locals are not a map copied per bind, and not the `Vec` the design
named: they are `Frame`s on the evaluator's own call stack, each pointing
at the one it was bound over, so a scope's locals are pushed as it binds
and gone when it returns. The reason is the list functions: an element
bound by reference into a list that is itself a local cannot be pushed
into the same `Vec` that holds the list, and a frame can. A `let` binds
for the rest of its block by running the rest over its frame. `Env` is
now `Copy` — references and a `Params` that is either a procedure's
`Args` or a helper's declared inputs beside the values the call
evaluated, borrowed where they could be, so calling a helper builds no
map and copies no row. A `NodeScope` keeps its binders in a `Vec` it
owns (`bind_ref` for one the caller keeps), under any frames.
`view::entry` binds the row by reference, builds the unprojected
default struct only for a node that does not project, moves its related
entries' deps and nodes out, and a read that keeps no entries moves its
nodes (`answer_owned`); `admits` compares each column in place. In
`store.rs`, `insert`/`upsert`/`update` borrow the table from the schema,
`well_typed` compares the column names in place each way, and
`MemoryStore`/`Overlay` answer `exists` from the key where the default
copied the row out — once per parent on every write with a reference.
`ark-client`'s `Peer::mutate` reads the mutator's IR from the domain
and copies none of it. The shadowing a map's `insert` gave is the newest
frame answering first; a helper input named twice reads the last, as
collecting the pairs into a map did.

**Guards, each falsified once** (`rust/ark/tests/allocations.rs`, an
allocator counting per thread). A `library` entry hydrated through its
plan — harken's tables, helper and query copied into the test, at the
demo's 285 tracks with a third on the playlist — allocates 55.8,
bounded at 64, which reading each field by copying the row (+150) or
binding the row by a copy (+15) would cross; making `Field` copy what it
reads before taking the field: 203.2. A `map` then a `filter` over a
local list of N text elements: 142 allocations at 100 and 446 at 400,
one an element, asserted at most three an element and linear; copying
every local on each bind, as `Env::bind` did: 20,542 and 322,046. Every
vector (`cargo test -p ark --test vectors`) and
`every_procedure_agrees_with_the_interpreter` pass unchanged.

**Before and after**, release builds on the shared VM,
`harken/domain/tests/perf.rs`: before at `ed5852b`, after at `209cbb5`
(the interpreted `create_playlist` rows at `2b4731a`, before R6's
`932a47e` changed that mutation, and run back to back with the old
`eval.rs` swapped in):

| row | n | before | after |
|---|---|---|---|
| `media.title` in a plan's expression | — | 806 ns, 16 allocs | 75 ns, 1 alloc |
| `library` read whole, per entry | 500 | 18.0 µs, 287 allocs | 4.6 µs, 55 allocs |
| `library` read whole, per entry | 2,000 | 21.4 µs, 286 allocs | 4.5 µs, 55 allocs |
| `library` read whole, per entry | 8,000 | 18.4 µs, 286 allocs | 4.6 µs, 55 allocs |
| `add_to_playlist` applied (native), allocs | 500–8,000 | 428 | 358 |
| `create_playlist`, a new name, interpreted | P=50 | 1,726 µs | 88 µs |
| `create_playlist`, a new name, interpreted | P=200 | 22,577 µs | 220 µs |
| `create_playlist`, a new name, interpreted | P=800 | 348,395 µs | 917 µs |
| `create_playlist`, a taken name, interpreted | P=800 | 795,857 µs | 164,165 µs (native: 179,288) |
| `add_song`, 80-character names, interpreted | — | 1,260 µs | 1,058 µs |

The interpreted `create_playlist` was the quadratic bind: every element of
a person's playlists bound with a copy of every local, the list among
them. It is now below its native twin; what remains of a taken name is
`free_number`'s own search, which R6 replaces.

**The row, measured.** `perf_row_share` (same file, ignored, printed)
wraps the store in one that counts every row a read hands out and every
row a change carries, by table, and splits a row's copy into its values
and the rest (media: 15 allocations, 10 of them keys and map nodes;
`playlist_item`: 7 and 6). With R5 in, the pull copies no row beyond
what the store hands out, so:

| operation | n | allocations | whole `Row` copies | `Row` keys and map nodes | the node's own names and map |
|---|---|---|---|---|---|
| `library`, whole | 285 | 15,909 (55.8 an entry) | 4,940 (31%) | 3,420 (21%) | 3,135 (20%) |
| `library`, whole | 8,000 | 437,653 (54.7 an entry) | 138,662 (32%) | 95,996 (22%) | 88,000 (20%) |
| `add_to_playlist`, native | 285–8,000 | 204 | 28 (14%) | 24 (12%) | — |

(The apply's rows: one item read for `MAX(pos)`, and the item written —
built once, copied into the procedure's overlay and into the caller's.
This `add_to_playlist` lacks harken's `owned` middleware; harken's is
358.) So a positional row with the names on the `Table` removes about a
fifth of a hydrate's allocations and an eighth of an apply's; an
`Rc<[Value]>` that a read hands out by count rather than by copy removes
about a third of a hydrate's. A fifth more is the nodes' own field names
and maps — a `Value::Struct`, which a positional *row* does not touch.
The other seven eighths of an apply are not rows: the native procedure's
context, the checked input, the plan the read builds and the change
list, not broken down here.

**Not verified.** The `library` guard holds a copy of harken's query in
`rust/ark/tests`, not harken's own module (the engine does not depend on a
domain); the two agree — 55 an entry in harken's harness at 500, 55.8
in the copy at 285. The shares are counted rows times a
measured per-row split, not a per-allocation attribution. Timings share
the VM with other agents' builds. `Std` still takes its arguments owned
(`stdlib::std(f, &[Value])`), so `len(xs)` and `first(xs)` of a local
copy the list; that is `stdlib.rs`'s signature to change and was not in
this round's files. The kotlin and swift runtimes were not touched.

### Round 3, R6 — what the fleet found

**What landed.** *A range after the held columns.* `Store::scan_where_eq`
and `scan_ordered` take `spans: &[Span]` beside the equalities — a column
held between two bounds, each `Included`, `Excluded` or `Unbounded` —
and `view::read` and `candidates` hand them down from every top-level
`Pred::Cmp` with `Ge`/`Gt`/`Le`/`Lt` (and those under a top-level `All`),
one span per column with the tightest bound of each side. `MemoryStore`
uses a span when it bounds the column right after an index's held
prefix: `lookup` scores a prefix with a bounded next column as one
column longer, and `Secondary::under` walks only `prefix ++ [lo] ..
prefix ++ [hi]` of the map (an inclusive upper bound and an exclusive
lower one end on `prefix ++ [v, struct]`, which is above every bucket
continuing `v`); crossed bounds are the empty range rather than
`BTreeMap::range`'s panic. `scan_ordered` narrows its walk the same way,
either direction. A span on any other column, or a table no index
serves, is the read it was; `keep` still decides everything. `Overlay`
passes the spans to its base and judges its own writes by `keep`.
`create_playlist` reads the person's names from `name` up to `name )`
— the name and every `name (…)`, a range of the unique `(user_id,
name)` — rather than all of theirs; `free_number` is correct over any
list holding the numbered names, so the answer is the old one. That is
one range rather than the name's `get` beside `[name (, name ))`: it
costs a handful of names that start with the name and sort before
`name )` ("name !", "name  x"), which `playlist_name` ignores. `add_song`
binds its work's name, its key, the recording and the credit once each;
emitted, a pure `let` is inlined (§6), so its closure is the same bytes
and only the helpers' first-call order had to be kept.

*A `Hello` past the head.* `Server::fanout` answers a connection whose
cursor is past the head with `SnapshotOf` of the authority's store at
the head — the below-horizon frame, no new frame or field — and the
client re-opens from it with its pending on top, which it pushed after
the `Hello`. A snapshot also keeps the verdicts the app had not yet
taken.

*Timeouts.* `ark_auth::client::{login, exchange, whoami, logout}` wait
`CONNECT_TIMEOUT` (10 s: three SYN retransmissions) for a connection and
`READ_TIMEOUT` (20 s: every one is answered from the server's memory and
session file, never a provider) on each read and write; `login_with` and
`exchange_with` take a `Patience`. The person's time at a browser is not
bounded. The native WebSocket transport holds its handshake to
`Timing::connect_timeout_ms` (5 s by default), then clears it.
`harken-peer --auth-patience-ms` bounds its sign-ins.

*Revocation.* `Auth::revoke` (what `/auth/logout` calls) tells whatever
registered with `Auth::on_revoke` `(user, session)`; `Builder::build`
registers the hub, weakly. The hub sends every replica connection of
that session `Denied` with `ark_server::REVOKED` ("signed out: this
login was revoked") through its held queue and takes it off the machine.
The token check at `Hello` is unchanged.

**Hashes.** `create_playlist`'s closure moved `904226b2…9c08` →
`2eb47c7f…f005`; `add_song`'s did not move (`1633aca2…`); module
`1a2119a2…` → `dfc028e2…`. `compat::check` of the previous `harken.ark`
against the new module found nothing, and `compat::check_retained` of
all forty-five of its closures against the new schema found nothing.
`agreement.rs` pins the new hash, naming the commit.

**The vectors.** `protocol/` pins frames' bytes and `rebase/` the state
hashes a seeded sim settles at; neither has a connection past the head,
the answer reuses `SnapshotOf`, and no vector file changed. `cargo test
-p ark --test vectors` is green.

**Guards, each falsified once.** `store.rs`: a span after the held
columns reads only its range — both bounds, each alone, inclusive and
exclusive, with and without a prefix, crossed and meeting bounds empty,
a span on another column the prefix read, no index the scan (ignoring
the span in `lookup`: 13 rows examined for 6; an inclusive upper bound
without the struct suffix: 1 for 2; no guard on crossed bounds: a
panic); an ordered read walks only its span, three rows examined each way
(walking without it: four); an overlay's writes inside, into, out of and
outside the range merge exactly (not leaving written keys out of the
base's answer: the row moved out comes back). `view.rs`: a filter's
bounds become one span per column, the tightest, exclusive at a tie (no
tie rule: inclusive). `harken/domain`: `keys::tests` holds
`playlist_name` over the siblings to the old body over a thousand names,
a hundred of them numbered with gaps (a range ending at `name (`:
"Favorites (1)"); `perf.rs` counts alice's 102nd "Favorites" among a
thousand of hers and a thousand of bob's "Favorites (k)" at 102 rows —
the last playlist, the name, its hundred siblings — where it was 1,001,
and her second playlist at 1 where it was 2, named "Favorites (101)"
(no span from `view::read`: 1,001; a range ending at `name (`: a unique
violation). `ark/tests/past_the_head.rs`: a replica at 30 meets a server
at 10, is sent `SnapshotOf` at 10, its three applicable intents are
acked 11–13 and the fourth refused with its reason, and it ends at 13
hash for hash with nothing pending (serving it nothing: no snapshot); a
verdict not yet taken survives a snapshot (not carrying them: none).
`ark-auth`: a login and an exchange into a listener that never answers
fail within a quarter-second patience (no read timeout: still waiting
after 5 s); a revocation is told to whoever asked, once (no listener
loop: nothing). `ark-client`: a handshake nobody answers closes the link
within 2 s at 300 ms of patience (no read timeout: nothing in 5).
`ark-server`: a revoked login's connection is denied with `REVOKED`, the
other login of the same person stays linked, and what the revoked one
says after is not taken (`revoked` doing nothing: linked at 10 s). The
fleet: 3c and 8, in `docs/plan-fleet.md`.

**Before and after**, release, the shared VM (`cargo test -p
harken-domain --release --test perf -- --ignored`), mean µs:

| row | n | before (R1 record) | after |
|---|---|---|---|
| create_playlist, distinct names, last 100 | 800 | 1,245 | 59 |
| create_playlist, all "Favorites", last 100 | 800 | 186,912 | 157,397 |
| rows examined, alice's 102nd "Favorites" of 1,000 | — | 1,001 | 102 |

What is left: a person whose every playlist is the same name numbered
still pays `free_number`'s square of them — 800 "Favorites (n)" is 157 ms
a call — because every one of those is a sibling; the range removed the
rest of their playlists from it, not the siblings. A fold over the
siblings in name order could be linear, but "(10)" sorts before "(2)",
so it is not a fold over the index's order; it is left as decided in R1.

**Not verified.** The kotlin and swift runtimes, frozen at spec v3, still
serve a cursor past the head nothing; the Haskell reference is not in
this tree to check. A peer whose cursor is *below* a new server's head
over a log that is not the one it confirmed (a server that lost its log
and has since sequenced more than the peer had) is served entries on top
of a state that is not theirs; nothing in the protocol names a log's
identity, so only `Verify` would notice — a log id in `Hello` is the
design if it matters. A session revoked while its peer is behind a black
hole is told nothing until the peer dials again, when `Hello` refuses
the token. The connect timeouts were not exercised against a route that
drops SYNs (loopback refuses or answers); the read timeouts were. A
browser's `web.rs` transport and ark-auth's `web.rs` have no timeouts of
their own; the browser's are the platform's.

### Round 4, and the pass closed

**What landed.** *A log's identity* (`a155a1e`). `Snapshot::log_id`,
so a log's base carries its name, `compact_to` keeps it and a page below
the horizon carries it; `Log::id` and `Log::name_if_unnamed`. The engine
has no randomness, so the hub draws the name (`Hub::new`, from
`ark_client::Autos::system`) for a log it creates or loads unnamed. On
the wire, `log`, an id, in `hello` (`Subscription::log_id`), `batch` and
`snapshot` — absent where no log is named, which is the one encoding of
that (`d6542c5`; a null is refused, so a frame has one form). The
server compares a `Hello`'s name with its log's only when both have one:
different, the connection is sent `SnapshotOf` at the head, named, once
and before anything else — wherever its cursor is, which is what makes it
the past-the-head case generalised. A client learns the name from the
first `batch` when it had none, and holds a snapshot's from it. On disk:
the server's snapshot has `base.log` (a file without it loads unnamed, is
named by the hub, and its next write is a snapshot); the client's
`replica` record has `log` (one without it opens unnamed, and learning a
name is written as a snapshot, not left for a compaction). *A verdict
once* (`21f0918`): `Replica::reject` reports only for an intent still
pending. *`stdlib` borrows* (`ded8965`): `std` is generic over
`Borrow<Value>`, so it takes `&Value`, the interpreter's `Cow` or an owned
`Value`, and copies only what it answers; `eval.rs` reads a call's
arguments with `eval_ref`, on the stack for the three arities there are,
and the form validator's `trim` borrows.

**The wire.** A frame that names no log is byte for byte what it was:
`protocol/client-hello.json`, `client-hello-facts.json`,
`server-batch.json`, `server-snapshot.json` and
`falsify/client-hello-bytes-of-v3.json` are identical to their content
at `5eb8864` (compared with `cmp` against `git show`; `a155a1e` had moved
them by writing a null, and `d6542c5` put them back). The named form is
pinned by three new files — `client-hello-named.json`,
`server-batch-named.json`, `server-snapshot-named.json` — and a new
falsify case, `falsify/server-snapshot-bytes-of-another-log.json`, built
from the named snapshot, since the existing one does not reach a
snapshot. `ir::SPEC_VERSION` did not move: a v4 runtime that never names
a log is conformant as it was, and one that names it emits the field.
Every frame from before decodes and is served as it was — an old
client's hello names no log and is paged as before, and an old client
ignores the field on a batch or a snapshot, as an old server ignores it
on a hello. `spec/README.md`'s §10 and §12 rows say so.

**Guards, each falsified once.** `ark/tests/past_the_head.rs`: a replica
confirmed to 30 of log A meets a server at 40 of log B and is sent B's
snapshot at 40 first, its three pending acknowledged at 41–43, its store
B's hash for hash and its item after B's thirty-nine (never comparing the
names: a `Batch` from 31 first, and another hash); a peer that names no
log is paged as before and learns the name from the page (ignoring the
page's `log`: still unnamed). `log.rs`: the name survives `compact_to`
and rides a page below the horizon (`snapshot_of` alone: unnamed).
`persist.rs`: the name survives appends, compaction, the horizon and a
reopen (leaving it out of `log_to_value`: unnamed); an unnamed file is
named at its next write, as a snapshot (not treating the rename as due:
unnamed on reopen). `ark-client` `peer.rs`: a peer at 20 of a server that
then names its log learns it on the next page, writes it as a snapshot,
and says it in the `Hello` after a reopen (not treating the rename as
due: the page is appended and the snapshot stays unnamed). Fleet 3c, its
other half: the log lost again, sequenced to 12 by a fourth device before
the three return at 9 — converged on 18, three runs of three, 0.8–1.0 s
restart to converged (never comparing the names: all three at 18 with a
hash that is not the log's). A verdict once:
`peer::tests::a_verdict_and_a_sign_in_rebase_by_changes` gives the
server's verdict after the replay's and keeps two rejections, and
`past_the_head.rs` asserts one intent with one reason through the
protocol (reporting before asking whether the intent is pending: three,
and the reason twice). `stdlib`: `len` and `first` of a local list cost
17 and 19 allocations at 100 elements and at 400 (copying the arguments
first: 118 and 418, 120 and 420); a `library` entry is 52.2 allocations
at 285, from 55.8 (its `first(items)` copied the items: 54.8 with the
copy put back). Every vector and `every_procedure_agrees_with_the_
interpreter` pass unchanged; the fleet suite is green twice.

**The closing measurement.** The three harnesses whole, release, one
thread (`cargo test -p <ark | ark-server | harken-domain> --release
--test perf -- --ignored --nocapture --test-threads=1`) at `ded8965` on
the shared VM, nothing else running; mean µs per operation at the two
largest sizes each row has (`l100` is the last hundred where that says
more than the mean). *Flat*: within the VM's noise over a fourfold (or
sixteenfold) size. *Logarithmic*: a few tens of percent over a fourfold
size, where the path is a B-tree's. *Constant*: a named factor, with its
number. *The answer*: the operation's result is the size of the data.
*Grows*: it grows, and the cause is said.

| row | sizes | µs per operation | category |
|---|---|---|---|
| (a) alone, one playlist: whole | 2,000 / 8,000 | 29.2 / 28.2 | flat |
| (a) … `Replica::mutate` / `local_commit` | 2,000 / 8,000 | 16.8 / 15.5; 12.4 / 12.7 | flat |
| (a) alone, playlists of 10: whole | 2,000 / 8,000 | 23.9 / 29.2 | logarithmic |
| (a) offline, all pending | 2,000 / 8,000 | 14.2 / 15.4 | flat |
| (d) receive by intent, native / interpreted | 2,010 / 8,010 | 18.0 / 23.5; 17.9 / 22.1 | logarithmic |
| (d) intent and facts, compared / facts alone | 2,010 / 8,010 | 18.5 / 25.3; 6.3 / 9.1 | logarithmic |
| (d) the server paging a fresh connection, per entry | 2,010 / 8,010 | 2.4 / 3.6 (last page 340 / 234 µs) | flat, after the first page — see below |
| (d) `entries_after(0)`, one page, measured once | 2,000 / 8,000 | 452 / 1,367 | grows cold; warm, flat — see below |
| (e) K=10 pending, M=256 one at a time, per entry | 2,000 / 8,000 | 189 / 218 | constant: K intents re-run, ≈ 20 µs each |
| (e) K=100, one at a time, per entry | 2,000 / 8,000 | 1,915 / 2,040 | constant: the same, K = 100 |
| (e) K=10 / K=100, one page, per entry | 2,000 / 8,000 | 19.5 / 22.6; 27.8 / 33.9 | logarithmic |
| (e) ack of K=1,000 own, per ack | 2,000 / 8,000 | 6.5 / 8.5 | logarithmic |
| (g) `sequence_entry` | 2,000 / 8,000 | 19.4 / 20.4 | flat |
| (g) fan-out, C=10 / C=160, per connection | 500 / 8,000 | 6.2 / 12.5; 4.4 / 7.0 | logarithmic at two sizes sixteen apart (2× and 1.6×); not separated from the cold page below |
| (g) `Verify` at the head, as served | 2,000 / 8,000 | 2,590 / 15,379 | the answer: `state_hash` of every row |
| (g) `state_at` + hash, a `Verify` below the head | 2,000 / 8,000 | 9,232 / 60,501 | the answer: a replay from the snapshot, by §10.2 |
| (h) a `Batch` of 256, encode / decode | 256 | 614 / 615 (with facts 1,036 / 1,087) | constant per entry, ≈ 2.4 µs |
| `MemoryStore::scan`, per row | 2,000 / 8,000 | 0.72 / 1.29 | grows — see below |
| (b) one playlist, no log file: round trip | 500 / 2,000 | 92.5 / 119.1 | logarithmic |
| (b) … `Peer::mutate` alone | 500 / 2,000 | 23.0 / 28.4 | logarithmic |
| (b) playlists of 10, no log file: round trip | 500 / 2,000 | 81.0 / 134.0 at `ded8965`; 120.0 / 125.2 and 133.8 / 134.3 in two runs since | flat — see below |
| (b) log on disk: round trip, one playlist / of 10 | 500 / 2,000 | 4,093 / 3,750; 4,016 / 3,476 | constant: the journal's `fsync` |
| (b) a watcher receiving, nothing pending | 500 / 2,000 | 32.1 / 34.8 | flat |
| (c) offline, Memory: mutate | 2,000 / 8,000 | 20.3 / 32.5 at `ded8965`; 18.6 / 25.6 at `bdff718` | flat in the work since `bdff718` — see below |
| (c) offline, Dir: mutate | 2,000 / 8,000 | 6,662 / 5,807 | constant: one file written and synced |
| (c) alone, Memory: mutate / pump | 2,000 / 8,000 | 25.5 / 39.5; 6.4 / 9.2 (27.9 / 35.6; 6.9 / 8.5 at `bdff718`) | the same instructions, a bigger heap — see below; pump logarithmic |
| (c) alone, Dir: mutate / pump | 2,000 / 8,000 | 133 / 116; 9,767 / 6,476 | constant: the `fsync`s |
| harken `add_song`: whole | 2,000 / 8,000 | 363 / 378 | flat (its dozen upserts, and the interpreter) |
| … `Replica::mutate` / `local_commit` | 2,000 / 8,000 | 283 / 290; 80 / 88 | flat |
| harken `add_to_playlist`, one playlist: whole | 2,000 / 8,000 | 45.6 / 52.3 | logarithmic |
| … `Replica::mutate` / `local_commit` | 2,000 / 8,000 | 31.8 / 35.4; 13.8 / 16.9 | logarithmic |
| harken `create_playlist`, distinct names | P=200 / 800 | 56.8 / 63.8 | flat |
| harken `create_playlist`, all "Favorites" | P=200 / 800 | 4,212 / 61,073 (l100 7,261 / 157,158) | grows: `free_number`'s square of the siblings, accepted (Round 4) |
| `playlist_name`, a taken name / a free one | P=100 / 1,000 | 3,282 / 274,809; 28 / 195 | the same square; a free name is linear in the list it is given, which the read bounds to the siblings |
| harken views, `library`: a toggle | 500 / 2,000 songs | 14.0 / 19.4 | logarithmic |
| harken views, add a Bach song: `albums` / `artists` / `composers` / `works` | 500 / 2,000 songs | 85 / 340; 203 / 833; 813 / 3,121; 47 / 114 | grows with the group: the whole-group rebuild, accepted (`docs/plan-v4.md` §1.13) |
| harken views, hydrate per row: `library` / `track_details` | 500 / 2,000 songs | 4.7 / 5.4; 12.5 / 16.6 | the answer; per row logarithmic |
| `library` read whole, per entry | 2,000 / 8,000 | 4.2 / 4.3 µs, 51 / 51 allocations | flat |
| `add_to_playlist` applied, allocations | 2,000 / 8,000 | 358 / 358 | flat |
| rows a mutation examines (four mutations) | 500 / 8,000 | identical: 14g 1r, 7g 0r, 7g 1r, 2g 1r, 2g 102r | flat |
| scanner: one new file / a full rescan, per file | 2,000 / 8,000 | 2.0 / 3.0; 1.54 / 1.72 | flat (a rescan is the answer: one probe a file) |
| an apply, native against interpreted | — | `create_playlist` 47–73 / 27–33; `add_song` 241–1,000 / 372–980 | constant |

**What still grew, plainly, and what it was.** Five rows grew at
`ded8965`; none is fitted into a category:

- *`(c)` offline, Memory: `mutate`*, 20.3 µs at 2,000 pending to 32.5 at
  8,000: real, linear in what is pending, about 2 ns an intent.
  `ark_client::Peer::persist_pending` decided that `mutate` only added at
  the end by comparing the whole list it last wrote with the pending list,
  id by id, on every `mutate` and every pump. **Fixed** (`bdff718`): the
  pending list changes only by intents leaving, the rest in order, and new
  ones joining at the end, so the last intent written is still in its
  place exactly when nothing before it left — one comparison, whatever is
  pending (a debug build still compares the lists whole). Held by
  `deciding_what_a_mutate_writes_compares_one_id`: one id compared by a
  mutate at 300 pending and at 2,400, and one by a pump that moved nothing
  (the old decision: 300 and 301). The row re-measured 18.6 and 25.6 µs;
  counted with callgrind over the same loop (a probe of the harness's
  shape, the loop's instructions less the setup's), a mutate is 162,900
  instructions at 2,000 pending and 166,500 at 8,000 — flat — and timed
  alone in that probe 23.6–27.6 and 26.3–28.5 µs over three runs each. The
  harness row is the VM's.
- *`(c)` alone, Memory: `mutate`*, 25.5 to 39.5 µs (27.9 to 35.6 after
  `bdff718`, and 33.7–35.5 to 44.5–48.0 in the probe): not the
  comparison, which alone has nothing pending across a pump to compare.
  Callgrind over a mutate and its pump: 225,900 instructions at 2,000 and
  220,100 at 8,000. The work is flat; what grows is the time the same
  instructions take over a heap four times the size — the authority's
  log keeping every entry and its facts, and the stores it and the replica
  hold — which is the memory hierarchy, as the scan's below. Reported,
  not changed.
- *`(b)` playlists of ten, no log file: the round trip*, 81 to 134 at 500
  and 2,000 in the closing run; 120.0 to 125.2 and 133.8 to 134.3 in the
  two runs since, beside a one-playlist row of 129–145 at both sizes. Flat;
  the closing run's 81 at 500 was the VM, and the persist fix is not what
  moved it — this peer is linked and pumped, so it had one or two pending
  to compare.
- *`MemoryStore::scan`, per row*, 0.72 to 1.29 µs at 2,000 and 8,000. A
  walk of the table and a copy a row, the same allocations a row whatever
  the table's size; a probe of the same shape measured 132, 375, 418 and
  589 ns a row at 500, 2,000, 8,000 and 32,000 rows, which is the memory
  hierarchy under a table that no longer fits in cache, not the walk.
- *The first page to a fresh connection*, measured once and cold:
  `entries_after(0)` 452 µs at 2,000 and 1,367 at 8,000, and the server's
  first page 1,252 and 7,405 µs where its last is 340 and 234. A probe
  that pages the same log fifty times measured 155, 155, 162 and 153 µs a
  warm page at 500, 2,000, 8,000 and 32,000 entries, and 557–770 µs cold
  at every size: the harness's single cold shot of a page whose entries
  were allocated long before. Flat as an operation; the harness does not
  show it.

Accepted and named, with their numbers: the whole-group rebuild of a
grouped or nested view (`composers` 3.1 ms per Bach song at 2,000 songs,
§1.13); `free_number`'s square of one person's numbered siblings (61 ms a
call on average, 157 ms at the last hundred, at 800); the row
representation (a fifth of a hydrate's allocations and an eighth of an
apply's, R5 — deferred); a rebase's K intents re-run per landing entry
(≈ 20 µs each); the `fsync`s of a directory (3.5–10 ms here, the disk's);
the memory hierarchy under a larger heap, which moves the time of
instructions that do not grow (the scan, a peer alone's `mutate`, a cold
page). A `Verify` and a replay are their answers. Everything else is flat
or logarithmic, and the one row that grew with the data in its work is
fixed: **the pass is closed.** `authoring/cx.rs`'s `std_op` passes its
arguments to `std` as they are too (`0e384ef`).

**Not verified.** The kotlin and swift runtimes, frozen at spec v3,
neither send nor read a log's name, and are served as before. An old
client against a new server and a new client against an old one are
argued from the decoders — both ignore a field they do not know, and
`log_of` reads an absent one as null — and tested only as a frame
without the field; no old binary was run against a new one. A data
directory from before this round was not upgraded by a real server: the
path is `persist.rs`'s test, which names an unnamed file at its next
write. The two gaps `persist.rs`'s docs state — a new log's name sent in
a snapshot at 0 before anything was appended, and a compaction failing
after its append — cost a peer one needless re-base after a restart and
are not tested. The browser's storage (`Local`) carries the name through
the same code; the wasm build was not run. Timings share the VM.

### Round 5, R10 (server) — the log kept to what somebody may ask for

**What landed.** `ark::retention::compact_to(head, horizon, Retention {
entries, days }, now_ms, [(cursor, heard_ms)])` → `Option<Seq>`: the
horizon moves to the lower of `head − RETAIN_ENTRIES` and the lowest
cursor of a session heard within `RETAIN_DAYS` (a cursor already below the
horizon pins nothing — that peer is served the snapshot whatever is kept),
and only when the log holds more than half again as much as it keeps.
*Half again of what is kept*, not of the constant: with every peer caught
up the two are the same 15,000 `docs/plan-alone.md` §3 states, and with a
session holding the log low the constant would make every step it
advanced a compaction rewriting everything held; relative to what is
kept, each compaction drops at least a third, so the snapshot bytes
written stay within twice the entries dropped. Defaults 10,000 and 30,
from `HARKEN_RETAIN_ENTRIES` and `HARKEN_RETAIN_DAYS` (and the NixOS
module's `retainEntries`/`retainDays`), `Builder::retain` in `ark-server`.

The hub records a cursor per `(user, session)` — not the session id
alone, since dev auth gives every login `dev` — in `cursors.cbor` beside
`live.cbor` (`ark_server::retain`): at every `Hello`, the cursor it names
(unless it names another log); at every page or snapshot delivered, where
the connection stood before it (a page's start, a snapshot's sequence).
No frame acknowledges a page, and the `Ack` answers pushes, so the
delivery is the plan's "ack", and recording the page's *start* keeps a
page in flight — and any `NeedFacts` about it — above the horizon. The
time heard moves with every frame and at a close; a session with a
connection open is heard now. The file is written by a rename, unsynced,
when a session arrives or leaves, when the horizon moves, at most once a
second otherwise, and when the hub stops: nothing correct rests on it (a
stale one keeps more, or serves a snapshot a page would have done). After
every message the hub asks the rule; `Authority::compact` moves the
horizon in memory — in place now, moving the kept entries instead of
cloning them and taking the head's state from its own store — and the
write that follows is the journal's own compaction (a snapshot at the new
horizon; `LogFile::write` already wrote one when the horizon moved), made,
like every write, before anything queued is sent.

**A finding, fixed in the machine.** A peer below the horizon with
nothing pending was sent the snapshot and then *nothing*: the machine
sent a connection one message per turn, and after `SnapshotOf` the client
has no `has_more` to say `Hello` again with — it sat at the horizon of a
quiet server until somebody else spoke. `Server::fanout` now follows a
snapshot below the head with the first page in the same turn
(`ark/tests/below_the_horizon.rs`); frame forms are unchanged, and the
`protocol/` vectors with them. (A first version patched it in the hub
with an empty `Push` turn; that is gone.) `converged` in the fleet reads
accepted ids from `log.ids` as well as `log.entries`, so an intent
compacted below the horizon is accounted for without the scenario's help.

**Measured** (`hub::tests::a_caught_up_hub_holds_retain_entries_and_serves_the_snapshot`,
debug build, the demo's `create_playlist` in thirty offline bursts of a
thousand, every burst paged back to its author before the next):

| at 30,000 sequenced, every peer caught up | before (never compacted) | after (R10) |
|---|---:|---:|
| entries in memory (`log.entries.len()`) | 30,000 | **12,000** |
| horizon | 0 | 18,000 |
| ids in memory (`log.ids`, kept below the horizon, §10.3) | 30,000 | 30,000 |
| snapshot on disk | 3,854,564 B | 3,991,846 B |
| journal on disk | 3,465,000 B | 462,000 B |
| time to sequence and page back | 9.0 s | 10.4 s |

The horizon moved at 16,000, 22,000 and 28,000 — each time the log held
more than 15,000 — to 10,000 below the head; two bursts since, so 12,000
are held, and never more than 15,000. A fresh peer at 0 is served
`SnapshotOf` at 18,000 and pages to 30,000 with the same state hash; a
hub started again over the directory has the same log and the same
cursors. The snapshot is about the same size either way because it is
mostly the *state* (30,000 playlists), which retention does not touch;
the journal is what stopped growing. The fleet's scenario 13 (below)
measured 705 ms for two peers to author 200 entries on a server keeping
fifty, and 188 ms for the black-holed third to return below the horizon,
take the snapshot, rebase its ten, push them and be confirmed.

**Tests, each falsified once.** The rule at its edges
(`ark::retention::tests`): the floor and the half-again threshold, a
session inside the window (the edge in, one millisecond past it out),
never fewer than the floor, a slow session not compacting every step, a
cursor below the horizon pinning nothing. The hub: 30,000 as above; a
session written into `cursors.cbor` as heard yesterday at 100 keeps every
entry above 100, and heard thirty-one days ago lets the log go to its
floor; the restart. `compacting_in_place_is_compacting` holds
`Authority::compact` to `Log::compact_to`. Fleet scenario 13: a server with
`HARKEN_RETAIN_ENTRIES=50` and `HARKEN_RETAIN_DAYS=0`, three peers, one
black-holed through two hundred entries, compacted past it, served the
snapshot on return, its ten landing after the two hundred, converged;
scenario 5 unchanged and green.

**Not verified, and for the coordinator.** A *sent* page is not a
*received* one: a black-holed connection's recorded place runs ahead of
what it holds, and it is then served the snapshot where a page might have
done — right, and one message more. The replica's own confirmed journal
and the alone peer do not use the rule yet (the other half of R10). The
ids map is kept whole below the horizon by design (§10.3) and is now the
log's one unbounded structure: 30,000 ids at 30,000 entries. Timings are
a debug build on a shared VM; the release build was not measured.

### Round 5, R8 — a rebase once per pump

**What landed** (`219195a`). `Replica::receive`, `receive_with` and
`receive_facts` place what arrived in the inbox and nothing else; `ack`
goes through `receive`, so it only places too. `Replica::settle` is the
one advance — what `retry` was, renamed now that it is the only way the
inbox moves — and costs a lookup when nothing is next. `receive_batch`
is a page handed in whole: placed, then settled, as before. `Client::recv`
no longer advances: a `Batch` places its entries, `FactsFor`, `Ack` and
`Closures` place or hold, and `Client::settle` applies the inbox once and
then says what a page left to say — `NeedFacts` for what still waits, and
the `Hello` for the next page, at the cursor the page moved the replica
to (said at the frame, before the page is applied, it named the old
cursor and would be sent the same page again). A `SnapshotOf` settles
what earlier frames of the pump placed before it replaces the replica, so
everything before it is what it was when each frame advanced.

**Where the one settle lives.** In the driver, because only the driver
knows where a pump ends: the sans-io `Client` gets its frames one at a
time and is told. `ark_client::Peer::pump` places every frame its link
polled and calls `Client::settle` once, before it sends and persists;
`Peer::recv` and `recv_frame`, the entry points for a transport of the
caller's own, are a pump of one frame and settle after it (which keeps
the hub's, the view's and the persistence tests' one-frame exchanges
exactly as they were). `Sim` settles after each step's delivery (once
when the network duplicated the frame) and after a drain's frames to each
client, and flushes what the client says only then. `local_commit`
settles per intent: the next intent is sequenced by its record only once
the one before is confirmed, so alone an intent is still run once. The
tests that relied on the implicit advance settle where it was; the
`rebase/three-peers` script settles per landing in step 3, because its
six transitions are two rebases — one settle over both entries is one
rebase and four. The vectors regenerated from the changed generator are
byte-identical to the tree's (`diff -r`), fleet transcripts included.

**Guards, each falsified once.** `peer::tests::fifty_pushes_in_one_pump_
rerun_the_pending_once`: K = 100 intents pending, fifty of another
peer's entries each its own `Batch` through `Client::recv`, then one
`Client::settle` — the frames run nothing, the settle runs 150 (each
landing entry once, the hundred once), where a settle per frame runs
5,050; the view is the replay's, told as transitions, and the same as the
per-frame client's (settling inside `Client::recv`: 5,050 at the first
assertion). `ark-client` `peer::tests::a_pump_is_one_rebase_however_
many_frames_it_polled`: the same shape through a real `Server` and a
`Queues` link, fifty frames polled by one `Peer::pump` — the view is told
250 transitions (a hundred undone, fifty landed, a hundred again), where a
settle per frame tells 10,050 (settling in `place`: 10,050). Unchanged
and green: every `ark` vector test, `past_the_head`, `demo_authoring`,
the churn suites (`ark` and `harken-domain` `views`, `converge`),
`ark-server`'s `sync`, `journal`, `live` and `bench`, and
`harken-server`'s `fleet`.

**The row.** `cargo test -p ark --release --test perf perf_e_rebase --
--ignored --nocapture --test-threads=1`; before at `f5ab5ac`, after at
`219195a` in two runs, µs per entry, M = 256 landing. Before, "one entry
at a time" advanced per `receive_with`; after, the harness says which
pump shape it measures — a settle per entry (a trickle, one push a pump)
or all 256 placed singly and settled once (a burst, one pump).

| row | S = 500 / 2,000 / 8,000, before | after, a pump each | after, one pump | one page, after |
|---|---|---|---|---|
| (e) K=10, one at a time | 185 / 196 / 197 | 189–202 / 196–198 / 201–204 | 22.4–23.7 / 22.7–23.1 / 22.9–25.7 | 21–22 / 20–21 / 22 |
| (e) K=100, one at a time | 1,704 / 1,761 / 1,940 | 1,684–1,780 / 1,783–1,858 / 2,586–2,838 | 29.1–29.6 / 28.4–30.8 / 29.5–30.8 | 24–25 / 27–29 / 31–32 |

A burst in one pump now costs what a page costs — constant in K up to the
one re-run of the pending, 30 µs an entry at K = 100 where it was 1,900.
A trickle, one push per pump, costs what it did: K re-runs per pump is
the rebase, and is what `docs/plan-perf.md` accepted in Round 4 at ≈ 20 µs
an intent. The after runs of K = 100 a pump each at 8,000 (2,586 and
2,838) are above the before (1,940); the work is the same — the guard
counts the runs, and `settle` is the advance the before made — and the
runs were taken while two other agents compiled and tested in the same
VM, so that cell is the VM's, not a regression, and is worth re-taking
on a quiet machine.

**Not verified.** The pump's one settle is exercised through `Queues`
and the in-process hub, not a real socket; the browser's `Web` link goes
through the same `pump` and was not run. A pump that polls both frames
and a close settles before the close is noted, so a `Hello` a page
deferred is queued and then dropped with the connection, as the one said
at the frame was; that order is argued from the code, not tested. The kotlin and swift runtimes still advance per frame;
nothing about the wire changed, so they are served as before.

### Round 5, R9 — a count or a sum kept as a number

**What landed** (`e41de7a`, `a0cf539`, `2b9e99b`). The aggregate is
recognised, not declared. At hydrate `view::shape` reads the plan and the
closure's helpers and finds, per node, each related list — and a group's
`members` — whose every use in the `having`, the projection and the
expression order keys is `len(list)` (`Agg::Count`) or `fold(list, init,
|acc, x| acc + f(x))` with `f` mentioning nothing but `x`
(`Agg::Sum { x, f }`); a helper whose whole body is one of the two over its
one parameter is the same use, which is how harken's `total` is seen
through. `is_empty()` is `len() == 0` in the vocabulary, so it is a count.
A related list is *kept* when its uses are all aggregates, its child plan
is all numbers — a table source, no lookups, no limit, every related plan
of its own kept — and its parent is the root or kept; anything else is a
list, and so is everything beneath it. The expressions are rewritten once
(`Face`): a count becomes a slot, a sum `init + slot`, a slot being a
negative symbol no plan binds. Nothing travels: `Shape` lives on the
`View` beside the plan, so no IR, frame, closure hash or vector moved
(`cargo test -p ark --test vectors` and `spec/vectors/views/*` unchanged;
the views vectors' patches are what they were).

An entry keeps, per `(node, dependency)` of a kept plan, a `Held`: the
numbers (a function of the dependency alone, since a child plan sees
nothing of its parent but its `on`), the child nodes with their rows when
the child plan has kept lists of its own, and which parent nodes read it.
Its keys are the entry's dependencies and are indexed in `by_dep` as
`deps` are. `push_all` routes as before; a hit on a kept node moves the
numbers there by the change's rows — `−terms(old) + terms(new)`, the old
and new rows `touched` already sees, read from the change and not the
store — or, for a child plan with lists of its own, reads the named child
rows again by key and builds those nodes over the numbers beneath them.
Then deepest first (a node's id is below every id under it, §1.8's
pre-order) each number that moved re-evaluates the child nodes that read
it from their kept rows and moves its own parent by the difference, up to
the entry, whose node is evaluated again over the numbers; a dependency
nobody reads any more is let go, with what it held. A dependency not held
yet is pulled from the store once, which is exact because the store is
already final and no change was routed to it. A group's kept members are
counted from the kept keys and summed by the changes' terms; a touched
group's keys are an overlay of arrivals and departures over the kept set
rather than a copy of it (`a0cf539` — the copy kept `artists` linear in
Bach's group after its count was kept). Any other use of a list is the
rebuild it was, over the same store; the contract is unchanged and is what
the churn suites hold.

**Which of harken's projections qualified.** Kept: `composers` at all
three depths (the works counted and summed, each work's movements summed,
each movement's songs counted); `works` and `work` (`recordings.len()`,
`total(movements)` over counted songs); `artists` (`media.len()`);
`recordings`' songs (`songs.len()` in the projection and the order key)
beside its credits, which are a list (`performers(credits, …)` filters
them); `credits`' `real` (`is_empty()`); `playlists_of`'s items
(`is_empty()`). Not kept: `albums` — its creators are read for their
`first`, and its child plan has a lookup; `library`, `album`, `artist`,
`recording`, `playlist` read their items' `first`; `track_details` hands
its credits to `performers`. A child plan with a lookup is never kept: a
change to the looked-up table would have to find the child node it moves,
which `by_dep` does not say.

**Guards, each falsified once** (`rust/ark/tests/aggregates.rs`).
`a_song_costs_composers_its_path`: harken's `composers`, node for node,
over a library with Bach's fifty works of ten movements and a quarter of
the songs under them — a song added under an existing movement costs one
`get` (Bach's row, which the node is evaluated over) and no row, at 500
songs and at 2,000; a new movement and its first song in one batch, two
`get`s and one row (the movement, Bach, the one song under the movement
through the index); taking the song away, one `get`. Each is one `Update`
and the view a fresh hydrate. With nothing kept (`face` answering `None`
for every child plan, which is the rebuild as before) the same three
pushes read 676, 678 and 677 rows at 500 songs and 1,051, 1,053 and
1,052 at 2,000 — Bach's works, movements and songs. `a_composer_
described_reads_one_row`: Bach's own row edited (harken's
`describe_person`) is one `get` and no row, where the rebuild read 675 at
500. `a_song_costs_its_group_one_row`: a song into the largest group of a
group source whose members are counted and summed is one `get` (the
lookup from the key) at both sizes, where members kept as a list read 127
and 502 — every member again. What is recognised:
`composers_is_kept_three_deep` (refusing a helper as an aggregate: the
works are a list and nothing is kept), `what_is_kept_and_what_is_not` (a
list mapped at the root, a list's `first` two deep, one list kept beside
one that is not; falsified by keeping a child plan with an unkept list),
`only_a_sum_of_the_element_is_a_sum` (`acc + acc`, `acc * x` and a step
over the parent's row are not sums; falsified by dropping the
free-variable test). The contract under churn, the kept and the broken
shape alike: `composers`, a sum in an expression order key under a limit,
a group counted and summed under a having, and `composers_mapped` (the
works mapped at the root), `composers_first` (a movement reads its songs'
`first`) and `mixed` (one list kept, one not) — falsified by never
letting go of a re-read child's old dependencies, by skipping the
re-evaluation of a child whose numbers moved, and by not subtracting a
row that leaves its group, each "the view is not a fresh hydrate" within
the first steps. Green unchanged: `rust/ark/tests/views.rs`,
`plans.rs`, `toggle.rs` (the library toggle still two reads),
`vectors.rs`, `harken/domain/tests/views.rs`, and the workspace.

**The rows.** Before at `df66ab2`, after at `a0cf539` (the harness row)
and `2b9e99b`'s `view.rs` over `df66ab2` (`bench_views`), release, one
thread, the shared VM with other agents building.

`cargo test -p harken-domain --release --test perf perf_f_views --
--ignored --nocapture --test-threads=1`, µs per push (median of five),
and the hydrate in ms:

| query | add a Bach song, 500 / 2,000 songs, before | after | hydrate 500 / 2,000, before | after |
|---|---|---|---|---|
| `albums` | 90.5 / 316.5 | 86.0 / 372.4 | 3.17 / 10.85 | 2.98 / 11.17 |
| `artists` | 201.9 / 916.5 | 10.0 / 12.6 | 2.09 / 9.60 | 2.26 / 10.36 |
| `composers` | 845.4 / 3,128.4 | 35.1 / 42.3 | 0.98 / 3.76 | 1.75 / 4.54 |
| `works` | 47.9 / 113.7 | 25.4 / 32.6 | 1.53 / 4.79 | 2.08 / 6.66 |

`cargo test -p harken-iced --features demo --release -- --ignored
--nocapture --test-threads=1 bench_views` — the desktop's views through
`ark_client::View`, a playlist toggle and Handel described
(`describe_person`), medians of 21 warm rounds, at 1× and 4× the demo:

| query | describe, 1× / 4×, before | after | toggle 1× / 4×, before | after | hydrate 4×, before / after |
|---|---|---|---|---|---|
| `artists` | 109.9 / 200.7 µs | 12.9 / 19.4 µs | 0.23 / 0.36 µs | 0.45 / 0.54 µs | 2.3 / 2.3 ms |
| `composers` | 319.3 / 333.6 µs | 11.3 / 14.2 µs | 0.30 / 0.59 µs | 0.89 / 1.1 µs | 4.8 / 6.6 ms |
| `works` | 0.42 / 0.50 µs | 0.77 / 0.94 µs | 0.43 / 0.49 µs | 0.89 / 1.0 µs | 0.45 / 0.96 ms |
| `playlists_of` | 0.27 / 0.30 µs | 0.57 / 0.74 µs | 5.7 / 6.0 µs | 9.5 / 10.5 µs | 0.06 / 0.07 ms |
| `albums` | 0.30 / 0.52 µs | 0.53 / 0.72 µs | 0.38 / 0.56 µs | 0.57 / 0.80 µs | 3.0 / 3.8 ms |

Handel described was his composer's whole tree and his group's every row
read again; it is now his row and the numbers kept. A push that touches
nothing costs half a microsecond more than it did — the per-push context
(`Cx`: the plan's nodes by id, the kept plans' filters evaluated) is
built before the changes are looked at — which is a constant, and where a
view is touched it is repaid many times over. `playlists_of`'s toggle
is one hit swept: 94 µs in the first `a0cf539` run, which was glibc
consolidating the bench's freed scans on the sweep's first allocation of
over a kilobyte (a `Vec::with_capacity(4096)` dropped before the push
took it to 7.6 µs); `2b9e99b` boxes the sweep's entries under that size,
and what remains is the sweep's own few microseconds.

`albums` is not kept and did not move (its two runs are within the VM).
`composers` and `works` still rise a little from 500 to 2,000 songs: the
harness's song is always a new movement, whose songs are pulled once
through the index, and the pushes are tens of microseconds on a shared VM;
the reads the guard counts do not grow. A hydrate of a kept view costs
more — it keeps each kept child node's row — `composers` 3.76 to 4.54 ms
and `works` 4.79 to 6.66 ms at 2,000; that is the price of never pulling
the list again.

**Not verified, and decided.** A sum kept is checked as a fold is, but a
fold refuses when a prefix of its terms overflows and a kept sum only when
its running total does: the two differ only for terms of both signs near
`i64`'s ends, which no harken projection has. An `Err` from `push_all`
now leaves the view stale with some entries' numbers possibly moved
(before, it left the view as it was, also stale); `ark_client::View`
re-hydrates on any `Err`, which is the only caller. Recognition is at
hydrate, in `view.rs`, not in the verifier: it needs the closure's
helpers, and the verifier's typing is what makes the fold's `+` an
integer one. `docs/plan-v4.md` §1.5's "A group source rebuilds a whole
group per member change" is now true only of unkept members; §1.13 says
so. The kotlin and swift runtimes, frozen at v3, have no views of this
kind.

### Round 5, R11 — the row is positional

**What landed.** `ark::store::Row` is a positional record: the values in
the table's column order behind one `Arc<[Value]>`, beside an
`Arc<Columns>` — the names, held once on the `Table` (`Table::new` lays
them out, with the key's positions) and shared by every row of it. The
row carries its names rather than the store supplying them: rows are read
far from any store (a view binding one, a change on the wire, a test
comparing two), and each would otherwise need the table threaded to it;
`store.rs`'s module docs say so. `Arc`, not the `Rc` this item named,
because a row crosses threads — the hub's `ServerMsg` carries facts to the
socket tasks and `HubHandle::rows` answers another thread. The API keeps
what callers used: `get` by name (the names walked, the length first, up
to sixteen columns; the sorted positions searched past that), `row["c"]`,
iteration in column order, `insert` and `with`, construction from pairs
(`Row::of(tbl, …)` lays them out, a missing nullable column `Null`), equality
by columns whatever the order, `Debug` as the map it was. A row built
before any table laid it out — decoded without a schema, a vector's, a
test's pairs, or naming a column its table lacks or leaving out one that
is not nullable — keeps its own names in name order (shared per shape
through a small per-thread cache), and a store lays it out as its table's
when it is applied (`stored`: a pointer or a name comparison, and a
permutation only when the order differs), or refuses it at `put` with the
message it always had (`complete`, `well_typed`). A raw fact that is not
the table's shape is kept as it came, as §4.5 applies facts. The store
hands a row out by reference count: `scan`, `get`, the overlay and
`Change` copy no row. The evaluator binds a row as the store holds it —
`eval_val` answers a `Val` that may be a row (a plan's row, a lookup's,
`get`'s, `update`'s old row, a helper's argument), a field of one is read
by position, and only a row used whole is built into its struct. The wire
(`change_value` writes `to_value()`), the state hash, the indexes'
keys, `Change`'s shape and the overlay's meaning did not move: every
vector passes byte for byte and `spec/` is untouched.

**Guards.** `a_library_entry_hydrates_in_a_bounded_number_of_allocations`
(`rust/ark/tests/allocations.rs`): 37.2 allocations an entry at 285, from
52.2; bounded at 44, and falsified by binding the plan's row as its struct
in `view.rs` (`Cand::bind`: 52.2, the number before, exactly) and by
making `Field` copy what it reads (194.8). `toggle.rs` is unchanged — a
toggle reads the same rows at 200 as at 3,200, as before. The fleet
suite is green (22, 37 s), with the workspace, `harken-iced --features
demo`, clippy `-D warnings`, `fmt --check`, `cargo test -p ark --test
vectors` and `nix build .#harken-web`.

**Before and after.** Release, one thread, the shared VM: before at
`cb47634` (built from a `git archive` of it, into the same target
directory), after at `a25c616`; the three harnesses whole, run back to
back, and `bench_views`. The frames harness counts allocations now
(`0073222`, a per-thread tally), and was run over both.

| row | n | before | after |
|---|---|---|---|
| `library` hydrate, allocations an entry (`allocations.rs`) | 285 / 8,000 | 52.2 / 51.0 | 37.2 / 36.0 |
| `library` read whole (harken harness), time and allocations | 2,000 | 11.30 ms, 102,281 | 6.68 ms, 72,291 |
| `library` read whole (harken harness), time and allocations | 8,000 | 38.65 ms, 408,285 | 29.79 ms, 288,295 |
| `media.title` in a plan's expression | — | 85 ns, 1 alloc | 72 ns, 1 alloc |
| `add_to_playlist` applied, interpreted (`allocations.rs`) | 285–8,000 | 204 | 194 |
| `add_to_playlist` applied, native (harken, `owned` middleware) | 500–8,000 | 358 | 355 |
| harken `add_to_playlist`, one playlist: whole | 2,000 / 8,000 | 62.3 / 47.8 µs | 38.5 / 40.9 µs |
| harken `add_song`: whole / `local_commit` | 8,000 | 316 / 76.3 µs | 257 / 38.4 µs |
| a `Batch` of 256 with facts: decode, time and allocations | 256 | 1,074 µs, 11,540 | 951 µs, 11,028 |
| … encode | 256 | 1,235 µs, 19,997 | 948 µs, 19,997 |
| a `Batch` of 256, intents only: decode / encode | 256 | 742 / 731 µs | 551 / 607 µs |
| `MemoryStore::scan`, per row | 500 / 2,000 / 8,000 | 0.39 / 0.73 / 1.13 µs | 0.04 / 0.05 / 0.06 µs |
| the scanner's full rescan, per look | 8,000 | 23.9 ms | 8.2 ms |
| (c) alone, Memory: `mutate` | 500 / 2,000 / 8,000 | 34.1 / 32.7 / 40.6 µs | 28.7 / 28.8 / 33.0 and 43.7 / 32.5 / 36.2 µs (two runs) |
| (c) offline, Memory: `mutate` | 2,000 / 8,000 | 19.1 / 24.1 µs | 17.8 / 21.3 µs |
| (a) alone, one playlist: whole | 2,000 / 8,000 | 29.1 / 27.1 µs | 22.3 / 21.6 µs |
| (d) receive by facts alone | 2,010 / 8,010 | 5.83 / 7.56 µs | 4.12 / 6.33 µs |
| `MemoryStore::clone` of the confirmed store | 8,000 | 12.1 ms | 8.3 ms |
| `state_at` + `state_hash`, log of 8,000 | 8,000 | 44.3 ms (hash 12.2) | 39.3 ms (hash 15.1) |

`bench_views`, the desktop's views, medians of 21 warm rounds:

| query | hydrate 1× / 4×, before | after | toggle 1× / 4×, before | after | describe 1× / 4×, before | after |
|---|---|---|---|---|---|---|
| `library` | 1.2 / 4.5 ms | 0.97 / 3.9 ms | 21.7 / 39.2 µs | 9.6 / 12.7 µs | 1.3 / 2.5 µs | 0.33 / 0.52 µs |
| `albums` | 0.67 / 3.0 ms | 0.43 / 1.7 ms | 0.50 / 0.86 µs | 0.29 / 0.31 µs | 0.46 / 0.82 µs | 0.25 / 0.26 µs |
| `artists` | 0.56 / 2.3 ms | 0.49 / 1.8 ms | 0.42 / 0.56 µs | 0.24 / 0.25 µs | 12.9 / 20.4 µs | 4.9 / 6.8 µs |
| `composers` | 1.4 / 5.9 ms | 1.6 / 6.1 ms | 0.70 / 1.1 µs | 0.41 / 0.42 µs | 11.7 / 14.4 µs | 5.7 / 6.9 µs |
| `track_details` | 3.9 / 16.7 ms | 3.8 / 15.7 ms | 0.49 / 0.57 µs | 0.24 / 0.24 µs | 0.63 / 0.64 µs | 0.31 / 0.32 µs |

An intents-only `Batch` carries no row, so its quarter is the VM's —
the size of the noise every timing here sits in. The warm toggles and
describes of `bench_views` halve and the cold ones do not move; that was
not taken apart.

**The rows the memory hierarchy had moved.** The scan per row is flat
now — 0.04 to 0.06 µs from 500 rows to 8,000, where it was 0.39 to 1.13:
it copied every row, and the copies were what outran the cache; handing
out a reference count does not. A peer alone's `mutate` still rises
somewhat from 2,000 to 8,000 (28.8 to 33.0, and 32.5 to 36.2, in two
runs; 32.7 to 40.6 before) — less than it did, and the same instructions
over a heap that still grows, the authority's log among it; not changed
further here.

**The row's share now.** A row handed out is two reference counts (26 ns,
no allocation), where it was a whole copy — media 15 allocations and
592 ns, `playlist_item` 7 and 198 ns — 33% of a `library` hydrate and 14%
of an interpreted `add_to_playlist`. What is left that is row-shaped is a
row built into its struct where one is used whole: in the hydrate, each
item the related plan finds (a bare node is the row as a struct) — 6% of
the allocations; in the interpreted apply, the item `MAX(pos)` read and
the row written, its values laid out once — 8 of 194 (4%). The largest
share of a hydrate is now the node the helper builds, its ten names and
its map (30%), which a positional row does not touch.

**Not changed, and why.** A native procedure's reads hand the domain a
struct (`cx::lit(row.into_value())` in `authoring/schema.rs`): the
authoring vocabulary's handles hold values, and a row-valued handle is a
change to that vocabulary rather than to the store — so harken's native
`add_to_playlist` is 355 allocations where it was 358. The state hash, a
change on the wire and a snapshot build each row's struct and then encode
it, as they did; encoding a row canonically without the struct would
remove that, and was left alone because those bytes are pinned. A row
decoded from the wire is laid out once more when applied (its names are
in name order, the table's are declared order): one allocation a fact row.

**Not verified.** Timings share the VM with whatever else runs, and single
shots (a native apply, 0.10 ms) move by tens of percent between runs; the
allocation counts do not. The wasm build was built (`nix build
.#harken-web`) and not run in a browser. The kotlin and swift runtimes
have their own rows and were not touched.

## Round 3 — decided with round 2's numbers

Round 2 removed every super-linear cost the harness found. What remains
is constant factors in the engine and a handful of edge cases the fleet
surfaced. Two pieces, disjoint in the files they touch.

### R5. The interpreter's clones (engine)

`eval.rs` clones a whole bound value to read one field (`Arg`/`Var`/
`Provided` then `Field`: 741 ns and 16 allocations for `media.title`) and
clones every local on every `bind` (a `map`/`filter` inside an interpreted
body over a large local is quadratic in it; an interpreted `create_playlist`
with a taken name at 800 was 345 ms). Hydrating `library` is 18 µs and 286
allocations per entry, and the plan expressions are always interpreted.

**Design.** Evaluate `Field(e, name)` where `e` is a binder or an argument by
borrowing the bound value and cloning the one field; more generally, give
the evaluator a borrowing path (`eval_ref(&Expr) -> Cow<Value>`) that
`Field`, `Var`, `Arg`, `Provided` and the list functions' element binders
use, so a clone happens only where a value is stored. Keep locals in a
`Vec<(Sym, Value)>` (or a `Vec<Value>` indexed by symbol, since symbols are
dense per function) that is pushed on `bind` and truncated when the scope
ends, rather than cloned per bind. Also fold in the small ones that live in
the same files: `Peer::mutate` cloning the mutator's IR `Function` per call,
a write cloning the table schema, `well_typed` building two sets per `put`,
`admits` cloning each field it compares.

**Guard.** An allocation count per hydrated `library` entry (the harness
already counts allocations) with the number stated; the interpreted
`map`/`filter` over a local of N elements linear in N (a counting test at
two sizes). Every vector and every agreement test between Native and the
interpreter unchanged.

**Then measure the row.** With R5 in, re-run the allocation profile of a
`library` hydrate and of one `add_to_playlist` apply and report the share
that is `Row`'s keys and map nodes. That number decides the row
representation (a positional `Rc<[Value]>` with names on the `Table`), which
is not done in this round.

### R6. What the fleet found (domain, protocol, transport)

- **A `Hello` whose `since` is past the head.** A server restarted over an
  emptied data directory serves such a peer nothing, and its pending is
  never acknowledged. Decision: it is the below-horizon case from the other
  side — the server answers with its snapshot at the head, and the peer
  re-opens from it and re-pushes pending, which is what `Replica::open` from
  a snapshot already does. The fleet's 3a falsification becomes a scenario:
  a server that lost its log re-bases everyone onto what it has.
- **`create_playlist` naming** (`free_number`) reads every playlist of the
  person and is quadratic for a taken name. Decision: read only the
  numbered siblings. The store serves a **range** on the column after an
  index's equality prefix (`Pred::Cmp` with `Ge`/`Gt`/`Le`/`Lt` on that
  column becomes a `BTreeMap` range over the `(user_id, name)` index), and
  the mutation reads `name >= "Favorites (" and name < "Favorites )"` for
  the person — the numbered variants and nothing else — then folds over
  that short list. `scan_where_eq`/`scan_ordered` gain the range; `read`
  passes it. Also `add_song` computes `work_title` and `work_id` about
  fifteen times each, eagerly in Native: bind each once with a `let`.
- **A login into a black hole waits forever**: `ark_auth::client::login`
  and `exchange` get connect and read timeouts, and the native WebSocket
  transport's handshake a read timeout; the fleet's fuzz may then include
  sign-in under a black hole again.
- **A revoked session's socket stays up** until it drops, because the token
  is checked at `Hello` only. Decision: revocation closes the session's
  connections at once — the auth server tells the hub which session was
  revoked and the hub closes its connections with the reason; scenario 8
  no longer cuts the connection itself.
- **The plan's scenario 9 wording**: a song's identity is its path, so files
  copied under new names *are* new songs; the scenario tests both halves
  and the plan should say so.

Each with its test (falsified once) and, where the fleet has a scenario,
the scenario extended rather than a second test written.

## Round 4 — what round 3 left, and when the pass ends

Round 3 removed the interpreter's clones and answered the fleet's four
findings. What it left is two correctness gaps the fleet uncovered and one
constant factor, each small and each decided:

- **A peer cannot tell one log from another.** A server that lost its log
  and then sequenced new entries serves a peer *below* its head new entries
  on top of a base that is not the server's; only `Verify` would notice.
  Decision: a log has an identity. `Log::base` carries a `log_id` (an `Id`
  drawn when the log is created, kept in the snapshot and the journal's
  base, and in the client's `replica` record); `Hello` carries the id the
  peer's confirmed store is a prefix of, and a server whose id differs
  answers with its snapshot at the head, exactly as it answers a cursor
  past the head. A `Hello` without an id (an older client) is served as
  today. This adds a field to `Hello` and to the snapshot, which moves the
  `protocol/` vectors that pin those frames' bytes and is therefore a spec
  change, made once and recorded in `spec/README.md`'s protocol row. The
  fleet's scenario 3c gains the half it could not test: the lost log that
  went on without its peers.
- **A verdict is reported twice** when the local replay after a rebase
  refuses an intent and the server's `Reject` for it then arrives.
  Decision: `Replica::reject` reports only for an intent still pending;
  one already dropped by a replay has had its reason reported.
- **`stdlib::std` takes owned values**, so `len` or `first` of a bound list
  still copies it. Decision: it takes borrowed values and clones only what
  it returns; the allocation guard's number is re-stated.
- `free_number` over a person's numbered siblings is quadratic in them
  (157 ms per call at eight hundred playlists all of one name); accepted —
  it is that person's own list and that shape of library does not occur.
  Stated here so it is not rediscovered.

**When the pass ends.** After round 4 the harness is run whole once more.
The pass is finished when every row's per-operation cost is flat in the
data or logarithmic through an index, and the remaining constant factors
are named with their numbers: the row representation (a fifth of a
hydrate, an eighth of an apply — `Rc<[Value]>` positional rows, deferred),
and the whole-group rebuild of a grouped or nested view (`docs/plan-v4.md`
§1.13, accepted).

## Round 5 — the four that remained, and a peer alone

Asked for after the pass closed: the four items the closing report named,
in this order, and — arriving mid-design — that a peer without a server is
first class, which is `docs/plan-alone.md` and carries item 3 with it.

### R8. A rebase once per pump, not once per frame

`Client::recv` hands each frame to the replica as it arrives and every
`receive` runs `advance`, so with K pending each live push costs the K
re-runs (≈ 20 µs each). Decision: the client drains every frame a pump
received into the inbox first and advances once — `Replica::receive` and
`receive_facts` stop calling `advance`; a `Replica::settle()` (or the
existing `retry`) is called once by `Client` after the frames of one pump,
and by the sans-io tests where they relied on the implicit advance. A
batch already lands whole. Guard: with K = 100 pending and 50 pushes
delivered in one pump, the pending intents are re-run once (the `runs()`
counter), not fifty times; the churn and fleet suites unchanged.

### R9. Grouped and nested views keep an aggregate, not a recount

A grouped node (`artists`: `members.len()`) and a nested one (`composers`:
`total(works)` over three levels) are rebuilt whole when one member moves,
O(group). Decision: a maintained aggregate for the projections whose only
use of a related list or of `members` is `len`, or a `fold` whose step is
`acc + f(x)` with `f` free of the accumulator (the verifier recognises the
shape and the plan records it as `Agg::Count` / `Agg::Sum(expr)` on the
`Related`/group binder); the entry keeps the running number beside the
dependency it belongs to, and a member arriving or leaving moves it by
`±f(x)` and re-evaluates the projection over the numbers without
re-pulling the list. Any other use of the list keeps the rebuild. The
contract (`view == hydrate`) is unchanged and is what the churn tests hold;
guard: adding one Bach song at 2,000 songs costs `composers` the rows of
that song's path, counted, not Bach's; `bench_views` rows before/after.

### R10. Retention

`docs/plan-alone.md` §3: one rule in `ark::retention`, the server's horizon
advanced by it (cursors per session recorded at `Hello` and ack, the two
constants as environment variables), the alone peer's authority holding
no entries in memory. Guard: a server that has sequenced 30,000 entries
with every peer caught up holds `RETAIN_ENTRIES` in memory and serves a
peer at cursor 0 the snapshot; a peer heard from yesterday at cursor 100
keeps the log above 100.

### R11. The row is positional

`Row` is `BTreeMap<String, Value>`; two thirds of a row copy is keys and
map nodes, and the closing measurement's "same instructions, bigger heap"
rows are this heap. Decision: `Row` becomes a positional record —
`Rc<[Value]>` in the table's column order, the column names held once on
the `Table`, `Row::get(&str)` resolved through the table's name→index map
— behind an API that keeps `get`, iteration by name, construction from
pairs and equality by columns, so the wire (`Value::Struct` on encode), the
indexes, the changes, the overlay and the vectors are unchanged in bytes
and meaning. The store hands out rows without copying (`Rc` clone). Done
last and alone, because it touches every store path; guard: the
allocation counts of a `library` hydrate and an `add_to_playlist` apply
re-stated, and the closing harness re-run whole.

Order of work: R8, R9 and the server half of R10 in parallel (disjoint
files); the peer-alone work of `docs/plan-alone.md` after R8 lands (both
touch `peer.rs`); R11 after everything else.
