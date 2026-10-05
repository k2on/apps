# Authorization: one log, and what each person may see of it

`docs/arkdb.md` §3.11 closed authorization at the coarsest grain — who may
receive the log at all, checked at `Hello` — and left everything finer to
"the mode that does not claim exactness". `docs/scopes.md` records the
design that had finer grains and why it went. This is the design that
brings a finer grain back without bringing scopes back, and the questions
it needs answered before it is built. **Draft: the decisions marked ▢ are
open and change the work.**

## What is true today

- **Everyone signed in receives everything.** The authenticator at `Hello`
  answers who a connection is; after that the whole log flows, every
  person's playlists included. harken is one household's server, so this
  has not mattered; it is the first thing that does the day two accounts
  that are not one household share one.
- **Writes are the mutator's to check.** `create_playlist` filters by
  `ctx.user`; nothing in the engine knows that a playlist has an owner. A
  mutator that forgot the filter would write anyone's playlist and the
  engine would sequence it. The server holds an entry to the *login* that
  pushed it (the actor is who the authenticator said), which is identity,
  not permission.
- **Every harken table carries `user_id`**, media included: who authored
  the row. So the ownership a rule needs is already a column, and no
  schema change is needed to say it.
- **Two modes exist and one is used.** `Mode::Whole` replays intents and
  is exact; `Mode::ByFacts` applies the authority's facts and is exact for
  the same reason (facts are what replay produced). Nothing today sends a
  peer *some* of the facts, which is the whole of what is missing.

## What it is for

Three needs wear the name, and they are not one design:

1. **Privacy between accounts on one server.** My playlists are mine; the
   library is everyone's. The rule is about rows and who may *see* them.
2. **Who may write what.** Only the scanner adds songs; only I edit my
   playlists. The rule is about rows and who may *change* them, enforced
   somewhere a mutator cannot forget.
3. **Not holding everything.** A phone that wants one playlist and not
   the library. This is partial sync, and it is a size question, not a
   permission one.

This design does 1 and 2 with one mechanism, and leaves 3 as the same
mechanism asked a second question, later.

▢ **Q1.** Is harken's server for one household that may see each other's
playlists, or for accounts that must not? The design below assumes the
second; if the first, only "who may write" is built and it is a third of
the work.

## The model: one log, a rule per table, facts filtered by it

### A visibility rule is a predicate on a row and a person

Each table declares who may see a row of it, beside its indexes, in the
authoring vocabulary:

```rust
columns()
    .visible(Everyone)                          // media, album, person, work…
    .visible(Self::user_id.is(Me))              // playlist, playlist_item
    .writable(Role("library"))                  // media: the scanner only
```

The default is `Everyone` for both, which is today. A predicate is a plan
filter over the row's own columns against the identity's `user` and
roles — the same `Pred` the query algebra has, with `Me` and `Role(_)` as
two more leaves — so it is verified, printed and hashed like any plan,
and it reaches the module's schema section. A rule that needs another
row (an item visible because its *playlist* is shared) is a lookup through
a reference, which the plan algebra also has; harken does not need one,
because every table carries `user_id`.

### The authority sends a peer the facts it may see, and nothing else

A peer that is not everyone's receives the log **by facts, filtered**:
for each entry the authority keeps the facts beside it already (§3.8);
the facts a given peer may see are those whose row satisfies the table's
rule for that peer's identity. An entry none of whose facts the peer may
see is not sent at all — its sequence simply passes, and the cursor is
the last sequence the peer was told about. Everything that follows is
what the log already does:

- **Own intents** are authored and replayed optimistically as today, and
  **confirmed from their facts** — which is already how an own intent
  lands in `confirmed` in whole mode (`docs/plan-perf.md` R2): the record
  is applied, the intent is not run again. So a peer holding its partition
  is exactly as optimistic and exactly as exact as a whole peer is about
  its own intents.
- **Others' entries** arrive as facts and are applied as facts. No replay
  of an entry whose read set the peer cannot hold, which is the divergence
  scopes existed to prevent — a partition peer never replays anyone
  else's intent, so the question of whether it could does not arise.
- **A snapshot** below the horizon, or on joining, is the visible rows.
- **Verify** is answered from a partition digest: the sum of the leaves
  of the visible rows per table (§8.1's additive digest restricted by the
  rule), computed at the verify from the authority's store. `O(partition)`
  per verify, and verifies are rare; maintained per rule if that ever
  shows.
