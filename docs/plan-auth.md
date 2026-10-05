# Authorization: one log, and what each person may see of it

`docs/arkdb.md` §3.11 closed authorization at the coarsest grain — who may
receive the log at all, checked at `Hello` — and left everything finer to
"the mode that does not claim exactness". `docs/scopes.md` records the
design that had finer grains and why it went. This is the design that
brings a finer grain back without bringing scopes back, and the questions
it needed answered before it was built. The questions marked ▢ were asked
and are answered under "Decided" at the end; one answer governs all of
them: **the engine carries the mechanism and decides no rule.** What a
person may see or write is the application's declaration, and ArkDB's
job is that whatever is declared is enforced at the authority, verified
by the fuzzer, and costs nothing when the declaration is "everyone".

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

## Decided

The four questions were put to the owner, and every answer about *what*
the rules should be came back the same way: that is harken's decision,
not ArkDB's. So the engine builds the general mechanism — both rule
kinds, the lookup form, the per-peer filter, the visibility-change
facts, the partition digest, the write refusal, roles — exercised by the
fuzzer over a two-account domain with private rows, whatever harken
declares. harken then declares a household:

- **Q1, privacy:** harken is one household and everyone sees everything,
  playlists included — every table `visible(Everyone)`. The engine
  supports private rows regardless; harken does not use them.
- **Q2, sharing:** moot for harken, since everyone already sees every
  playlist. The lookup form of a rule and the visibility-change facts are
  engine mechanism and are built and fuzzed; no `playlist_member` table.
- **Q3, library writes:** `add_song` and the other library tables are
  `writable(Role("library"))`, which the server's scanner account carries
  by construction; a client pushing one is refused with `Forbidden`.
  Playlists and their items are `writable(Self::user_id.is(Me))` — one
  person's list is theirs to edit, though the whole household reads it.
  **Assumed, not asked:** the question was about adding songs; if a
  household should edit each other's playlists, that line becomes
  `Everyone` and nothing else moves.
- **Q4, roles:** the server's configuration — `services.harken.roles`,
  a list of account ids per role — and the scanner's own identity. Dev
  auth takes a role from the name, `alice:library`, and says so at
  startup. A provider's groups claim is not built; the `Identity.roles`
  it would feed exists either way.
- **Q5, no mode switch:** agreed by the answers' shape. A peer whose
  tables are all `Everyone` to it receives the log whole by intents, so
  harken's clients are unchanged on the wire until a rule says otherwise.

What this means for the order of work: the engine round is the same size
it was, the harken part is ten declarations and one NixOS option, and the
proof that the private case works lives in the fuzzer and the fleet
rather than in harken.

## Landed

Eight steps, in the order above, each a commit or two (`3dab6b8` A1,
`d8dac32` A2 and A3, `fb84620` A4 and A5, `bb5baca` A6, `2c31802` A7,
then these docs). The engine carries the mechanism and decides no rule; harken
declares a household.

**Rules** (A1, `schema.rs`, `rules.rs`). `Table::visible` and
`Table::writable`, each `Option<Pred>` — `None` is `Everyone` and is not
encoded, so every module before this, `harken.ark` until A7, and every
vector kept its bytes. A rule is the plan algebra's `Pred` over the
table's own columns with two new leaves and one reuse: `Pred::Role(name)`;
`Pred::Exists(table, column, pred)`, the one lookup — some row of a table
whose reference column names this row, admitted by that table's own
predicate, one table away and no further; and `Me`, which is
`Expr::CtxUser` as a comparison's right-hand side, because in a rule the
context *is* the identity asked about (the author for `writable`, the
peer served for `visible`) — a second expression for the same user would
have been two spellings of one thing. `check_schema` holds a rule to its
table (`RuleUnknownColumn`, `RuleBadValue`, `RuleEmptyRole`,
`RuleNotAReference`, `RuleNested`); a role or a lookup in a plan is
`RuleLeafInPlan`. `compat` does not compare rules. The vocabulary is
`.visible(..)`, `.writable(..)`, `Everyone`, `Role("x")`, `col.is(Me)`,
`exists(Child::fk, pred)`. Vectors: `module/rules`, `verify/rules-ok`,
`verify/falsify/a-rule-naming-no-column`.

