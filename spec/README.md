# The ArkDB specification

Spec version 4. `rust/ark` is the specification: its modules are
normative, and a runtime in any language is conformant when it agrees
with them on the vectors under `vectors/`, which `rust/ark` writes and
`rust/ark/tests/vectors.rs` reads — two programs against one set of
files. This file is the index: which module is normative for which
section, the few requirements that cannot be a function, and the
vectors. The prose that would otherwise be a specification document is
in the doc comments beside the definitions it explains. `AUTHORING.md`
is the contract a domain is written against; `docs/arkdb.md` is the
architecture, and `docs/plan-v4.md` the design of this version.

Until spec v3 the specification was a Haskell program here, and every
rule was written twice. Swift and Kotlin are frozen at v3
(`swift/FROZEN.md`, `kotlin/FROZEN.md`) and come back against the
vectors of the day they do.

## Reading order

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1 | `value.rs` | the eight run-time values and the one total order over them |
| 1.3 | `canon.rs` | the canonical encoding: RFC 8949 §4.2.1 deterministic CBOR plus the mapping; the decoder that refuses anything else |
| 2 | `schema.rs` | tables, columns, indexes, references; the derived relationships; well-formedness; a table's rules (`docs/plan-auth.md`) — `visible` and `writable`, each a predicate over the table's own columns with `Me` (the context's user), `Pred::Role` and the one lookup `Pred::Exists` (some row of a table referencing this one, admitted by that table's own predicate), absent for `Everyone` and then not encoded, so a module that declares none is the bytes it was |
| 3 | `ir/mod.rs` | the function language: mutators, queries, helpers, guards and providers; routers (§3.10); input fields and their checks (§3.11); statements — the three table writes among them — and expressions; the standard library's names |
| 3.3 | `ir/mod.rs` (`Plan`) | a query is a plan and nothing else: source (a table or a group), filter, lookups, related plans, having, projection, order, limit (`docs/plan-v4.md` §1.3) |
| 4 | `store.rs` | the store as a value: `get`, `scan`, `put`, `delete`, and the three writes a body makes of them — insert (§4.3a), upsert (§4.3b), update (§4.3c); constraints as refusals — and `Forbidden`, a row a table's `writable` rule does not admit for the author, judged after the run by the authority for the connection's identity and by a device for its own login's roles before an intent is pending (`rules.rs`, `docs/plan-auth.md`); what a write reports. Its indexes, each a hint a read is served by and none a fact about the rows: per declared index and reference column, the rows under each value, read by equalities, a range and an order (`scan_where_eq`, `scan_ordered`; `docs/plan-perf.md` R1, R6); and per text index, the rows under each trigram of a column's value folded by the pinned `lower`, which serves `Pred::Has` as the intersection of the needle's trigrams' postings (`scan_where_text`, `docs/plan-db.md` D4). A posting names a row by the ordinal its table numbered it with, and a read through an index is put in key order (`docs/plan-db.md` D7.2); each table is shared between a store and its clone until one of them writes it (D7.3) |
| 5 | `stdlib.rs`, `unicode_tables.rs` | the standard library's semantics; the three pinned Unicode tables, generated from UCD 16.0.0 by `bin/gen-unicode.rs` |
| 6 | `eval.rs` | what a mutator means, which every runtime's native procedures are held to: `apply`, evaluation order (checks, middleware, body), checked arithmetic (§6.5) — with one exception, a sum in a plan's node: a `fold` whose step is `acc + f(x)` with `f` free of the accumulator, there or in a helper a node calls, adds `init` and every term in an `i128` and checks the total once, so an overflow on the way, which depends on the members' order, is not a refusal; each term is still checked. The reason is §13: a view keeps that sum as a number moved by its members in no particular order, and the two evaluation orders must agree. A procedure's body keeps step-by-step checking, since its natives run a fold as a closure no recogniser can see into (`docs/plan-db.md` D4); the form validator (§6.7) and the default messages (§6.8) |
| 7 | `ir/encode.rs`, `ir/normalize.rs` | the module as a value (its wire and storage form); alpha-normalisation (§7.1), a plan's binders with the function's |
| 7.2 | `ir/decode.rs` | a module, a closure, a schema or a type from its value: the inverse of §7, strict about shape |
| 8 | `hash.rs`, `sha256.rs` | the state hash (§8.1, below): a sum of row leaves per table, so that a store keeps it as it writes; closures and the function hash, which covers the helpers a function calls and the middleware it runs |
| 9 | `verify.rs` | what a module must satisfy before anything runs or hashes it, a plan's typing and scope included (`docs/plan-v4.md` §1.10) |
| 10 | `log.rs` | an entry; the module's one log with the facts kept beside each entry; snapshots, the horizon, the state at any retained sequence from facts alone; the log's identity, on its snapshot |
| 11 | `peer.rs` | the replica: pending intents, the optimistic view, the rebase — once per settle, after every frame a pump received was placed in the inbox, not once per frame (`receive` places, `settle` applies; `docs/plan-perf.md` R8) — work authored before anyone signed in made the signer's, applying by intent or by facts, divergence detection; the fork a replica remembers, the log and cursor it last shared with a server, and the local history since it taken back out of the confirmed store and re-queued as pending (`fork_back`, `docs/plan-alone.md` §1); the authority: sequencing, dedupe, verdicts, compaction, retirement of closures no retained entry names, adoption of a log; `local_commit`, the serverless peer, whose log keeps a head and ids and whose entries are kept on its storage (`journal.rs`) |
| 12 | `protocol.rs` | the frames as values and their encodings; the client machine and the server machine (authenticate once, hold every entry to its identity, give every refusal a reason, sequence, fan out a page at a time, rooms); the log's identity as `log`, an id, in `hello`, `batch` and `snapshot` — absent where no log is named, which is the one encoding of that (a null is refused), so a frame naming none is the bytes it was and a v4 runtime that never names a log is conformant as it was, while one that names it emits the field; a `hello` naming another log than the authority's is answered with its snapshot at the head, as one past the head is, and one naming none is served as before (`docs/plan-perf.md` Round 4); across versions (`docs/plan-db.md` D1), three decisions about meaning: **closure provenance** — an authority keeps the closures of every module it has run (`Authority::ran`, kept by `retire`), so an intent at a hash it once ran is sequenced whatever module is current; **an unknown function is a hold**, not a verdict — `held` (id, reason) beside `reject`, whose bytes are unchanged, and the client keeps the intent pending and pushes it again on its next connection (a `reject` saying `unknown function <the intent's hash>`, from a server before holding, is read the same way); **an older schema applies facts projected** — `batch` and `snapshot` carry the server's module hash as `module`, bytes, absent where none is said (as `log`), a server that says one answers a `hello` at its head with an empty page carrying it, and a client whose own module differs is `behind`: facts are projected to its schema before they are compared or applied (`store::project`: a column or table it lacks dropped, a nullable column it has and they lack `Null`, a required one missing still `MalformedRow`), and it says no `verify`; a fact row that only lacks a nullable column is widened whether or not the client is behind; a `verify` names the log its sequence is of as `log`, absent where the client knows none (as `hello`'s), and an `agree` carries `unknown: true`, present only when true, where the authority cannot say — the sequence below its horizon or past its head, or a `verify` naming another log than the authority's, which is never compared (`docs/plan-db.md` D3, D2) — so a `verify` naming none is answered as it always was and a client older than the field is conformant as it was; and a table's rules (`docs/plan-auth.md`): a push is sequenced only if every row it writes is admitted by its table's `writable` rule for the connection's identity, which carries the roles the authenticator said (`Identity::roles`; dev auth's token `name:role,role`), and is otherwise a `reject` naming the table; a connection to which every table is `Everyone` is served exactly as before, and any other is **partial** — every connection of it starts from a `snapshot` with `partial: true` of the rows it may see and their state hash (`rules::partition_hash`), and every page after carries `after` and `upto`, the sequences it covers (a client told a page continues from past where it has been told asks again from its cursor, and a partial connection asking from anywhere but where it was sent is started over from a snapshot), with only the entries that have a fact it may see (or are its user's own), each with those facts (an edit across the line as the add or the remove it is to that peer, then the rows the entry made visible or hid through a rule's lookup as adds and removes) and another person's intent as its envelope, arguments and autos empty; a partial client applies facts only, never replays another's intent, moves its cursor to `upto`, keeps `partial` beside its cursor, and says `partial: true` in its `hello`, which a server that finds the identity whole answers with its snapshot at the head; its `verify` says `partial: true`, is said only after its connection's snapshot, and is answered from the partition digest of the state at its sequence — `unknown` when it claims the other kind than the connection is served. `partial` and `upto` are absent where false or none, so every frame of a whole peer is the bytes it was |
| 13 | `view.rs` | what a plan means (`pull`, the one evaluator of plans: a query, a mutator's `select`, a hydrating view) and a plan kept up to date: entries, dependencies by plan node, patches, the window under a limit, the correctness contract (`docs/plan-v4.md` §1.5); and the aggregates a view keeps as numbers rather than lists, recognised from the plan and never sent — four of them: a count and a sum (`docs/plan-perf.md` R9), a least and a greatest value (`docs/plan-db.md` D4), the last two only where an index serves the column, a departure of the extreme being one indexed read. A recognised aggregate is arithmetic over the integers and only its result must fit an `i64`: a kept count or sum is moved in an `i128` by each member that arrives or leaves and judged only when a node reads it — an overflow there is the refusal a fresh `fold` gives, and one on the way is none, matching the fold's own rule (§6.5's exception). `views/a-maintained-sum-*` are D2's finding of the two disagreeing, both ways |
| 13.7 | `ir/reads.rs` | what a query's middleware reads, by static inspection: the tables whose change re-runs it before a view is pushed (`docs/plan-v4.md` §1.7) |
| 14 | `live.rs` | rooms per account over opaque frames: arrive, speak, depart; a snapshot kept when a room empties; a repeated hello is paging |
| 15 | `sim.rs` | the seeded fleet: a server, clients, a network that reorders, duplicates and drops; partition, heal, step, settle |
| 15.1 | `sim.rs` (`Op`, `run_script`), `bin/fuzz/` | the fleet over a deployment's life — a restart from the journal or over an emptied one, a moved horizon, a client re-opened from what it made durable, late joiners by facts or without closures, a sign-in — as ops of a script; and `arkc fuzz` (`docs/plan-db.md` D2), which runs random modules (held to §9) through random scripts and checks convergence, replay from facts and from intents, every frame's bytes, every maintained view against a fresh hydrate, and the demo's natives against `eval`. A finding is written under `--out` as a vector in this suite's format with a line in its `README`: a session as `rebase/fleet-fuzz-*` (`module`, `clients`, `seed`, `script`, `fuzz`), held to convergence and replay rather than to a hash; a view as a `views/` case whose steps carry the fresh answer and no patches; a frame as a `protocol/` frame; a verdict as an `eval/` case. A plain bug is fixed with its file copied, unchanged but for its name, into `vectors/<directory>/`; a finding that would change what this specification says stays under `rust/ark/tests/fuzz-findings/`, where `tests/fuzz.rs` holds it to still failing until it is decided |
| — | `rules.rs` | a table's rules asked (`docs/plan-auth.md`): whether every table is `Everyone` to an identity (`whole_to`), whether a row is seen or may be written, the first table an entry's facts write that its rule forbids (`forbidden`), what a partial peer is sent of an entry (`filter_facts`), and the rows and state hash of what it may see (`visible_rows`, `partition_hash`) |
| 17 | `compat.rs` | `arkc check`: the additive-only rule between two modules, and re-verifying retained closures against a new schema |
| — | `authoring/` | the vocabulary of `AUTHORING.md`: a mutator run `Emit` (the module) or `Native`, a query described under `Emit` as its plan |
| — | `bin/vectors/demo.rs` | the two-table domain every vector is over: `AUTHORING.md` Appendix B |

§16 (the printable form) and §18 (`arkc gen`, `arkc roundtrip`) left
with the Haskell and are not in version 4.

### §8.1 The state hash, exactly

What a `Verify` compares, what a snapshot carries and what the `hash/`,
`eval/` and `rebase/` vectors pin. With `enc` the canonical encoding
(§1.3) and `‖` concatenation:

    leaf(t, row)  = sha256(enc(Text t) ‖ enc(Struct row))
    digest(t)     = Σ leaf(t, row) over the rows of t, mod 2^256
    state_hash(s) = sha256(enc(List [List [Text t, Bytes digest(t)]
                                     for every table t of the schema,
                                     in schema order]))

`Struct row` is the row as §4 has it, every column by name, whatever
order a runtime holds the values in. A leaf is read as a 256-bit
big-endian unsigned integer and a digest is written back as 32
big-endian bytes; a table with no rows has the digest of 32 zero bytes,
and contributes its name and that. The sum is order-independent and
subtraction undoes it, so a store moves a table's digest by one leaf in
and one out per row written, and a `Verify` hashes as many pairs as
there are tables rather than every row (`docs/plan-db.md` D3; the
reasoning, and what a 256-bit sum does not defend against, is in
`hash.rs`). Until D3 the state hash was SHA-256 over every table's rows
in key order; the vectors carrying one were regenerated once, and only
their hashes moved.

**A stored snapshot names its construction.** The server's `log.ark-log`,
a peer alone's `log` and a client's `replica` record carry `hashing: 2`
beside the snapshot; absent means 1, the construction before D3. A
snapshot of construction 1 is checked by that construction when it is
opened and held hashed by this one from then on, never refused; a
construction a runtime does not know is refused (`ark::hash::HASH_VERSION`,
`ark::journal::hashing_of`). `SPEC_VERSION` stays 4: the vectors are the
spec, and they moved once.

**Not a commitment — a stated non-goal.** The state hash is for replicas
of one log to tell whether they hold the same rows, which nobody is
choosing adversarially. It is not a commitment to a set of rows against
somebody who chooses them: for a sum of 256-bit leaves, Wagner's
generalised birthday attack finds a different multiset with the same
digest far below 2^128 work. So a snapshot from a source not trusted is
verified by replaying the log to it (`docs/arkdb.md` §3.9), never by its
hash alone. Should a hash alone ever have to carry that weight, the digest
wants a much wider group — a lattice hash of a couple of kilobytes, as
LtHash does — and that would be a construction 3.

## The toolchain

`rust/ark` has three binaries:

- `ark-vectors OUTDIR` writes every vector (`bin/vectors.rs`, a module per
  directory under `bin/vectors/`), asserting each claim as it goes, so a
  change that breaks one fails the generator rather than emitting a wrong
  vector.
- `arkc` is everything about a module rather than about running one. A
  `.ark` file is a module's canonical CBOR, and `arkc` verifies it before
  anything else:

      arkc verify  M          the module hash
      arkc hash    M          the module hash and every function's
      arkc check   OLD NEW    every break of §17, exit 1; or nothing
      arkc vectors OUTDIR     as ark-vectors

- `gen-unicode UCD-DIR OUT` writes `unicode_tables.rs` from the three UCD
  files it names; run by hand when the pinned Unicode version moves.

## Building

    nix run ..#vectors -- vectors/       # regenerate the vectors, from this directory
    nix run ..#arkc -- check old.ark new.ark
    nix develop ..#rust -c bash -c 'cd ../rust && cargo test -p ark'
    nix flake check ..                   # the vectors are what rust/ark writes, and
                                         # rust/ark passes them

## The vectors

`ark-vectors` writes one JSON file per case under `vectors/<directory>/`.
JSON cannot say bytes, ids or 64-bit integers, so values are written with
wrappers: `{"$bytes": "<hex>"}`, `{"$id": "<8-4-4-4-12>"}`,
`{"$int": "<decimal>"}` — an integer is always wrapped, and an object with
exactly one of those keys is a wrapper, so no field name begins with `$`.
Text, booleans and `null` are themselves; a list is an array; a struct is
an object, keys in code-point order. Only `"`, `\` and the C0 controls are
escaped in a string. A file is an object of named parts, one per line,
which is what makes a regeneration that changes nothing show nothing in a
diff (`bin/vectors/json.rs`).

Every vector also carries the expected canonical bytes of the values it
names, so a runtime is tested on encoding as well as on meaning. Every
directory carries at least one deliberately wrong vector under
`falsify/`, marked `"expect": "fail"`, which a conformant runner must
*fail* — a runner that passes by not reading the expected value is caught
by it.

| directory | one vector is | holds |
|---|---|---|
| `codec/` | a value and its canonical bytes | §1.3 |
| `order/` | values and their sorted order | §1.2 |
| `verify/` | a module and whether it verifies; `rules-ok` the demo with a rule of every form | §9 |
| `eval/` | a module, a store, a mutator applied step by step — the changes, the rows, the hash; the input checks as verdicts; the form validator | §6, §8 |
| `hash/` | a store and its hash, with the module it belongs to; `leaves-and-digests` also every row's leaf and every table's digest, so the construction is checked step by step | §8 |
| `rebase/` | `three-peers`: a scripted session of replicas and an authority, asserted step by step; `fleet-seed-N`: a seeded simulation's script and the hash every replica must reach after settle | §10, §11, §15 |
| `module/` | a module as a value, its canonical bytes, its hash; decode of encode is the identity. `rules`: the demo with a rule of every form declared (`docs/plan-auth.md`) | §7 |
| `protocol/` | every frame as a value and its bytes; decode of encode is the identity | §12 |
| `views/` | a query of the vector's own module (`query`, and its `plan` as the module writes it), the context and arguments it is read with, `store_before` and the answer at hydrate (`rows_before`); then `batches` of changes, and after each batch the `patches` and the answer (`rows`). One file per plan feature: a projection, a having that admits a node when a child arrives, a group source, a lookup chain, a related plan on a non-key column, an expression order key under a limit, a related tree three deep, and the two v3 plans (`top-two-by-pos`, `playlist-with-items`) | §13 |

A change in a batch is the protocol's fact form (§12): `{"t": "add",
"table", "row"}`, `{"t": "remove", "table", "row"}`, `{"t": "edit",
"table", "old", "new"}`. A batch is applied to the store whole and then
pushed as one, so the patches are §1.5's: touched keys settle in
ascending key order, each against the list as it stands; an entry that
stays in place is an `update` only if its node changed; a move is a
`remove` at the old place then an `insert` at the new; under a limit an
entry leaving the window is a `remove` then an `insert` of the next at
the last place, one entering an `insert` then a `remove` past the limit;
a key added, edited and removed in one batch gives nothing. A patch is
`{"t": "insert", "at", "node"}`, `{"t": "remove", "at"}` or `{"t":
"update", "at", "node"}`, positions into the list as it stands when the
patch is applied.

## Requirements that are not functions

These bind a runtime and cannot be a vector. Each names the design note
that argues it.

- **Durability of a local write.** A pending intent is committed before
  the peer reports the mutation as made, in one atomic commit with nothing
  else; the optimistic state it produced is never committed. One fsync per
  tap is the floor and the design does not go below it. (`docs/arkdb.md`
  §3.6.)
- **A rebase reports the transitions it made.** A replica keeps, beside
  each pending intent, the changes its run made to the optimistic store.
  A rebase undoes them newest first — each inverts exactly: an `add` by
  deleting its key, a `remove` by putting the row back, an `edit` by
  putting the old row — applies what landed, and runs the surviving
  intents again; every view is told the inverse of what it undid, what
  landed and what it re-applied, in that order. Each is a transition the
  store made, never one invented for it. `Rebuilt` is reserved for a store
  replaced whole: a replica opened, a snapshot adopted. (§3.6, §3.13;
  `docs/plan-perf.md` R2.)
- **A live frame never touches the optimistic view.** `Heard` is taken
  before any rebase logic runs. (§3.14.)
- **The transport pings, the engine does not.** An authority's transport
  sends a keepalive on a quiet connection and closes after three
  unanswered; no client is required to. The engine has no clock. (§3.8.)
- **Nothing consults the platform for Unicode.** `trim`, `lower` and
  `is_alnum` use the tables in `unicode_tables.rs` and nothing else. (§5.)
- **A verified module only.** A runtime executes generated code only for a
  module that passed `verify`, and the hash it records for a function is
  of the form `verify` returned. (§9.)
- **Text is compared by code point.** Never by locale, never by canonical
  equivalence, whatever the host's string type does by default. (§1.2.)

## Changing the spec

A change to a module of `rust/ark/src` named above is a change to what
every runtime must do. It is made by editing the module, running
`ark-vectors` and reading the diff of `spec/vectors` — every file that
moved is named in the commit, with why — and moving `ir::SPEC_VERSION`
when a conformant runtime would now be non-conformant. A change that
moves a mutator's hash, or that `arkc check` reports against the module
it replaces, is a break of the log, and is recorded as one. A new rule
gets a vector, and a directory that gains a vector keeps its `falsify/`
case failing.
