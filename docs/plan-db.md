# The database, over time: versions, fuzzing, a hash that moves, and what is left of the algebra

The engine is fast and its fleet is green. What has never been exercised is
the life of a deployment — an old client meeting a new server, a server
loading a new module over a retained log, a column added under running
peers — and what has never been built is the machinery that finds the bugs
nobody thought of. This is the design for both, plus the three smaller
items the engine still owes. Authorization and partial sync (§3.11) are
deliberately not here; they need design input first.

Six pieces: **D1** versions, **D2** fuzzing, **D3** the state hash, **D4**
the algebra's gaps, **D5** open time and the backend decision, **D6**
operations. D1 is the largest and carries three decisions about what the
protocol *means* across versions; the others are mostly mechanical.

## D1. Versions: the fleet runs mixed, and what mixed means

### What is tested

Two kinds of mix, both in the repository, both built by nix from
**pinned previous revisions of this repository** so that the matrix grows
as releases do:

- **The process fleet** (`harken/server/tests/fleet.rs`) gains version
  scenarios that take older binaries from the environment:
  `HARKEN_OLD_SERVER=/path/to/harken-server`,
  `HARKEN_OLD_PEER=/path/to/harken-peer`, one pair per pinned revision
  (`HARKEN_OLD_1_*`, `HARKEN_OLD_2_*`, …, or a directory per revision; pick
  one and document it). Without them the scenarios print that they are
  skipped and pass, so `cargo test` on a laptop is unchanged. A new flake
  check, `checks.versions`, builds the pinned revisions' `harken-server`
  package (which ships both binaries) and runs the fleet's version
  scenarios with those paths set — in the sandbox, on loopback, no KVM.
- **The NixOS VM test** (`packages.fleet-vm`) gains machines on different
  versions: a server on the previous revision with two peers on the
  current one, then the server upgraded in place (`systemctl` restart onto
  the new package over the same state directory) under the running peers;
  and a peer on the previous revision against the current server.

