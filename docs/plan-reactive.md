# No loop: a client wakes when something happens

Both clients drive the sans-io engine from a timer: the Android session
calls `Peer.pump()` every 50 ms on its own thread, and harken's iced client
does the same on `iced::time::every(50ms)`. Each pump drains the socket's
queue, settles, flushes what is outgoing, and tells every listener that
something may have happened — whether it did or not. So a screen at rest
hears from the session twenty times a second, the radio and the CPU wake
twenty times a second, and the owner's reading is right: nothing is
changing, so nothing should run. This replaces the timer with events. The
engine does not change; only the drivers do.

## The rule

**The peer's thread sleeps until one of four things wakes it, and a screen
hears only about what moved.**

1. **A frame arrives.** The socket's callback queues it and asks the
   peer's thread to drain — one wake for a burst, coalesced by a "drain
   requested" flag, so R8's settle-once-per-pump is settle-once-per-wake
   and needs no clock.
2. **The app asks for something** — a mutation, a sign-in, a view opened
   or closed, a verify. It is a task on the peer's thread; it runs at once,
   and whatever the engine then has to send goes to the socket in the same
   task.
3. **The socket opens or closes.** One wake, like a frame.
4. **A timer the engine asked for.** Redialling after a close is one
   one-shot timer at the backoff's deadline, cancelled when the socket
   opens; the keepalive is the transport's own (OkHttp's ping interval; the
   native transport's ping). Nothing else is timed. Connected and idle, a
   client has no timer at all.

**Publishing is by change.** Each maintained view publishes its patches only
when a wake moved its rows. The status a screen may read (linked, pending,
held, denied) is published only when one of its fields changes, compared
by value. A listener is told *what* moved, never "a pump happened".

## What moves off the timer

- **The listening session** (who is playing where) is a live frame like any
  other and wakes the thread the same way. The player's position on screen
  is the player's own (Media3's position, the `<audio>` element's), read
  while it is visible and playing; it never polls the engine.
- **iced's tick** also drove the media session, the address bar and the
  covers. Each becomes its own subscription or message: the media session
  and the scrubber tick only while audio is playing; the address bar is
  written when the route changes and read on `popstate`/`hashchange`; covers
  arrive as messages from their fetch. With nothing playing and nothing
  arriving, the desktop and the page run no timer either.

## Where it lands

- `rust/ark-client`: the link's socket delivers events into a waker the
  host supplies (`Peer::drive(Waker)` or the equivalent): native, a channel
  the host's runtime awaits; wasm, a callback from the `WebSocket`'s
  `onmessage`. `pump()` stays as the thing a wake runs and for the tests
  and the sim, which drive it by hand. Backoff becomes a deadline the host
  arms.
- `harken/iced`: an `iced::Subscription` fed by that waker replaces
  `time::every(50ms)` for sync; the other tick consumers move as above.
- `kotlin/ark-client` + `kotlin/ark-android`: `ArkSession` drops
  `scheduleWithFixedDelay`; OkHttp's listener posts a coalesced drain to the
  peer's executor; the backoff is one `schedule(delay)`; `onPump` becomes
  `onChange(what)`; `status` emits by value. harken's `Library` reacts to
  view changes instead of ticking.

## Guards

The engine's semantics, `spec/vectors`, the trace corpus and every runtime's
conformance are untouched. The risk is a missed wake — a frame arrives and
nobody drains it — and the mixed fleets catch exactly that (a peer that does
not drain never converges). New tests: at rest, a connected client runs no
task and publishes nothing for N seconds; a burst of frames is one settle; a
close arms exactly one redial timer and an open cancels it; a screen's row
recomposes zero times at rest (Android) and iced's view is not rebuilt at
rest (desktop/web). Falsify each once.
