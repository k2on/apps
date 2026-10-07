# Authorization is the functions

`docs/plan-auth.md` built authorization as rules on tables and it was
deleted (G1, below). What replaces it was designed in conversation with
the owner, and the direction is tRPC's: *authorization logic lives in
the functions and the shared middleware they compose; what a client does
not get is rows, tables and columns the way a REST API simply does not
return them; a function may do more on the server than on the client;
and a database explorer edits data as the authority.* Every mechanism
below is the engine's; every rule is the application's.

## What is true, and what every piece stands on

**Every client mutation is checked by the server.** A pushed intent is
re-run at the authority with the identity of the connection that pushed
it — `ctx.user`, `ctx.session`, `ctx.roles` — middleware first, then the
body; the client's own run was a preview. The entry is held to the login
that pushed it. So a guard in a function *is* server-side authorization,
and nothing declarative is needed beside it.

**A domain function is one closure that runs two ways.** Natively on the
server and the desktop, and once in emit mode to record the module the
phones and the browser load. So "the client's version" of a function is
never generated: it is the same closure with its server parts skipped,
and the hash the log names a function by is of the public part.

## Decided

### D1. Writes: guards

Authorization of a write is a guard or a provide on a router or a chain,
as the IR already has them: `is_auth`, `has_role("library")`, `owned`
(harken's provide, which *is* an ownership check). One engine addition:
`Expr::HasRole(name)`, `ctx.has_role(..)` in the authoring, true when the
author's roles hold the name. A guard's refusal is the entry's verdict.

**Roles are frozen in the entry.** An `Entry` carries `actor` and
`session` and replays with them; a body that reads a role needs the
roles frozen the same way, encoded only when non-empty so no existing
vector moves, or a peer replaying somebody else's intent evaluates
`has_role` as false where the authority evaluated it true and every
replica, the authority's own `Log::state_at` and compaction diverge.
The authority **stamps** the entry's roles with the connection's at
sequencing — the device's run was a preview, and a device that believed
wrongly is refused by the guard at the server — rather than checking a
subset, which would let a device leave out a role a `!has_role(..)`
depends on.

### D2. Reads: scopes, a middleware over `ctx`

What a person holds is said by **scopes**: a third middleware kind beside
`Guard` and `Provide`, a plan over the tables as a function of `ctx`
alone, naming the rows and the columns of each table that person holds.

```rust
let is_user = is_auth
    .server(|ctx, db| db.users.where(User::user_id.eq(ctx.user)).exclude(User::password));
is_user.client("me", |ctx, db, input: &Me| db.users.where(User::name.has(input.q)));

user_api                                 // or once, on the router
    .server(|ctx, db| ..)
    .router(|r| { r.client("me", ..); r.client("list", ..); });
```

- **Composable and inheritable, as guards are.** A scope sits on a chain
  (`is_user`, reused by every procedure built on it) or on a router
  (every procedure on it inherits it), and a procedure's `uses` lists it
  in order with the rest. It hashes into the closure like any middleware.
- **A person's holdings are the union**, per table, of every scope on
  every procedure of the module, evaluated for that person: rows by the
  predicates, columns by the projections. Computed once per identity at
  `Hello`. A table no scope names is held whole; a module with no scope
  is today's whole replica, byte-for-byte, and every client of it keeps
  replaying by intents. That union is what makes the whole-peer fast
  path decidable and what a forgotten `where` on one query widens — so
  the router form is the one to reach for first.
- **`ctx`, never `input`.** A scope is a function of who the person is,
  so what they hold is fixed per person, complete offline, and
  computable at `Hello`. `get_org(org_id)` needs no input in its scope:
  membership (`exists membership(user = ctx.user)`) says which orgs are
  held, and the client half picks one. Authorization never needs input;
  only *size* does — a catalogue of ten million rows, a workspace of
  years of messages, a map's viewport — and that is the **active-scope
  extension**, noted and not built: a scope taking `input`, active while
  a client uses the procedure, the holdings the union of what is active,
  rows arriving and leaving as screens change, and a screen opened
  offline for the first time empty until the connection returns. A
  `Scope` that takes `ctx` today takes `input` tomorrow without anything
  existing moving. harken, a household holding everything, never needs
  it.
- **The client's half runs over what it holds**, as every query does
  today. A client half that names a column or table its person's union
  does not give it is a verifier error naming both; the union is static
  per identity, so this is checked at build for every role set the
  module names.
- **An excluded column does not exist on the client.** The client's
  table has fewer columns, the wire never carries them, the shared state
  hash is over what is held. The desktop's typed row structs are the
  schema's, so the field is a typed hole no domain code can name, by the
  rule above — not a null.
- **The authority serves the union.** For a person whose union is not
  whole: facts filtered per connection by the predicates and projected
  by the columns; an entry none of whose facts the person holds is not
  sent; a row entering or leaving the union — ownership moved, a role
  granted — is sent as an `Add` or a `Remove` fact; snapshots and pages
  hold the union; `Verify` is answered from the digest of the held rows
  and columns. The client applies other people's entries by facts (it
  cannot replay an intent that read rows it does not hold) and previews
  its own, confirmed from the authority's facts. This is the machinery
  G1 deleted (`db79b6e`, its commits a reference), rebuilt with the
  predicate coming from the module's scopes and a column projection
  added — a row projection the store already does. A scope that reads
  other tables (the `exists membership` form) declares its read set
  through `ir::reads`, which is what keeps the visibility diff cheap.

