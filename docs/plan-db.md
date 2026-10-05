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

### Landed

**The wire.** Two additions, both absent from every frame written before,
so every existing `protocol/` vector is byte-identical:

- `held` — `{t: "held", id, reason}`, beside `reject`. A server answers it
  for an intent whose function no module it has run shipped
  (`Authority::can_apply` false and the id not in the log). The `Client`
  keeps the intent pending (`Client::held_ids`, `Client::held`), tells a
  view nothing, and pushes it again on its next connection; an `ack`
  clears it. A `reject` whose reason is exactly `unknown function <this
  intent's hash>` (`protocol::unknown_function`) — what every server before
  this one said — is read as the same hold, which is what lets scenario 3
  work against a pinned server. A peer older than this cannot decode
  `held`, counts a bad frame, and keeps the intent pending: the right
  answer by accident of being undecodable.
- `module` — the server's module hash, bytes, on `batch` and `snapshot`,
  absent for `None`, decoded as `log` is (a null is refused). A server
  built from a module (`Server::with_module`; ark-server always) says it
  on every page, and answers a `hello` at its head — which used to get
  nothing — with an empty page carrying it, so a peer at the head learns
  it is behind before it would verify. A server that says no module (the
  sim's, the vectors') is answered exactly as before, which is why no
  `rebase/` vector moved.

New vector files: `protocol/server-held.json`,
`protocol/server-batch-module.json`, `protocol/server-snapshot-module.json`.
The generator also asserts the empty page at the head and the hold.

**Closure provenance.** `Authority::modules` (module hash → the function
hashes it shipped) and `Authority::ran`, which records a module and holds
its closures; `retire` keeps every one. `ark-server` writes
`DATA/modules.cbor` — every module started with, its closures, in the order
first run — at the first start with each, synced and renamed, and refuses
to start on one that does not decode. `/healthz` lists `module <hex>` per
module run, the current one marked `current` (and the JSON form D6 added
carries both).