The pinned revisions live in one file, `nix/versions.nix`: a list of
`{ name, rev, hash }` (the flake fetches each with `builtins.fetchGit` or a
flake input per revision — choose whichever evaluates offline from the
lock; a flake input per revision is the lockfile's job and is preferred).
The first entry is `v4-journal`, the revision at which the log gained its
identity and the server its journal (`abbf861`); a second is the current
head at the moment this lands, so the matrix has a real pair from day one.
`harken/README.md` says how to add a release: append to `versions.nix`,
`nix flake lock`, the matrix grows.

The scenarios, each falsified once and each named for the case:

1. **Old peer, new server**: an old `harken-peer` signs in, syncs, authors
   intents whose closure hashes the new server also holds (unchanged
   mutators) and converges with a new peer beside it.
2. **Old peer, new server, a mutator whose body changed**: the old peer
   authors `create_playlist` at its old hash. See *closure provenance*
   below for what must happen: it is applied, because the server keeps the
   closures of every module it has ever run.
3. **New peer, old server**: the new peer's intents at hashes the old
   server never had are *held*, not dropped (see *hold* below); the peer
   says so in `status`; when the server is upgraded in place they land.
4. **Server upgraded in place**: a data directory written by the old
   server is opened by the new one (snapshot, journal, cursors, sessions);
   every peer reconnects and converges; the log's identity is unchanged.
5. **Client upgraded in place**: a directory written by the old
   `harken-peer` (replica, pending pages, alone log) is opened by the new
   one; pending lands; alone history joins.
6. **Schema grows under running peers**: a current-revision server whose
   domain gained a nullable column and a new table (a test-only domain
   variant built into `harken-peer`/`harken-server` behind a flag, or the
   next pinned revision once one exists) serves an old peer: facts
   carrying the new column reach the old peer, which applies them
   *projected* (see below), stays usable, reports `behind`, and converges
   on everything it can see; a peer at the new revision sees the column.
7. **Module update over a retained log**: a server restarted with a newer
   module replays its log through the closures it retained, and an entry
   naming a hash the new module no longer ships is still applied from the
   retained closure.

### Three decisions about meaning

- **Closure provenance.** Today `Authority::retire` keeps only closures a
  retained entry names or the current module ships, so an old client
  authoring a mutator whose body since changed can be refused as an
  *unknown function* by a server that once knew it. Decision: a server
  keeps the closures of **every module it has ever started with**, in a
  `modules` file beside its log (module hash → the closures that module
  shipped, written at first start with each module); `retire` keeps those
  too. A hash from a module the server never ran is still unknown, and
  that is the honest answer: a peer older than the server's first deploy
  is told to update. The hub's `/healthz` lists the module hashes it has
  run.
- **Unknown function is a hold, not a verdict.** A `Reject` whose reason is
  an unknown function (give it its own kind on the wire: `held` beside
  `reject`, or a structured reason — pick the smaller change, keep the
  existing `reject` bytes for existing cases, regenerate only what is new)
  leaves the intent **pending** on the client: it is not a refusal of what
  the person did, it is a server that cannot yet run it. The client
  reports it (`status.held`), re-pushes after reconnecting, and the fleet's
  scenario 3 shows it landing once the server is upgraded. The sans-io
  `Client` and `ark_client::Peer` both keep the intent; a view is told
  nothing new.
- **An older schema applies facts projected.** A peer whose schema lacks
  columns a fact carries applies the fact with the columns it knows
  (unknown names dropped; a missing non-nullable column it does know is
  still a `MalformedRow`), marks itself `behind` (its module hash differs
  from the server's — the server's hash travels in `Hello`'s answer, a new
  optional field the way `log` did), and **does not Verify** while behind
  (its hash is over a narrower schema, so disagreement would mean
  nothing). A peer whose schema is *newer* than the server's authors
  intents the server holds (above) and applies the server's facts as they
  are — a new nullable column simply stays `Null`. The frozen phones are
  older schemas and this is what makes them age gracefully rather than
  break. `spec/README.md` §12 records all three.

### Guards

Each scenario is its own guard. Beside them: a unit test that `retire`
keeps a module's closures with no entry naming them; a sans-io test for the
hold (an unknown hash is pending after the server's answer and lands after
`hold`); a sans-io test for projection (a fact with an extra column applied
by a narrower schema, `behind` set, Verify skipped, the row readable); and
the `protocol/` vectors regenerated only for the new frames, every existing
file byte-identical.

## D2. Differential fuzzing

`arkc fuzz [--seed N] [--seconds S] [--out DIR]` (in `rust/ark/src/bin/`),
the `--fuzz` of §3.16 that was never built: a seeded generator of random
schemas (tables, nullable columns, references, unique indexes, plain
indexes), random modules over them (mutators that insert/upsert/update/
delete with reads, helpers, guards, a few plan queries with related
lists, lookups, groups, aggregates, ranges and limits), and random fleet
sessions through `ark::sim` (partitions, duplicates, drops, rebases,
sign-ins, a server restart from its log, a peer below the horizon). After
each session: every replica's hash equals the authority's; every query's
maintained view equals a fresh hydrate (the churn contract over the random
plans); every verdict the interpreter gives equals the native one where
both exist (the demo and harken natives; random modules are interpreted
only); the log replays from facts to the same hash; and every frame the
session produced decodes to what was encoded. A failure writes the seed,
the module and the session as a vector under `--out` in the suite's own
format, so a found bug becomes a permanent `verify/`, `eval/`, `views/` or
`rebase/` case by copying the file. `packages.fuzz` runs it for a stated
time; `checks.fuzz-smoke` runs a few seconds of it so the generator itself
cannot rot. Run it here for an hour before reporting and say what it found
— the point of this item is the findings, and each one becomes a vector
and a fix (a fix in the engine is designed with the coordinator first if it
changes meaning; a plain bug is fixed in place with its vector).

## D3. A state hash that moves with the store

`state_hash` is SHA-256 over the canonical encoding of every table's rows
(§8.1), O(rows) per `Verify`, so verification is a diagnostic rather than a
habit. Decision: **the hash becomes incremental and order-independent per
table** — each row's leaf is `sha256(table ‖ canonical row)`, a table's
digest is the sum of its leaves modulo 2^256 (an additive hash: O(1) to add
or remove a row, collision-resistant under the standard lattice assumption,
and exactly what a store that reports changes can keep beside the rows),
and the state hash is `sha256` over the list of `(table name, digest)` in
schema order. `MemoryStore` keeps the per-table digest and moves it on
every `set`; `state_hash` reads it; the `Overlay` computes its delta from
its writes. A `Verify` is then O(tables), cheap enough that the client
verifies after every settle when linked, and divergence is reported as it
happens rather than when asked. This is a spec change: the `hash/`,
`eval/` and `rebase/` vectors carry hashes and are regenerated once, with
`spec/README.md` §8 revised and the reasoning in `hash.rs`; a vector whose
only change is a hash value is expected, any other byte moving is not.
Guard: the digest after N random puts and deletes equals the digest of a
store built from the surviving rows (falsify by skipping the subtraction),
and `Verify` after a settle costs no scan (the counting store).

## D4. The algebra's remaining gaps

- **Maintained `min` and `max`.** `Agg::Min(col)`/`Agg::Max(col)` beside
  count and sum (R9's recognition, extended to `fold` with `min`/`max` as
  the step, and to `first`/`last` of a list ordered by one column with no
  limit): an arrival compares; a departure of the current extreme re-reads
  the extreme through the index (`scan_ordered`, limit 1), O(log n); an edit
  is both. The child plan must be served by an index on that column for the
  re-read to be bounded; otherwise the list stays a list.
- **`distinct`** is group-by with no aggregate; expose it in the vocabulary
  as `.distinct(cols)` lowering to a group source whose projection is the
  key — no engine change beyond the builder and its emit.
- **Text search.** `Pred::Has(column, needle)` — substring match on a text
  column, case folded by the pinned Unicode tables (`lower`) — served by a
  **trigram index** declared on the column (`.index_text(col)`; a new
  `Index` kind on the wire, additive under `compat`, so a module hash moves
  and no mutator's does): the store keeps, per trigram of the folded text,
  the keys of rows containing it; a `Has` is the intersection of the
  needle's trigrams' postings, then `keep` confirms. A view over a `Has`
  filter is maintained like any filter — a changed row is re-admitted by
  the same predicate. harken's `search` becomes a query over `media.title`
  and `media.creator` with `Has` instead of a client-side filter, and the
  browser's search box drives a view. Guard: rows examined by a `Has` on a
  table of 8,000 equals the postings' intersection, not the table; the
  churn contract over a `Has` plan.

## D5. Open time, and the backend decision

Add to the harnesses: time to **open** a client replica from a snapshot of
N rows (`Peer::open` over `Dir`), a server from its `log.ark-log` and
journal, and an alone peer from its log, at 10,000 / 100,000 / 400,000
rows, with resident memory after open (read `/proc/self/status`). Report
the table. The decision that follows is the coordinator's: whether a paged
backend behind `Store` (the design names SQLite as a backend and not a
dependency; `redb` is the other candidate) is next, or memory as the
dataset stands for now. Do not build a backend in this round.

## D6. Operations

- `arkc backup DIR OUT` copies a running server's state consistently
  (snapshot, then the journal up to the length it had when the snapshot
  was read, then cursors and sessions) and `arkc restore` is the inverse;
  `arkc verify-log DIR` replays a data directory and prints its head, hash
  and horizon. Tests over a server that is appending while it is backed
  up.
- `/healthz` answers JSON when asked for it (`Accept: application/json`):
  head, horizon, log id, module hashes run, sessions with cursors and
  last-heard, connections.
- The `ids` map below the horizon keeps an 8-byte prefix per id instead of
  the id (a false positive is 2^-64 against random ids; a true duplicate
  below the horizon is answered `Duplicate` at the sequence the prefix
  recorded); above the horizon it is exact. State the memory per million
  entries before and after.

## Order and rules

D1, D2, D3, D4 and D5+D6 are disjoint enough to run in parallel: D1 in the
protocol, the hub, `ark-client`, the fleet and the flake; D2 a new binary
over `sim`; D3 in `hash.rs`, `store.rs` and the vector generator (`hash/`,
`eval/`, `rebase/`); D4 in `view.rs`, `store.rs` (the trigram index),
`schema.rs`, `ir`, the builder and harken's search; D5+D6 in the harnesses,
`arkc` and the hub's `/healthz`. D1 and D3 both regenerate vectors, in
different directories, and both touch `protocol.rs` only in D1's case; D3
and D4 both touch `store.rs` in different places (the digest; the trigram
index) — commit early, retry on `index.lock`, never revert another's hunk.
Every test falsified once; the commit rules of `docs/plan-v4.md` Part 2;
what is not verified said plainly.
