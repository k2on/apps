# The phones: Kotlin and Swift at v4, held to one suite

Swift and Kotlin were frozen at spec v3 (`kotlin/FROZEN.md`,
`swift/FROZEN.md`) while v4 moved. It has settled — plans and maintained
views, the rebase by records, the additive digest, roles stamped into
entries, scopes and unions, private blocks, the authority's raw writes —
and the owner's decision is to bring both runtimes to it rather than put
the Rust engine on the phone behind bindings, **on one condition: a
common test suite, in files, that Rust, Kotlin and Swift are each checked
against.** This is the design for that suite and for the two ports.

The risk the condition answers is specific. Nearly every engine bug found
this month was found by the fuzzer running the Rust engine against
itself — about ten, each now a vector. A second engine written in a
second language gets none of that unless the fuzzer's scrutiny can be
carried to it as data. Vectors carry the *cases* the fuzzer found; they
do not carry the fuzzer. So the suite has to.

## What a phone's runtime is, and is not

A phone runs a **client**: it loads a module (`harken.ark`), verifies it,
interprets it, maintains views, holds a replica, rebases, speaks the
protocol to a server, holds its union when it is partial, takes private
and raw entries by facts or replay, and — alone — is its own authority
over a local log. That is the whole surface.

It does **not** author modules (the domain is written once, in Rust, and
`arkc` emits the module: the v3 Kotlin and Swift authoring vocabularies
and `harken/domain/gen/kotlin` are deleted, not ported), run a server,
run the fuzzer, or serve the explorer. Each of those stays Rust, and the
suite says which vectors a client runtime skips and why.

## The suite

`spec/vectors` is already the contract: language-neutral JSON, written by
`ark-vectors` from the Rust engine, checked byte-identical by
`checks.vectors`. Three things make it the *whole* contract for a client.

### S1. `spec/RUNNERS.md`, the runner's contract

One section per vector kind — `codec`, `order`, `hash`, `module`,
`verify`, `eval`, `views`, `protocol`, `rebase`, and the new `trace` —
saying exactly what a runtime reads, does and compares, and whether a
client runtime must run it. A runtime is conformant when it runs every
vector its section marks *client* and passes. Where a kind is server-only
(an authority's sequencing, retention, the raw-write refusal at the
server) the file names it and the reason. The Rust runner
(`rust/ark/tests/vectors.rs`) is the reference reading of it, and every
runner reports the vectors it ran and skipped by name, so "passes" can
never mean "ran nothing".

### S2. Trace vectors: a client as a pure function

A **trace** is one client's life in a session, written down as its inputs
and its outputs, with nothing left to a scheduler:

- **Inputs**, in order: open (fresh, or from what this client last wrote
  to its own storage — a restart); a local mutation (function, arguments,
  and the autos the Rust client drew, frozen); sign-in with an identity
  and roles; a frame received, as bytes; going alone, joining; settle.
- **Outputs**, after every input: every frame the client emits, as
  canonical bytes; its cursor, its log id, its pending ids in order, its
  rejections and divergences; the hash of its confirmed store and of its
  view; for a partial client, the union it holds; and, for each view the
  trace hydrates, the patches it reports.

A runtime replays the inputs into its own client and must produce the
outputs **byte for byte**. The server is not in the trace: what the
server said is already the frames. So the client is tested as a function
of its inputs, and the Rust simulator's random scheduling never needs to
be reproduced by anyone.

Traces come from the fuzzer. `arkc fuzz --traces DIR` records, for every
client of every session, the trace the Rust client lived — so a seeded
run of the fuzzer *is* a corpus, as wide as the fuzzer is: scopes, roles
and stamps, private blocks, raw writes, restarts, horizons, logs lost and
named, alone and joined. Two forms:

- **Checked in**: `spec/vectors/trace/`, a few dozen chosen for coverage
  (one per feature, each fuzzer finding that was a client bug), readable,
  diffed by `checks.vectors` like any vector.
- **Generated**: `packages.trace-corpus`, `arkc fuzz --traces` at a fixed
  seed and case count (thousands of traces), built once by nix and read by
  every runtime's check. It is reproducible from the seed, so it is not
  checked in, and moving the seed or the count is a deliberate edit.