- **A row that becomes visible or stops being** — a playlist shared or
  unshared, a role granted — is sent as an `Add` or a `Remove` fact to
  the peers it changes for, at the sequence of the entry that changed it.
  The authority evaluates each rule on the row before and after an entry
  and sends the difference; nothing new on the wire.

A peer whose every table is `Everyone` to it receives the log whole, as
today, by intents, and may replay: whole mode is what the rules reduce to
for that person, not a mode anyone picks. ▢ **Q5.** Agreed that there is
no switch?

### Writes are checked against the same rules, after the run

An entry's facts are what it changed. After the authority runs an intent
and before it sequences it, every fact's row — old and new, for an edit
— must satisfy the table's `writable` rule for the author; one that does
not is a refusal (`Refusal::Forbidden(table)`), rolled back and answered
as any refusal is. The mutator's own `ctx.user` filter stays, as the
thing that makes an honest client's preview right; the authority's check
is what makes a dishonest or mistaken one harmless. A rule that reads a
row the entry did not write (may I add to *this* playlist: the playlist's
owner, from `playlist_item`'s `user_id`) is answered by the same
predicate on the fact's row, which is why `playlist_item` carrying
`user_id` is load-bearing rather than redundant.

The optimistic peer applies the same check to its own preview before
recording it pending, so a forbidden write is refused on the device
rather than two seconds later from the server.

### Roles

`Role("library")` is a claim about a person that the log does not hold:
putting roles in the log makes granting one an entry, which needs a rule
about who may grant, which is a role. The identity the authenticator
returns carries them instead, from one of two places:

- the provider: an OpenID Connect groups claim, mapped in the server's
  configuration;
- the server: `services.harken.roles.library = [ "<sub>" ]` in NixOS, and
  the server's own scanner account carries it by construction.

▢ **Q4.** Which? The recommendation is the server's configuration, with
the claim as a later option: it is the deployment that knows who its
scanner is, and dev auth (`nix run .#serve`) has no provider to carry a
claim.

▢ **Q3.** Who may `add_song`: the scanner only, or any signed-in person
(the day uploads exist)? The rule is one word either way; the question is
what harken means.

### Sharing

A shared playlist is a `playlist_member (playlist_id, user_id)` table and
`playlist` visible where `user_id.is(Me)` *or* a member row exists — the
lookup form of the rule, plus membership facts when a member is added or
removed. Everything above is designed to carry it and nothing above
builds it.

▢ **Q2.** Sharing now, or the mechanism now and sharing when wanted? The
recommendation is the second: the lookup form of a rule and the
visibility-change facts are built and tested in the engine either way
(the fuzzer needs them to exist to hold them), and harken's two tables
and screens come when sharing is a feature somebody asked for.

## What this is not

- **Not scopes.** One log, one order, one authority, every reference
  checked, every entry one transaction. What a peer holds is derived from
  the rules and the facts, never declared, so a domain changes its rules
  without a migration: the next `Hello` is answered by the new rules.
- **Not partial sync.** A phone that wants less than it may see asks the
  same filter a second question — "and of those, only these" — which is
  the projection §3.11 names. Same machinery, a plan instead of a rule,
  later.
- **Not a change to the log's meaning.** Entries, facts, hashes and
  vectors are what they were. What changes is which of them a connection
  is sent, and one refusal the authority can give.

## Where it lands, if built

- `rust/ark`: `Pred::Me`/`Pred::Role` leaves; `Table::visible`/`writable`
  in the schema and the authoring; `Refusal::Forbidden`; the per-peer
  filter and the visibility diff in `protocol::Server`; the partition
  digest; `Identity.roles`.
- `rust/ark-server`: the roles option and the scanner's identity.
- `rust/ark-auth`: roles on `Login`/`Account`; dev auth takes them from
  the name (`alice:library`), said loudly.
- `harken/domain`: the rules on ten tables, one line each.
- The fuzzer: two accounts, rules on the generated schema, a check that
  no peer ever holds a row its rule forbids and that every peer's
  partition digest agrees; the fleet: a scenario with two accounts and a
  revoked role.
- The spec: §-rules for visibility in a plan, a protocol vector for a
  filtered batch and a visibility-change fact, `spec/vectors` moved
  deliberately for the new schema fields.

Costs, stated: the authority evaluates every rule on every fact for every
connected peer — a filter per fact per connection, which is cheap for a
household and is the thing to measure first (`perf_c_fanout`, beside the
others); and a peer that was whole becomes a facts peer the day a rule is
added, which is one snapshot.