### D3. A mutation's server half: `ctx.private`

```rust
r.mutation("hello", |ctx, db, input: &Hello| {
    ctx.private(|db| db.audit.insert(Audit { who: ctx.user, .. }));
    db.users.get(input.id)
});
```

- Natively, `ctx.private` runs only when the `Ctx` is the authority's;
  on a device it does nothing. In emit it records a `Private` block; the
  module a client loads has those blocks stripped (`arkc` strips; the
  server's keeps them) and the function's hash is of the public IR.
- **Private runs last**, wherever written, in order, after the public
  body — which is what makes the public body reproducible on a client:
  it cannot have read what private wrote.
- **Its facts ride the entry.** A function with a private block says so
  in the stripped module (`private: true`, hashed); a client previews
  the public body and, when the entry is confirmed, takes the authority's
  facts for it rather than its own record — the facts path every peer
  has. Private may therefore write anything; what it writes to columns
  or tables outside a person's union is projected away with the rest.
- **Private may refuse**, and the refusal is the entry's verdict.
- **Server hooks** are for the world outside the database:
  `Server::on_committed(name, |entry, facts| ..)` in `ark-server`, after
  the entry is durable, off the engine's path. The engine stays sans-io.
- A query whose plan needs data outside every union is a **server
  query** — a request frame answered once by the authority — noted and
  not built.

### D4. Per-table CRUD, and the explorer

- **Every table's CRUD is one line on a router.** `r.crud::<User>()`
  emits `insert_user`, `update_user`, `delete_user` and `put_user` as
  ordinary mutations on that router, under its guards and scopes, hashed
  and replayed like any mutation. Growing one is writing it: a hand-
  written `insert_user` on the same router replaces the generated one
  under the same name, a new hash, the old version kept for the entries
  that carry it. No client API moves.
- **The explorer writes as the authority**, not as somebody's account.
  Two raw writes the authority alone can author — `put_row(table, row)`
  and `delete_row(table, key)`, judged by constraints, replayed by every
  client as ordinary confirmed entries, their actor the server's own
  identity — and no client has a path to them, connected or alone: the
  server refuses any pushed entry naming them. Where a table's CRUD is
  exposed on a router, the explorer edits through those functions by
  default, so the domain's logic applies to the dashboard too, with a
  visible switch to write raw.
- **The explorer is one iced component, hosted twice.** An `ark-explorer`
  crate generic over a schema: a table browser over a `&dyn Store` with
  dynamic rows (server-only columns included where the store has them),
  a cell editor and row delete through a `Writer` the host supplies, a
  read-only query console (a domain query by name, or a plan in the IR's
  JSON form), and the log — entries, facts, who pushed each, every
  connection's cursor and pending count, the Verify answers. **On the
  server** it is the admin page: compiled to wasm as the browser peer is,
  served at an admin path bound to loopback by default
  (`HARKEN_ADMIN_BIND`), the `admin` role required to open it when
  exposed wider, over the authority's store with the raw writer. **In
  harken's desktop** it opens on a key, as the debug screen does, over
  the peer's own replica, read-only but for the CRUD mutations the domain
  exposes, which it authors as the signed-in person like any button. A
  peer alone is a client here too: it edits through exposed CRUD, never
  raw, so what it did alone can be pushed and judged later as any
  offline work is.

## G1. The row rules are deleted, cleanly

Landed; see below. What stays from plan-auth because this design needs
it: `Identity.roles`, `Ctx.roles`, roles from the server's configuration
(`Auth::with_roles`, `services.harken.roles`, dev auth's `name:role`),
the scanner's `library` role, `Refusal::Forbidden`, the fuzzer's two
accounts and role changes.

## Order, rounds, and guards

**Status: G1 and D1 landed; D2–D4 decided, not started.** Rounds, each ending
in a green `nix flake check`:

1. **D1**: roles frozen in the entry and stamped by the authority;
   `has_role`; harken's `library` guard; the fuzzer's forbidden writes
   through a generated guard.
2. **D2**: `Scope` middleware; the per-identity union; the authority
   serving it (filter, projection, visibility facts, union digest), the
   client holding it; the fuzzer generating scopes over two accounts
   with a check that no client ever holds a row or column outside its
   union and every client's hash is the authority's for it; harken
   declares no scope and changes nothing on the wire.
3. **D3**: private blocks, the facts path for them, server hooks.
4. **D4**: `r.crud`, the two raw writes, `ark-explorer`, the admin page,
   harken's key.

Guards throughout: falsify every new test once; `spec/vectors`
byte-identical except where a round's Landed says a file is added; the
fuzzer at 0 findings for 300 seconds after each round; a whole peer
costs on the wire exactly what it costs today; what is not verified said
plainly.

## Landed

**G1, the rules deleted** (`db79b6e`). Syntax and machinery both: what
plan-auth's A1, A3's fact check, A4, A5 and A6's rule half added is gone —
`Table::visible`/`writable`, `Pred::Me`/`Role`/`Exists` and their verifier
errors, encoding, normalisation and authoring vocabulary; `ark::rules`;
`Authority::sequence_as` and the device's own check; the per-connection
filter, `Covers`, whole-or-partial at `Hello`, the partial snapshot, page
and `verify`, the partition digest, the replica's `partial`, `through` and
`confirm_partial`, and the client's durable `partial`; the fuzzer's rule
generation and its three checks; `perf_g_partial_fanout`; harken's ten
declarations and the private-playlists fleet scenario. `rust/ark` is
`b62ee3c` again but for roles: `Identity.roles`, `Ctx.roles`,
`dev_identity`'s `name:role,role`, and in `ark-auth`, the server, the
desktop and `harken-peer` everything that carries them, with the fuzzer's
role grants and role changes (`Op::Roles`) — read by nothing until G2.
`Refusal::Forbidden` stays as a variant nothing produces, which its doc
comment says. A client's view starts over again when a login's roles
move, as it did before plan-auth, since G2 makes a body read them.

`spec/vectors` is `b62ee3c`'s: the ten files plan-auth added are removed,
the tree diffs empty against `b62ee3c`, and `ark-vectors` writes it
identically. `harken.ark` is byte-identical to `b62ee3c`'s, module hash
`abf1cbdd8b0ccd361adf4916184d3e81f89f633734025f4642b0915b1499165b`; no
closure hash moved. harken's converge test keeps its removal half as
`removing_a_track_takes_it_off_everybodys_playlists` — the library takes
a track off two people's playlists, each keeps their other item, every
peer at the server's hash — falsified by not deleting the items in
`remove_media` (all four stayed). The refusal halves went with the rules:
nothing refuses a library write until G2's guard.

Green: `cargo test --workspace`, `fmt --check`, clippy over the workspace
and over `harken-iced --features demo` with `-D warnings`, the demo's
tests, `allocations.rs` with its ignored case. `arkc fuzz --seed 1
--cases 200`: 400 sessions, 36,861 ops, 8,190 entries, 1,707 role
changes, 0 findings; `--seconds 120`: 5,205 cases, 970,336 ops, 221,233
entries, 45,065 role changes, 0 findings (8 generated modules of 4,165
failed to verify, `TypeMismatch filter on id` — the generator's misses,
counted, not findings). Not run: `nix flake check`, `perf.rs` in release.

**D1, guards and stamped roles** (`5d6b329`, `528bed9`, `6545356`,
`aaff24d`, `c8b54e1`; after `checks.versions`, `e757a39`, `ef701df`,
`ded02c2`). `Entry` carries `roles`, frozen by
`Replica::mutate` from the author's `Ctx` and written `roles` — texts in
ascending order, present only when not empty, so every entry from before
is the bytes it was. The authority stamps: `Server` runs and logs a pushed
entry under its connection's roles, whatever the device believed, and
every later run (`peer::ctx_of`: replay, rebase, adoption, the sim's and
the fuzzer's replays) reads the entry's. `Expr::HasRole` (`has_role`, a
`Bool`, `EmptyRole` for the empty name) and `ctx.has_role(..)`, in a guard,
a provide, a body and — through the new `ctx()` — a check. harken's
`library` router runs `is_library` before its six mutations; the queries
run nothing, the playlists keep `owned`. Module hash
`2339009b3ae76594e5ed6b115aa92267f96e4400c49b64e04765526134088f2f`, from
`abf1cbdd…`: the six library mutators' closures move (a closure is the
middleware it runs) and are pinned anew in `agreement.rs`; no query's and
no playlist mutator's does; `arkc check` of old against new is additive.
The fuzzer gives three modules in five a `guarded` router whose `holds`
guard tests a role, grants the module's own roles too (harken's
`library`), lets devices believe their login's roles, all, or none, and
holds every entry the log gains to its login's roles (`stamp`) and to its
middleware under them (`forbidden`).

One thing moved after it was committed, because the fuzzer found it
(seed 112): the stamp first sent an entry's facts as a `facts` frame
ahead of the ack, so a device whose ack and page were dropped, never told
a log's name, kept them in its inbox through a server that lost its log,
and confirmed the new log's first entry by them. They ride the ack now —
`ack`'s `facts`, a duplicate's included — and arrive with the log they
are of; and only when the stamp changed the roles *and* the closure reads
a role outside its guards (`hash::reads_roles`: a guard only refuses, so
an admitted entry's facts cannot depend on it).
`the_stamps_facts_arrive_with_the_log_they_are_of` holds it; the
finding's own vector was not kept, since its interleaving hinged on the
frame that is gone.

`spec/vectors` gains four files — `protocol/client-push-roles.json`,
`protocol/server-ack-facts.json`, `module/guarded.json`,
`eval/has-role.json` — and every other file is byte-identical;
`ark-vectors` writes the tree exactly. Falsified: the stamp left out
(`the_authority_stamps_the_roles_an_entry_is_logged_with`, the converge
refusal half, the fleet's `a_role_granted_by_a_restart_and_revoked_by_
another`, and the fuzzer's `stamp`, 185 findings over 3 signatures in
200 cases); `HasRole` answered false in `eval` and natively (`roles.rs`,
and `eval/has-role.json`); the ack without its facts (the device kept a
preview the log does not hold) and with them sent as their own frame as
well (diverged at [1, 2]); the library guard admitting everybody (the
device took bob's removal); the guard skipped at the authority (42
`forbidden` findings); harken's fuzz granting only `r0`/`r1` (its guard
never met at the authority).

**What the version matrix found** (`checks.versions` on `9306447`, five
scenarios). The guard moved the six library mutators' hashes, and what
that does across versions is three things. A fresh server of this build
never ran the module an old peer authors at, so every old `add_song` was
held for ever (1, 2, 5): `ark_server::Builder::ran_before` records a
module as one an earlier start ran — `HARKEN_OLD_MODULES`,
`services.harken.oldModules` — and the flake hands each pinned
revision's committed `harken.ark` to the matrix, whose fresh servers are
told them (`Fleet::beside`); those intents run the old closures,
unguarded. A peer upgraded in place over intents at a hash its module no
longer ships rejected them as "no closure for a pending intent" (5b):
such an intent is kept now, pushed, and confirmed by the authority's
facts, previewed meanwhile by the facts the alone journal kept
(`Replica::known`, from `fork_back`) while they are still a transition
from the view, and by nothing otherwise — previewing them over a view
somebody else's entries had moved tripped the rebase's own assertion; a
peer alone refuses one, "no closure, and no authority to run it". And
an old server holds every intent of this build at a moved hash, so
scenario 3 holds three where it held one, and all land after the
upgrade. The narrowing of the ack's facts came from the same reading:
every old peer's push is restamped, since it sends no roles, and was
being acknowledged with facts it did not need. `cargo test -p
harken-server --test versions` with both pinned revisions: 9 passed, 1
ignored (5c). Falsified: rejecting again in `run_pending`, `known` left
empty, the check in `local_commit` removed, `reads_roles` ignored,
`transitions` always true — each fails its test; without the old module
told, 1, 2 and 5 fail as the matrix did.

Numbers. `arkc fuzz --seed 1 --cases 200` (debug): 400 sessions, 37,737
ops, 7,070 entries, 1,764 role changes, 2,120 writes forbidden on the
device and 173 at the authority, 8,253 entries held to the stamp (1,832
from a device that believed otherwise), 0 findings. `--seconds 300`
(release): 13,412 cases, 2,511,622 ops, 500,389 entries, 117,052 role
changes, 113,371 forbidden on the device and 10,342 at the authority,
576,433 entries held to the stamp (129,009 corrected), 0 findings (12 of
10,781 generated modules failed to verify, `TypeMismatch filter on id`,
the generator's misses); after the matrix's fixes, the same 200 cases to
the op, and `--seconds 300`: 13,304 cases, 2,491,652 ops, 496,448
entries, 116,117 role changes, 0 findings (the fuzzer's generated guards
read no role in a body, so it sends no ack's facts at all now).
`perf.rs` in release, before and after the matrix's fixes: every flat
line flat, l/f 0.96–1.46; `perf_a_mutate_alone` 22–24 µs at 500 to
8,000 items; a
Batch of 256 encodes in 13,598 allocations and decodes in 7,439, as
before — a whole peer's frames are the bytes they were. The fleet
scenario takes about 20 s.

Not verified: `nix flake check` (the coordinator runs it; `checks.versions`
was run here with the two pinned revisions' binaries and modules by
hand, `checks.harken-module` built). An entry a client authors at a
library mutator's old hash still runs that closure, unguarded, at a
server that ran — or was told it ran — the module before `is_library`
(closure provenance keeps it, and the matrix needs it); nothing
withdraws a hash yet. A body that reads a
role beyond refusing is confirmed by the ack's facts, and the device's
preview of it is recorded as a divergence — right, and loud, and not
something harken does.
