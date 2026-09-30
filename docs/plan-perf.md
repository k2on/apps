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