**Roles** (A2). `Identity { user, session, roles }`; `Ctx::roles` is an
author's own login's, never in an entry and never read by a body. The
engine's dev auth reads a token `name:role,role` (`dev_identity`).
`ark-auth`: `Account.roles` (absent when empty), kept on a session as
issued; `Auth::with_roles` from configuration, merged at every ask and
never written into a session, so a restart grants or revokes for every
login at once; the exchange and `/auth/me` answer with the merged roles;
`Auth::announce` says them at startup.

**Writes** (A3). After the run and before anything is appended,
`Authority::sequence_as(entry, who)` holds every fact's row — old against
the state before, new against the state after — to its table's
`writable` for the connection's identity, or answers
`Refusal::Forbidden(table)` (`"<table>: not this login's to write"` on
the wire; `protocol/server-reject-forbidden`). `Replica::mutate` holds its
preview to the rule for the author's `Ctx` roles, so a forbidden intent is
refused on the device and never pending. Replay, adoption and a peer
alone's authority (`sequence_entry`) do not judge again.

**Reads** (A4, `protocol.rs`). Whole or partial is decided at `Hello` by
`rules::whole_to` — every table's rule absent or decided true by the
identity's roles — and never asked for. A whole connection is served
exactly as before. A partial one:

- starts every connection from a `snapshot` with `partial: true` of the
  rows it may see at the head and their digest — what it may see can move
  while it is away with no entry saying so;
- is paged with `Covers { after, upto }` (`after` and `upto` on the wire),
  sent even when no item survives, so its cursor is always the last
  sequence it was told about; a page's items are the entries with a fact
  it may see, or its own, each with `rules::filter_facts` between the
  states before and after the entry (overlays over the head with the
  facts above taken back): an edit across the line as the add or the
  remove it is to that peer, then the rows the lookup form's rule made
  visible or hid without the entry touching them, found through the
  referencing facts' old and new column values; another person's intent
  goes as its envelope, arguments and autos empty;
- asking `need_facts`, is answered with the filtered facts and no others.

The client: `Subscription::partial` in its `hello`; `Replica::partial` and
`through`, confirmed entries applied by facts only, every sequence to
`upto` in order (an empty list for one that passed with nothing, so the
journal stays contiguous), an own intent confirmed by its record where
the facts equal it and rebased otherwise, never a divergence; `partial`
durable beside the cursor (a client's `replica` record, the sim's kept
copy). A client told a page continues from past where it has been told
asks again from its cursor; a partial connection asking from anywhere but
where it was sent is started over from a snapshot. Vectors:
`protocol/client-hello-partial`, `server-batch-partial`,
`server-snapshot-partial`.

