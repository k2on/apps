# Live state: a typed state machine per scope, beside the log

Status: **decided**, not yet built. Supersedes the opaque live rooms of
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
| presence: which devices are here, their kind, whether they can play | live, kept by the server | authored by nobody; it is what the server knows of its connections |
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
- **Presence is the engine's.** A device declares itself at `Hello` (an id
  that survives its reconnects — a login's session for a client, an entity
  for a speaker — a kind, a name, whether it can play). The server keeps the
  list per instance, passes it to transitions as `ctx.presence`, and runs
  `on_join` and `on_leave` when it changes.

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