**The Rust client passes its own traces first.** `ark-client`'s trace
runner replays the corpus against the same code that recorded it; that
proves the trace is complete — that nothing the client's outputs depend
on was left out of its inputs. Falsified by dropping one input kind from
the recording and seeing the replay disagree.

harken's own traces join the corpus: sessions over `harken.ark` with the
library and playlist views hydrated, so the patches harken's screens
splice are part of what every runtime must reproduce.

### S3. One check per runtime, over the same files

`checks.vectors` (Rust, as now), `checks.kotlin` and `checks.swift` each
run their runtime's runner over `spec/vectors` and over
`packages.trace-corpus`. A vector added for Rust is a vector the phones
must pass the moment it lands, and a fuzzer finding becomes a test of all
three on the commit that fixes it.

## The ports

Each port replaces its frozen tree in place, keeping what is still right
(the canonical codec, the value model, the client's storage and transport
shape) and rewriting what v4 changed. The engine's semantics are Rust's;
where the Rust code and a spec section disagree, Rust is right and the
section is fixed.

- **Kotlin** (`kotlin/ark-runtime`, `kotlin/ark-client`): the core —
  values, codec, hash, order, module decode and verify, the IR
  interpreter, plans and maintained views; then the client — store with
  its indexes and digest, replica and rebase, the protocol client with
  roles, unions, private and raw entries, the alone authority and its
  journal, storage, the WebSocket transport and sign-in. A JVM
  `harken-peer` beside the Rust one, speaking the same JSON-lines, so the
  fleet — the version matrix included — runs Kotlin peers beside Rust
  ones. Then the Compose app (`harken/android`) over it, loading
  `harken.ark`; `nix build .#harken-apk`.
- **Swift** (`swift/`): the same, in the same order, built and tested on
  Linux with nixpkgs' Swift toolchain. The iOS app's sources are rewired
  and type-checked where the Linux toolchain can; building and running it
  needs Xcode, which nothing here has, and says so.

## Rounds

Each ends in a green `nix flake check`.

1. **The suite.** `spec/RUNNERS.md`; trace recording in the sim and
   `arkc fuzz --traces`; the Rust trace runner over a checked-in set and
   the corpus; harken traces; `packages.trace-corpus`.
2. **Kotlin core**: codec through views; `checks.kotlin` over the kinds
   that need no client.
3. **Kotlin client**: everything else a client is; `checks.kotlin` over
   every client vector and the whole corpus.
4. **Kotlin in the fleet and on the phone**: the JVM peer in the fleet
   and the version matrix; the Compose app; `harken-apk`.
5. **Swift core**, 6. **Swift client**, 7. **Swift peer and the iOS
   sources** — the same three steps.

Guards throughout: every runner names what it ran and skipped; a vector
that fails is fixed in the runtime, never edited; falsify every new test
once; `spec/vectors` changes only by additions this document names; what
is not verified said plainly — above all, that no phone ran here.

## Landed

**Round 1, the suite** (`e148df4`, `0e57d36`, `5ed3518`, `1ce8de2`).
`ark::trace` is S2 as code: a client's life as its inputs — open, reopen,
connected, disconnected, mutate with its frozen autos, sign-in, token, a
frame received as bytes, settle, verify, alone, join, views hydrated — and
its outputs after each: the frames it queued as canonical bytes, a local
refusal, what each kept view reports (rows, patches, a verdict), and its
state — cursor, log, pending, verdicts, divergences, verify answers,
denial, union, the hash of its confirmed store and of its view — written
only where it moved, all of it after an open. The client recorded is the
engine's, `protocol::Client` over its `Replica` with what it makes durable
and, alone, its own authority: the sim's clients are those, and what
`ark_client::Peer` adds over them — a dial and a backoff on a clock, a
verify after every settle, ids and times drawn from the platform, a
storage's encoding — is a driver's policy a phone may choose otherwise, and
in part a clock no trace can carry. The module is embedded whole in every
trace (about 10 KB of a typical one's 28 KB, harken's ~70 KB); the client
holds its closures stripped.