**Verify** (A5). From `rules::partition_hash` of the state at the
sequence (the head's store, or `Log::state_at`); a `verify` says `partial:
true`, a partial replica says none before its connection's first page or
snapshot, and a claim of the other kind than the connection is served is
`unknown` (`protocol/client-verify-partial`).

**The cost of a whole peer is what it was.** Every new field is absent
for it; `fanout` reads one more boolean per connection; `whole_to` is
asked once per `Hello`; `forbidden` looks up each fact's table and finds no
rule. Measured in release against this plan's
parent commit, `perf.rs` whole: every byte count and every allocation
count is identical — 1,960, 7,720 and 30,760 bytes per push to 10, 40 and
160 connections, a `Batch` of 256 intents 41,388 bytes in 13,598 and 7,439
allocations, with facts 59,992 in 21,534 and 12,559 — and every time moved
within run-to-run noise, in both directions (fan-out 3.9–5.6 µs per
connection against 3.4–6.6 before; `sequence_entry` 8.5–10.5 µs per entry
against 10.9–14.1; `Verify` at the head 2.4–6.0 µs against 2.3–66).

**What a partial peer costs** (`perf_g_partial_fanout`, release, this
machine). Every connection partial, the
demo's `playlist` ruled `user_id is Me` (or that, or the lookup: one of
its items' `track_id` is `public`), each connection the author: 5.3–10.0
µs per connection per fact for `Me`, 6.0–10.6 for the lookup, across 10,
40 and 160 connections over logs of 500 and 8,000 — about one and a half
times a whole connection's 3.6–5.6 µs, which carries the intent and no
facts. A partial page is 290 bytes per connection where a whole one is
196: the facts travel and the intent's arguments do not. Most of it is
encoding, done once per connection because each page is its own; the
server's `recv` — the filter and the page — is 1.2–4.8 µs per connection
against a whole connection's 0.6–3.3.

**The fuzzer** (A6). Rules on three generated schemas in five — `Me` on a
text column, a role, both, the lookup on one parent in two that has a
child — roles granted as a session's first ops and changed (`Op::Roles`)
one op in sixty-seven, one device in four believing every role. Checks:
every partial client holds exactly what it may see at its cursor under
the identity it was served as, after every op; every sequenced entry is
held to `writable` for the pushing connection; at the end every client at
the head with its partition's digest, and no `Verify` disagreed. Found and
fixed, each with its vector: a dropped partial page stepped over by the
next one's `upto` (why `after` exists:
`rebase/fleet-fuzz-a-partial-page-missed`); a lookup through an overlay
admitting a row naming nothing, because `scan_where_eq`'s equality is a
hint an overlay's own writes are not in (`rebase/fleet-fuzz-a-lookup-
reads-an-overlays-writes`); a partial client verifying after a role
change before its new snapshot landed (why a `verify` says `partial` and
waits for `served`). Runs after the fixes: seed 1, 200 cases — 400
sessions, 36,580 ops, 7,226 entries, 0 findings, 783 peers ended partial
and 875 whole, 437 writes refused as forbidden on the device and 79 by the
authority; seed 20261005, 300 s — 3,213 cases, 6,426 sessions, 601,682
ops, 127,262 entries, 563 verifies answered and 97 unknown, 0 findings,
10,491 partial and 15,851 whole, 28,022 role changes, 8,920 forbidden on
the device and 1,980 by the authority. Falsified: the filter skipped for
one table (49 findings in the 200 cases), pushes sequenced unjudged (17),
the lookup's diffs sent nowhere (4), the partition digest as the whole
store's (58), `after` ignored (2).

**harken** (A7). Nothing declared `visible` — the household reads
everything, playlists included, and its clients see no wire change. The
eight library tables `writable(Role(LIBRARY))`; `playlist` and
`playlist_item` `writable(Self::user_id.is(Me))`. `services.harken.roles`
(role → account ids) becomes `HARKEN_ROLES` (`library=a,b;admin=a`) and
`Auth::with_roles`; the scanner's account holds `library` by construction
(the server's configuration grants it, and its device holds it whatever
its login says). The desktop and `harken-peer` hand a login's roles to
their peer; `harken-peer --roles` names more for a device with no login.
`harken.ark` moved and its module hash with it (`abf1cbdd…` to
`e9383d95…`); no mutator's closure did — the schema is in no closure — so
`agreement.rs`'s pinned hashes stand. `ark_client` views no longer start
over when a login's roles move: a body never reads one.

**The fleet.** `a_partial_peer_a_role_granted_by_a_restart_and_a_library_
write_refused`: harken hosting its module with private playlists
(`visible(Role("admin") or user_id is Me)`), alice and bob partial, bob's
song refused on his device; the server restarted with `admin=alice;
library=alice`, alice signed in again, whole, her song sequenced; restarted
with `library` revoked, her device still believing it, her next song
refused by the server and in the log nowhere; every peer at each step at
the head holding its partition's digest. 1.9 s. Falsified by building the
server without its configured roles (alice's song after the grant
refused) and by serving every connection whole (bob held the store).
The fleet's own people hold `library` (`FLEET_ROLES`) because its
scenarios add songs from every peer, and hold to convergence.

**Not verified.** OpenID Connect groups are not read; roles come from
configuration only. The Swift and Kotlin runtimes know nothing of rules
(frozen at v3). A partial peer's preview runs over what it holds, so a
write whose effect depends on rows it cannot see is right only once the
authority answers. A lookup costs the referencing rows of one parent per
evaluation, and an entry touching many referencing rows asks each named
parent twice; nothing here measures a lookup over a large child table.
`remove_media` takes a song off every playlist holding it, which under
`playlist_item`'s rule the scanner may not do for somebody else's — only
tests call it, and a caller would need `Role(LIBRARY)` beside `Me` there.
`nix flake check`, `checks.harken-module` with the new role checks, and the
NixOS module on a machine were not run here.
