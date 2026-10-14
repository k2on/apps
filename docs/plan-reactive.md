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

## Landed: the Rust half

`rust/ark-client`, `rust/arkui`, `harken/iced` and `harken-peer`. The engine,
the protocol, `spec/vectors` and the trace corpus did not change. The
Android half is a later round.

**The client.** A transport handed a `Wake` (`ark_client::link::Wake`) calls
it after every event it queues: the native transport's reader thread after
`Opened`, each frame and `Closed`; the browser's `WebSocket` from `onopen`,
`onmessage` and `onclose`. `Peer::set_waker` puts the host's wake behind a
"drain requested" flag that the next `pump` clears before it drains, so a
burst is one wake, one pump and one settle. `pump()` is still the unit of
work, and the tests, the sim and `ark-server`'s in-process transport (which
takes no waker) still call it by hand. The redial after a close is
`Peer::deadline()` — a monotonic millisecond the host arms one timer for,
unchanged until the link moves, `None` once it dials, and `None` while open.
`mutate`, `say` and `verify` send at once rather than on the next pump.
`Pumped` says what moved — `views` (the store moved since the last pump),
`heard`, `status` (only when it differs by value), `deadline` — and
`Pumped::quiet()` is "nothing". The keepalive stays the transport's: the
native ping and its read slice wake nobody.

**What wakes harken's window now**, source by source:

| what | before | now |
|---|---|---|
| the sync socket (log, acks, verdicts, the listening session's frames) | the 50 ms tick pumped | the peer's wake rings the window's `arkui::wake::Bell` → `Message::Wake` |
| the redial after a close | polled every tick | `arkui::wake::at(deadline)` → `Message::Due`, one timer keyed by the deadline |
| a second of quiet before the default playlist | `QUIET_TICKS = 20` ticks | `QUIET_MS = 1000`, a deadline moved by each frame, on the same timer |
| what this window authors, says or asks | sent on the next tick | sent by the call (`mutate`, `say`), and every update ends in `App::react` |
| the address bar | polled every tick | `popstate`/`hashchange` ring the bell; `Router::poll` after every update |
| a lock-screen button, the `<audio>` element | polled every tick | the media session's handlers and the element's `play`/`pause`/`ended`/`waiting`/`seeked` ring the bell |
| the scrubber, the media session's position, the report heartbeat | every tick | a 250 ms clock only while audio plays here or on the device this one draws, or `D` is open |
| the explorer's highlight fade | every tick | a 50 ms clock only while `Explorer::fading()` |
| a submenu's dwell | every tick | a 50 ms clock only while `Context::dwelling()` |
| covers | a message from their fetch | unchanged |

With nothing playing, nothing fading and nothing arriving, `App::clock()` and
`App::deadline()` are `None` and the window has no timer. `harken-peer` is
driven the same way: it pumps on a wake, after a command that asks
something of the peer, and at the deadline — a read pumps nothing, and
`wait`/`settle` pump only on wakes — so the process fleet is the test of the
wakes. `--pump-ms` is now how long `settle` waits with no wake before the
cursor counts as still.

**Measured** in headless Chromium (SwiftShader, 1100×700) on the demo page
(`nix build .#harken-web`), the build before this and the build after, the
same script: requestAnimationFrame callbacks, `setTimeout` callbacks fired,
and Chrome's `Performance.getMetrics` `TaskDuration` over the window, as CPU.

| window | before: rAF / timeouts / CPU | after: rAF / timeouts / CPU |
|---|---|---|
| idle, 10 s | 112 / 520 / 7.2 % | **0 / 0 / 0.01 %** |
| a track playing, 4 s | 43 / 217 / 7.9 % | 16 / 42 / 2.9 % |
| paused again, 5 s | 49 / 264 / 8.5 % | 0 / 0 / 0.4 % |
| a highlight fading, 1.8 s | 19 / 97 / 8.2 % | 21 / 66 / 7.3 % |
| faded, 5 s | 53 / 264 / 8.8 % | 0 / 0 / 0.01 % |

Playing, the `<audio>` element advanced 4.1 s in 4 s and two screenshots of
the bar differ (the scrubber at 0:05, moving); the row the app put on a
playlist is lit in the explorer's `playlist_item` table and gone 2.8 s later.
The audio was a local WAV served for every media request, since the demo's
Wikimedia URLs are not reachable from where this was measured.

**Tests, each falsified once:**

- `ark-client` `wake_tests`: at rest a linked peer is a minute of simulated
  seconds with no wake, no deadline and every pump `quiet` (falsified by
  publishing the status on every pump; and by `Link::deadline` answering
  `Some(0)` while open); a mutation is pushed by `mutate` itself (falsified
  by removing that flush); fifty frames are one wake and one settle
  (falsified by ringing the host per event: fifty wakes); a close arms one
  redial deadline that does not move until due, and the dial and open clear
  it (falsified by `deadline` answering `None` while waiting).
- `ark-server/tests/wake.rs`: two peers on real sockets, pumped only on
  wakes and deadlines, converge; linked and idle for two seconds with pings
  both ways every 150–200 ms, nothing rings (falsified by the native reader
  not ringing: never converges); a server going away is redialled at the
  deadline (falsified as above).
- `arkui::wake`: a bell rung before anyone listens is still said, fifty
  rings are one message (falsified by dropping `missed`; by yielding per
  ring); a deadline fires once and is one timer however often asked for
  (falsified by hashing the delay).
- `harken-iced` `tests/wake.rs` runs the window's real subscriptions with no
  input events and counts what reaches `update`: at rest, nothing in 1.5 s
  (falsified by a clock that is always on: thirty ticks); an open on the
  socket is one `Wake` and a close one `Due` at the redial (falsified by not
  handing the peer the bell); a fading highlight and a dwell run the clock
  and stop it (falsified by `clock` ignoring each).
  `the_default_playlist_waits_for_the_log` now drives the deadline through
  a link that opens (falsified with `QUIET_MS` at 1 and at 1500).
- The process fleet (`harken/server/tests/fleet.rs`) with `harken-peer` on
  wakes: all 25 pass; with the native reader not ringing,
  `three_peers_online_agree…` does not converge in 60 s.

**Not verified.** The desktop window was not opened (no display here): its
wakes are tested through the subscriptions, not seen. The CPU numbers are
SwiftShader's, not a GPU's. A lock-screen button and the element's own
events ringing the window are wired and compiled, not exercised — headless
Chromium has no media session to press. The Kotlin and Swift fleets
(`fleet_kotlin.rs`, `fleet_swift.rs`) were skipped here for want of their
binaries, so the mixed fleets have not run against a waking `harken-peer`.
The native transport's thread still reads in 20 ms slices so it can write;
that is inside the transport and wakes no host, but it is a thread that
runs.
