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
