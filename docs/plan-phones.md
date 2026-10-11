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

**Round 2, the Kotlin core** (`883655f`, `37c9045`, `e003a18`, `4230f66`,
`724fab4`, `17f72f0`, `1d75af0`, `ef48a6d`, and the docs). A phone's
runtime is a client, so `dev.arkdb.authoring`, the `demo` and `selfdemo`
sources that exercised it and `harken/domain/gen/kotlin` were deleted, not
ported, and `harken-apk` left the flake until round 4 (option (b): one
runtime, not a frozen copy beside it to drift; `apk.yml` still names it).
`ark-runtime` was rewritten in place as a port of `rust/ark`, file for
file (`kotlin/README.md` maps them): Value and the canonical codec kept, as
they were right; IR v4 decode, encode and normalisation; the function hash
over the stripped function, the additive digest, `stripped`,
`reads_roles`; the verifier with every complaint by the reference's name
and the verifier's half of `scope.rs`; the interpreter, the standard
library over Unicode tables generated at build time from
`rust/ark/src/unicode_tables.rs`, the store with its secondary, reference
and trigram indexes and the Overlay, raw writes; `pull`/`read`,
`hydrate`/`push_all`/`settle` and R9's kept numbers — counts and sums of
any size, judged on their total (§13), extremes read again through an
index. Two representations differ and say nothing different: a row is a
struct of every column (the Rust's positional row, R11, is
representation), and index postings are keys, not ordinals. `ark-client`
is out of the gradle build until round 3, its sources untouched.

`checks.kotlin` builds the runtime offline from `kotlin/deps.json` (which
needed no re-recording), holds every source to `ktfmt --kotlinlang-style`,
and runs three mains. The conformance runner walks `MANIFEST.json` and runs
54 of the 129 vectors — codec 14, order 2, hash 4, module 8, verify 5,
eval 7, views 14, each falsify vector failing its check — and skips 75 by
name: `protocol` 40, `rebase` 9 (8 fleets `rust-only`, `three-peers` the
client's) and `trace` 26, round 3's kinds. It fails on a vector that fails,
a falsify vector that passes, a manifest out of step with the files, and
a claimed kind with nothing run. KeptNumbers holds kept numbers three
levels deep — further than any views vector, which keeps at most one level
— through 400 seeded batches to a fresh hydrate, to `View.read`'s fold and
to the splice; StoreHints holds 600 indexed reads, over a store and an
overlay, to the scans they stand for. `nix build
.#checks.x86_64-linux.kotlin`: BUILD SUCCESSFUL, 1m15s of gradle.

Falsified, each restored: struct keys unsorted (codec/struct-key-order and
both hash vectors), text ordered by UTF-16 units (order/mixed, and
order/falsify/utf16 then passes), the digest dropping its carries (both
hash vectors), the encoder dropping `private: true` (module/private), an
`on` that is no unique index let through (verify/falsify/insert-on-no-
unique-index then passes), `has_role` always false (eval/has-role), no
parent check on put (eval/raw-put-row at the step wanting MissingParent),
no refill after a removal from a window (views/top-two-by-pos at batch 4),
a node's sum checked step by step (views/a-maintained-sum-answers-where-
the-fold-overflows at batch 7), a kept sum wrapping at 64 bits
(views/a-maintained-sum-overflows-where-the-fold-answers at batch 0), kept
numbers that moved not telling their readers (KeptNumbers at step 11), an
inclusive upper bound read as exclusive in an index range (StoreHints at
round 11), an unformatted source (the check's ktfmt step), a claimed kind
with no vector and a stray or missing vector file (the runner). Two
falsifications missed and say so: dropping a plan's row binder or its
projection from the encoder fails no module vector, whose plans have
neither — the views vectors compare plans that do.

Not verified: the JVM only — nothing here built the runtime for Android or
ran it on a phone. Rust, `spec/vectors` and the Swift tree are untouched.

**Round 3, the Kotlin client** (`f673623`, `34c9b08`, `ec7563d`, and the
docs). The engine half is in `ark-runtime`, file for file with `rust/ark`:
`Log.kt`, `Peer.kt` (the replica, the rebase by records, settle once per
pump, verdicts and divergences, roles frozen, private and raw entries,
intents with no closure, a union by facts, `fork_back`; the authority a
peer alone is, `local_commit`), `Protocol.kt` (every frame both ways and
the `Client`), `deviceSchema`, `Journal.kt` and `Trace.kt`. `ark-client`
was replaced in place — the v3 `Session`, `Durable`, `Link` and `LocalHub`
deleted, `Transport` and `WebSocketTransport` kept — by `rust/ark-client`'s
storage records (`Storage.kt`), what its peer writes and when
(`Writer.kt`), the peer's driver half without its link (`Peer.kt`: open,
mutate by name, sign-in, leave and join with the alone journal) and
`Autos`; the link is round 4's.

