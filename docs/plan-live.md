# Live state: a typed state machine per scope, beside the log

Status: **decided**; phase 1, the spec and the engine, is built (see
"Landed: phase 1" at the end). Supersedes the opaque live rooms of
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
