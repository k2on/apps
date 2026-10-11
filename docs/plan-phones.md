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
