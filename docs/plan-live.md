# Live state: a typed state machine per scope, beside the log

Status: **decided**; phases 1 (the spec and the engine), 1b (Kotlin and
Swift), 2 (harken's session, the desktop and the web) and 3 (the Android
app) are built — see the "Landed" sections at the end — and so are each
device's volume and joined speakers on the desktop, the web and the bridge
("Volume and groups", last). Supersedes the opaque live rooms of
spec §14 (`ark::live`, `ark_server::live`) for anything an app shares in
real time, starting with harken's listening session.

## Why

Harken's listening session works today on the web and the desktop and not at
all on the phone. The reason is where it was written: the rules are a
hand-written `Desk` in `harken/server`, the protocol is hand-written types in
`harken/domain/src/listening.rs` decoded by hand, and each client carries its
own copy of the client half (`harken/iced/src/listening.rs`). A phone would
need a fourth copy, in Kotlin, and a fifth in Swift, each of which could
disagree with the others about what a press means.

That is exactly the problem the log already solved for mutations: write the
meaning once, in the domain, compile it into the module, and let every
runtime evaluate it through the same interpreter, held to the same vectors.
Live state is that, for the state that is about *now*.

## Two kinds of state, and the test for which is which

**Is it worth replaying tomorrow, and must it be there offline?** Then it is
the **log**: permanent, totally ordered, synced, on the device. Otherwise it is
**live**: one current value per scope, held by the server, never replayed.

| State | Where | Why |
|---|---|---|
| plays (track, when, from) | log: `record_play` | last played, recently played and play counts, offline and on every device. One entry per play, not per pause; authored by the device making the sound, once a track has played 30 s or half its length. Tens a day |
| library, playlists | log | as now |
| the listening session: queue, current track, playing or paused, position, which device plays | live | thousands of changes an afternoon, none worth replaying; snapshotted so a restart keeps the queue |
| presence: which devices are here, their kind, whether they can play | live: an ordered `PerDevice` field the app keeps by `on_join` and `on_leave` | authored by nobody; the server runs the hooks as its connections come and go |
| where each device is pointing, what it is scrubbing to | live: an ephemeral `PerDevice` field | each device writes its own slot, many times a second; nobody needs it ordered and nobody needs it tomorrow |
| each output's volume | live | current only |
| recent searches, the diagnostics unlock | the device | nobody else's business |

Home's "last played" is the session's current track when the session is live,
else the newest play in the log: the same on every device, and there offline.

## Live state, as the domain declares it

```rust
enum Session {
    Idle,
    Active {
        queue: NonEmpty<Track>,
        at: Index,            // always inside the queue
        clock: Clock,         // Paused { at_ms } | Playing { at_ms, since: ServerTime }
        output: Output,       // Device { id, moving: bool } | Away { id }
    },
}

r.live::<Session>("listening")
    .scope(|ctx| ctx.user)                 // one instance per account
    .transition("start", |s, ctx, input| …)   // pure: state × ctx × input → state, or a refusal
    .transition("pause", …)
    .transition("transfer", …)             // a guard: the target is here and can play
    .on_join(|s, device| …)                // system transitions: the server runs them
    .on_leave(|s, device| …)
```

- **The type rules out impossible states.** Playing an empty queue,
  a position with no track and an output that is not a device cannot be
  written down. The spec gains record and sum types for this (a struct, an
  enum of records, `Option`, a list, a non-empty list), carried in the
  module's type section and printed as Kotlin and Swift types by the
  generators, as tables are today. A value is checked against its type
  wherever it crosses a boundary: an input, a snapshot, a transition's
  result.
- **A transition is a mutation's kind of thing.** Pure, deterministic,
  guarded by the same guards and roles, written in the same vocabulary and
  run by the same interpreter, so Kotlin, Swift and wasm get the session from
  the module exactly as they get `apply`. The shared suite holds every runtime
  to the same results.
- **The scope is a function of `ctx`** (`ctx.user` here; a household, a
  document or a channel elsewhere), as a scope's holds are. Who may call a
  transition is a guard, as for a mutation.
- **Presence is the hooks'.** A device declares itself at `Hello` (an id
  that survives its reconnects — a login's session for a client, an entity
  for a speaker — a kind, a name, whether it can play). The server runs
  `on_join` when a device first stands in an instance and `on_leave` when
  its last connection goes, each handed the device; who is here is whatever
  the app keeps with them, a `PerDevice<Device>` field in harken's case.
  There is no `ctx.presence` (see "Landed: phase 1").

## Ordered and ephemeral fields (decided)

Presence and live state are one mechanism. A state's record may hold
`PerDevice<T>` fields — a slot per device, keyed by its id — and mark some
of them **ephemeral**. Every change is a transition, and every transition
is classified from its body, statically, as one of two kinds:

- **ephemeral**: it reads nothing of the state — only its input and the
  caller's device — and writes nothing but the caller's own slot of
  ephemeral fields. Relayed by the server to every other subscriber,
  coalesced and rate-limited, never sequenced; applied by its author at
  once and never pending; taken by each receiver only if its counter —
  per device and instance, rising, set by the sender — is above the last
  it took from that device.
- **ordered**: everything else, sequenced per instance as above.

Three rules make the two kinds safe to mix in one state:

1. **No ordered reads of ephemeral fields.** The verifier refuses an
   ordered transition that reads one, by name. It may write its caller's
   own slot — a drop clears the dropper's pointer — but what an ordered
   transition computes never depends on a value that was coalesced away on
   one replica and not another, which is what lets every replica reach the
   server's state whatever pointers it saw.
2. **Ephemeral fields are left out of the state's hash and out of what the
   server keeps.** A snapshot frame carries them, slots and counters both,
   so a device joining sees where everybody is pointing; a restart keeps
   none of them. A device leaving empties its slots in ephemeral fields,
   and only there.
3. **The same guards and type checks.** An ephemeral call is held to its
   input's checks, its transition's guards and its state's type exactly as
   an ordered one is, and refused to its author the same way; a slot's
   value is held to 1 KiB.

**Why one mechanism.** A separate presence tier — its own frames, its own
fan-out, its own idea of who is here — would be a second answer to every
question the state already answers, and the two would disagree exactly
when something had gone wrong. Here the pointer and the queue are two
fields of one value, read by one interpreter, sent in one snapshot, and an
app that wants a cursor writes a transition, not a protocol.

**Delivery.** Log frames and ordered live frames are never dropped or
reordered. An ephemeral frame may be dropped — a newer from the same device
for the same transition replaces it while it waits behind a slow socket —
but it never overtakes anything queued before it, and what waits to be
relayed on an instance goes out before the next ordered frame on it is
told. A connection's ephemeral calls on one instance are relayed at most
about thirty a second; what comes faster is coalesced to the newest, never
refused. A client throttles a finger to one send per ~50 ms, trailing edge,
so the final value always goes.

**Positions are written in content terms** — an item's id and a fraction
along it, a time in milliseconds — never in pixels: two devices drawing one
pointer at two sizes have to agree on what it points at.

**A WebSocket, not datagrams.** These semantics — droppable, never
reordered past what came before, newest by counter — are exactly what a
datagram transport carries well, and a socket with head-of-line blocking
carries them adequately at thirty a second. A transport such as
WebTransport could carry the ephemeral frames later without changing the
API or the frames' meaning; nothing here waits on it.

**A party is a scope of its own.** Every instance today is one account's,
so everybody pointing is somebody signed in as the same person. Other users
in one session — a shared listening party, two people on one document —
need a scope that is not a user, with a guard on who may call (a
membership the log holds). That is the app's choice, and a later one.

## How it runs: the log's model, without the log

The rule ArkDB already lives by — **confirmed, then pending replayed on
top** — holds unchanged:

1. A press applies the transition locally at once, so the screen moves under
   the thumb, and sends the call.
