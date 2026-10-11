# Running the vectors

What a runtime in any language does with each file under `vectors/`: what
it reads, what it does, what it compares, and whether a client runtime
must run it at all (`docs/plan-phones.md` S1). A runtime is conformant
when it runs every vector `vectors/MANIFEST.json` marks `client` and
passes each one marked `"expect": "pass"`, and fails each one marked
`"expect": "fail"`. `rust/ark/tests/vectors.rs` and
`rust/ark/tests/traces.rs` are the reference reading of this file; where
the two disagree, the code is right and this file is fixed.

## Every runner

**It reports by name.** A run prints, per kind, the vectors it ran and the
vectors it skipped, each by path, and says why it skipped one (the
manifest's `side`). "Passed" must never be able to mean "ran nothing".

**The manifest is the list.** `vectors/MANIFEST.json` is written by
`ark-vectors` from the files it wrote, so it lists every vector there is.
Each entry:

| field | meaning |
|---|---|
| `path` | the file, relative to `vectors/` |
| `kind` | its directory: `codec`, `order`, `hash`, `module`, `verify`, `eval`, `views`, `protocol`, `rebase`, `trace` |
| `form` | which of the kind's shapes the file is, named in the sections below |
| `side` | `client`: every runtime runs it. `rust-only`: a fleet run through Rust's simulator and its seeded scheduler, which no other runtime reproduces; its coverage reaches them through the traces of its clients (§ trace) |
| `expect` | `pass`, or `fail` for a `falsify/` vector: a deliberately wrong expectation that a runner which reads the expected value must reject |

A runner walks the manifest rather than the directories, and fails if a
file the manifest names is missing — or if a directory holds a `.json` the
manifest does not name, which is a manifest out of date.

**The JSON dialect** is `spec/README.md`'s ("The vectors"): `{"$int":
"decimal"}`, `{"$bytes": "hex"}`, `{"$id": "8-4-4-4-12"}`, and an object
with exactly one of those keys is the wrapper; text, booleans, `null`,
arrays and objects are themselves. An object is a struct whose keys are in
code-point order. Where a field below says *hex*, it is a plain JSON string
of lowercase hex (`bytes`, `hash`, `function_hash`), not a `$bytes`
wrapper; where it says *bytes*, it is the wrapper.

**A store** is written `{table: [row, …], …}`, every table of the schema
present, each table's rows in key order, a row a struct of every column. It
is read by putting each row into an empty store of the module's schema.

**A change** is the protocol's fact form: `{"t": "add", "table", "row"}`,
`{"t": "remove", "table", "row"}`, `{"t": "edit", "table", "old", "new"}`.

**A ctx** is `{"user", "session", "roles"}`; `roles`, a list of texts in
ascending order, is absent in vectors written before roles were read,
which is no roles.

**Comparing.** Values compare structurally: a struct by its fields, a list
by its elements in order, an `Int` as a 64-bit integer, `Bytes` and `Id` by
their bytes, text by code points. Bytes compare byte for byte.

## codec — client

`form: codec`. `{value, bytes}`, `bytes` hex. **Do**: encode `value`
canonically (`spec/README.md` §1.3); decode `bytes`. **Compare**: the
encoding is `bytes`, and the decoding is `value`.

## order — client

`form: order`. `{input, sorted}`, two lists. **Do**: sort `input` by the
total order over values (§1.2). **Compare**: the result is `sorted`,
element for element.

## hash — client

`form: hash`: `{module, store, hash}`. **Do**: decode the module (§7.2),
build `store` under its schema, take its state hash (§8.1). **Compare**: it
is `hash` (hex).

`form: parts`: also `tables`, one per table of the schema in schema order:
`{table, rows: [{row, leaf}], digest}`. **Compare**, from the canonical
encoding and SHA-256 alone: each `leaf` is `sha256(enc(Text table) ‖
enc(Struct row))`; each `digest` is the sum of its table's leaves as
256-bit big-endian integers modulo 2^256 (32 zero bytes for none); `hash`
is `sha256(enc(List [List [Text t, Bytes digest]…]))`; and the store's own
state hash is `hash` too. Writing the 256-bit sum yourself, rather than
trusting a library's, is the point of this form.

## module — client

`form: module`: `{module, bytes, hash}`, `bytes` and `hash` hex. **Do**:
decode `bytes` (§1.3); read the result as a module (§7.2); encode the module
back to a value (§7). **Compare**: the decoded value is `module`; the
module's `spec` is 4; the module re-encoded is `module` and its encoding is
`bytes`; its module hash (§8) is `hash`; its schema is well formed (§2).

`form: stripped` (`module/private.json`): a server's module with the module
a client loads beside it — also `stripped` (a module value),
`strippedBytes` and `public` (hex), and `functions`, `[[name, hash], …]` in
name order. **Compare** as above, and: `strippedBytes` decodes to
`stripped`; the module hash of `stripped` is `public`; every function of
both modules hashes to the hash `functions` lists for its name (§8: a
function's hash is of its closure with private blocks stripped). The one
step a client may skip, naming it, is checking that `stripped` *is* the
server's module with its private blocks taken out (`ir::strip_module`):
stripping is what `arkc strip` does on the way to a phone, which a phone
never does. Every other step of this form is a client's.

## verify — client

`form: verify`. `{module, verifies}`, `verifies` always `true` (a `falsify/`
vector is a module that does not verify, still saying `true`). **Do**:
decode the module and verify it (§9). **Compare**: it verifies; its verified
form is the module unchanged; its schema is well formed; its `spec` is 4;
every function has a closure and a hash.

## eval — client

`form: apply`: `{module, function, function_hash, store_before, ctx, autos,
steps}`, each step `{args, changes, store_after, hash_after}`. **Do**:
build `store_before`; for each step, apply `function` (§6) under `ctx`,
`autos` and the step's `args` to the store as it stands, then again through
the function's closure (§8) over a second copy. **Compare**: the function's
hash (hex) is `function_hash`, and is one an entry of
`rebase/three-peers.json` names; per step, the changes are `changes` in
order, the store is `store_after`, its state hash is `hash_after`, and the
closure's run gave the same changes; at the end the two copies are equal.
A refusal or an evaluation fault at any step is a failure.

`form: cases`: `{module, store_before | store, ctx?, cases}` — one store
for every case, each case run over a fresh copy of it. A case with
`refused`: apply `function` under the case's `ctx` (or the vector's, or
`alice`/`session-1`), `autos` and `args`; `refused: null` means it must
apply, and its `changes`, when given, are compared; a text means it must be
refused by exactly that text (`Refusal::Refused`). A case with `input`:
the form validator (§6.7) over `input`; compare its `messages`, `[{field,
message}]` in order, and its `normalised` values.

`form: raw` (`eval/raw-*`): `{module, raw, function_hash, store_before,
steps}` — a raw write (`raw.rs`), run natively by every peer. **Compare**:
`function_hash` is `sha256(enc(Text raw))`; per step, run the raw write
with `args` over the store: either it applies and its changes are
`changes`, or it is refused and the refusal's text (§12.5) is `refused`
and the store did not move; then `store_after` and `hash_after`.

## views — client

`form: views`: `{module, query, plan, ctx, args, store_before,
rows_before, batches, steps}`. **Do**: the query's plan is `plan` as the
module writes it (compare); run the query's middleware (§3) under `ctx`
and `args` over `store_before` for its checked arguments and provided
values; hydrate the plan (§13). Then per batch: apply the batch's changes
to the store whole, and push them to the view as one. **Compare**: the
answer at hydrate is `rows_before`; per batch, the patches are the step's
`patches`, the answer is its `rows`, the view is a fresh hydrate of the
store (§1.5's contract), and splicing the patches into the answer before
gives the answer after. Patch semantics are `spec/README.md`'s.

`form: fuzz`: a view the fuzzer found wrong. The same, but a step carries
no `patches` — what a correct engine patches is not what the broken one
did — so only the answer, the contract and the splice are compared.

## protocol — client

`form: client-frame` (`client-*.json`, a frame a client writes) and
`form: server-frame` (`server-*.json`, one a client reads), both `{frame,
bytes}`. **Do and compare**, both forms: `bytes` decodes to `frame` and
`frame` encodes to `bytes`; the frame reads as a frame of its side
(§12) and writes back as `frame`, and that frame read again is itself.
For a server frame, also as a client reads the socket
(`ServerMsg::decode_for`): under the demo's schema (the module of
`rebase/three-peers.json`) and under the empty schema, a `snapshot`'s rows
built as they are decoded are the frame's rows, and any other frame reads
as itself (closures compared through their values, since a decoded
closure carries no symbol names). A client runtime reads every server
frame and writes every client frame, so every file here is a client's —
`client-push-raw` included: a client never authors a raw write, but its
codec must still say one.

## rebase — the scripted half client, the fleets Rust's

`form: three-peers` (`rebase/three-peers.json`) — client. `{module,
entries, facts, alice_alone_pos_of_9, alice_after_rebase_pos_of_9,
final_hash, final_store}`; each entry is `{seq, id, actor, session, fn,
args, autos, roles?}`, `fn` bytes. **Do and compare**, in order:

1. An authority sequences every entry: each lands at its `seq` with the
   matching `facts`; its state hash is `final_hash` and its store
   `final_store`.
2. A whole replica (§11) given every entry in reverse order, then settled,
   is at the last sequence with `final_hash` and no divergence.
3. A replica with no closures given every entry needs facts for every
   sequence; given the facts and settled, it reaches `final_hash`.
4. The scenario, step by step, as `rebase_three_peers` in
   `tests/vectors.rs` runs it: alice creates the playlist; bob adds two
   tracks while alice, alone, adds one — first on her view
   (`alice_alone_pos_of_9`), a local mutation reporting one change; the
   two land in two settles, reported as six transitions, after which her
   track is third (`alice_after_rebase_pos_of_9`); her intent pushed,
   acknowledged with facts saying `pos` 3; three replicas at `final_hash`;
   a duplicate delivery changes nothing; a replica whose closure steps by
   two diverges at 2, 3 and 4 and is healed by the facts; eve, alone,
   confirms her own intents by a local commit (§11) at her authority's
   hash.

   Server-only, and skipped by a client runner, naming them: the authority
   adopting eve's log by replaying it, and refusing it with `FactsDiffer(2)`
   once its facts are touched (`Authority::adopt` is what a server does
   with a log a peer hands it); and compaction — the horizon moved to 2, a
   page at 0 answered with the snapshot and at 2 with the tail, the state
   at the head from facts. Everything else in it — sequencing included,
   since a client alone is its own authority — is a client's.

`form: fleet-seed` (`rebase/fleet-seed-7.json`, its `falsify/`) and
`form: fleet-fuzz` (`rebase/fleet-fuzz-*.json`) — **rust-only**. A
seeded fleet: `clients`, `seed`, `script`, and either the head, hash and
store it must reach or, for a session the fuzzer found a bug with
(`fuzz`), convergence and replay. They are run through
`ark::sim::run_script` — Rust's simulator, whose network picks which frame
moves next from the seed with Knuth's MMIX generator, duplicates one in
eight and drops one in sixteen, over Rust's server. To run one is to
reimplement that scheduler and that server exactly, which is not a client.
What these sessions do to a client reaches every runtime anyway: the
fuzzer records each client's life in each session as a trace, and every
trace is a client vector (§ trace) — the frames the scheduler delivered
are its inputs, so nothing of the scheduler is left to reproduce.

## trace — client

`docs/plan-phones.md` S2. One client's life in one session, as its inputs
and its outputs. `form: fuzz` (`trace/*.json` but harken's): chosen from
a fixed run of `arkc fuzz --traces` (`bin/vectors/trace.rs` says which
feature each file is the smallest trace of, and which few are pinned to
the one fuzzer case that found a behaviour the run reaches nowhere —
`alone-leaves-the-inbox`, today); `form: harken`
(`trace/harken-*.json`): sessions over `harken.ark` with its `library`
and `playlist` views kept. `packages.trace-corpus` is thousands more in the
same format, every client of every session of a fixed fuzz run, not
checked in; a runtime's check runs it as it runs these.

### The file

`{from, module, steps}`, and `expect` on a `falsify/` trace. `from` says
where it came from (`arkc fuzz --seed N --cases 1, session S, client C`
reproduces it). `module` is the server's module, private blocks and all:
the client holds its closures **stripped** (§8, `hash::stripped`), each
under the hash the full module gives it. `steps` is a list, one per line,
each `{"in": input, "out": output}`.

### The client

The engine's client (`ark::protocol::Client`) over its replica
(`ark::peer::Replica`), with what it makes durable and, when alone, its
own authority — not `ark_client::Peer`, whose dial, backoff, verify
after every settle, drawn ids and times and storage encoding are a
driver's policy a phone may choose otherwise, and in part depend on a
clock no trace can carry. A runner holds, beside the client:

- **its durable copy** (§11.1): after every step, whatever the confirmed
  store moved by since the last step (`Replica::take_confirmed`: the facts
  of each sequence applied, or "replaced", which copies it whole), applied
  to a copy; with the cursor, the log id and the held columns beside it.
  Pending intents are durable as the replica's own list.
- **alone**, the fork (cursor and log id where it left) and an authority.
- **its views**: the specs of the last `hydrate`, and a maintained view of
  each, or none where it was refused.
- **the state it last reported**, field by field.

### Inputs

Each `in` is `{"t": …}` and fields. Apply it, then read the outputs.

| `t` | fields | the client |
|---|---|---|
| `open` | `mode` (`whole`, `facts`), `closures` (bool), `token` (text or null) | opened at cursor 0 over an empty store of the module's schema, holding the module's closures stripped, or none; no log id; unlinked. It reports itself rebuilt. A fresh durable copy, no fork, no views kept from before |
| `reopen` | — | stopped and opened from its durable copy: the replica over the copy's store at its cursor — under the device's schema when the copy holds a union (§12, `scope::device_schema`) — with its log id, its union, a page horizon (`through`) at the cursor, the pending intents in order, and the closures it held; the client over it with the same mode and token, and the module's schema. Unlinked; its verify answers and denial are gone; it reports itself rebuilt. Alone it stays alone |
| `connected` | — | `Client::connected`: linked; queued frames dropped; `hello` at its cursor, mode, log id and `partial`, with the token and spec 4; then everything pending pushed |
| `disconnected` | — | `Client::disconnected` |
| `mutate` | `ctx`, `function` (its name, for a reader), `fn` (bytes, the hash), `id`, `autos`, `args` | `Client::mutate`: authored under `ctx` with exactly these autos — frozen by the originating peer, never drawn again — and pushed if linked; a local verdict is the output `refused`. Alone, everything pending is then sequenced by its authority (`peer::local_commit`) |
| `sign_in` | `ctx`, `token` | `Client::sign_in`: every intent pending as nobody made `ctx`'s (user and session), and the token set |
| `token` | `token` | the token its next `hello` carries; nothing else |
| `receive` | `frame` (bytes) | the frame read as a client reads the socket (`ServerMsg::decode_for` under the module's schema): a snapshot adopted at once (`Client::recv_snapshot`, which first applies what earlier frames placed), anything else placed (`Client::recv`) and applied by the next `settle` |
| `settle` | — | `Client::settle`: the inbox applied once, the facts it waits on asked for, the next page asked for |
| `verify` | — | `Client::verify_all` |
| `alone` | — | `trace::alone`: the fork is its cursor and log id; an authority over its stripped closures, not a server's (no private block runs), its log based at the cursor with no entries, its state the confirmed store; the replica's log id cleared, and its inbox — what the server sent and was not applied, a page past a sequence that never came — left behind, since applied it would be the local history's own next sequence (`trace/alone-leaves-the-inbox.json`); everything pending sequenced locally |
| `join` | `ctx` | `trace::join`: every entry the authority sequenced above the fork, in order, re-stamped with `ctx`'s user and session, taken back out of the confirmed store and re-queued ahead of anything pending (`Replica::fork_back`); not alone any more. Connecting is the next input |
| `hydrate` | `views`: `[{query, ctx, args}]` | these views in place of any kept: each query's middleware run under its `ctx` and `args` over the client's optimistic store, and its plan hydrated (§13); a verdict is that view refused |

### Outputs

After the input, in this order: take every frame the client queued
(`take_outgoing`) and what its optimistic store did since the last step
(`Replica::take_changes`: rebuilt, or the changes in order); move the
durable copy; then write `out`, a struct of only these fields:

- `frames` — every frame queued, in order, each its canonical encoding;
  present when there is one.
- `refused` — the text (§12.5) of a mutation's local verdict; present only
  then.
- `views` — one element per kept view, in the order of the last `hydrate`;
  present when any is not `null`. Each is `{"rows": answer}` when the view
  was hydrated this step — by `hydrate`, or because the client reported
  itself rebuilt, which re-hydrates every kept view over the store as it
  now is; `{"patches": [patch…]}` when the step's changes were not empty
  and the view is held: the changes pushed as one batch over the store
  after them (§13; `[]` is a push that moved nothing); `{"refused": text}`
  when a hydrate or a push was refused (by a verdict, or `bug: …` for a
  fault), after which the view is held no more until the next rebuild or
  `hydrate`; and `null` otherwise.
- the **state**, each field present only where it differs from the value
  the last step wrote — and every one of them on the step of an `open` or a
  `reopen`:

  | field | value |
  |---|---|
  | `cursor` | the replica's cursor |
  | `log` | the log id it holds, or `null` |
  | `pending` | the ids of its pending intents, in order |
  | `rejections` | `[{id, reason}]`: every verdict against its own intents the replica holds, oldest first, `reason` its text. Kept across a snapshot, which appends what its replay refuses; started over by a `reopen`, from what the reopen's replay refuses |
  | `diverged` | the sequences at which its replay disagreed with the authority's facts; started over by a snapshot and by a `reopen`. Empty in every trace of a correct session |
  | `agreed` | `[{seq, ok}]`: every answer to a `verify` this client has had, oldest first; `ok` `true`, `false`, or `null` where the authority could not say (`unknown`). Started over by a `reopen` |
  | `denied` | the reason of a `denied` frame, or `null` |
  | `union` | `{table: [column…]}`, the tables it holds in part with the columns held, or `null` for a client that holds the log whole |
  | `hash` | the state hash (§8.1) of its confirmed store, under the schema it holds (the device's, for a union) |
  | `view_hash` | the state hash of its optimistic store |

### Compare

Replay every step into a fresh runner and compare each `out` with the one
recorded, field by field: an event field (`frames`, `refused`, `views`)
must be present exactly when it is recorded and equal when it is — every
frame byte for byte; and the state, read as the recorded fields folded
over the state before, must equal the runtime's whole state at every
step, so a field absent from both is still compared. On the first step
that differs, report the trace, the step, its input's `t`, and each field
with both sides. `trace/falsify/` holds a trace with one byte of one frame
changed, which must fail.

`ark::trace::Player` is this section as code, and `tests/traces.rs` runs
it over `trace/` always and over a corpus directory when
`ARK_TRACE_CORPUS` names one. That the Rust client passes its own traces is
what says the inputs are complete: the recording was falsified by leaving
the frozen autos out of a mutation (the push replays with other bytes) and
by leaving a reopen out (the state after it differs).