One representation differs and says nothing different: a replica's view is
its own store, copied at each replay and moved by every change the confirmed
store moves by, where the Rust shares the tables it has not written (D7.3).

`checks.kotlin` runs every `client` vector — 121 of 129: codec 14, order 2,
hash 4, module 8, verify 5, eval 7, views 14, protocol 40,
`rebase/three-peers` 1 (adopting a log and compaction named and skipped as
a server's), trace 26 — and skips by name only the 8 `rust-only` fleets;
nothing marked client is skipped. It replays `packages.trace-corpus`: 2,565
of 2,565, 6 s of the runner on this machine and 9.8 s inside the nix
sandbox; and again, with the durable copy kept in the storage records, in
10.1 s. `nix build .#checks.x86_64-linux.kotlin`: BUILD SUCCESSFUL, 1m39s
of gradle. Beyond the check, `arkc fuzz --seed 9000 --cases 400 --traces`
replays 3,465 of 3,465 both ways. The storage is held to a fixture a Rust
`ark_client::Peer` wrote (`kotlin/ark-client/fixtures/`): 73 steps alone,
reopened, joined to an in-process server, online and offline, and after
each the Kotlin storage is the Rust one byte for byte.

Two things the runs found. The corpus reaches nothing that holds round 1's
fix (going alone leaves the inbox behind): falsifying it failed no vector
and no corpus trace, and 1 of seed 9000's 3,465 — `arkc fuzz --seed 9046
--cases 1`, session 1, client 0 — which is now
`kotlin/ark-runtime/traces/alone-leaves-the-inbox.json`, unedited and
replayed by the runner; it is a candidate for `spec/vectors/trace/`, which
this round does not change. And replaying the corpus through the storage
records first failed 4 traces at a reopen: a sign-in rewrites intents in
place under the same ids, which the pending pages' one-comparison rule
cannot see; the reference's peer writes the intents whole after a sign-in
or a join, and the runner's storage copy now does too.

Falsified, each restored: the rebase not landing what arrived under
pending (three-peers, 8 traces, 1,510 corpus traces), `hello` without its
`partial` flag (a protocol vector, 4 traces, 756 corpus traces), going
alone without clearing the inbox (only the trace above), a facts page not
applied on load (224 corpus traces through storage), a held column dropped
from the replica record (228), compaction at twice the snapshot and pages
merged at equal sizes (the fixture, at steps 29 and 4).

Not verified: the JVM only — no phone, and no Android build of either
module. The fixture generator is a scratch program over `rust/ark-client`
that no build here compiles; Rust, `spec/vectors` and the Swift tree are
untouched. `DirStorage` was not run against a real disk crash, and does
not sync its directory (`java.io` cannot), where the reference does.

**Round 4, Kotlin in the fleet and on the phone** (`5fa5d42`, `6d6a7bd`,
`92599b0`, `c239083`, `bbdc1ad`, `439e3d6`, and the docs).

Two things round 3 left first. Going alone leaves the inbox behind is now
`spec/vectors/trace/alone-leaves-the-inbox.json`: `ark-vectors` pins the
one fuzzer case that found it (`arkc fuzz --seed 9046 --cases 1`, session
1, client 0) and asserts of it what it is published for — a client gone
alone with a page of the server's past its cursor unapplied — so a fuzzer
change that moves the case fails the generator. An addition: every other
file byte-identical, `MANIFEST.json` 130 vectors, 122 a client runs; the
Kotlin copy and its runner's special-casing are gone. Falsified against
Rust's `trace::alone` and Kotlin's `goAlone` without the clear: both fail
the file at step 53. The corpus never reached it because `leave` drew its
client uniformly and a client almost never holds something unapplied when
drawn; the draw now gives way to a client that does and could leave, the
draw still made, so a session with no such client is unchanged — every
vector came out byte-identical. `packages.trace-corpus` keeps its seed and
count (2026, 300 cases, 2,565 traces) and its content moved: 2 of its
traces now fail under the falsification, in both runtimes, where none did.
And `DirStorage` syncs its directory after a rename
(`FileChannel.open(dir, READ).force(true)`, Android from API 26), degrading
where a platform refuses to open a directory (`syncsDirectory`); held by
StorageTests, falsified by leaving the sync out.

The client's driver half (`kotlin/ark-client`): `Link` is
`ark_client::link` — dial when due, back off half a second doubling to
thirty, reset on an open, stop and resume — over OkHttp's WebSocket on the
JVM and Android alike (one dependency both have; already recorded, so
`kotlin/deps.json` did not move), pinging every `pingEveryMs` and failing a
connection whose ping goes unanswered, its handshake bounded by
`connectTimeoutMs` (without that, a handshake into the fleet's black hole
waited for ever). `Peer` gained `connect`, `pump` (one settle per pump,
R8), `status` field for field, `query`, `check`, `view`, `standing`,
`join(url, login)`; `ClientView` is `view.rs` over `Reads` (`ir/reads.rs`);
`Auth` is `ark_auth`'s client — dev auth answers with a code, a provider's
page goes to `open` and a loopback port; `Json` is `ark::json`'s decoder.
`ClientTests` holds the link, the dialect, the sign-in's URLs and views
over harken.ark spliced against a fresh read.

`harken-peer-kt` (`kotlin/harken-peer`, `packages.harken-peer-kt`) speaks
`harken-peer`'s lines exactly — every flag, command and answer, the same
directory records — and interprets every function of `harken.ark` (in its
jar) or of `--module`'s file; no command is Rust-only. In the fleet
(`harken/server/tests/fleet_kotlin.rs`, `checks.fleet-kotlin`, its
`FLEET_KOTLIN_PEER` and the pinned revisions set as `checks.versions` sets
them; `docs/plan-fleet.md` §6) six scenarios run mixed: two Rust peers and
a Kotlin one through a black hole, kill -9s of a peer and of the server
and a cut; a Kotlin peer alone then joining, and leaving and joining again
without a restart; through a server restart and a lost log; new Kotlin
peer, old server and old Kotlin peer (that revision's own `harken.ark`),
new server, per pinned revision; and a Kotlin peer partial under a scoped
module (harken plus a scope holding a person's own playlists), its
confirmed hash the person's union of the log's state and equal to a Rust
peer's of the same person. All pass: `nix build
.#checks.x86_64-linux.fleet-kotlin`, 7 tests in 14.7 s, each scenario
0.2–1.1 s to converged once started and a Kotlin peer up and signed in in
0.6–0.7 s. No disagreement between the runtimes was found. Each falsified
once with the Kotlin peer broken — no push of pending on an open, a join
without its fork-back, snapshots ignored, an unknown function read as a
verdict, `--module` ignored, a partial snapshot opened whole — and one
falsification that did not fail is said in its scenario: ignoring acks,
which every phase of K1 recovers from by reconnecting.

The app (`harken/android`) loads `harken.ark` from its assets (copied at
build time from where the domain crate writes it), verifies it and
interprets it; one `Peer` on a thread of its own, pumped every 50 ms;
`library`, `playlists` and the open `playlist` are maintained views whose
patches are spliced into the screens' lists, read again only on a `Reset`
or a new selection; writes are `create_playlist`, `add_to_playlist`
("Favorites" first for somebody with none), `remove_from_playlist`, each
with its standing; harken's module has no rename, so neither has the app.
Sign-in is dev auth only, said in the app and its README: a provider's page
is not opened. Settings: the server's address, work alone, and the status
(cursor, not synced, held, diverged, the link, linked, connections, behind,
last close, denied). `harken-apk` and `harken-apk-deps` are back in the
flake, built from 883655f^'s derivation over the new sources;
`harken/android/deps.json` did not need re-recording. `nix build
.#harken-apk`: `harken-debug.apk`, 10,193,386 bytes, `assets/harken.ark` in
it, after three retries through Maven Central's 429s.

Green, each by its own `nix build .#checks.x86_64-linux.<name>` (not `nix
flake check`): `kotlin` (122 of 130 vectors, 2,565 corpus traces, again
through storage, ClientTests), `vectors`, `traces`, `fuzz-smoke`,
`harken-domain`, `rust` (fmt, both clippys, the workspace's tests and the
demo's), `versions` and `fleet-kotlin`; and `harken-apk`. The first run of
`checks.rust` ran the disk out and was run again after a garbage
collection.

Not verified: no phone or emulator ran the APK, and it was installed
nowhere; the client's link, sign-in and views ran on the JVM only. The
provider sign-in flow on a phone is not built. `DirStorage`'s directory
sync was not run on Android nor against a real crash.

**Round 5, the Swift core** (`88ba922`, `c8810c2`, `537a388`, `9b973bb`,
`72f6afe`, and the docs). A phone's runtime is a client, so `ArkAuthoring`,
the `Demo`, `Kitchen`, `HarkenDomain` and `HarkenPhone` targets and their
tests, and `harken/domain/gen/swift` were deleted, not ported. `ArkDB` was
rewritten in place as a port of `rust/ark`, file for file (`swift/README.md`
maps them), with the Kotlin port as the nearest worked reading: values, the
canonical codec, the order and the JSON dialect; IR v4 decode, encode and
normalisation; the state and function hashes; the verifier with every
complaint by the reference's name and the verifier's half of `scope.rs`;
the store with its secondary, reference and trigram indexes, its digest
and the Overlay; the interpreter and the standard library over Unicode
tables generated at build time from `rust/ark/src/unicode_tables.rs`; raw
writes; `pull`/`read`, `hydrate`/`push_all` and R9's kept numbers.
`ArkDBClient` is out of the package until round 6. Three things are Swift's
own and say nothing different: a value's text is compared, hashed, prefixed
and searched by its UTF-8 bytes, never by `String`, whose `==` is canonical
equivalence; nothing observable iterates a `Dictionary`, whose order is
seeded per process (sorted arrays stand for the Rust's `BTreeMap`s); and
`Wide` stands for Rust's `i128`, which Swift 5.8 lacks.

`checks.swift` (`packages.arkdb-swift`, `devShells.swift`) builds it with
SwiftPM in release over nixpkgs' Swift 5.8 — the old derivation's
Foundation and Dispatch inputs and `tools/nix-swiftc` — and runs three
things in one executable (no XCTest here): the conformance runner, which
walks `MANIFEST.json` and runs 54 of the 130 vectors — codec 14, order 2,
hash 4, module 8, verify 5, eval 7, views 14, each falsify vector failing
its check — and skips 76 by name: `protocol` 40, `rebase` 9 and `trace` 27,
round 6's kinds; StoreHints, 600 indexed reads over a store and an overlay,
each the scan it stands for; and KeptNumbers, kept numbers three levels
deep through 400 seeded batches. It fails on a vector that fails, a
falsify vector that passes, a manifest out of step with the files, and a
claimed kind with nothing run. `nix build .#checks.x86_64-linux.swift`:
about 20 s.

Falsified, each restored: struct keys sorted by name (codec/struct-key-
order); text ordered by UTF-16 units (order/mixed, and order/falsify/utf16
then passes — ordering by `String`'s `<` does not falsify it, since the
vector's texts are NFC); a stray and a missing vector file and a claimed
kind with none (the runner); the digest dropping its carries (both hash
vectors); the encoder dropping `private: true` (module/private); an `on`
that is no unique index let through (verify/falsify/insert-on-no-unique-
index passes); `has_role` always false (eval/has-role); no parent check on
put (eval/raw-put-row); an inclusive upper bound read as exclusive
(StoreHints at round 11); no refill after a removal from a window
(views/top-two-by-pos at batch 4); a kept sum checked step by step
(views/a-maintained-sum-answers-where-the-fold-overflows at batch 7); a kept
sum wrapping at 64 bits (views/a-maintained-sum-overflows-where-the-fold-
answers at batch 0); kept numbers that moved not telling their readers
(KeptNumbers at step 11). One falsification missed and says so: an unkept
plan node's wide fold turned off fails nothing — no views vector evaluates
one whose steps overflow and whose total does not.

Not verified: Linux only — nothing here built the runtime for iOS or ran
it on a phone. Rust, `spec/vectors` and the Kotlin tree are untouched.

