# Authorization is the functions

`docs/plan-auth.md` built authorization as rules on tables: who may see a
row, who may write one, a predicate per table enforced by the authority
on facts. It worked, was fuzzed, and is the wrong shape for this engine.
The owner's direction, in three sentences: *authorization logic lives in
the functions and the shared guards they compose, tRPC-style; row-level
security is not wanted; what a client does not get is tables and
columns, the way a REST API simply does not return them.* This is the
design that replaces the rules with that — and, because the same
mechanism carries it, the two other things asked for: a function whose
server half does more than its client half (Zero's server mutators), and
a database explorer that edits data as the authority.

## What is true, and what every piece stands on

**Every client mutation is checked by the server.** A pushed intent is
re-run at the authority with the identity of the connection that pushed
it — `ctx.user`, `ctx.session`, `ctx.roles` — guards first, then the body;
the client's own run was a preview. The entry is held to the login that
pushed it. So a guard in a function *is* server-side authorization, and
nothing declarative is needed beside it. That is the whole argument, and
the rules go because they were a second mechanism the first one did not
need.

## G1. The row rules are deleted, cleanly

Everything plan-auth's A1, A3's fact check, A4 and A5 added goes:
`Table::visible` / `writable`, `Pred::Me` / `Role` / `Exists`, their
verifier errors, encoding, printing, authoring vocabulary and vectors;
the authority's post-run check of facts against `writable`; the
per-connection filter, `Covers`, whole-or-partial decided at `Hello`,
the visibility-change facts, the partition digest, the client's durable
`partial`; the fuzzer's rule generation and the three checks over it;
harken's ten declarations. `spec/vectors` goes back to what it was
before A1 byte-for-byte plus nothing: the files that round added are
removed, and `checks.vectors` says every remaining one is unchanged.
harken's module hash moves back; no closure hash moves.

What stays, because this design needs it: `Identity.roles` and
`Ctx.roles`; roles from the server's configuration (`Auth::with_roles`,
`services.harken.roles`, dev auth's `name:role`); the scanner's own
`library` role; `Refusal::Forbidden` as what a guard's refusal is named;
the fuzzer's two accounts and role changes; the fleet scenario, reworked
to hold guards rather than rules. A client that may connect receives
every table and every column that is not server-only (G3), by intents,
and replays: there is no partial peer.

## G2. A guard can test a role

`Expr::HasRole(name)` beside `CtxUser`: true when the author's roles
hold the name. In the authoring, `ctx.has_role("library")`, usable in a
guard, a provide, a body or a check. harken: the `library` router gains
`guard("is_library", |ctx, _| ctx.has_role(LIBRARY))`, so `add_song` and
the rest refuse a login without the role at the device and at the
server, with the message the guard gives. Playlists keep `owned`, which
is already the ownership check written as a provide. Shared guards are
plain Rust functions returning the closure — `is_auth()`, `has_role(r)`
— and a domain composes them on its routers; nothing in the engine knows
their names.

**Roles must be frozen in the entry, or `has_role` diverges.** Found
while reading for the build, before a line was written: an `Entry`
carries `actor` and `session` and no roles, and a replay runs with empty
roles — so a peer replaying somebody else's intent would evaluate
`has_role` as false where the authority evaluated it true, and every
whole replica, the authority's own `Log::state_at` and compaction would
diverge. plan-auth never met this because no body read a role. The fix
is the one `actor` and `session` already have: the author's roles are
frozen in the entry, encoded only when non-empty so no existing vector
moves, and the authority holds them to the connection that pushed the
entry — **stamping** them (the entry's roles become the connection's;
the device's run was a preview and a guard refuses a device that believed
wrongly) rather than checking a subset, since a subset would let a
device leave out a role a `!has_role(..)` depends on. Also noted: a
guard refuses with `Refusal::Refused(msg)` today, so `Forbidden` is used
by nothing once the rules go until G5's built-ins bring it back.

## G3. Server-only columns and tables

`.server_only()` on a column or a table in the schema. The marker is
part of the schema section and so of the module hash, encoded only when
present, so a schema without one is byte-identical to today.

- **It never leaves the authority.** Stripped from every snapshot a peer
  is sent, from the facts beside every entry, from every page; absent
  from a client's replica. An entry's arguments are the client's own and
  carry nothing to strip.
- **The verifier refuses a client-run read of it.** A query's plan, a
  public body, a check, a guard or a provide that names a server-only
  column or table is a verifier error naming both — what makes "never
  leaves" a property of the build rather than of care. Only a private
  block (G4) may read or write it.
- **The shared state hash excludes it.** A row's leaf is over its shared
  columns; a server-only table contributes no digest. The authority keeps
  its full digest for its own integrity and answers `Verify` with the
  shared one. A schema without markers hashes exactly as it did, which
  `checks.vectors` holds.

Where the client keeps a view over a table that has a server-only column,
nothing changes: the view never saw the column.

## G4. A function's server half: `ctx.private`

```rust
let hello = r.mutation("hello", |ctx, db, input: &Hello| {
    ctx.private(|db| db.audit.insert(Audit { who: ctx.user, .. }));
    db.users.get(input.id)        // the public body
});
```

A domain function is a closure that runs natively on the server and the
desktop and is run once in emit mode to record the module, so "the
client's version" is not generated: it is the same closure with the
private parts skipped.

- **Natively**, `ctx.private` runs its block only when the `Ctx` is the
  authority's; on a device it does nothing. **In emit**, it records a
  `Private` block, and the module a client loads has those blocks
  stripped (`arkc`'s build does it; the server's module keeps them). The
  function's hash is of the public IR, so server and client agree what
  `hello` is while the server holds more of it.
- **Private runs last.** Wherever it is written, every private block of
  a run is deferred until the public body has finished, in order. That
  is what makes the public body reproducible on a client: it cannot have
  read what private wrote, by construction.
- **Its facts ride the entry.** The authority's facts for the entry are
  the whole run's; a client replays the public body for its preview and,
  when the entry is confirmed, takes the authority's facts for it rather
  than its own record — the facts path every peer already has. A
  function with a private block says so in the stripped module
  (`private: true`, hashed), which is how a client knows which entries
  to take by facts; a batch carries the facts for exactly those entries.
  Private may therefore write anything, shared tables included; what it
  writes to server-only tables is stripped with the rest (G3).
- **Private may refuse.** The refusal is the entry's verdict, as any
  server refusal is, and the client's preview is undone on the rebase.
- **Server hooks** are for the world outside the database — analytics, a
  webhook, an email. `Server::on_committed(name, |entry, facts| …)` in
  `ark-server` runs a Rust closure after the entry is durable, off the
  engine's path; it can do anything and cannot touch the log's meaning.
  The engine stays sans-io.

A query whose plan needs server-only data cannot run on a client at all.
That is a **server query** — a request frame answered once by the
authority, the tRPC query with no subscription — and it is noted here and
not built: nothing in harken needs one.

## G5. The explorer writes as the authority

The explorer edits data as the server, not as somebody's account. Two
built-in mutations the engine provides for every table, present in every
module by construction the way `std` is, with fixed hashes:

```
ark.put_row(table, row)        // insert or replace, judged by the table's constraints
ark.delete_row(table, key)
```

- **Authority-only.** `Authority::edit(..)` in the server process is the
  one way to author them; `Replica::mutate` has no path to them, so no
  client — connected or alone — can, and the server refuses any pushed
  entry naming them with `Forbidden`. Their actor is the server's own
  identity.
- **Replayed by everyone.** They are deterministic puts and deletes, so
  every client applies them as ordinary confirmed entries and ends at the
  authority's hash. A put that breaks a reference or a unique index is
  refused at the authority like any write.
- **The explorer page** is served by `ark-server` for any module, since
  the server knows the schema: the tables (server-only ones included,
  this is the authority), a cell edit and a row delete through the two
  built-ins, a read-only query console (a domain query by name, or a plan
  in the IR's JSON form), and the log — entries, facts, who pushed each,
  every connection's cursor and pending count, the Verify answers. It is
  bound to loopback by default (`HARKEN_ADMIN_BIND`), and when exposed
  wider it requires the `admin` role to open; the writes it makes are the
  authority's either way.

**Status: designed, not started.** The owner paused the build before a
line of it was written (tree at `ddc4477`). The row rules of plan-auth
are therefore still in the code and in harken's declarations; deleting
them is G1 and is the first thing to do when this resumes. G5 is to be
reshaped before building: the per-table operations become ordinary
domain mutations a router exposes in one line (`router.crud::<User>()`,
emitting insert / update / delete / put with the router's guards) and
grows by writing the function by hand under the same name; the explorer
uses those where exposed and the authority's raw writes otherwise.

## Order, rounds, and guards

Three rounds, each ending in a green `nix flake check`:

1. **G1–G3**: delete the rules; `has_role`; server-only columns and
   tables. harken: the `library` guard, no declarations, module hash
   moved once.
2. **G4**: private blocks, the facts path for them, server hooks. The
   fuzzer generates private blocks over shared and server-only tables and
   holds every peer to the authority's shared hash.
3. **G5**: the built-ins and `Authority::edit`; the explorer page; docs.

Guards throughout: falsify every new test once; `spec/vectors`
byte-identical except where this document says a file is removed or
added; the fuzzer at 0 findings for 300 seconds after each round; a
whole peer costs on the wire exactly what it costs today for an entry
with no private block; what is not verified said plainly.
