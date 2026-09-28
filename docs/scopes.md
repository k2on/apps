# Scopes: what they were, what they solved, and why they are gone for now

ArkDB was designed with **scopes**, and they have been removed. From spec
version 3, a module has one log, one set of tables and one authority. This
document records what scopes were and what problem each part of them
answered, so that they can be designed again deliberately rather than
rediscovered piecemeal. Nothing in the code refers to them any more.

## What a scope was

A scope was the unit of replication. Each one had:

- **its own append-only log**, with its own sequence numbers, snapshots and
  compaction horizon;
- **its own tables**: every table belonged to exactly one scope;
- **its own authority**: the peer or server that sequences its entries;
- **its own access rule**: who may receive it, checked at `Hello`.

A mutator was declared in one scope and could read and write only that
scope's tables; the verifier refused one that looked sideways. In the v2
vocabulary a router named its scope, and every procedure on it inherited
it. A peer subscribed to any set of scopes, each either *whole* (replaying
intents, an exact replica) or *by facts* (receiving the rows' effects).
harken split its domain into `library` (the tracks the scanner authors) and
`playlists` (what people make).

## What each part solved

- **Partial replication.** A peer holds the logs it needs and not the
  others. A phone does not have to replay every scan of a large library to
  see a playlist, and a device can hold one person's data without holding
  everyone's.
- **Authorization at the grain intents can honour.** Replaying intents
  needs the whole of the state they read, so read access cannot be granted
  row by row to an exact replica. A scope is the smallest thing that can be
  replicated exactly, which makes it the natural unit of "who may receive
  this". (Row-level rules live in the by-facts mode, which does not claim
  exactness.)
- **Independent sequencing.** Each scope has its own total order, so
  unrelated writes — a scan of the library and a person editing a playlist —
  do not queue behind each other, and each log can have a different
  authority: a server for the shared library, a phone for a scope only it
  writes, with no server at all.
- **Independent history.** Snapshots, compaction and the retention of old
  function versions are per log, so a busy scope can be compacted without
  touching a quiet one.

## What they cost

- **No references across scopes.** A column naming a row of another scope's
  table could not be checked at write time: the row may not be in the
  replica, or may not exist yet. `playlist_item.track_id` was an unchecked
  id, and a playlist entry whose track had not arrived was drawn as
  unavailable.
- **No transactions across scopes.** Two writes in two scopes were two
  entries in two logs; nothing spans both.
- **No queries across scopes.** A query read its router's scope only, so a
  screen that needed both joined two queries' results in memory.
- **A modelling decision up front.** Splitting a scope later is a migration,
  so every domain had to choose its scopes early.
- **Protocol surface, and a bug in it.** Every frame named its scope, a
  client held a cursor per scope, and a connection held a set of
  subscriptions. The server replaced that set whenever a client paged one
  scope with a second `Hello`, so a peer holding two scopes stopped hearing
  the other until it reconnected. It was found while porting harken's
  client, whose library is longer than one page.

## What removing them changes

- **One log per module**: one sequence, one authority, one snapshot series,
  one cursor per peer. A `Hello` carries one subscription; `Push`, `Batch`,
  `NeedFacts`, `FactsFor`, `SnapshotOf`, `Ack`, `Reject`, `Verify` and
  `Agree` carry no scope. The paging bug cannot happen.
- **One set of tables**: the schema is a list of tables. Every reference is
  checked, so `playlist_item.media_id` references `media`. Any query may
  read any table.
- **Routers stay**, as groups of procedures and the middleware chains built
  on them; they name no scope. In the vocabulary the struct of tables a
  body's `db` is (formerly a scope struct) is the module's one `Tables`
  type: `router::<Harken>("playlists")`.
- **Access** is a rule on the whole log: who may receive it at all.
- **Whole and by-facts** modes remain, per peer.

What is lost for now: partial replication, per-person logs, independent
authorities, and read authorization finer than "the whole log". Every
peer that syncs holds everything, which is how Petros worked.

## Coming back to it

Any future design has to answer these questions, which the first one
answered with scopes:

1. **What is the unit a peer can hold exactly?** It must be closed under
   what its intents read, or replay diverges.
2. **How are references between units checked, if at all?** An unchecked
   id is honest but moves the check into every screen.
3. **Is there one order or several?** Several orders buy throughput and
   independent authorities, and cost atomicity across them.
4. **Where does authorization sit?** At the unit, or at the row in a mode
   that gives up exactness.
5. **How does a domain change its units later**, and what does that do to
   the entries already in the log?

A promising alternative to static scopes is **derived partitions**: one
log, with the authority serving a peer the entries that touch the rows it
may see. That needs the effect of every entry (the facts, which the
authority already keeps) rather than a declared scope. It keeps one order
and checked references, and moves exactness to a question about each
entry's read set.