`Sim::recorded` writes a trace per client, taking what a client queued and
what its view was told at the step and handing them to the wire and the
durable copy where the fleet always took them, so a recording fleet runs
as one that is not — `arkc fuzz --seed 1 --cases 200` is the same 37,492
ops and 7,570 entries with `--traces` and without, 6.1s without (6.2s at
`c492b36`, in release, best of two) and 8.9s writing 1,717 traces.
`arkc fuzz --traces DIR` writes every client of every session that found
nothing; `tests/traces.rs` runs it twice to the same bytes (falsified by
putting the clock in the file names). The sim and the fuzzer gained
`leave` and `rejoin` (`docs/plan-alone.md` §1; a settle joins every client
alone first), since alone and join were inputs S2 names and no session had
them. The first harken session that sent a client alone found a bug: going
alone with something of the server's not yet applied in the inbox — a page
past a sequence that never came — applied it as the local history's next
sequence, so bob's playlist was in alice's local history. Going alone now
leaves the inbox behind, in `trace::alone` and in
`ark_client::Peer::start_alone`; `going_alone_leaves_what_the_server_sent_
and_was_not_applied` holds it, falsified by not clearing the inbox.

The Rust client passes its own traces: 300 cases from seed 100, 2,576
traces, every one replayed. Completeness falsified twice: the frozen autos
left out of a mutation's input (95 of 170 traces from seed 100 fail at the
push, on `frames`), and a reopen left out (8 of 170 fail on the state after
it). Both restored.

`spec/vectors/trace/`: 25 traces chosen by `bin/vectors/trace.rs` from
`arkc fuzz --seed 7000 --cases 160`, the smallest not already published for
each of 22 features (a plain sync, a rebase over landed entries, a
rejection, a local refusal, a restart, a snapshot below the horizon, a log
lost and named, alone then join, roles stamped, a partial client excluding
a column through `exists`, a partial client reopened, a private entry taken
by facts, a raw write replayed, a verify answered unknown and one agreed,
a sign-in, needing facts, without closures, fed by facts, a frame delivered
twice, the demo rebasing, views patched); and harken's three — the fuzzer's
session generator run by `ark-vectors` over `harken.ark`, interpreted, with
its `library` and `playlist` views kept, whose patches are outputs. 632 KB,
and `falsify/`, a frame byte changed, which fails. The C2 commit message
says 26; it is 25 and the `falsify/` directory. `MANIFEST.json` lists all
129 vectors with kind, form, expectation and side: every `rebase/` fleet
`rust-only` (8), the rest `client`; `tests/vectors.rs` holds it to naming every
file once (falsified by leaving `trace/` out of it). `spec/RUNNERS.md` is
S1, a section per kind; within `three-peers` the authority adopting a log
and compaction, and within `module/private.json` stripping, are the steps
it names server-only.

`packages.trace-corpus` is `arkc fuzz --seed 2026 --cases 300 --traces`:
2,565 traces, 77 MB, 0 findings, the fuzz run 14s and the whole build with
the crate 2m26s here. `checks.traces` runs `tests/traces.rs` over it —
its own check, the Rust instance of what `checks.kotlin` and
`checks.swift` will be, and kept out of `checks.rust` so the workspace's
tests do not wait on a fuzz run.

`spec/vectors` otherwise byte-identical: `ark-vectors` in release (14.5s,
the trace selection included) writes the tree exactly, the additions only
`MANIFEST.json` and `trace/`. Green: `cargo test --workspace`, `fmt
--check`, clippy over the workspace and over `harken-iced --features demo`
with `-D warnings`, the demo's tests, `allocations.rs` with its ignored
case, `perf.rs` in release; `arkc fuzz --seed 1 --cases 200` and
`--seconds 120` at 0 findings both ways (120s: 3,620 cases, 676,524 ops,
6,450 clients alone without traces; 2,571 cases and 22,069 traces, 624 MB,
with them). Not run: `nix flake check`, and `checks.traces` as a nix build
— its test was run with `cargo test` against the built corpus, 2,565
replayed. The 22,069 traces of the 120-second run were written and not
replayed. No phone ran anything; Kotlin and Swift are untouched.