2. The server sequences calls **per instance** (a sequence of its own, not the
   log's), runs the transition with the real context — who, their roles,
   presence, the **server's** time — and sends every subscriber the call and
   the hash of the state it produced. A refusal goes to its author alone.
3. A client applies confirmed calls, drops its own matching pending ones and
   replays the rest. A hash that disagrees, a gap in the sequence or a fresh
   connection gets a snapshot; the state is small.
4. **One socket.** These are typed protocol frames on `/sync` — a call, an
   applied call, a refusal, a snapshot — under the same sign-in and the same
   keepalive. The opaque `say`/`heard` frames and the hand-written listening
   protocol go away.

**Time is the server's.** A live transition is never replayed, so unlike a
log mutation it may be stamped by the server's clock. `Playing { at_ms, since }`
lets every device compute the position for itself (with its offset from the
server's clock, estimated from the stamps it receives); the output no longer
heartbeats its position once a second, and reports only what happens — a
track ended, a seek, a stall.

**A snapshot survives a restart** (decided): kept on the server beside the
logs, named by the hash of the state's type so a changed type is dropped
rather than misread, and restored paused, with no output. Presence never
survives: it is about connections.

## Devices follow the state; nothing routes commands

Today a command is routed to whichever device is the output. Here **the state
is the instruction**. Pause on the phone makes the shared state `Paused`; the
laptop is the output, its player reconciles against the state, and it pauses.
The output reports facts as transitions — `advance` when a track ends,
`started` when a hand-off lands. `transfer(to)` sets `output` to that device,
`moving` until it reports `started`. The whole routing layer goes, and with it
the "who should hear this command" code that each client carried.

The Home Assistant bridge stays server code: a participant that subscribes to
the instances it stands in and reconciles a speaker against them, exactly as a
phone reconciles its player.

## Offline, and coming back

- **A device offline is its own session.** The phone plays on its own; its
  plays are logged as pending intents and reach the server when it reconnects.
- **Coming online when something is already playing makes the device a
  controller of the device playing** (decided). That is the rule for a device
  that was idle and for one that was playing offline: if the session is
  playing elsewhere when the phone reconnects, the phone's own sound stops and
  it shows, and controls, what is playing. A device takes the sound only when
  it is picked (`transfer`) or when somebody presses play on it while nothing
  is playing.
- **If nothing is playing when it reconnects**, a phone that was playing
  offline offers its queue and position as a `start`, and the session becomes
  what it was already doing.

## Plays, in the log (decided)

`record_play(track, played_ms, from)`: a mutation like any other, authored by
the output once a track has played 30 s or half its length, whichever comes
first, once per play. Offline it is pending like any intent. `last_played()`
and `recently_played()` are queries over it; the phone's local
`SharedPreferences` copy goes.

## Harken's session

Transitions, all small: `start(queue, at, position, from)`, `play`, `pause`,
`seek(ms)`, `skip(±1)`, `jump(index)`, `transfer(to)`, `stop`, and the output's
own `started`, `advance(from_track)`, `stalled`. System: `on_join` (nothing
moves; a device arriving takes nothing), `on_leave` (the output leaving keeps
the sound — the output survives its socket — and the clock pauses: `Away`).

The four rules the `Desk` was written around survive as properties of these
transitions, and its tests become tests of the transitions, run on every
runtime.

## Order of work

1. **Spec and engine.** Types (records, sums, non-empty lists) in the module;
   live state, scopes, transitions and system hooks in the IR; the frames; the
   server's per-instance sequencer, presence and snapshot; the client's
   confirmed-plus-pending state and resync. Vectors: transitions under
   `eval/`, frames under `protocol/`, a client's live inputs and outputs in
   the trace format, so Rust, Kotlin and Swift are held to the same bytes.
2. **Harken's session in the domain**, replacing `Desk` and the hand-written
   protocol; the server keeps only the bridge. Desktop and web move onto it.
3. **Android:** the peer subscribes on the same socket; Media3 is an output
   that reconciles against the state; the device picker in the player;
   lock-screen buttons become transitions; `record_play`; Home's last played.
4. Swift follows the same way when iOS is built.

## Landed: phase 1

The spec and the engine (step 1 of the order of work), in `rust/ark`,
`rust/ark-client`, `rust/ark-server` and `spec/`. Harken is untouched: its
`Desk`, its `say`/`heard` session and its clients move in phase 2, and
Kotlin and Swift in phase 1b. The old rooms (`ark::live`, spec §14) stay
beside it until then; §14.1 is the new half.

**Types** (`ark::types`, spec §2.2). The type section is a list of
`{name, ty}`; a record becomes a named type by `Fields::named`, a sum by
`impl SumType`, and everything else is written where it is used. `Ty`
gains `Named`, `NonEmpty`, `Sum` (variants by name, each a record's
fields), `PerDevice` and `Ephemeral`. `conforms` holds a value to a type
wherever it crosses a boundary: an input, a snapshot, a transition's
result.

**The IR** (`ark::ir::State`, spec §3, §7). A module carries `types` and
`states`, each written only when not empty, so a module without live state
is the bytes it was: every existing vector is byte-identical, the trace
corpus is unchanged, and `checks.versions` passes. A state is a name, a
type, an initial value, a scope (an expression of `ctx` giving a key or a
list of keys) and transitions — each an input, guards, and a body over
`live_state`, `ctx` and the input that returns the state or refuses — with
the hooks `on_join`, `on_leave` and `on_restore`. Eight expressions are
new: `variant`, `case`, `live_state`, `ctx_now`, `ctx_device`, `slot`,
`set_slot`, `slots`. A state's closure is the state with the types it names
and the helpers it reaches; its hash names every `applied`, and the hash
of its type alone names what a server keeps.

**Authoring** (`spec/AUTHORING.md` §2.6, Appendix B.2). `r.live::<S>(name)
.scope(..).init(..).transition::<I>(..).guard::<I>(..).on_join(..)
.on_leave(..).on_restore(..)`, run once under `Emit` and on every call under
`Native`, held to agree (`NativeLive::agrees`). The demo gained a `room` on
a router of its own, after the demo's, so no vector of the demo moved.

**Frames** (spec §14.1, `protocol/` vectors). On `/sync`, beside the log's:

| frame | carries | vector (`spec/vectors/protocol/`) |
|---|---|---|
| `hello` + `device` | `{id, kind, name, can_play}`; absent, the bytes it was | `client-hello-device` |
| `call` | `live`, `key`, `transition`, `input`, `id`, and an ephemeral call's `counter` | `client-call`, `client-call-ephemeral` |
| `applied`, ordered | `seq`, `id` (none for a hook), `user`, `session`, `roles`, `device`, `at`, `hash` | `server-applied`, `server-applied-hook` |
| `applied`, ephemeral | `device`, `counter`, and nothing of order | `server-applied-ephemeral` |
| `refused` | `live`, `key`, `id`, `reason` | `server-refused` |
| `live_snapshot` | `live`, `key`, `seq`, `value` (ephemeral slots included), `counters` | `server-live-snapshot` |
| `resync` | `live`, `key` | `client-resync` |

Two of them whole, as the vectors have them:

```
resync   {"key":"alice","live":"room","t":"resync"}
         a3617466726573796e63636b657965616c696365646c69766564726f6f6d
applied  {"counter":41,"device":"phone-login","input":{"at":3},"key":"alice",
          "live":"room","t":"applied","transition":"point"}
         a76174676170706c696564636b657965616c696365646c69766564726f6f6d65696e
         707574a162617403666465766963656b70686f6e652d6c6f67696e67636f756e7465
         7218296a7472616e736974696f6e65706f696e74
```

An `applied` with both a `seq` and a `counter` is refused by the decoder
(`protocol/falsify/server-applied-ephemeral-and-ordered`).

**The server** (`ark::live::state::Desk`, `ark_server::Hub`). Sequences
each instance on its own sequence; runs `on_join` when a device first
stands in an instance and `on_leave` when its last connection goes, as
ordered calls of its own, after emptying the leaver's ephemeral slots;
answers an ordered call pushed again with the `applied` it was; relays
ephemeral calls at most every 34 ms per connection and instance, coalesced
to the newest per transition, flushing what waits before the next ordered
frame on that instance; and the hub's per-socket `Outbox` coalesces an
ephemeral frame behind a slow socket the same way, never reordering. It
keeps every instance's ordered part in `live-states.cbor` beside the log —
canonical CBOR of `{state: {type: <type hash>, instances: {key: {seq,
value}}}}` — written when an ordered call lands, and drops at start any
state whose type hash moved. `HubHandle::participate` stands a device the
server speaks for in instances (the bridge's way in), and
`HubHandle::call` calls as it.

**The client** (`ark::live::state::Watch`, `ark_client::Peer`).
`peer.live(name)` is the confirmed state with this device's pending
ordered calls replayed on top, ephemeral slots and all; `peer.call(name,
transition, input)` previews it at once, natively or interpreted, and
sends it; `peer.call_throttled(..)` holds a finger to one send per 50 ms,
trailing edge; `Pumped::live` says the view moved, which a host woken by
`set_waker` reads like any other pump; `take_live_refusals()`;
`server_offset()` from the `at` stamps. A gap in the sequence or a hash
that is not the server's asks one `resync`; every connection says the
pending calls again. `say` and `heard` are untouched.

**Vectors** (`MANIFEST.json`, side `rust-for-now`). `verify/live-room`,
`module/live`, `eval/live-room` (every transition's kind, the initial value
and its hash, the room through every transition and hook and every kind of
refusal), nine frames under `protocol/`, two traces of a device in a live
fleet under `trace/live/`; a falsify beside all but the module. Kotlin's
and Swift's conformance runners skip anything whose side is not `client`,
by name, so they skip these with no change to either. Their storage tests
are the exception: they replay every file of `trace/` and `trace/falsify/`
without reading the manifest, which is why the live traces have a
directory of their own — found by `checks.kotlin`, which failed on them
when they sat in `trace/`. `RUNNERS.md` says what a runtime that holds
live state does with each vector, including the trace's `call` input and
its `live` and `live_refused` outputs.

**Tested**, each test falsified once by breaking the thing it claims to
hold (the doc comment beside each says how, and what failed): the
classifier and the verifier's refusal of an ordered read of an ephemeral
field; the hash and the kept snapshot ignoring ephemeral fields; a stale
counter, at the server and at a replica; a pending call replaced by the
confirmed one; a gap and a wrong hash each asking a snapshot; an ephemeral
frame never reordering around an ordered one, at the relay and in the
socket's outbox; a leave emptying the leaver's slots; `on_restore`; the
throttle; a seeded `Sim` fleet of three devices partitioned and healed,
and `ark-server`'s fleet of three over real sockets, one partitioned while
the others call, all three agreeing on the heal with the partitioned
device's calls sequenced or refused; a participant, and a restart that
keeps the room. One test passed for the wrong reason when first falsified
— a gap was being caught by the hash rather than the sequence — and was
rewritten so that only the sequence can tell.

### Decisions the plan left open

- **A sum's value is a struct of its variant's fields with the variant's
  name under `t`**, as the IR's own nodes are, so no variant may have a
  field `t`. One encoding for every runtime, and nothing new in the codec.
- **A non-empty list is a list to the checker**, and held to having one
  element wherever a value crosses a boundary. Typing every `list` that
  might be empty as a different type would have made the vocabulary prove
  what only a run can.
- **A `PerDevice<T>` is a struct keyed by device id**; a slot is read as
  `Option<T>`, absent is none, and writing none removes the key. `T` is
  never itself an option, so absent has one meaning.
- **An ephemeral transition may read only its input and `ctx.device`.**
  Reading `ctx.user`, `ctx.now` or a role would make what it wrote depend on
  something the server did not stamp, and nothing is stamped on an
  ephemeral frame.
- **There is no `ctx.presence`.** Who is here is a field the app keeps by
  its hooks, so that it is ordered, hashed and typed like the rest; a
  second, engine-held list would be the second answer the design exists to
  remove.
- **`on_restore` empties nothing for the app.** The engine cannot know
  which field is presence, so a restored room still lists whoever was here
  when the server stopped unless `on_restore` clears it — which the demo
  does. `on_restore` that changes the state moves the sequence by one.
- **A device's id is the one its `hello` declares, or its login's session
  when it declares an empty one.** A hook is an ordered call with no id,
  run under the arriving or leaving device's identity at the server's time.
- **An ordered call remembered for dedupe: the last 512 per instance.** A
  call pushed again past that runs again; a client says its pending calls
  again on every connection, so that window has to cover a reconnect, not
  an afternoon.
- **The rates.** Relay at most every 34 ms per connection and instance;
  a client's throttle 50 ms; a slot 1 KiB of canonical CBOR.
- **A device's counter starts at its wall clock in ms × 1000**, so a
  device that restarts — and has forgotten its last counter — starts above
  where it was.
- **Live calls are not durable on the client.** A pending ordered call is
  held in memory and said again on every connection, but an app killed
  with one pending loses it. The log is where durability lives; a live
  call worth surviving a restart is a mutation.
- **The live traces are in `trace/live/`**, not `trace/`, for the storage
  tests above; phase 1b can move them back once both runtimes replay them.
- **The vectors are marked `rust-for-now`**, a side of their own rather
  than `rust-only`: `rust-only` means a fleet no client can reproduce,
  while these are a client's in time. The Kotlin and Swift runners' skip
  message still says "Rust's simulator" for both; phase 1b changes it.
- **`ark::live` became `ark::live::{mod, state}`**, the old rooms in
  `mod.rs` and the new state in `state.rs`, rather than a second top-level
  module, so the two halves of §14 sit together until the first goes.

### Open, for phase 2 and 1b

- Harken's `Session` as a state: the four `Desk` rules as transitions, the
  bridge as a participant, `transfer`'s `moving`, and `Away` from
  `on_leave`. The bridge's own polling stays server code.
- Whether harken's `devices` field should be cleared by `on_restore` (the
  demo's answer) or kept and marked away; it is the app's choice now.
- Generators: printing the type section and live states as Kotlin and Swift
  types, and the runners' `rust-for-now` handling (and their skip message).
- A transport for ephemeral frames that is not the socket, if thirty a
  second over a WebSocket ever proves too little.
- A non-user scope with a membership guard, when a party is wanted.

## Landed: phase 1b

Kotlin and Swift hold live state, and every vector phase 1 marked
`rust-for-now` — eighteen of them, in `verify/`, `module/`, `eval/`,
`protocol/` and `trace/live/` — is `client` now. Both conformance runners
run 140 of the 148 vectors the manifest names, skipping the eight
`rust-only` fleets by name; both replay the corpus's 2,565 traces. The side
`rust-for-now` is gone from the generator, the manifest's test and
`RUNNERS.md`.

**The runtimes** (`kotlin/ark-runtime`, `swift/Sources/ArkDB`) are ports
of phase 1's engine, file for file: the type section and `conforms` with
the reference's sentences (`Types`), the IR's states, transitions, guards,
hooks and the new expressions through decode, encode, normalize, verify —
`OrderedReadsEphemeral` included — and the three hashes (`Hash`), the
classifier (`Ordering`), the interpreter's live half (`Eval`), and the
frames, an `applied` with both a seq and a counter refused (`Live`,
`Protocol`). The trace player runs `call` inputs and reports `live` and
`live_refused`.

**The clients** (`ark-client`'s `Peer.kt`, `ArkDBClient`'s `Peer.swift`)
hold each instance as the Rust peer does, and say so in the house style:
`live(name)`, `call(name, transition, input)` — which throws a preview's
refusal, where Rust returns it — a throttled call, `serverOffset` and
`serverNow`, `takeLiveRefusals`, and `Pumped.live` for a view that moved.
Neither has a timer: a held call goes out on the pump after its window.

**`say` and `heard` are gone from both peers.** Their only callers were
the headless peers' drains (`kotlin/harken-peer`, `swift/Sources/HarkenPeer`),
which threw what they read away; `harken/android` reads
`Status.heardFrames`, which stays and now counts live frames, and
`harken/ios` reads neither. The codec still reads and writes the opaque
`say` and `heard` frames — their vectors are the codec's — and a client
ignores a `heard`.

**`arkc gen kotlin|swift`** prints the type section and each state's type:
a record as a data class or a struct, a sum as a sealed class or an enum,
read from the runtime's `Value` and written back as one, with `LiveStates`
naming every state. The plan said "as tables are printed now", and nothing
printed tables: `print` and `gen` went with v4 (`AUTHORING.md` §4), so this
is a printer of types alone, beside `arkc`'s other commands. The demo's
room, printed, is `spec/vectors/gen/` — written by `ark-vectors`, held
byte for byte by `checks.vectors`, not in the manifest because it is not a
vector — and each runtime compiles it and reads every state of
`eval/live-room.json` through it and back (`Printed`).

### Decisions

- **The live traces stay in `trace/live/`.** Both storage tests read the
  manifest now rather than every file of `trace/`, which was the only
  reason for the directory; moving them would rename three vectors every
  runner's report names, for nothing.
- **A trace holds live state only where its `open` declares a device.**
  That is the rule the Rust player has in phase 2's working tree, where
  harken's module gains a state and its traces, recorded before, declare
  none; Kotlin and Swift follow it, so the harken traces replay with or
  without phase 2.
- **Swift's peer has no waker**, so its `deadline()` is the throttle's
  window alone and a host pumps on its own tick, as it already does for
  the link. Kotlin's is the sooner of the link's and the throttle's, as
  Rust's is.
- **The printed names are bare.** `Mode`, `Room` and `LiveStates` are
  printed as the module names them, so in Swift a printed type shadows one
  of `ArkDB`'s of the same name in the module that holds it — the demo's
  `Mode` shadows the protocol's in the test target, where nothing names
  the protocol's. A prefix is one argument away when an app needs it.

**Tested**, each test falsified once, and the doc comment beside each says
how: `LiveTests` in both clients — the hello's device, an ordered call
said again on the next connection, an ephemeral call never pending, stale
and equal counters, a gap asking a resync and applying nothing, a refusal
taking a call out of pending, the throttle, the server's clock, and both
orders on one `applied` — and `Printed` in both, falsified by printing
`can_play` as `canPlay`.

## Landed: phase 2

Harken's session in the domain (step 2 of the order of work), in
`harken/domain`, `harken/server` and `harken/iced`. The hand-written
protocol (`harken_domain::listening`'s frames), the server's `Desk` and its
`Watch`, and the clients' copy of the client half are gone; the session is
a live state, the server holds no rule of it, and the desktop and the web
reconcile against it. Plays are in the log.

**The state** (`harken_domain::listening`, a router of its own after the
playlists, so its helpers come after every function):

```
listening {
    devices: PerDevice<Device>,          // ordered, kept by on_join and on_leave
    scrub:   PerDevice<Int>   ephemeral, // where each device's scrubber is dragged to
    session: idle {}
           | active { at, clock, from, output, queue: NonEmpty<track> },
}
clock  = paused { at_ms } | playing { at_ms, since }   // since: the server's clock
output = device { id, moving } | away { id, name } | nowhere {}
track  = { album, creator, duration_ms, file, id: Id<media>, title }
```

**The transitions**, each with the guards that refuse it:

| transition | does | refused |
|---|---|---|
| `start(queue, at, position, from)` | this queue, from here, playing; while something is sounding the output keeps it, else the caller takes it if it can play, else a paused output that is here keeps it | `at` past the queue; nothing sounding, nothing reachable, and the caller cannot play |
| `play` | resume, on the output where it is here, else on the caller | nothing queued; nothing reachable and the caller cannot play |
| `pause` | the clock stops where it is; nobody takes anything | nothing queued |
| `seek(ms)` | the clock at `ms`, playing on if it was | nothing queued |
| `skip(±1)` | the next or previous track from its start, playing; before the first is the first; a press that means a sound | `by` not ±1; nothing after the last; as `play` |
| `jump(index)` | that track from its start, playing; a press that means a sound | the index past the queue; as `play` |
| `transfer(to)` | the output becomes `to`, `moving` until it says `started`; the device that already has it, settled, is nothing | nothing queued; `to` not here or cannot play |
| `stop` | `idle` | — |
| `started(at_ms)` | the output has the hand-off: `moving` ends and the clock re-anchors at `at_ms` | not the output |
| `advance(from_track)` | the output's track ended: the next, or paused at the start of the last; about a track no longer playing, nothing | not the output |
| `stalled(at_ms)` | the output stopped by itself: paused there | not the output |
| `release` | the output lets the sound go: paused, `nowhere`, the queue kept | not the output |
| `scrub(at_ms?)` | ephemeral: this device's slot, or empty | — |
| `on_join(device)` | the device is in `devices`; an `away` output whose id it is gets the sound back, paused | — |
| `on_leave(device)` | out of `devices`; the output leaving is `away` (named) and the clock pauses | — |
| `on_restore` | `devices` empty; paused where it had got to (never past the end of the track), `nowhere` | — |

**The `Desk`'s four rules, as properties of these**:

- *Exactly one output*: `active.output` is one value.
- *The output survives its socket*: `on_leave` makes it `away` and pauses;
  `on_join` gives it back, still paused.
- *A command goes to the output*: a command is a state change — `pause`,
  `seek` and `transfer` move nothing but the session — and the output's
  reconciler obeys it. Nothing routes anything.
- *A press that means a sound takes an output that cannot be reached;
  arriving takes nothing*: `play`, `skip` and `jump` use `pressed`, which
  keeps an output that is here and otherwise hands it to the caller;
  `on_join` takes nothing.

Every case `harken/server/tests/listening.rs` held the `Desk` to is a test
of these transitions in `harken/domain/tests/listening.rs`, run by the
engine's own sans-io server (`ark::live::state::Desk`) twice over — once
running the domain's Rust, once interpreting its emit — and the two held
to the same frames and the same state after every step.

**The decided reconnect rule** is the client's (`harken/iced/src/
listening.rs`). A device unlinked, or linked before the server's snapshot
on that connection, is its own session and calls nothing. Coming back: if
the session is sounding elsewhere its own sound stops; if not, and it was
playing on its own, it says `start` with its queue, track and position —
and `start` hands the caller the sound exactly when nothing is sounding,
which is the same rule for a track pressed.

**The bridge as a participant** (`harken/server/src/assistant.rs`). The
thread reads every `listening` instance from the hub (`Hub::live`), stands
each configured speaker as a device (`HubHandle::participate`, kind
`speaker`, `can_play`) in each one a client is in and in no other — so a
speaker never keeps a session's presence open — and detaches it when the
last client goes, pausing what it held there. A participant's frames only
wake the thread. A pure `Bridge` is handed each session and what the house
said: a session naming the speaker its output is a hand-off of the whole
queue from where it was (`clear_playlist`, `play_media` `play` then
`add`s, `media_seek` after the enqueue, `media_pause` if paused); a
change of the session after that is `media_pause`, `media_play` or
`media_seek`, and another queue or place a fresh hand-off; what the
speaker did is said as it — `started` once it plays what it was handed,
`advance` or `jump` when it moves itself, `stalled` and `play` when it is
paused or resumed by hand, `seek` when it is three seconds from where the
session counts it. `SETTLE` (five polls) before a hand-off that does not
start is let go, `LAPSE` (three) for one that was playing, and `QUIET`
(two) after the bridge told the speaker anything, during which a
difference is the house catching up; letting go is `release`, said as the
speaker, and printed with what it reported. A session that takes a
speaker another holds releases the other, once.

**`record_play(track, title, creator, album, duration_ms, file, from)`**,
the `played_ms` an auto: the device making the sound authors it once a
track it loaded has played thirty seconds or half its length, whichever is
first, once per load. A play row (`play`) carries the track as it was
heard — the media id as text, no reference — so `remove_media` is not
refused a played track and a play of a track this replica lacks is not
refused either. `last_played()` and `recently_played()` read it newest
first, the caller's alone. The log surface moved additively (`arkc check`:
"every retained entry still applies"); `record_play` is pinned at its
first hash in `agreement.rs`.

**The desktop and the web.** `Remote::reconcile` after every update: the
output loads the session's track at its clock's position (copying the
queue as its own), resumes and pauses with the clock, seeks when it is two
seconds off, says `started` once a hand-off is loaded, `advance` once per
end, `stalled` after two and a half seconds asked to play and silent, and
`record_play`; every other device is silent. Every button, key, row and
lock-screen button is a transition (`App::press`), or, unlinked, a
command to the local player. The bar draws the session's track and its
clock against `server_now`; the slider says `scrub` while dragged
(throttled, ephemeral) and `seek` on release. `D` shows the link, the
instance's sequence, pending and synced, the server offset, the calls,
refusals and plays, the session, the devices and every scrub slot. The
picker keeps 23c40a0: no stop row, this device the default while nothing
plays; an away output is drawn and not a target.

**Tested**, each test falsified once, and the doc comment beside each says
how. `harken/domain/tests/listening.rs`, every case the `Desk` was held
to and the new ones, on two desks at once: the sound staying with a device
whose socket went and coming back to it paused, arriving taking nothing,
a command as a state change, only a press that means a sound taking it
from a device that has gone, a device that cannot be heard never taking
it, a track pressed playing where the sound is or here, a speaker picked
up where the track was, no hand-off to a device gone or mute, a second
connection keeping a device here, the output's own transitions refused to
anyone else, a restart paused with no output and nobody here, `stop`, two
accounts, a call nobody can read, and the scrub, ephemeral and never
kept. `harken-iced`'s `listening` tests drive the reconciler against a
stand-in player through real peers and the engine's sans-io server: the
output obeys and nothing else sounds, a track's end said once, a play
recorded once by the output alone, and both branches of coming back.
`harken/server/tests/bridge.rs`, over real sockets with a stand-in house:
a speaker the whole way round, a speaker taken or sent elsewhere let go
out loud, paging that leaves the session alone, and a browser, a desktop
and a speaker in one session with the decided rule both ways;
`assistant.rs`'s own tests hold the `Bridge`'s rules as values.
`harken/server/tests/plays.rs`: plays made offline reach every device
when the device comes back, newest first and the caller's alone.
`agreement.rs` holds `record_play`, `last_played` and `recently_played`
native against interpreted.

### Decisions phase 2 had to make

- **A device that was the output, coming back, is the output again** —
  paused, until somebody presses play. Arriving takes nothing; this is the
  `Desk`'s "the output survives its socket", which a device's departure
  and return as two hooks has to say.

- **`output` has a third variant, `nowhere`.** The plan's `Device | Away`
  cannot say "paused, no output", which `on_restore` must, and which a
  released speaker leaves.
- **`away` carries the device's name.** It is the one row a picker most
  needs to draw, and the device is no longer in `devices` to be named from.
- **`release` is a transition of its own**, guarded to the output. `stop`
  from a speaker would stop a session that had already moved its sound
  elsewhere.
- **`stop` is `idle`**: nothing queued. A pause is `pause`.
- **`start` hands the caller the sound when nothing is sounding**, where
  `play`, `skip` and `jump` hand it only when the output cannot be reached.
  A track pressed on the phone while the laptop is paused plays on the
  phone; play pressed there plays the laptop. That is the decided rule's
  "play pressed on it while nothing is playing", and it makes the
  reconnect rule's second branch an ordinary `start`.
- **`from` is a route** (`album/Water Music`), carried on the session and
  on each play, for a screen to say where a track was played from.
- **A play's time is an auto**, not an argument: drawn once, at the
  device, frozen in the entry, like every other clock a mutation reads.
- **A play is self-contained** (above), and once per *load* of a track.
- **`scrub` has no UI beyond the desktop's own slider and `D`.**
- **The bridge reads instances from the hub** rather than keeping a client
  copy of each from its frames: it is in the hub's process, and a read is
  the state the server holds.
- **A repeated `hello` stays paging and nothing else.** Answering one with
  a snapshot was considered (a client that took it for a new connection
  would otherwise wait for one), and left: no client does that, and every
  live trace would have moved.

### Found on the way, in the engine

- **A native record of some of another's fields** (`Away { id, name }` off
  a device) came out the whole other record: `authoring::cx::whole` handed
  back a struct when every field was read off it, without asking whether
  it had others. Natively `on_leave` refused ("no field can_play") where
  the interpreter took it — found by the two desks disagreeing. Fixed, and
  `rust/ark/tests/live.rs` holds it.
- **The trace player held live state for a client that declared no
  device**, which `RUNNERS.md` already said it must not; harken's log-only
  traces would have reported an empty `live` once harken had a state.
- **`ark_client::Peer::live_status`**: the instance's sequence, pending and
  whether this connection's snapshot has come — what a reconciler needs to
  know whether the view is the server's yet, and what `D` draws.
- **`ark_server::Hub::live`**: the instances, for server code that stands
  in them.
- **A snapshot written under a schema since grown by a table did not
  open.** The state hash names every table of the schema, so a peer
  alone's `log` written before `play` existed was checked over a table it
  never had and refused — "the snapshot's hash does not match its rows" —
  which `checks.versions` scenario 5b found against `v4-rows`, the last
  time it ran against a pinned revision. The server's own snapshot was
  never at risk (`persist::rehome` hashes it again under a new module);
  an alone directory has no such step. `ark::journal` now checks a
  snapshot over the tables it names, where those are some of the
  schema's, and holds it hashed under this one; `canon::decode_rows`
  hands back every table a snapshot names, an empty one as an empty list,
  so that the names are there to check by. `journal::tests::
  a_snapshot_written_before_a_table_was_added_opens` holds it.

### What went with the protocol

The opaque rooms nothing used any more (`ark::live`'s `Machine`, `Rooms`,
`arrive`, `speak` and `depart`; `ark-server`'s `Live`, `Echo`, `Quiet`,
`Builder::live`, `HubHandle::stand` and `say`, and `live.cbor`;
`ark_client::Peer`'s `say`, `say_value`, `heard` and `heard_values`, and
`Status::heard_frames`) are gone, and `ark::protocol::Server` is no longer
generic over a machine. The frames are not: `say` and `heard` are still
read, and dropped, so that a peer of an older revision is not cut off by a
server of this one (a frame that does not decode closes the socket) and a
peer of this one is not cut off by an older server; nothing sends either.
`/healthz`'s rooms are each account's open replica connections, which is
what they counted. Kotlin and Swift dropped their halves of the API in
phase 1b and decode the two frames as this does, so the vectors that pin
the decoding, `protocol/client-say` and `protocol/server-heard`, stand
(spec §14).

### The versions matrix, emptied

`v4-journal` and `v4-rows` were development revisions, not releases: the
flake inputs and their lock entries are gone and `nix/versions.nix` is an
empty list, its mechanism and comment kept, until a shipped revision is
pinned. `checks.versions` then runs scenarios 6 and 7, which need none,
and 1–5 say they are skipped and why; `packages.fleet-vm` has no
`oldserver` and says its mixed subtests are skipped (`docs/plan-db.md`
D1).

### Not verified

No browser has played a track through the session and no speaker has
been handed one: the reconcilers ran against a stand-in player and the
bridge against a stand-in house, between real sockets and real peers. A
speaker's plays are not recorded — the bridge is not a peer of the
account — and the Android app is phase 3's.

## Landed: phase 3

The Android app (step 3 of the order of work), in `harken/android`,
`kotlin/ark-client`, `kotlin/ark-android`, `kotlin/harken-peer` and the
headless peer and the fleet in `harken/server`. The phone stands in the
account's session on the same `/sync` socket as its log, can be its output
through Media3, and controls it from everywhere: its buttons, its lock
screen, a headset. Plays are the log's; Home's last played is the
session's, or the log's.

**Typed access.** `harken/android/app/src/main/kotlin/dev/harken/android/
listening/Listening.kt` is `arkc gen kotlin harken.ark
dev.harken.android.listening`, checked in, and `checks.harken-domain` holds
it to that output byte for byte beside the `harken.ark` check it already
had — so a domain change that moves a field of the session moves
`harken.ark`, which fails the check until the file is printed again,
rather than the phone reading a field that is no longer there. A package of
its own, because the printed `Track` and `Session` would otherwise meet the
app's library `Track`. Falsified by a one-byte edit of the printed file
(`can_play` spelt `canPlay`): "not what arkc gen kotlin prints of
harken.ark: print it again".

**Joining.** `AppIdentity.device` (ark-android) is what an app declares at
`hello`; `ArkHost` hands it to `ArkSession.open(device = …)`, which opens
the peer `Options.withDevice`. harken's is a phone, that can play, named as
its owner named it in the system's settings — else its model, else
"Phone" — with an empty id, so the server reads the login's session and a
device is a login. No second socket and no timer: live changes reach the
app as the log's do, through `ArkSession.onChange`. The Kotlin client gains
`Peer.liveStatus(name)`, the Rust peer's `live_status` — the instance's
sequence, its pending calls, and whether this connection's snapshot has
come — which is what "part of the session" is read from.

**The reconciler** (`Remote.kt`, Android-free) is the desktop's
(`harken/iced/src/listening.rs`) one for one, with three differences that
are the phone's:

| the desktop's `Remote` | the phone's `Remote` |
|---|---|
| `joined`: linked, as itself, the snapshot met | the same, through `liveStatus` |
| `reconcile` after every update | after every change the session reports and every event the player raises (`Listen`) |
| the output loads one track; its end is `advance` | the output loads the **whole queue** into ExoPlayer, which plays on through it by itself (gapless, and still playing the queue if the socket goes); the player moving on is the end, said once as `advance` and waited for, never fought |
| `started` once a hand-off is loaded | the same |
| `stalled` after 2.5 s asked to play and silent | when the player says it stopped by itself — `playWhenReady` cleared for any reason but a press (focus taken by a call, the headphones out), or an error — event-driven, so no timer |
| seek when 2 s off | 1.5 s off, and not while a buffer fills (a seek then restarts the fill, and a slow stream would chase itself) |
| `record_play` by the output | by the output, and — the decided rule — **offline too**: a phone unlinked is its own session and its plays are pending intents; a wake (`Sound.wakeIn`, a one-shot on the main thread) at the moment a play will count, since nothing else would reconcile then |
| coming back: sounding elsewhere, stop; else `start` with its own queue, track and position | the same, its own queue being the player's |

A press (`Press`) is a transition while the phone is part of the session,
read the desktop's way — the one button pauses what sounds and otherwise
plays; with nothing queued it plays what the phone last had — and refused
with `NOT_PART` otherwise, which the caller answers by doing it to the
player, its own session.

**The ExoPlayer and the lock screen.** `PlaybackService` holds the
ExoPlayer and a `RemotePlayer`, a Media3 `SimpleBasePlayer`
(`androidx.media3:media3-common:1.5.1`, already recorded in `deps.json` —
nothing was re-recorded), and swaps the media session's player between
them (`MediaSession.setPlayer`): while the phone is part of the session the
lock screen, the notification and a headset reach the `RemotePlayer`,
which is the session drawn — its queue the playlist, its track, its clock
counted on from the server's (`PositionSupplier.getExtrapolating`) — and
whose every command is a press: play, pause, a seek, next and previous
(as `skip`, so `next` still means next if the queue moved), a track of the
queue (`jump`), a queue set from a car or a watch (`start`), an item added
(play next), and a headset's stop, which is a pause, since `stop` would
forget the queue. It answers each once the press has been previewed and
the session published again, so the masked state never flickers back.
With nothing queued it is empty, and there is no notification. The
ExoPlayer is then the reconciler's alone (`ExoSound`, a value it keeps of
what the player last said, read on the session's thread, its commands
posted to the main thread and written into that value at once); unlinked
it is the media session's player again, this phone's own, as it always
was. **When the phone is the output** the lock screen is the same
`RemotePlayer`, so it is the session, which the ExoPlayer follows.

**The screens.** `Playback` draws the session while the phone is part of
it — the track, playing or not, up next from its queue, and where the
sound is (`PlayerState.on`) — and presses it for every button; alone it is
the controller it was. The position is still its own flow, ticked twice a
second only while something plays, here or anywhere, and read by the
hairline and the scrubber alone: `ScrollTests` holds at rest `{}`, over
four ticks `{Progress=4}`. Another device's scrubber is a third flow,
`scrubbing`, read by the scrubber alone; dragging this one says `scrub`
(throttled, ephemeral) and the release says `seek` and empties the slot.
The session has no shuffle and no repeat: shuffle is a queue in another
order, this track first, and neither toggle is drawn while joined.

**The picker** (`Picker`, `DevicesSheet`), the desktop's (23c40a0): a row
per device, by name, with the device's kind as its glyph — phone, computer,
speaker, and a browser, which the web build declares as a computer named
"Browser"; a device that cannot play drawn and not selectable ("can't make
a sound"); the output that has gone drawn, ticked, "not answering";
"connecting…" while the sound is on its way. No stop row. While nothing
plays this phone is the default, ticked, and the cursor — the lit row —
starts on the output or on this phone. Picking a device is `transfer`. It
opens from a chip beside the queue's in the full player (the kind and the
name, lit while the sound is here); the mini player says "On Browser"
where the artist was while the sound is elsewhere.

**Home's last played** (`lastPlayed`): the session's current track while
the phone is part of a session that has one — "Playing on Browser" while
it sounds — else the newest play in the log, `last_played()`, a
maintained view in `Library`. `Device.lastPlayed` and its preference are
gone (an older build's key is removed on first open); recent searches and
the diagnostics unlock stay local. A track's `from` is written in the
desktop's spelling (`#album/Water%20Music`, `Page.place`) so a page named on
one device is the same page on another. No "Recently played" shelf was
added: Home already opens on last played, and a second shelf of the same
records belongs to a later pass of Home's design.

**Diagnostics** gain a "Listening" group: the device, joined, linked, the
epoch, the instance's sequence, pending and synced, the server offset, the
calls, refusals and plays, the last refusal, the session, the output,
every device, and every scrub slot, this phone's marked.

**The headless peers and the fleet.** `harken-peer` (the reference) and
`harken-peer-kt` take `--device KIND,NAME,CAN_PLAY` and answer three new
lines, `call`, `live` and `live_refusals` (the reference's first page has
them). **K7** (`harken/server/tests/fleet_kotlin.rs`,
`tests/support/mixed.rs`): a Rust server, a Rust peer and a Kotlin peer on
one account. The Rust laptop starts a queue; it hands the sound to the
Kotlin phone, which is connecting until it says `started` (the laptop
saying it is refused, "not the output"); the laptop pauses it; the phone's
socket goes into a black hole and the laptop sees it away, keeping the
sound, paused; the phone presses play into the black hole, pending; the
network comes back, the phone stands again with the sound, and the play
lands — read off both devices at every step. The Swift peer speaks no live
state yet, so `fleet_swift.rs` does not run it.

**Tested**, each test falsified once (the comment beside each says how,
and what failed):

- `RemoteTests` (8), real Kotlin peers against a server in the test
  (`Hall`: the module's transitions and hooks, sequencing, relays and
  refusals, interpreted): the output obeys and nobody else sounds
  (falsified by leaving a device that is not the output alone); a track's
  end said once and the next one played without a reload (falsified by
  taking the player's own move for somebody else's, and by saying an end
  with neither the memory nor the clock — either alone holds it, and each
  was taken away alone first and passed); a play recorded once, by the
  output alone, offline too (falsified by counting every reconcile, and by
  counting from the session's clock on every device — counting the
  watcher's own player first passed, since it had nothing loaded); both
  branches of coming back (always and never `start`); unlinked is its own
  session (`joined` without `linked`); a stall said and not fought; a
  press as a transition (a toggle that always plays).
- `ListenTests` (4): the picker's default, tick, cursor, away row and
  transfer (no default: nothing ticked); Home's last played from the
  session, else the log (the log preferred); a scrub on another device
  shown here, relayed and not sequenced, the final value sent by the pump
  after the throttle's window (every slot read, this phone's own
  included); the lock screen's player forwarding every command as a
  transition, on layoutlib's Looper (a seek within the track taken for a
  jump; a stop answered with nothing).
- `LiveTests` in ark-client: `liveStatus` (falsified by reading `asked` for
  `synced`).
- The fleet's K7 (falsified by a `harken-peer-kt` that dropped
  `--device`, and by a Kotlin client that did not say its pending live
  calls again on a new connection).
- `checks.harken-domain`, the printed types (above).

**Screenshots** (`Screens.g1_listening`, light and dark): the full player
with the sound on another device, here, and connecting to a speaker; the
mini player with the sound elsewhere; a scrub from another device; the
picker playing, with nothing playing, and with the output not answering;
Home's last played from the session; and the diagnostics' listening
numbers. Every other screenshot is byte for byte what the parent commit
draws, but `f2_diagnostics_numbers`, which gains the "Listening" group.

### Decisions phase 3 had to make

- **The lock screen is the session even when the sound is here.** One
  player for the media session while joined, whichever device plays, so a
  press there is always a transition and the ExoPlayer is only ever the
  reconciler's; the cost is that the notification counts the position from
  the session's clock rather than reading the stream, and does not show a
  buffer filling.
- **The phone's output holds the whole queue** (above), and the end of a
  track is the player's own move, said as `advance` and waited for.
- **Previous is `skip(-1)`**, as on the desktop, not Media3's "back to the
  start of this track past three seconds".
- **Play next is a `start`** of the queue with the track after this one,
  where it was — and a `pause` after it if it was paused, so nothing
  sounds. There is no insert; if nothing was sounding, `start` hands the
  sound to the phone, which is the rule for any track pressed.
- **A headset's stop is a pause.**
- **The listening session lives as long as the app's session does**: it is
  made with the `ArkSession` in the activity's `ViewModel`. With the
  activity gone the socket goes with it; the server sees the phone leave,
  keeps the sound with it, away and paused, and the ExoPlayer — the media
  session's player again — plays on through the queue it holds, this
  phone's own; opening the app again is coming back, and the decided rule
  applies. *Since changed*: the session is the process's now, held by the
  activity and by the media service (`docs/plan-android.md`, "The session
  outlives the activity"), so the activity going while the phone plays
  leaves it in the session, the output, playing.

### What ran green

`checks.harken-domain`, `checks.kotlin`, `harken-apk` (the release
`harken.apk`, certificate SHA-256 `1a8859fb…df431139`, the committed key),
`harken-android-screens` (every app test with it: RemoteTests, ListenTests,
ScrollTests, RestorationTests, LifecycleTests, SourceTests, MotionTests,
LaunchTests, FlingTests), `checks.fleet-kotlin` (K1–K7, K4 and K5 skipped
as before, with nothing pinned), `checks.rust`, and `harken-fdroid-repo`.
The fling bench, run beside the parent commit: 32,874–32,875 B per entering
row against 33,147–33,151, the draw 27,977 B a frame in both.

### Not verified

No real phone has stood in a real session beside a real browser or a real
speaker: the reconciler ran against a stand-in player and a server in the
test, the lock screen's player on layoutlib's Looper with no system UI to
draw it, and the fleet with headless peers. Not seen: what Android's media
notification draws for the `RemotePlayer` (and whether a remote's
"playing" keeps the service in the foreground as Media3 says it does), the
swap between the two players on a running media session, ExoPlayer's own
moves through a queue against the reconciler on a device, audio focus and
"becoming noisy" read as a stall, and the wake that counts a play with
the screen off.

## Volume and groups

Each device's volume, and speakers joined into one group, as the session
holds them (`harken/domain`, `harken/server`, `harken/iced`, and the phone,
`harken/android`, below). Like every other part of the session, it is
state: anybody changes it, the device obeys it, and what was done on the
device itself is said back as the device's own transition.

**The state.** `devices` keeps a harken record per device now — what the
engine's hook hands `on_join` (`can_play`, `id`, `kind`, `name`, `user`)
and five fields of the app's own:

```
devices: PerDevice<{ can_play, can_volume, family: Option<Text>,
                     follows: Option<Text>, id, kind, muted, name, user,
                     volume: Int }>
```

The record is written in place rather than named, so the Kotlin printer
keeps calling it `ListeningDevices`, whose constructor gained the five.

| transition | does | refused |
|---|---|---|
| `equip(can_volume, family?, volume, muted)` | the caller's entry: what it can do and where its volume is; a family that moves takes it out of any group | the caller is not here |
| `set_volume(device, level)` | that device at `level` (0–100), unmuted | not here, or no volume |
| `nudge_volume(device, delta)` | moved by `delta` (±100, not 0), clamped, unmuted — what a volume button sends | as `set_volume` |
| `set_mute(device, muted)` | | as `set_volume` |
| `report_volume(level, muted)` | the caller's own entry, ordered, since everybody reads it | the caller has no volume |
| `join(device)` | the speaker follows the output; whoever followed it follows nobody | no output that is here; not joinable |
| `unjoin(device)` | it follows nobody, and goes quiet | it is in no group |
| `report_group(members)` | the caller's word on who follows it: each member that is here and of its family follows it, anybody it no longer names follows nobody, and so does anybody who followed a device that is a member now; names it cannot take are passed over | the caller has no family |
| `set_group_volume(level)` | every member of the output's group with a volume, in proportion, unmuted | nothing in the group has a volume |
| `nudge_group_volume(delta)` | the same, to the group's level moved by `delta`, clamped | as `set_group_volume` |
| `set_group_mute(muted)` | every member with a volume | as `set_group_volume` |

`transfer(to)` gained one thing: a `to` that follows a leader leads that
group now — the old leader and every other member follow it, as Sonos makes
a member the coordinator — and a `to` that leads keeps its group.
`on_join` writes the new fields as nothing said: no volume anybody can move,
drawn at 100, no family, following nobody. `on_leave` takes the leaver's
group with it: whoever followed it follows nobody.

### Decisions this had to make

- **A group is who follows whom, not a list on the output.** Each device
  holds at most the one leader it follows, so a device in two groups cannot
  be written down; a leader follows nobody, a member leads nothing, and a
  member shares its leader's family — those three are held by the
  transitions (`joinable`, `followed`, `regrouped`, `led_by`, `dissolved`)
  and asserted over every device after each step of the domain's tests. It
  is also what lets `transfer` to a leader keep a group the output never
  had: speakers grouped in the Sonos app are a group in the session before
  harken plays anything on them, and stay one after the sound moves away.
- **The output is a group when anything follows it.** The group's volume is
  the mean of its members' that have one, rounded down, as Sonos reads a
  group's level; moving it scales each member by the new level over the old,
  rounded to the nearest and clamped at 0 and 100 — integers only, so every
  replica reaches the same numbers — and from a group at 0, where there is
  no proportion, every member takes the level. A group of one is its
  leader, so the bar always calls the group transitions while something is
  the output.
- **A leader is not unjoined.** The sound handed to a member first makes
  that member the leader, which is how the Sonos app removes a coordinator.
- **The family is named `family`**, not `group`, because a group is what a
  set of joined speakers is.
- **Moving a volume unmutes**, as a volume button does on every phone; a
  report sets both, since it says what the device is.
- **Nothing has a volume until it says so**, once a connection, with
  `equip`. Defaulting a device that can play to having one would put a
  slider on the phone that moved nothing until the phone learns to follow
  it, and would set a browser that just joined to 100 before it said where
  it was.
- **No ephemeral "being dragged" slot.** A slider drag is throttled ordered
  `set_volume` calls — the speaker has to change while the finger moves,
  which an ephemeral slot set only on release would not do — and those
  already reach every device twenty times a second. `call_throttled` sends
  an ordered call at once, its order being its point, so the throttle is
  the client's: 50 ms (`ark_client::THROTTLE_MS`), trailing edge, the last
  value sent on release or by the clock.
- **Only speakers group.** A phone's, a browser's and a desktop's audio
  cannot be kept in step with another device's, so they declare no family
  and `joinable` refuses them; a speaker's family is the integration Home
  Assistant reports.

### The bridge

`Bridge::tune` reconciles each standing speaker's volume and group against
each session it stands in, by the playback's rule: what moved in the
session since the bridge last looked is told to the house, and what moved
in the house while the session stood still — outside `QUIET` — was a hand,
said as the speaker.

| the session | the house |
|---|---|
| `equip`, once a session has it standing | `supported_features` & `VOLUME_SET` (4) and & `GROUPING` (524288), `MediaPlayerEntityFeature` in `homeassistant/components/media_player/const.py`; `volume_level` (0–1, read as 0–100), `is_volume_muted` |
| `set_volume`, `nudge_volume`, the group's | `media_player.volume_set {volume_level: level/100}` per member |
| `set_mute`, `set_group_mute` | `media_player.volume_mute {is_volume_muted}` |
| `join` | `media_player.join {entity_id: leader, group_members: [..]}` |
| `unjoin` | `media_player.unjoin {entity_id: member}` |
| `report_volume`, when the house differs and the session did not move | `volume_level`, `is_volume_muted` |
| `report_group`, likewise, from the leader | `group_members`, the leader first |

- **The family is the integration**, read once per grouping player through
  the template API — `config_entry_attr(config_entry_id(entity), 'domain')`,
  which is `sonos` — because an entity's state does not name its platform.
  A house that cannot answer (a Home Assistant older than that template)
  puts every grouping player in one family, `home-assistant`: right for a
  house of one make, and harmless in one of two, since the house refuses a
  join across makes and the group it then reports is what the session takes.
- **Groups are reconciled from the leader's side**: who follows the speaker
  in the session against who plays with it in the house. An unjoin is said
  only of a speaker the session has in no group at all, since a member
  another leader took is that leader's `join` — which is what keeps a
  `transfer` to a member from unjoining it while it is made the leader. The
  first look at a speaker takes the group the house already has rather than
  breaking it up.
- **A member is never handed a queue.** It plays what its leader plays,
  which the house arranges; and a leader that becomes a member when the
  sound moves to one of its group is not paused, since a pause said to a
  member is a pause of its whole group. The group calls go out before the
  hand-off on each pass, so a member about to lead is the leader in the
  house before its queue arrives.
- **Every standing speaker is polled**, not only held ones: its volume and
  group are worth reading while somebody is listening. A speaker stands
  only where a client is, so a house nobody is listening in is still asked
  nothing.

### The clients

The browser's `<audio>` element follows the browser's own entry: it says
`equip` once a connection with the element's level, sets the element to
whatever the session says, and says `report_volume` when the element moved
behind the session's back (it rings the window on `volumechange`). Setting
a volume or a mute is not what an autoplay policy refuses — starting a sound
is — so it needs no gesture. iOS keeps a media element's volume at 1, so the
element is probed once (moved, read back) and a browser where it does not
move says it has no volume. The desktop sounds nothing and says nothing.

The bar draws a mute and a slider for what is playing — the group's while
the output is one, this device's while nothing is the output, the local
player while unlinked. `+` (or `=`) and `-` nudge it by five, `M` mutes it,
over the page or a menu as the transport keys are. The picker (`d`, now 336
wide) keeps 23c40a0 — no stop row, this device the default while nothing
plays — and gains, per row, its slider and mute where it has a volume,
every member of the playing group ticked, a `+` on a speaker that can join
and a `−` on one that can leave, and, while the output is a group, a first
row for the group as a whole ("Kitchen + 1") whose slider is the group's.
On a row `+`, `-` and `M` are the row's and `J` joins or unjoins it; the
name is what hands the sound over, the controls beside it their own
targets. `D` draws each device's volume, family and leader, and the group.

**The phone** (`harken/android`; `docs/plan-android.md`, "Volume and
groups", has where it sits among the app's rules) is a device of the session
with a volume, and a remote for every other device's.

- **Its own volume is its media stream.** Once a connection, after the
  snapshot, the reconciler says `equip(true, null, level, muted)` at the
  level `STREAM_MUSIC` is at, read as 0 to 100; from then on the stream
  follows the phone's entry, set without a flag so no panel rises, and a
  stream moved behind the session's back — the buttons while the sound is
  here, the system's panel, another app — is said once as `report_volume`.
  **Never its own write**: a stream has fewer steps than a hundred, fifteen
  on most phones, so the 30 another device asks for is the fifth step and
  reads back as 33 — and 33 compared with the 30 asked for would be said
  back, and the 30 would become 33. What the stream reads is compared with
  what it read after the phone's own last write, never with the level asked
  for. What rings the reconciler is a `ContentObserver` on
  `Settings.System`, which the system writes a stream's level to whatever
  moved it: public, where `VOLUME_CHANGED_ACTION` is hidden and its extras
  undocumented; its cost is the half second the system takes to write the
  setting down, and a mute that is not written down is read at the next
  reconcile. Phones have no family, so a phone joins no group.
- **The buttons are whatever is playing's**, by Android's own mechanism for
  a remote, the way Cast does it. While the sound is on another device with
  a volume, the media session's player (`RemotePlayer`, which is the session
  whenever the phone is part of it) says it plays remotely —
  `DeviceInfo.PLAYBACK_TYPE_REMOTE`, 0 to 100, at the output's level, its
  group's while it leads one — and offers Media3 1.5.1's device-volume
  commands (`COMMAND_GET_DEVICE_VOLUME`, `COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS`,
  `COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS`, and the two older ones). The
  media session then gives the platform a volume provider rather than the
  stream, so the buttons and the system's panel reach the player: a press
  is `nudge_volume(output, ±5)` — `nudge_group_volume` for a group — the
  panel's slider `set_volume` (or `set_group_volume`), throttled as the
  screen's is, and its mute `set_mute`. While the sound is here or nowhere
  it plays locally and the buttons move the phone's stream natively, which
  the phone then reports. **In a car** the phone is the output by the car's
  rule, so it is local there; and a volume a car's controller asks for is
  never sent to a speaker, since the car's knob is the car's.
- **On screen**: the full player draws a mute and a slider under the
  transport (ark-ui's `VolumeSlider`, which springs to a level that arrives
  from elsewhere and follows the finger exactly while dragged) — the group's,
  the output's, this phone's own while the sound is here or nowhere, and its
  stream alone. A drag is throttled ordered calls, 50 ms trailing, the last
  always, by the app (`Drag`). The picker keeps its rules and gains the
  desktop's: a slider and a mute on every row with a volume, every member of
  the playing group ticked, `+` on a speaker that can join and `−` on one in
  the group, and the group first ("Kitchen + 1") with the group's slider;
  the device chip names a group the same way. The mini player is unchanged.
  The diagnostics draw each device's volume, family and leader, and the
  group.

### Tested

Each falsified once; the doc comment beside each says how. In
`harken/domain/tests/listening.rs`, on the native and the interpreted desk
at once: nothing has a volume until it says so; a volume set, nudged,
clamped and muted, and moved unmutes; a report is the caller's own and
ordered; a speaker joins only its own family and only while here; a group
never in a state Sonos cannot be in; the sound handed to a member makes it
the leader; a group's volume in proportion, exactly; a leader's report.
In `rust/ark/tests/live.rs`: the slots of a `PerDevice` of records can be
walked (below). In `assistant.rs`: the equip said once; a volume told, and
a hand reported only after the quiet; a group told and a group made by hand
reported, and the first look taking the house's; a member regrouped without
an unjoin or a pause. In `server/tests/bridge.rs`, over real sockets, a
client, the bridge and two stand-in Sonos: equip, join, a group volume told
per speaker, a hand on a knob, the Sonos app's grouping both ways, unjoin.
In `harken-iced`: the player following its volume and a change behind it
said, through real peers and the engine's desk; the picker's rows for a
group, the keys on a row, the transition a control calls. On the phone, with
real Kotlin peers and the test's server (`Hall`): the phone's volume said once
a connection, followed, a hand said once and its own write never
(`RemoteTests`); the buttons a speaker's, a group's, nothing from a car, and
local with the sound here (`ListenTests`, `RemotePlayer` on layoutlib's
Looper); the picker's sliders, mutes, group row and joins (`ListenTests`); a
stream's steps and the drag's throttle (`VolumeTests`). The falsifications
are in `docs/plan-android.md`.

### Found on the way, in the engine

`PerDevice::slots()` of a record could not be walked: `Slot<T>` was a
record, read as one handle per field, and a record value is many handles,
so the build panicked ("is 44 bytes, not 2 values of the vocabulary").
`Slot` is its own `Data` now, one handle taken apart by field, its value
whatever its type makes of the field — no IR, no runtime and no vector
moved.

`arkc gen kotlin` printed an optional field of a record as
`"family" to (family)?.let { .. } ?: Value.VNull`, and `?:` binds looser
than `to`: the entry was a pair or a value, `Any`, and the APK did not
compile (found by `harken-apk`; no printed type had had an optional field).
The option is parenthesised whole now, in Kotlin and in Swift; the demo's
printed room has none, so `spec/vectors/gen/` is unmoved.

**And in the bridge, before this: a playing speaker's position was read
as now.** Home Assistant refreshes `media_position` at a change of state and
dates it `media_position_updated_at`; a Sonos playing on says the position
its track started at for the whole track. Read as now, the session's clock
was three seconds ahead of it a few seconds in, and the bridge said `seek`
back to 0 — every bar ran to six seconds and reset, over and over, while the
speaker played on. `Playing::position_at` counts it on from its date, read
against the house's own clock as its answer's `Date` says (to the second,
taken at its middle); an undated playing position is never a seek. The
stand-in house in `server/tests/bridge.rs` freezes and dates its positions
as the real one does. Its own commit, ahead of this section's.

### Not verified

No real Home Assistant and no real Sonos: the service calls and the
attributes are as the documentation gives them, and the template that names
the integration is unasked of a real house. No browser has had its
element's volume set by another device. No real phone has had its
volume panel handed to a real speaker: what Android's panel draws for the
remote volume, whether a real phone's buttons reach the `RemotePlayer` while
the screen is off, and the settings observer ringing on a real phone are
unseen (`docs/plan-android.md`).

## The speaker that stopped at the first song

Reported as "if I cast from the browser to the speaker, it stops at the
first song". The bridge found where a speaker was in the queue by
comparing the URL Home Assistant reports (`media_content_id`, which the
Sonos integration takes from the speaker's own `TrackURI`) with the URLs
it was handed, byte for byte. harken hands every path segment
percent-encoded outside RFC 3986's unreserved set, so a library's names —
a space, a parenthesis, an apostrophe, an `&`, an accent — are all
escapes; a speaker that says back the same address spelt another way (a
space or a parenthesis decoded, an escape in lower case, an `&` escaped
again for the XML it travelled in, a host in capitals) was playing
something "not ours". After the grace (`SETTLE` for a hand-off that has
not started, `LAPSE` for one that was playing) the bridge let it go:
`release`, said as the speaker — the session paused, `nowhere`, on the
track it was at — while the speaker played on through the queue it holds,
so every bar sat on the first song. A first track whose name has nothing
to spell starts and the same happens at the first one that has.

Compared as an address now (`assistant::address`): HTML entities decoded,
twice escaped is still `&`; the scheme and host lower-cased and a default
port dropped; a fragment dropped; path and query percent-decoded;
`x-sonos-http(s):` read as `http(s):`. A report that names the speaker's
queue rather than a track (`x-rincon-queue:…`) is read by the
integration's `queue_position`, counted from where it was handed, since
the speaker's queue is ours from there. The release line in the journal
now says what was expected beside what was reported.

**What is confirmed and what is not.** Every other candidate was checked
and holds over real sockets against a stand-in house that keeps a queue
as the Sonos integration drives it (`enqueue: play` inserting after the
integration's possibly stale `queue_position` and playing from there,
`add` appending), moves to its next item when a track ends, and says it
is playing nothing for two polls in between: with the URLs said back as
given, the speaker plays through three tracks and the session follows it
0, 1, 2 on two devices, with one hand-off, no pause and no release
(`a_speaker_plays_through_the_queue_it_was_handed`). Advance and jump are
reported; the next track's `advance` lands before the bridge reads the
session again (the hub answers commands in order), so no reconcile pulls
the speaker back; the window pushes the whole queue from `at`; the old
output's reconciler is silent once it is not the output; and `advance` is
the speaker's, guarded to the output. The same speaker spelling the URLs
back its own way failed before and passes now
(`a_speaker_plays_through_the_queue_however_it_spells_it`). That a real
Sonos spells them back differently is not seen from here: the line to
look for in `journalctl -u harken` is

    harken-server: house: media_player.<x>: playing "<url>", which is not ours (playing "<url>", next "<url>"); let go of <account>'s session

where the reported URL and one of the expected differ only in how they
are escaped (a build before this one prints the same line without the
parenthesis). The same line with a URL of another file, or `"" which is
not ours` repeated, is a different cause (a speaker sent elsewhere, or
silent longer than `LAPSE` between tracks).

## A hand-off is a pause where the track was

Reported as "playing a song on a device and connecting to a speaker, it
keeps playing and then connects to the speaker and restarts the song". Two
defects, one in the domain and one in the bridge.

**The clock ran on through the hand-off.** `transfer` set the output to the
target, `moving`, and left the clock as it was — `playing`, counting from its
`since`. Every reconciler was already right that the device losing the sound
is not the output from that moment (`outputs` is any `device` with that id,
moving or not, in iced, on Android and in the bridge; `harken-iced`'s
`the_output_obeys_the_session_and_nobody_else_makes_a_sound` holds it), so
the old device went quiet; but nothing was making a sound, and every bar —
the old device's too — went on counting a track nobody heard for as long as
the speaker took to fetch it, which over Home Assistant is seconds. That is
the "keeps playing". Now `transfer` re-anchors the clock at where the
session stands at the server's moment of the transfer, and a session whose
output is `moving` *stands*: its position is the clock's anchor, playing or
not, until the target says `started` (`standing`, and `read::Active::
position` and Android's `Session.Active.position` reading it the same way).
A pause meanwhile pauses it there (`halted`, which `pause`, `release`,
`on_leave` and `on_restore` use where they used `paused_at`), play plays on
from there, a transfer elsewhere hands on that same point. The target loads
the track where the session stands, so a browser or a phone taking the
sound starts where it stopped; `started` re-anchors at what the target says.

**The speaker started the track from the beginning, and the session went
with it.** The bridge's hand-off enqueued the queue and sent `media_seek`
straight after, against a speaker that had not read a byte of the stream; a
Sonos drops a seek then. The speaker played from 0, the bridge said
`started` at what it reported — 0 — and `started` re-anchored every device
there. Now a speaker that reports the handed track from its beginning, when
it was handed from more than `DRIFT_MS` in, is told to seek again once it
has the track (`SEEKS`, three, a `QUIET` apart), and has not started until it
is where the track was; then `started` is said at that position. One that
never goes there is believed after three tries. The session stands still
meanwhile, so nothing goes back.

Decided: the old output goes quiet at the `transfer`, not at `started` —
the gap is a pause, never two sounds. A transfer to the device that already
has it, settled, still changes nothing, clock included.

**Tested**, each falsified once: `a_hand_off_stands_still_where_the_track_was_until_it_has_started`
(the domain, both desks: falsified by a `transfer` that kept the clock as it
was — it stood at 90 000 where 91 500 was right — and by a `standing` that
ignored `moving` — a pause seven seconds into the hand-off landed at
98 500); `a_speaker_that_starts_the_track_from_its_beginning_is_sent_where_it_was`
(the bridge's rule: falsified by saying `started` at the first report, at
0); `a_hand_off_picks_up_where_the_track_stopped` (over real sockets, with a
stand-in Sonos that keeps a queue and drops a seek sent before it has loaded
the track: a browser forty seconds in hands to the kitchen and back, no
device's position ever goes back, the kitchen ends up at forty seconds and
not at 0, and is paused once the browser has it — falsified both ways
above: "went back 344 ms on device 1", "went back 39857 ms on device 0");
and `a_hand_off_between_two_players_picks_up_where_the_track_stopped`
(`harken-iced`, through real peers and the engine's desk: the browser silent
on its next reconcile, the session standing at forty seconds through a
slow laptop, the laptop loading there — falsified by loading where the
clock counts, 41 506).

Not verified: a real Sonos dropping, or keeping, the early seek; the phone
as a target on a device.

## A speaker plays on with nobody listening

Asked for as "when I play to a speaker it will keep playing the session and
the queue even if I close the browser". It did not: a speaker stood only in
a session a client was in, so that a speaker never kept a session's presence
open, and the bridge detached it when the last client left — `on_leave` made
it `away`, its clock paused, and `left` paused the speaker in the house. A
closed tab stopped the music in the kitchen.

**The rule now** (`assistant::playing`, `ha::run`): where a client is, every
configured speaker stands, as before. Where none is, the speakers *making
the sound* go on standing — the session's output, here or on its way, and
whoever follows it — and the bridge goes on reconciling them and saying what
they do: `advance` at the end of each track, `stalled`, a volume, a group.
Once such a session has been silent — paused, at the end of its queue, or
moved off the speaker — for `ha::IDLE`, ten minutes, its speakers go: the
bridge detaches them, `on_leave` makes the output `away`, already paused, and
`left` pauses what the speaker held; the session is kept as any is and
snapshotted with it. A speaker that is not making the sound still stands
only where a client is, so a house nobody is listening in and that plays
nothing is asked nothing.

The server side needed nothing: an instance is never dropped by the
engine's desk, a participant's calls are sequenced as any connection's,
and the last client leaving runs `on_leave` for that client alone, which
touches the session only when the leaver is its output — and the speaker
is. Held by the first test below, which reads the session from the hub
after the browser has gone.

**A restart keeps it paused** (decided): `on_restore` leaves every session
`nowhere` and paused, and that stays. The server stopping is not a choice
to resume a speaker, and a house starting to play by itself when a server
comes back up — after an update at three in the morning — is worse than one
that waits for a press. The speaker itself, which nobody told to stop, may
play on to the end of the queue it holds; the session picks it up as soon
as somebody plays it again.

Ten minutes: long enough that a pause to answer the door, or a queue that
ran out while somebody chose the next, finds the speaker standing and the
session as it was; short enough that a house left paused at night is not
being asked about by morning.

**Tested**, each falsified once, in `server/tests/bridge.rs` over real
sockets with the stand-in Sonos: `a_speaker_plays_on_through_the_queue_with_nobody_listening`
(a browser hands three tracks to the kitchen and closes; the kitchen plays
on past three `idle`s, `at` 1 then 2 as the bridge says its `advance`s,
never paused, and a browser opened later finds the third track playing
where the kitchen is — falsified by standing only where a client is: the
kitchen left with the browser and its `advance` was never said, "never on
track 1");
`a_speaker_left_paused_with_nobody_listening_goes_after_a_while` (still
standing a moment after the browser goes, gone not before `idle`, the
session kept paused and `away` on the kitchen, the house asked nothing more
— falsified by never letting a silent speaker go: "the kitchen never
went"); `an_idle_house_is_asked_nothing` (no reads before anybody signs in,
and none once the only client has gone with the sound its own — falsified
by polling every configured speaker: thirteen reads before anybody signed
in).

Not verified: a real Sonos left playing overnight with no browser; how long
Home Assistant keeps answering about it.