**The first start with a new module re-homes the log** — not in the
brief, and found by scenario 7. A server upgraded to a domain with a new
table refused its own `log.ark-log` at start ("the snapshot's hash does not
match its rows": a table more is a pair more in the state hash), and so did
this build over any data directory a pinned revision wrote, because D3
redefined the hash. `modules::start_with` now says whether the module is
new; on that start only, `persist::rehome` reads the snapshot without its
hash check — the file is this server's own, written by a rename — widens
every row and writes it back hashed under the current schema. Every other
start checks as before.

**Projection.** `store::project_row` / `store::project`: a row laid out as
its table's, a column the table lacks dropped, a nullable one the row lacks
`Null`, a required one missing `MalformedRow`; a change to a table the
schema lacks is `None`. The replica (`Replica::behind`, set by the `Client`
from the two hashes) projects facts before it compares them with a run or
applies them; the `Client` projects a snapshot's rows the same way; it says
no `Verify` while behind. **One widening applies whether or not a peer is
behind**: a fact row that names nothing the schema lacks but leaves out a
nullable column is completed with `Null`, and the server widens its head
store at start the same way (`persist::widen`). Without it a peer of the
grown module that replays an old entry by intent writes `note: Null`, while
the server and a peer fed that entry's facts hold the row without the
column, and the two hash apart over rows that say the same thing. The
store's own `apply_change` is unchanged — a fact is still applied raw
(§4.5) — and the projection lives where `behind` is known.

**The matrix.** `nix/versions.nix`: `v4-journal` at
`abbf861feed6468baea238babc7baa8e1750d3bc` and `v4-rows` at
`71f7b0c40e81a226d980f52d20c259d73ee4e055`, the head when this began. One
flake input each, `git+https://github.com/k2on/apps?ref=main&rev=…`,
following this flake's `nixpkgs` and `rust-overlay`, locked by `nix flake
lock` over the network (it was reachable); the flake refuses to evaluate if
a locked revision is not the one `versions.nix` names.
`packages.harken-server-<name>` is each revision's own `harken-server`
package. `checks.versions` runs `harken/server/tests/versions.rs` with
`HARKEN_OLD_<n>_SERVER`, `_PEER` and `_NAME` set, `n` in `versions.nix`'s
order — one variable per binary rather than a directory per revision,
because that is what a shell sets by hand. The scenarios are a test target
of their own over the fleet's support so that the check builds and runs
exactly these; `Server::old`, `PeerProc::old`, `Server::upgrade`,
`PeerProc::upgrade` are the support's.

Scenario 6 needs a schema no pinned revision has. `harken_server::grown` is
harken's domain grown as a release would grow it — a nullable
`playlist_item.note`, a table `tag`, `set_item_note`, `tag_playlist`, the
query `item_notes`, and `create_playlist`'s body moved (one more input
check that always holds: a new hash, the same behaviour) — chosen by the
flag both binaries have for another module, `HARKEN_MODULE` and
`harken-peer --module FILE` (new). The column is on `playlist_item` and not
`playlist` because every base function that returns a whole playlist row
declares its type, and a column there fails `verify` until those functions
move too — which says something about growth under §17 (below).

**The scenarios**, each falsified once (its doc comment says how), the
timings from upgrade or start to converged — a debug build in this
container, then the same in release in the `checks.versions` sandbox:

| scenario | v4-journal | v4-rows | sandbox, release |
|---|---:|---:|---|
| 1. old peer beside two new ones, a new server | 831 ms | 869 ms | 461 / 419 ms |
| 2. old peer authors `create_playlist` at its hash after the server moved to the grown module | 1,530 ms | 1,684 ms | 506 / 517 ms |
| 3. new (grown) peer held by an old server; server upgraded in place; lands | 993 ms | 1,431 ms | 523 / 506 ms |
| 4. old server upgraded in place under peers of both revisions; identity and head kept | 642 ms | 657 ms | 374 / 482 ms |
| 5. old peer's directory, pending behind a black hole, opened by this build | 290 ms | 314 ms | 265 / 250 ms |
| 6. schema grows under a running peer: projected, `behind`, hashing as the log projected | 2,191–2,775 ms | — | 858 ms |
| 7. module update over a retained log: an entry at a hash the module no longer ships | 1,566–1,612 ms | — | 536 ms |

6 and 7 need no pinned revision and run in every `cargo test`. The whole
target is 3.2 s in the sandbox (8 passed, one ignored). The process fleet
(`tests/fleet.rs`) and the workspace are unchanged by the empty page at the
head and pass whole.

**`fleet-vm`, under TCG**, here with no `/dev/kvm` (`--option
system-features "nixos-test benchmark big-parallel kvm"`): every subtest
passed, the two new ones included — the `v4-rows` server with a `v4-rows`
peer and a current one, upgraded in place under both by
`switch-to-configuration` into the specialisation whose only change is the
package, `/healthz` then listing its module, the log's head kept, both
converging after (39 s); and the `v4-rows` peer against the current
`server` (6 s). The test script took 529 s and the whole build 21.5
minutes, five machines on four cores.

**Found, and for the coordinator.**

- **D3's hash makes yesterday's files unreadable.** A server's snapshot is
  re-homed on the first start with a new module (above); a peer's alone log
  is not: this build cannot open a directory `v4-rows`'s `harken-peer` used
  `--alone` — `storage: reading log: the snapshot's hash does not match its
  rows`. Scenario 5b (`#[ignore = "witness: …"]`) holds it. And the
  server's re-homing is keyed to a new module, so a future change to the
  hash's definition under the same module would not trigger it: recording
  the hash's version beside the snapshot would make that the key. Both are
  D3's to decide; neither is fixed here.
- **An alone directory from before plan-alone does not join.** `v4-journal`'s
  peer alone kept no history; opened with a server, its store is taken as
  the fork and paged on top of, it reaches the head with nothing pending,
  and its playlists are not the log's — its alone work never reaches the
  server. plan-alone said this case was untested; 5b is the witness.
- **A column on a table a function returns whole moved that function —
  decided: a row in a signature is the table's row.** There is no row type
  of its own (a row is `Ty::Struct` of its columns, which `ret`, a
  provider's result and `Expr::None` carry and the hash covers), and a
  nominal one would be a new `Ty` on the wire, moving `module/`, `verify/`
  and every row-naming function's hash. So `verify::same` compares rows as
  the table's: two structs that differ only in nullable columns of one
  table, carried whole by the wider, are the same type. harken with
  `playlist.note` verifies and `playlists`, `playlists_of`, `owned`,
  `create_playlist` and `add_to_playlist` keep their hashes
  (`a_nullable_column_moves_no_function_that_returns_its_rows`, falsified
  by making the query-result comparison exact). No vector moved.
- **Below the head, facts are widened too — decided and done.**
  `Log::state_at` and `hash_at` fill a nullable column a fact's row lacks
  with `Null` (`log::widened`, borrowed where nothing is filled), so a
  `Verify` below the head over pre-upgrade facts agrees
  (`a_verify_below_the_head_over_older_facts_agrees`, falsified by
  `hash_at` taking the raw facts off). No vector moved.
- **A peer behind is never verified.** By the decision, and so a divergence
  on a frozen phone is invisible until the phone is updated. Confirmed by
  design.

**Not verified.** A real older phone (they are frozen at spec v3 and do not
sync with v4 at all); a pinned revision with a schema of its own (none
exists — the grown domain stands in); `held` against a server that
actually lacks a module in production rather than in a fleet.

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

### Landed

**What runs.** `arkc fuzz [--seed N] [--seconds S] [--cases K] [--out
DIR] [--without OP,…]` and `arkc fuzz --replay FILE`, in
`rust/ark/src/bin/fuzz/` (a subcommand of `arkc`, not a binary of its
own: the demo and the JSON dialect it shares with `arkc vectors` are
already there). Case `k` of a run from seed `N` is the case of seed
`N + k`, so `--seed N+k --cases 1` runs one alone. `packages.fuzz` is
`nix run .#fuzz [SECONDS]`, ten minutes by default; `checks.fuzz-smoke`
is seed 1, 25 cases, under a second.

**The generator** (`gen.rs`) builds IR directly, not through the
authoring vocabulary, because the vocabulary's types keep an author
inside what it can say and a module may arrive from anywhere. A schema
is two to four tables, each keyed by an id of its own, a text or an
int, or two columns (a parent and a position); scalar columns, some
nullable; references to earlier tables and, rarely, a table's own;
unique and plain indexes. A module is up to two helpers, up to three
middleware (a guard that refuses when a table is full, a provide that
counts one, a provide over `ctx.user`), three to eight mutators — insert,
upsert, update, delete with a cascade by hand, an append whose position
is read in the body (the rebase-visible shape), a bulk update or delete
over a select, a get-then-insert-or-update — with input checks,
refinements, autos, guards and helper calls; and one to four queries: a
filtered list with a projection, a tree on a reference (to three deep),
a lookup up a reference, a group with `Len` and a `fold` sum and a
having, an expression order under a limit. Everything is typed by
construction; the verifier names a query's result type and the
generator takes it. **Of 218,623 modules generated in the hour, 218,338
verified (99.87%)**; all 285 misses were one complaint, a filter
literal on an id the context does not type.

**A session** (`session.rs`) is two to four clients and 30–150 ops
drawn against what the clients hold — a mutation (42%) with arguments
drawn mostly from the author's own view, a random delivery (33%), a
partition, a heal, a settle, a restart from the journal, a restart over
an emptied directory (`wipe`), a compaction to a random retained
sequence, a late joiner (by facts or by replay, with closures or
without, signed in or used by nobody), a sign-in, a client re-opened
from what it made durable, a verify. One case in five is the demo, its
natives held by the server and every even client. The `Op`s are
`ark::sim`'s and the script is the vector: `Sim::run` is the one
meaning of an op, and `rebase/` runs a `fleet-fuzz-*` file through
`ark::sim::run_script`.

**The checks**, after every op: every frame it put on the wire decoded
from its bytes as itself; every maintained view of every query on every
client pushed what its replica told it and held to a fresh hydrate, a
fresh read and the splice; the changes a view was told reach the view;
on the demo, every mutation run native and interpreted (`agrees`) over
the author's view; a restart reads its journal back as the log it wrote;
a re-open finds the durable store at the cursor equal to the confirmed
one. After every session: settle, then every replica at the server's
head and hash with nothing pending and no divergence, every view its
confirmed store, the log replayed from its facts and from its intents
to the same facts and state, no `Verify` answered "disagreed" (see
below for what a compaction and a wipe do to that), and every query
driven 25 batches under the churn generator over what the session left.

**A finding** is shrunk — ops removed by halving chunks, then every
function the script does not name — while `run_script` still fails the
same way, deduplicated by its text with the numbers taken out, and
written under `--out` in the suite's format with a line in `--out/README`
(`spec/README.md` §15.1 says the forms).

**The run.** Two processes for 3600 s each, from seeds 20261001 and
900000000, against `rust/ark` as of `d551dd2` (the harness as of
`f051abc`) with the one engine fix below applied — it is in `peer.rs`
and waits on the coordinator, and without it a third of the sessions end
in it and hide the rest:

| | |
|---|---|
| cases | 272,874 (54,536 the demo) |
| sessions | 542,888 |
| ops | 49,090,055 — 20,391,614 mutations, 6,926,967 refused by the author's own view; 11,500,692 entries sequenced |
| restarts (journal or wiped) | 1,214,091 |
| compactions | 971,241 |
| snapshots sent (below the horizon, or another log) | 1,839,931 |
| re-opens / sign-ins | 1,212,833 / 94,788 |
| frames round-tripped | 62,796,539 |
| view pushes | 24,480,008 in sessions, 14,455,453 under churn |
| native-against-interpreted mutations | 4,077,249 |

Findings, every one read and its cause named:

1. **Plain bug — an ack at or below the cursor leaves the intent pending
   for ever** (`Replica::ack`). An intent is sequenced, its ack is lost,
   the client comes back below the horizon and re-opens from the
   snapshot — which holds the intent — then pushes it again; the
   duplicate is acked at `n <= cursor`, `receive` drops anything at or
   below the cursor, and the intent stays pending: pushed on every
   reconnect, and applied twice in the view. 1,015 of 3,392 sessions on
   `ac4424a` (two minutes from seed 32000000); none in the hour with the
   fix. The fix drops the intent from pending and rebases with no
   verdict, as `reject` does. Vector: 5 ops (an update of an absent row,
   two deliveries, a compaction to the head) and one mutator,
   `rust/ark/tests/fuzz-findings/fleet-fuzz-an-ack-at-or-below-the-cursor.json`,
   which `tests/fuzz.rs` holds to failing until the fix lands — then it
   moves, unchanged, to `spec/vectors/rebase/`, where `rebase_fleet`
   runs it. Not committed: `peer.rs` is D1's, and the hunk went to the
   coordinator.
2. **Meaning — an `ack` names no log.** A client confirmed only by an ack
   (the page carrying the log's name lost) never learns the log it is
   at; after the server restarts over an emptied directory its `hello`
   names none and is served as before from its cursor, so it holds a
   state of the old log under the new one's sequence — a different hash
   at the same head (3,856 sessions), a divergence the facts then heal
   (19), or a fleet that never settles because a later entry refuses on
   that state and a replica in `Whole` mode never asks for facts for an
   entry its replay refuses (1,201). Every one needs `wipe`: with the
   wipes taken out of each shrunk script it converges. Either the ack
   carries the log, or a client does not confirm by ack until it knows
   the log; the protocol has to say which. Vectors:
   `rust/ark/tests/fuzz-findings/an-ack-names-no-log*.json`.
3. **Meaning — a maintained sum and a checked `fold` disagree about
   overflow** (`view.rs`, R9). `fold` adds in member order and refuses on
   the first intermediate overflow; the maintained sum moves by `s - old
   + new`. So a group whose total fits but whose fold passes through
   `i64::MIN` answers when maintained and refuses when read fresh (246),
   and an edit whose `s - old` overflows refuses when maintained where
   the fresh fold answers (401). No order of maintenance can match a fold
   that is order-dependent; the spec has to say what a sum's overflow is
   judged on (the total, say, in wider arithmetic). Vectors:
   `rust/ark/tests/fuzz-findings/a-maintained-sum-*.json`.

Nothing else: no frame that did not come back as itself, no native
disagreeing with the interpreter on the demo, no log that did not
replay, no journal read back as another log, no view wrong but for the
sums. `tests/fuzz.rs` holds the generator to nine in ten verifying, an
op to its value, a restart and a re-open to the same fleet (each
falsified), and every file under `tests/fuzz-findings/` to still
failing — falsified by taking the wipes out of one.

**Confirmed on the tree as it stands.** Ten minutes more on `ac4424a`
(D1, D4, D5 and D6 landed) with the fix, from seed 31000000: 26,906
cases, 53,574 sessions, 4,847,381 ops, 21,564 of 21,594 modules
verified; the same two meaning findings and nothing else, every session
one needing a wipe.

**Not done, and not verified.** harken's natives are not run against the
interpreter here: `arkc` cannot depend on harken, and a session over
harken's module would have to live in `harken/domain/tests/`, which is
not this item's (`agreement.rs` there holds them on fixed cases).
`checks.fuzz-smoke` fails on the tree until finding 1 is fixed (it is
seed 1's first case). `nix flake check` itself was not run here; the
smoke command was, by hand.

**Decided, and fixed** (after the coordinator's answers). Finding 1 is
fixed in `Replica::ack` (`11d7af1`) and finding 2 in the protocol
(`7d894b3`): `ack` carries `log` as a page does, absent where the server
names none — every existing `protocol/` vector byte-identical,
`protocol/server-ack-named.json` new — the client learns the name from
an ack, and a replay of a confirmed entry that refuses with no facts in
hand is recorded as a divergence, so `needs()` asks for them. The three
sessions are `rebase/fleet-fuzz-an-ack-*.json`, byte for byte as the
fuzzer wrote them, carried by `ark-vectors` from `bin/vectors/fuzzed/`
and written only while they hold; each fix falsified by disabling it,
and two sans-io tests in `tests/fuzz.rs` (an ack names the log; a
refusing replay asks for facts), each falsified once. Finding 3 went to
D4. `checks.fuzz-smoke` passes (`nix build .#checks.x86_64-linux.fuzz-smoke`).
Ten minutes more, two processes, wipes on: 61,232 cases, 122,336
sessions, 11,065,173 ops, 272,349 restarts, nothing but the sums.
`harken/domain/tests/fuzz.rs` holds harken's natives to the interpreter
under forty of these sessions (falsified by a `create_playlist` that
reads a host counter).

**The verify check, after D3.** Every answer to a `Verify` was held to
"agreed" only in a session whose script neither compacted nor wiped:
below the horizon the authority had no "cannot say", and answered
"disagreed". D3 gave it one (`Agree::unknown`, recorded as `None` in
`Client::agreed`), so the check runs after a compaction now: an answer
of `Some(false)` is a finding, `None` never is, and the summary line
counts both. Seed 20261005 for 120 s: 7,186 cases, 14,372 sessions,
1,545 verifies answered and 96 unknown, 0 findings; seed 1, 200 cases:
41 answered, none unknown (they are rare), 0 findings. Falsified by
counting `None` as a finding: 43 findings in 60 s, the one written a
script with a compaction and no wipe.

A wipe is still not held, and that is the protocol's gap rather than
the check's. A `Verify` names no log, so one said at a sequence of the
log a client held before the server came back emptied, under a new
name, is answered against the new log at that sequence whenever it
arrives before the client has the new log's snapshot — "disagreed",
about two different logs. With wipes held the 120 s run found exactly
this (3 sessions, seed 20265421 the first written, a `verify` right
after a `wipe`); with `--without wipe`, 0 findings over 13,436 sessions.
So a disagreement in a session that wiped is counted (5 in the run
above) and not reported. The fix is finding 2's again: the `Verify`
carries `log`, and an authority on another log answers `unknown`. It
changes the wire, so it is the coordinator's to decide.

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

### Landed

**The construction** (`hash.rs`'s module doc, `spec/README.md` §8.1):
`leaf(t, row) = sha256(enc(Text t) ‖ enc(Struct row))`; `digest(t)` the sum
of its rows' leaves mod 2^256, each a big-endian integer, zero for no rows;
the state hash `sha256(enc(List [[Text t, Bytes digest(t)]]))` over the
schema's tables in schema order. The table name is encoded on its own so it
is delimited, and the row is its §4 struct so no runtime's layout reaches a
leaf.

**Where it is kept.** `Store::digest(table)` is the hook, `None` by default;
`state_hash(&dyn Store)` reads it and sums a scan where a store keeps none.
`MemoryStore::set` subtracts the old row's leaf and adds the new one's —
nothing for an equal row — and drops a table's digest with its last row,
asserting zero there in debug. `Overlay::digest` is its base's moved by its
writes. `Log::hash_at` below the head takes the facts above the sequence
back off the head's digests (an add's leaf out, a remove's in, an edit's
new for its old), so a `Verify` that lands behind a busy head costs the
facts since rather than a copy of the store — a change to `log.rs` this
item did not name, made because the client now verifies after every
settle and would otherwise land there and cost the server a replay.

**The client.** `ark_client::Peer` says a `Verify` after every settle that
moved its cursor, once its connection has sent it a page or a snapshot and
not while paging; answers are matched to questions by a queue per
connection. An answer to `verify()` reaches `agreed()` as before; an
automatic one only when it disagrees, with a note on the pump. A verify the
engine does not say (behind, D1) is not queued.

**The cost of a `Verify` at 8,000 rows**, release build, this machine
(`ark/tests/hash_digest.rs` `perf_verify_at_8000_rows`, mean of 200; and
`ark/tests/perf.rs` (g)):

| | before | after |
|---|---|---|
| the state hash of the store (the claim; the answer at the head) | 12,253 µs | 2.3 µs |
| `Server::recv(Verify)` at the head, as served | 15,054 µs (`plan-perf.md`) | 0.5 – 4.4 µs |
| the answer 100 entries below the head | 13,034 µs (replay) | 92 µs (facts taken back) |
| summing a scan, for a store that keeps no digest | — | 9,056 µs |
| what a write adds, per row in or out | — | 0.84 µs (one leaf) |

**Vectors.** Seven files moved and only their hashes: `eval/add-to-playlist`
and its falsify (`steps[].hash_after`), `hash/demo-state` and its falsify
(`hash`), `rebase/three-peers` (`final_hash`), `rebase/fleet-seed-7` and its
falsify (`expected_hash`). Every other directory is byte-identical; the
protocol vectors carry placeholder hashes. New: `hash/leaves-and-digests`
(every leaf, every digest, the pairs) and `hash/falsify/digest-by-xor`.

**Guards**, each falsified once: the digest after 4,000 random writes
equals a rebuilt store's (fails at 250 without the subtraction); a peer's
claim and the authority's answer after a settle read no row through the
counting store (2,000 rows each when `MemoryStore` keeps none); `hash_at`
below the head equals the replay's at every sequence of a log of adds,
removes and edits (fails at 0 with an edit swapped); the client's
verify-after-settle reports `(11, false)` for a raw fact applied by hand
and nothing over ten clean settles (no verify said at all without it). The
fleet's replay scenario now replays the last push, since a `Verify` follows
it; the fleet is green.

**Not decided here, and the coordinator's:**

- *A 256-bit sum is not a commitment.* Agreement between replicas of one
  log is what it is for; against somebody *choosing* rows, Wagner's
  generalised birthday attack finds a multiset with a given 256-bit sum far
  below 2^128 work. A snapshot from an untrusted source is still verified by
  replaying it, not by its hash. If a snapshot's hash should ever be trusted
  alone, the digest wants a wider group (a lattice hash of a couple of
  kilobytes, as LtHash does) — a second spec change, cheap now and dear
  once other runtimes exist.
- *`ir::SPEC_VERSION` did not move.* `spec/README.md` says to move it when a
  conformant runtime would become non-conformant, which this does to any
  v4 runtime's state hash; moving it also moves every module hash. Left at
  4, and said here.
- *A `Verify` below the horizon* is answered `ok: false`, as it was: the
  frame has no "cannot say". A client verifying after every settle meets
  that only when the server compacts past a cursor it is still paging up
  from, which today it is not, but the answer would read as a divergence.

**Decided after, and landed.** A stored snapshot names its construction
(`hashing: 2` on the server's `log.ark-log`, a peer alone's `log` and the
client's `replica` record; absent is 1): one of construction 1 is checked
that way on open and held hashed the new way, never refused, and
`persist::rehome` rewrites such a server snapshot once even with the same
module — which is what lets D1's 5b, an alone directory of `v4-rows`
upgraded in place, pass (its pre-plan-alone half, 5c, stays an ignored
witness). A `Verify` below the horizon or past the head is answered
`unknown: true` (absent otherwise, so every existing `protocol/` vector is
byte-identical; `protocol/server-agree-unknown` is new), and the client's
automatic verify reports nothing for it. The sum not being a commitment is
a stated non-goal in `spec/README.md` §8.1, and `SPEC_VERSION` stays 4.

**Not verified:** another runtime reproducing the construction from the
prose alone — the runner recomputes it from the canonical encoding and
SHA-256, which is the nearest this repository gets.

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

### Landed

Commits: `99f36fa` (min and max), `8ce987e` (distinct), `23b0fcb` (text
search), `d9b402c` (the two vectors), `fb316b8` (harken), and the docs.

**Min and max** (`view.rs`, R9's `shape` extended; nothing travels).
`Agg::Min(col)`/`Agg::Max(col)` keep the column's value, `Null` for an
empty list. Recognised from `fold(list, init, |acc, x| min(acc, x.col))`
(either argument order, `max` likewise) — rewritten to `match slot {
v => min(init, v), None => init }` — and from `match first(list) { y =>
some, None => none }` (`last` too) where the list is ordered by one column
and then only by its key, has no limit, and `some` reads `y` only as
`y.col` of that column — rewritten to a match on the slot with `y.col`
read as `y`. Reading the value alone is what makes ties harmless, and why
`first(list)` read for another column stays a list. Kept only where the
column is the row's (a child plan with no projection, having or lists of
its own), is not nullable, and an index of the schema serves it under the
list's `on` columns and the child filter's equalities — `MemoryStore`'s own
`serves` rule, restated for one order column; elsewhere the list is a
list. A group's `members` fold the same way, its pins the `by` columns. An
arrival compares; a departure equal to the extreme marks it stale, and it
is read again from the final store through `scan_ordered(.., limit 1)`
(comparing later arrivals against that is idempotent); an edit is both.
A list kept only as extremes is pulled as one indexed read each, not the
list. Not recognised: an extreme through a helper (R9 sees `total` through
one; no helper here has the shape), and `first`/`last` of a group's
members (they are in key order).

Guard (`rust/ark/tests/extremes.rs`, the counting store): Gould's greatest
position over 2,000 songs leaving costs one row examined and one `get` (his
row) — the same at 500; one arriving above it or below it, one `get` and no
row; through a group source, the departure is one row and no `get`.
Falsified by reading the extreme without the index: 1,999 rows (499 at
500), and through the group 2,000 (500). Churn: a kept max (fold), a kept
min (first, behind a having that flips), a max of text (last, in an order
key), both ends and a count in an order key under a limit, a group's max,
and the two shapes left lists — each falsified once (`take_out` never
stale, `put_in` comparing the wrong way, a group's stale extreme ignored).

**Distinct** (`authoring/schema.rs`). `.distinct(c)` is `group_by(c)` with
`project: Field(Var(row), c)` — the plan `group_by(c).map(|c, _| c)`
writes, byte for byte (`rust/ark/tests/distinct.rs`); a tuple projects the
key struct. No verifier rule was needed: the module verifies as built.

**Text search.** `Pred::Has(column, needle)`; both sides folded by
`store::fold`, the pinned `to_lower_simple` one character for one, so no
accent is folded away and `ß` is not `ss`. A `Null` holds nothing; every
text holds `""`. The text index is `Table::text` — the columns with one —
rather than a field of `Index`, so that an `Index` is still the two fields
every literal in the workspace writes (D2's generator among them); on the
wire it is an `index` node with `"kind": "text"` after the table's other
indexes, and a `Has` is `{"t":"phas","column","e"}`, each written only where
used. `MemoryStore` keeps, per text index, `[char; 3] → keys`, moved in
`set` by the difference of the old and new value's trigrams (nothing when
the column did not change). `Store::scan_where_text` carries the hint — a
disjunction of conjunctions of `(column, folded needle)`, so harken's
title-or-creator is two branches unioned — and its default is
`scan_where_eq`; `MemoryStore` intersects each branch's postings from the
shortest, unions the branches, then asks `keep`; a branch with no trigram
on an indexed column (a needle under three characters) leaves the read to
the scan. The overlay merges its writes as for equalities. `compat` treats
a text index as additive, the verifier holds `Has` to a text column
(nullable or not) and a text needle, and a view over a `Has` filter
re-admits a changed row by the predicate, as any filter.

Guard (`rust/ark/tests/text.rs`, the counting store): a search of 8,000
media for "sonata" in the title or the creator examines 2,514 rows — the
postings' intersection, computed in the test from each row's trigrams —
of which 1,829 answer (a decoy title holding every trigram and not the
needle is refused by `keep`); the same rows with no text index examine
8,000. Falsified by skipping the index: 8,000. Beside it, each falsified
once: the Unicode case (`É` found by `é` and `ÉLÉ`, `Σ` by `σ`; folding
with `to_ascii_lowercase` breaks it), a needle under three characters
read as the scan, the overlay, the wire round trip, the verifier, `compat`,
and the churn contract over three `Has` plans and three needles (reading an
`or` through its first branch breaks it).

**The trigram index's cost per row**, `harken/domain/tests/perf.rs`
`perf_search`, release, the shared VM: over 8,000 media with harken's two
text indexes, 14.66 postings a row (its title's trigrams and its
creator's), and a put costs 13.0 µs a row with them against 5.5 µs without.

**harken.** `media` has a text index on `title` and on `creator`;
`search(playlist_id, needle)` is the library's plan over a `Has` on either.
There was no search box: the client's search was vim's `/`, a jump to the
next row whose `title creator` lower-cased contains the text. On the Songs
page `/` now narrows the list as it is typed — each keystroke opens a view
over `search` with the needle and drops the last; `<Enter>` keeps it and
lands on its first row, `<Esc>` widens back; on every other page `/` is
the jump it was. What a keystroke costs (`perf_search`, 8,000 media, a
view hydrated, median of five): a needle answering 160 rows, 0.9 ms; 1,760
rows, 9 ms; one every row holds (the harness's creators are all "Artist
N"), 35–47 ms — the hydrate is per row answered, about 5 µs, where the old
`/` scanned every row in 1.6 ms to find one. Module hash
`dfc028e2f07a3d78ee7696dbfb2cc032799d9c058a2b96a96deae4959f311dcc` →
`abf1cbdd8b0ccd361adf4916184d3e81f89f633734025f4642b0915b1499165b`; every
existing function's hash unmoved (`arkc hash` of both), `arkc check`
additive, the pinned mutator hashes unchanged.

**Vectors.** `views/kept-max.json` and `views/has-search.json`, in a module
of their own (every views file carries its module whole, and this one has a
text index): regenerated whole, every existing file byte-identical.

**Not verified.** The desktop's search was driven through `update` in a
test, never on a screen; the browser build (`nix build .#harken-web`)
compiles it. A store other than `MemoryStore` serves no text index and no
extreme (the defaults scan), which is correct and unmeasured. A kept
extreme whose departure meets a store that serves nothing (`scan_ordered`
answering `None`) reads every candidate — `shape` only keeps one where the
schema's indexes serve it, so that path is a store's choice, not a plan's.
For the coordinator: the per-keystroke hydrate is linear in the rows
answered, so an unselective needle on a large library is tens of
milliseconds; patching the last needle's view (a needle extended only
narrows) would make a keystroke cost the rows that leave, if that is
wanted.

## D5. Open time, and the backend decision

Add to the harnesses: time to **open** a client replica from a snapshot of
N rows (`Peer::open` over `Dir`), a server from its `log.ark-log` and
journal, and an alone peer from its log, at 10,000 / 100,000 / 400,000
rows, with resident memory after open (read `/proc/self/status`). Report
the table. The decision that follows is the coordinator's: whether a paged
backend behind `Store` (the design names SQLite as a backend and not a
dependency; `redb` is the other candidate) is next, or memory as the
dataset stands for now. Do not build a backend in this round.

### Landed

`perf_d_open` in `rust/ark-server/tests/perf.rs` (`cargo test -p
ark-server --release --test perf perf_d_open -- --ignored --nocapture`).
The stores are written directly, as rows, so the setup is quick: media
rows as `add_song` writes them, nine columns, under harken's own schema
(`harken.ark`, as the server hosts it). Each open runs in a process of its
own, so the resident memory read from `/proc/self/status` after it is the
open's own and not the generator's. The baseline before any open is
10.4 MB.

- **store** is one `MemoryStore`, from the `replica` record: decoded,
  built, the decoded tree freed. It is what a row costs before any program
  holds it twice.
- **client** is `Peer::open` over `Dir`: a `replica` of N rows at cursor
  N of a named log, nothing pending.
- **server** is what `Builder::build` does: `persist::LogFile::open` on a
  snapshot holding the horizon's rows and the id of every entry below it,
  plus a journal of the last tenth as `add_song` records, then the state
  at the head.
- **alone** is `Peer::open` with `Options::alone`: the `replica` at its
  head and a local history of N records, all read for their ids.

Harken as it stands, with D4's two text indexes on `media.title` and
`media.creator` (`fb316b8`):

| rows | | on disk | open | resident | split, ms |
|---:|---|---:|---:|---:|---|
| 10,000 | store | 1.5 MB | 0.19 s | 46 MB | decode 14, build 138, free 8 |
| | client | 1.5 MB | 0.33 s | 86 MB | decode 16, build 152, free 12, replica open 156 |
| | server | 2.0 MB | 0.27 s | 62 MB | decode 14, build 103, free 12, journal 1,000 records 7, state at head 48 |
| | alone | 5.7 MB | 0.39 s | 87 MB | decode 10, build 121, free 11, replica open 123, history 10,000 records 86 |
| 100,000 | store | 14.9 MB | 2.3 s | 412 MB | decode 122, build 1,760, free 100 |
| | client | 14.9 MB | 4.0 s | 818 MB | decode 150, build 1,538, free 110, replica open 1,505 |
| | server | 20.4 MB | 3.5 s | 570 MB | decode 136, build 1,666, free 138, journal 10,000 records 65, state at head 661 |
| | alone | 57.7 MB | 5.0 s | 823 MB | decode 244, build 2,399, free 176, replica open 2,423, history 100,000 records 1,316 |
| 400,000 | store | 61.0 MB | 13.5 s | 1,658 MB | decode 615, build 9,466, free 441 |
| | client | 61.0 MB | 23.9 s | 3,342 MB | decode 529, build 10,126, free 414, replica open 7,087, the rest 7,218 |
| | server | 83.7 MB | 15.3 s | 2,317 MB | decode 491, build 6,312, free 455, journal 40,000 records 233, state at head 2,763 |
| | alone | 235.1 MB | 17.4 s | 3,362 MB | decode 412, build 6,276, free 426, replica open 5,055, history 400,000 records 3,285, the rest 1,694 |

The same, under the schema before D4's text indexes
(`ARK_PERF_MODULE` names a `.ark` to open with):

| rows | | open | resident | split, ms |
|---:|---|---:|---:|---|
| 10,000 | store / client / server / alone | 0.11 / 0.16 / 0.13 / 0.25 s | 32 / 44 / 37 / 45 MB | |
| 100,000 | store / client / server / alone | 0.83 / 1.4 / 1.3 / 1.8 s | 264 / 375 / 303 / 381 MB | |
| 400,000 | store | 3.3 s | 1,035 MB | decode 400, build 1,965, free 304 |
| | client | 4.7 s | 1,481 MB | decode 428, build 2,116, free 324, replica open 1,717, the rest 223 |
| | server | 6.3 s | 1,192 MB | decode 510, build 2,378, free 449, journal 40,000 records 250, state at head 610 |
| | alone | 9.8 s | 1,501 MB | decode 466, build 2,313, free 354, replica open 4,101, history 400,000 records 3,071 |

**Where the time goes**, coarsely:

- **build** is putting rows into their tables and indexes, each row's
  leaf of the state hash included (D3). It is most of every open: about
  5 µs a row without text indexes and 16–24 µs with them.
- **decode** is canonical CBOR into values, about 1 µs a row, and
  **free** is dropping those values afterwards, about another 1 µs.
- **replica open** is a peer's optimistic store starting as a copy of
  the confirmed one (`Replica::open`). It costs nearly a second build,
  and it is why a client holds about twice what one store does.
- **journal** and **history** are records decoded and appended: 6–8 µs
  a record. A peer alone reads its whole local history at every open,
  for the ids.
- **state at head** is the server copying the snapshot's store and
  applying the journal's facts. It holds both stores afterwards: the
  log's base, which it serves to a peer below the horizon, and the head.
- **hash** is the snapshot's own check, O(tables) since D3: under a
  millisecond, so it is not a column.
- **the rest** is the open less what was timed apart. It is noise at 10k
  and 100k. At 400k it is seconds, and it moved between runs.

**Memory**: resident per row, the baseline taken off.

| | one store | client | server | alone |
|---|---:|---:|---:|---:|
| without text indexes | 2.6 KB | 3.7 KB | 3.0 KB | 3.7 KB |
| with D4's two | 4.1 KB | 8.3 KB | 5.8 KB | 8.4 KB |

On disk a row is about 150 B in a snapshot. Time and memory both grow
linearly over the three sizes, and no term grows faster.

Not verified: the timings were taken on a 4-core machine shared with four
other builds, at load averages between 7 and 12. The same open moved by
up to half between two runs, so read the times as a scale. The resident
memory was the same to 0.1 MB in every run. Nothing here was measured on
a phone. The rows are one shape, media; a library's other tables (songs,
works, playlists) would add their own indexes.

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

### Landed

- **`arkc backup DIR OUT`, `arkc restore BACKUP DIR [--same-log]`,
  `arkc verify-log DIR [M]`** (`e32aa0f`; `rust/ark/src/bin/ops.rs`).
  - A backup reads the snapshot, then the journal up to the length it
    had once the snapshot had been read, cut to its last whole record
    that follows on, then every other top-level file: cursors, sessions,
    rooms, modules.
  - A compaction between the two reads is seen, because the snapshot's
    file changes (inode, length, mtime), and the pair is read again.
    Without that check the copy is still a consistent prefix, but an
    older one than the disk held when the backup began.
  - Directories (the scanner's `library/` replica) are a peer's, not the
    server's, and are not copied.
  - `verify-log` reads without the module, schema-free: head, horizon,
    log id, entry and id counts, stale records, a torn tail, and the
    modules `modules.cbor` lists. Given the module, it checks the
    snapshot's rows against its hash, replays the journal, prints the
    state hash at the head, and says whether that module is one the
    server has run. The brief's `verify-log DIR` cannot print a hash
    alone, because the state hash is over the schema's tables in schema
    order, and a snapshot does not carry its schema.
  - The guards are `ops_tests.rs`, each falsified once (its doc comment
    says how):
    - the length rule: reading to the journal's end gives a head of 13
      where it was 10;
    - the re-read: without it the copy's head is 5 where the disk held 8;
    - the unnamed restore;
    - a backup taken while another thread appends and compacts as fast
      as it can, forty times, each a loadable prefix at least as long as
      the disk was when it began.
- **A restore is a new log** (a decision this item made). The backup is a
  prefix of a history some peers saw more of. Under the same log id, a
  peer that dials only after the restored server has sequenced past its
  cursor would be handed the new entries on top of the old ones, and
  nothing would tell the two apart (§12.4). So `restore` writes the
  snapshot unnamed, the hub names it at start, and every peer is sent the
  snapshot once. `--same-log` keeps the name, for a stopped server moved
  elsewhere.
- **The fleet** (`70b06c0`): `a_backup_taken_mid_stream_restores_to_its_moment`.
  - Three peers push round after round while another thread backs up the
    running server's directory with `ops::backup`. On this run the backup
    took 3 ms and its head was strictly between the heads converged
    before and after it.
  - `verify-log` on the copy gives the full log's hash at that head.
  - The copy is restored into a new directory and the server started
    there. A fourth device sequences past the others' cursors before
    they return.
  - All four converge on the restored log, 733 ms from the restart, and
    no entry appended after the backup's journal length is in any replica
    or in the log.
  - Falsified by restoring with the name kept: the three are paged the
    new history on top of the old and never converge (different hashes
    at one head, 31). Falsified again by `/healthz` answering text
    whatever is asked.
- **`/healthz` as JSON** (`70b06c0`). Under `Accept: application/json` it
  is one object (`ark_server::health_json`): `head`, `horizon`, `log`,
  `module`, `modules`, `connections`, `rooms`, and `sessions` with `user`,
  `session`, `cursor`, `heard_ms` and `open` (the connections it has open
  now). Ids and hashes are hex. The text form is unchanged. A session's
  `cursor` is where `cursors.cbor` records it: the start of the last page
  delivered, at the lowest of its open connections. That can be below the
  head of a peer that is caught up.
- **The ids below the horizon** (`ce4bbed`):
  - `Log::below` keeps each id at or below the horizon as 8 bytes and its
    sequence, in a sorted vector. `seq_of` asks it after the exact map,
    so a re-push older than the snapshot is answered `Duplicate` at the
    sequence it recorded.
  - **The 8 bytes are the first of the id's SHA-256, not of the id**, a
    departure from the brief's wording. An id built from a counter (every
    test's, and any peer that numbers its own) differs only in its last
    bytes, and a UUID has a version nibble in its first half. A digest's
    prefix is uniform whatever the id looks like, which is what makes
    2^-64 true.
  - The hub folds after each compaction and when it opens its log.
    `compact_to` and reading a file back do not fold, so a log written
    and read is the log it was. A peer alone never folds.
  - The snapshot writes `{ key: Bytes(8), seq }` beside `{ id, seq }`,
    and readers take both, so old files open. D1's `rehome` was taught the
    keys too; dropping them would have let a re-push be applied twice.
  - Measured (`perf_ids_below_the_horizon` in `rust/ark/tests/perf.rs`,
    the bytes this thread holds, counted by the harness's allocator):

    | | per id | per million |
    |---|---:|---:|
    | before: `BTreeMap<Id, Seq>` | 38.8 B | 38.8 MB |
    | after: below the horizon, by key | 16.0 B | 16.0 MB |

    The first fold of a million ids takes 198 ms. Later folds move only
    what the horizon passed, plus one merge. `seq_of` by key costs 0.6 µs
    against 0.3 µs exact, and every pushed intent that is not a duplicate
    asks it once.
  - Guards: a duplicate below the horizon refused through the authority
    (falsified by `seq_of` forgetting the keys); the keys surviving the
    snapshot (falsified by writing them out of it); the hub's R10 test
    counting 12,000 exact and 18,000 keyed (falsified by the hub not
    folding: 30,000 exact).

Not verified:
- A backup or restore against a NixOS deployment's state directory.
- A backup of a directory larger than the fleet's: the copy reads the
  snapshot and journal whole into memory.
- The stamp check on a filesystem whose inode numbers or mtimes do not
  move on a rename. That is never the case on Linux, and off unix the
  check is the length alone.
- A restore's new log name costs every peer one snapshot. Nothing here
  measured that on a large library; at 400,000 rows a snapshot is the
  61 MB a client's replica is.

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

## D5, decided: no paged backend yet — a memory round first

The open-time table says the dataset is memory and open time is linear in
it: a client holding 100,000 media rows opens in about 4 s and 820 MB with
harken's two text indexes, 1.5 GB per 400,000 rows without them. Two
things in that number are not the store's fault and are cheaper to remove
than a backend is to build: the trigram postings (a `BTreeSet` of
`Vec<Value>` keys per trigram — about half of every row's footprint once
the two text indexes exist; sorted `Vec<u32>` row ordinals would be a
fifth of it) and the optimistic store opened as a full copy of the
confirmed one (two map trees and two sets of indexes over the same `Arc`
rows). Decision: a **memory round** before any backend — postings as
ordinals; the view sharing the confirmed store's tables and indexes
copy-on-write until the first pending write; per-row bytes measured by
component (map node, `Arc<[Value]>`, `Value` width, index postings, digest
leaf) and the two or three largest shrunk — with the open-time table re-run
after. A paged backend behind `Store` is decided again when a dataset
exceeds what that round leaves, which no app here approaches: harken's
largest library is tens of thousands of rows.

## D7. The memory round

What D5 decided, worked out. The dataset is memory and open time is linear
in it, and the open-time table says where the bytes are only coarsely: a
client at 100,000 media rows is 818 MB with harken's two text indexes and
375 MB without, against 14.9 MB on disk. Three things in that number are
the representation's and not the data's, and this round takes them in the
order they can be *measured*, because the last time a number was reasoned
about rather than read (`cargoVendorHash`, in harken's covers commit) the
reasoning was wrong.

### D7.1 Bytes by component, before anything is shrunk

`perf_d_bytes` in `rust/ark-server/tests/perf.rs`, beside `perf_d_open`:
a counting global allocator (the one `rust/ark/tests/allocations.rs`
already has) reads *live* bytes, and the table is built by subtraction —
the rows alone held in a `Vec<Row>`; the store under a schema stripped of
every index and text index (the primary map); with the secondaries back;
with the text indexes back — at 10,000 and 100,000 rows, printed as bytes
per row per component beside the resident figure `perf_d_open` reads from
`/proc`. Two gaps are expected and each decides something:

- **Live against resident.** `perf_d_open`'s build decodes a `Value` tree
  first — a `Struct` of nine fields per row, each a `BTreeMap` node of
  hundreds of bytes — and frees it after. glibc keeps what was freed in its
  arenas, so the resident figure carries the tree's high-water mark for the
  process's life. If live is well under resident, the fix is a decode that
  builds each `Row` straight from the CBOR (`Row::new(cols, vals)` as the
  row's map is read, no tree), which also removes the `decode` and `free`
  columns of the open-time table — about 2 µs a row of 16–24.
- **The sum of components against the rows alone.** 2.6 KB a row where
  the row itself is perhaps 500 bytes says the structure around a row costs
  more than the row. Which structure is what D7.2 and D7.3 are sized by.

### D7.2 Postings as ordinals

A posting today is a `Key`, which is a `Vec<Value>`: twenty-four bytes of
header, a heap block of thirty-two per key column, the allocator's word on
top, and a `BTreeSet` node around it — close to a hundred bytes to say
"this row". A media row is said about forty times by its two text indexes
(a title and a creator's trigrams) and once by each secondary, which is
why the text indexes doubled a client. The ordinal is the one answer for
both kinds of index:

- **Each table numbers its rows.** `tables` holds, per table, the map it
  has (`BTreeMap<Key, Row>`, by key, which is what every scan's "in key
  order" reads) with a `u32` ordinal beside each row, and the reverse —
  `Vec<Option<Row>>` by ordinal, a free list of ordinals whose row went —
  so that a posting resolves to its row in one index. A `Row` is two
  `Arc`s, so the reverse table is twenty-four bytes a row and shares every
  value. Ordinals are per table and never cross the store's boundary:
  nothing durable, nothing hashed and nothing on the wire names one.
- **A posting list is a sorted `Vec<u32>`.** Four bytes a posting.
  Insertion is a binary search and a shift, removal the same; a trigram
  nobody holds any more is taken out of its map as its set is today. A
  `Pred::Has` is the intersection of its needle's trigrams' lists, which
  over sorted vectors is a merge from the shortest — linear, where the
  `contains` probe of a set was logarithmic per candidate — and the
  result is the rows, in key order (D7.2's third rule).
- **Key order is restored by sorting what was read, not kept in the
  index.** The sets gave each bucket's keys in order for free; vectors of
  ordinals give ordinal order, and `scan_where_eq`, `scan_ordered` and
  `scan_where_text` all promise key order. So a bucket read through an
  index is sorted by key before it is returned — by `compare_rows` on the
  key, over the rows, in place. That is `O(b log b)` over the rows *read*,
  never over the table, and in practice near linear: a store opened from
  a snapshot numbers its rows in the order the snapshot holds them, which
  is key order, so a bucket with no row written since open is already
  sorted and Rust's sort sees one run. `scan_ordered` sorts one bucket at
  a time as it walks them and stops at `limit`, so `MAX(pos) + 1` still
  costs the rows up to the first `keep` admits (R1).
- **Equality ignores ordinals.** Two stores built in different orders
  number the same rows differently and are the same store;
  `PartialEq for MemoryStore` compares rows, and the fuzzer's equality
  checks (D2) are what would catch a derived `Eq` that compared the
  numbering. Falsify once by deriving it.

Nothing about `Overlay` changes: it never held an index.

### D7.3 The view shares the confirmed store, copy-on-write, per table

A replica's optimistic view starts as `confirmed.clone()` and is rebuilt
that way at every full replay — a second map tree and a second set of
indexes over the same `Arc` rows, which is why a client holds twice what a
store does. The copy is of tables pending never touches: harken's pending
writes `playlist` and `playlist_item`, and the copy is of `media`.

- **A table is an `Arc`.** `MemoryStore` holds, per table, one
  `Arc<TableState>` — rows, ordinals, secondaries, text postings, digest,
  together — and `Clone` is a clone of that map: `O(tables)`, sharing
  everything. `set` reaches its table through `Arc::make_mut`, so the
  first write a store makes to a table it shares copies that table and
  nothing else, and every write after is in place. A pending
  `add_to_playlist` copies `playlist_item`; `media` is never copied until
  something pending writes it.
- **The view shares what it has not written, and keeps what it has.**
  The view is `confirmed` with `pending` replayed, so every table no
  pending write has touched since the last replay is *equal* to
  confirmed's and is confirmed's `Arc`. When confirmed moves — a landed
  batch, an own intent confirmed from its record, a snapshot adopted, the
  horizon's rewrite — one wrapper around the write has the view *release*
  the tables it shares first, writes confirmed in place (its `Arc`s now
  unique), and has the view take them back as confirmed's `Arc`s, or as
  absent where confirmed has none. Without the release, confirmed's write
  would be the copy — `make_mut` on an `Arc` the view also holds — so a
  quiet client would copy `media` once per batch; that is the trap, and
  the test for it is a client receiving a hundred batches and copying no
  table. A table the view has written is its own copy from then until the
  next replay or open: it takes landed changes as R2 always applied them,
  and it is **not** re-shared when the intent that wrote it is confirmed.

  It was going to be — the first draft of this section re-shared every
  table no record touched after every move of confirmed, so that an own
  `add_song`'s copy of `media` went when it was confirmed — and that rule
  was measured before it was committed: for a peer whose pending empties
  between mutations, which is every peer alone and every connected client
  at rest, each mutation copied its table (`make_mut` on a shared `Arc`),
  the confirm freed the copy, and the next mutation copied it again.
  `perf_a_mutate_alone` went from 23.6 µs flat to 2.8 ms at 8,000 items,
  growing with the table, which is the property every round of
  `docs/plan-perf.md` held and harken's "sub-10 ms, held under spamming"
  rests on. Freeing is `O(table)` as copying is, so no cheaper point to
  re-share at exists without a persistent map, and that is not this
  round. The cost of keeping the copy is the status quo for that table
  — a client held two of every table before — and nothing for the
  tables pending never writes, which for harken's clients is `media`.
  The replica keeps the set of tables the view has written since the
  last replay (`diverged`), which is what the wrapper releases around and
  what the assertion below is checked against.
- **Tables pending has written take landed changes as before.** R2's
  rebase — undo the records newest first, apply what landed, run pending
  again — is unchanged for a table the view has diverged on; sharing only
  takes the tables it has not.
- **What is counted changes.** `store::clones()` counted whole-store
  copies and the replica is held to none per mutation (`peer::tests`);
  that holds still and is cheap now. `store::copies()` counts *table*
  copies — a `make_mut` that found its `Arc` shared — and three tests pin
  it: a hundred landed batches on a quiet client copy nothing; one pending
  intent over `media` copies it once, and confirming it and then writing
  it again copy nothing more; a replay with ten pending intents over one
  table copies it once, not ten times.
- **The server shares for free, and keeps two copies of `media` anyway.**
  The log's base store and the state at the head are one `clone` and a
  journal of facts, and the journal's `add_song`s write `media`, so the
  head copies it on the first. Dropping the base and rebuilding it from
  disk for the rare peer below the horizon is the way to lose that copy;
  not this round.

A debug assertion holds the invariant after every pump — the view's `Arc`
is confirmed's exactly for the tables not in `diverged` — beside the equality
the replica tests already assert, so the two cannot drift silently.

### D7.4 The two or three largest components after that, and the table

D7.1 is re-run after D7.2 and D7.3 and whatever is then largest is next,
up to two or three of them, each measured before and after. The candidates
by reasoning, to be confirmed by the numbers and not taken on them:

- **The primary map's key** is a second `Vec<Value>` per row, holding
  values the row already holds. A row can be its own key: a map keyed by a
  wrapper over the `Row` whose `Ord` compares the key columns, found
  through positions `Columns` carries for them. Seventy-odd bytes and an
  allocation a row, and the row's ordinal goes beside it.
- **The decoded tree at open** (D7.1's first gap), if live and resident
  disagree.
- **`Value` itself**: thirty-two bytes a column whatever it holds. Not
  this round unless the rows alone are the largest component, which the
  estimate says they are not.

Then the open-time and memory tables of D5 are re-run on the same
machine and recorded here, with the per-component table beside them. The
spec moves not at all — the store's representation is this crate's and
the state hash, the snapshot and the vectors are what they were — and
`spec/vectors` stays byte-identical, which `checks.vectors` says.

### Guards

- `checks.rust` and `checks.fuzz-smoke` as they stand; `allocations.rs`'s
  bounds — a sort in place allocates nothing beyond the answer.
- The three `copies()` tests above, each falsified once: dropping the
  release step, re-sharing on confirm as the first draft did, cloning the
  store in `replay` as before — and `perf_a_mutate_alone` stays flat.
- `perf_d_bytes` before and after each step, in `--release`, in the
  commit message of the step.

### Landed

Five commits: `cbb799d` (D7.1), `ed5028e` (D7.2), `f9adeb2` (D7.3),
`119b798` (D7.4), and this record. All numbers are release builds of
`rust/ark-server/tests/perf.rs`, media rows under harken's schema, on the
4-core machine D5 was measured on. Load averages were 1 to 2.5, against
D5's 7 to 12.

**D7.1, the instrument.** `perf_d_bytes` counts *live* bytes — what the
allocator was asked for and has not had back — with a counting global
allocator, and builds the table by subtraction, each variant in a process
of its own. `perf_d_open_child` prints live beside resident, so the open
table has a live column now. The first reading (100,000 rows, live bytes
a row):

| component | before | after D7.2 | after D7.3 and D7.4 |
|---|---:|---:|---:|
| the rows, alone in a `Vec<Row>` | 393 | 393 | 393 |
| + the primary map | 87 | 134 | 134 |
| + the secondaries (`file`, `pos`) | 888 | 296 | 296 |
| + the text indexes (`title`, `creator`) | 1,271 | 90 | 90 |
| one store | 2,639 | 912 | 912 |

At 10,000 rows every figure is within a few bytes of these. The two
expected gaps were both there:
- The structure around a row cost more than five times the row.
- An open's resident memory was 1.5 to 1.7 times its live bytes:
  4,208 B a row against 2,638 for one store, 8,464 against 4,925 for a
  client.

**D7.2, postings as ordinals** (`ed5028e`).
- Each table is a `TableState`. Its rows are held by key with a `u32`
  ordinal beside each, and again by ordinal, with a free list.
  Secondaries and trigrams post sorted `Vec<u32>`s.
- An Edit keeps its ordinal, so an index whose columns did not move is
  not touched at all.
- Key order is restored by an unstable sort in place over the rows
  read. `scan_ordered` sorts a bucket as it reaches it and asks `keep`
  of the same rows as before.
- `PartialEq` and `Debug` read rows only.
- Where it moved: the secondaries went from 888 to 296 B a row (a bucket
  of one key was a whole `BTreeSet` leaf), and the text indexes from
  1,271 to 90. These titles are short, about sixteen trigrams a row.
- The primary map grew by the ordinal and the reverse table, 87 to 134 B.

**D7.3, the view shares what it has not written** (`f9adeb2`).
- One `Arc<TableState>` per table. `Clone` is `O(tables)`, and a write
  goes through `make_mut`.
- A store records the tables it has written since it was made or cloned
  (`MemoryStore::written`) on the one path every write takes. A
  replica's view therefore knows its `diverged` (`Replica::written`)
  without a call site remembering it.
- Every move of the confirmed store goes through `move_confirmed`: a
  landed batch, an own intent confirmed from its record, `fork_back`'s
  rewrite. The view releases the tables it has not written (asserted to
  be exactly the ones it shares), confirmed is written in place, and the
  view takes them back. A snapshot adopted is a `Replica::open`.
- A table the view has written is its own copy until the next replay,
  and takes what landed as R2 applied it.
- `Replica::settle` asserts after every pump that the view shares
  exactly the tables it has not written.

The first draft re-shared on confirm, and was measured before it was
committed (above, in the design): `perf_a_mutate_alone` went from
23.6 µs to 409 µs at 2,000 items and 2,808 µs (l/f 170) at 8,000. As
landed:

| `perf_a_mutate_alone`, whole | at D7.2 | landed |
|---|---|---|
| 2,000 items | 22.5 µs, l/f 1.33 | 22.6 µs, l/f 1.17 |
| 8,000 items | 23.6 µs, l/f 1.23 | 23.0 µs, l/f 1.18 |

A client now holds what one store holds: 87.3 MB live at 100,000 rows,
against 87.0 for the store, 134.9 after D7.2 and 469.7 before.

The three `copies()` tests, each falsified once:
- `a_quiet_client_copies_no_table_per_batch`: a hundred batches of
  three copy no table. Without the release it fails on batch 0. The
  retake's debug check fires first, and with that check off the count is
  1 against 0.
- `a_pending_intent_copies_its_table_once_and_keeps_it`: one add over
  500 items copies `item` once, and its confirm and two more adds and
  confirms copy nothing. Re-sharing on confirm made it fail ("the view
  keeps its copy"; with that assertion off, the second add copied
  again, 1 against 0).
- `a_replay_copies_each_table_it_writes_once`: an open replaying ten
  adds copies `item` once. Making every write copy its table counted 10.

**D7.4, the decoded tree** (`119b798`). With D7.2 and D7.3 in, an open
was 2.4 times live (87.0 MB live, 206.5 resident above the baseline, at
100,000 rows), so
the tree came first. `canon::decode_rows` reads the rows at a path of a
record — `confirmed` in a replica, `base.rows` in a log's snapshot — and
hands each row's fields to the reader in key order, names borrowed. It
is exactly as strict as `decode`, the key checks shared through one
`each_pair`. `Row::from_fields` lays them out as the table's row.
`decode_replica` and `journal::decode_snapshot` read this way;
`MemoryStore::from_value` stays for the vectors. The guard,
`rows_decoded_in_place_are_the_rows_decoded`, was falsified by dropping
what is not a struct instead of keeping it.

| open, 100,000 rows | live MB | resident MB less baseline |
|---|---:|---:|
| store, before D7.4 | 87.0 | 206.5 |
| store, after | 87.0 | 102.6 |
| client, before | 87.3 | 206.5 |
| client, after | 87.3 | 105.2 |

Resident is 1.18 times live now, which is the allocator's rounding. The
decode, build and free columns of D5's split are one pass, "read".

**And the wire.** The third place a store arrives whole — a `snapshot`
frame, to a peer below the horizon or of another log — reads the same
way. `ServerMsg::decode_for` decodes every frame with its `rows` (only a
snapshot has them) through `decode_rows`, each row built by
`Row::from_fields` as it is read, and hands what is left to
`ServerMsg::from_value`, so the same frames are refused in the same
words. `Client::recv_snapshot` adopts the rows, projecting them there
when the peer is behind, since the frame's `module` follows its `rows`.
`ark_client::Peer` reads the socket only this way. The sender and the
wire are untouched: `spec/vectors` is byte-identical. `perf_d_frame`
decodes and adopts a frame of harken's media rows into a client, each
way in a process of its own:

| snapshot frame, 100,000 rows (14.9 MB) | high-water live MB | held MB | resident MB | ms |
|---|---:|---:|---:|---:|
| decoded whole, before | 165.0 | 87.0 | 213.8–232.7 | 1,142–1,223 |
| as read, after | 90.0 | 87.0 | 103.3 | 769–797 |

`a_snapshot_read_as_it_is_decoded_is_the_snapshot_read_whole` holds the
two reads to one store, behind and not, and to the same refusals;
`check_protocol` reads every `protocol/` vector both ways; and the
fuzzer's frame check reads every server frame both ways.

The primary map's key, a row as its own key, was **not** done. At 134 B
it is the second smallest component, and the brief made it conditional
on being the largest. The largest is now the rows themselves (393 B:
nine 32-byte `Value`s and five strings), which the design deferred
unless they were the largest. They are, and shrinking `Value` changes
every match on it in every crate, so it is the next round's question if
there is one. After the rows come the secondaries (296 B for two
indexes, most of it the bucket's key: a copy of the column's value in a
`Vec<Value>`).

**The open table again**, harken's schema with D4's two text indexes.
Resident is as `/proc` reads it, the baseline (10.5 MB) included as in
D5:

| rows | | on disk | open | resident | live | split, ms |
|---:|---|---:|---:|---:|---:|---|
| 10,000 | store | 1.5 MB | 0.08 s | 17 MB | 9 MB | read 70 |
| | client | 1.5 MB | 0.09 s | 20 MB | 9 MB | read 69, replica open 1 |
| | server | 2.0 MB | 0.10 s | 27 MB | 17 MB | read 67, journal 1,000 records 6, state at head 13 |
| | alone | 5.7 MB | 0.15 s | 20 MB | 10 MB | read 71, replica open 1, history 10,000 records 64 |
| 100,000 | store | 14.9 MB | 0.89 s | 113 MB | 87 MB | read 855 |
| | client | 14.9 MB | 0.89 s | 116 MB | 87 MB | read 808, replica open 1, the rest 82 |
| | server | 20.4 MB | 1.2 s | 203 MB | 164 MB | read 774, journal 10,000 records 93, state at head 163 |
| | alone | 57.7 MB | 1.6 s | 119 MB | 92 MB | read 872, replica open 1, history 100,000 records 620 |
| 400,000 | store | 61.0 MB | 3.9 s | 433 MB | 351 MB | read 3,539 |
| | client | 61.0 MB | 3.9 s | 436 MB | 351 MB | read 3,686, replica open 1, the rest 296 |
| | server | 83.7 MB | 4.9 s | 788 MB | 658 MB | read 3,346, journal 40,000 records 255, state at head 711 |
| | alone | 235.1 MB | 6.5 s | 453 MB | 370 MB | read 3,599, replica open 1, history 400,000 records 2,451, the rest 485 |

Under the schema before D4's text indexes (`ARK_PERF_MODULE`):

| rows | store / client / server / alone: open | resident | live |
|---:|---|---|---|
| 10,000 | 0.05 / 0.06 / 0.08 / 0.13 s | 17 / 19 / 25 / 19 MB | 8 / 8 / 15 / 9 MB |
| 100,000 | 0.57 / 0.57 / 0.81 / 1.2 s | 106 / 108 / 187 / 111 MB | 78 / 79 / 145 / 84 MB |
| 400,000 | 2.4 / 2.3 / 3.4 / 5.0 s | 403 / 406 / 725 / 423 MB | 315 / 315 / 582 / 334 MB |

**Memory**: resident per row at 400,000, the baseline taken off, against
D5:

| | one store | client | server | alone |
|---|---:|---:|---:|---:|
| without text indexes, D5 | 2.6 KB | 3.7 KB | 3.0 KB | 3.7 KB |
| without text indexes, now | 1.0 KB | 1.0 KB | 1.8 KB | 1.0 KB |
| with D4's two, D5 | 4.1 KB | 8.3 KB | 5.8 KB | 8.4 KB |
| with D4's two, now | 1.1 KB | 1.1 KB | 1.9 KB | 1.1 KB |

What moved:
- **A client is one store**, and at 400,000 rows a client is 436 MB
  resident where it was 3,342 MB.
- **Open time**: a client at 100,000 rows from 4.0 s to 0.9 s, at
  400,000 from 23.9 s to 3.9 s. The "replica open" column went from
  seconds to a millisecond, and "free" went altogether.
- **The text indexes now cost** about 0.1 KB a row where they cost
  1.5 KB.

What did not:
- The server still holds two copies of `media`: the log's base, and the
  head, which copies it at the journal's first `add_song`. That is
  1.9 KB a row against a store's 1.1, as the design said it would stay
  this round.
- "History" for a peer alone, reading its whole local history for the
  ids, is still 6 µs a record (2.5 s at 400,000), and is now the larger
  part of an alone open.
- "Read" is still about 9 µs a row with the text indexes and 5 µs
  without. Building is the rest of every open, as in D5.

Green at the last commit:
- `cargo test --workspace` and `cargo test -p harken-iced --features
  demo` (harken's domain, server and `fleet.rs` included).
- `cargo fmt --all --check` and both clippy invocations, `-D warnings`.
- `arkc fuzz --seed 1 --cases 25`: 0 findings.
- `ark-vectors` diffed against `spec/vectors`: identical. The spec did
  not move.
- `rust/ark/tests/perf.rs` and `rust/ark-server/tests/perf.rs`, all
  ignored tests, release. Everything flat that was flat:
  `perf_c_pump_alone` and `perf_b_watcher` l/f between 0.5 and 1.6.
- `allocations.rs`'s bounds.

Not verified:
- `nix flake check` itself was not run here.
- Nothing was measured on a phone.
- Rows are one shape, media, with short titles, so the text indexes'
  90 B a row is these titles' and not a real library's. Real titles of
  forty characters would post about forty ordinals a row, 160 B.
- The decode in place is used by the client's replica, the server's
  snapshot and, since, a snapshot frame on the socket. The vectors'
  runner and the fuzzer still also decode frames whole, as the reading
  every other is checked against.
- The timings were taken once each, so read them as a scale. The live
  bytes moved by under 0.1 MB between runs.
