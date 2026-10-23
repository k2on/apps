# Observing a system of peers: one trace from a tap to a job

Status: **decided**, not yet built. Replaces `ark-explorer` (the admin page
and the in-app explorer), which is deleted.

## Why

A harken press in the browser was rolled back and nothing said why. The
reason, "overtaken by a later one of its origin", existed only in a verdict
frame the browser received and the server's live authority forgot; finding
it took an afternoon of SSH, journal greps and a person reading devtools
aloud. What was missing was not a page that shows tables. It was the answer
to *what did this tap cause, everywhere, and what did each peer make of it* —
on a phone in Kotlin or Swift, through the bridge, the device's peer, the
socket, the server's authority, the fanout, a reconciler or a job, and the
Rust code inside that job — readable by a person and by an agent, against a
live system and inside a test, with the same tool.

## The idea: causality is ark's, not the app's

**Every entry carries its cause, and every entry is a span.**

- `Entry.cause: Option<Cause>`, `Cause { trace: TraceId, parent: SpanId }`.
  An entry's own span id *is* its `EntryId`; a `SpanId` is 16 bytes so that a
  parent may be an entry. Frozen in the entry like `now`: part of its
  encoding and its chain hash, so every replica holds it and every replay
  reads it. It changes no state unless a function chooses to write it down.
- `Ctx` says the entry's trace and span to a function, a job and a view.
- **Changes say who made them.** `Output::Changes` carries, with each change,
  the span of the entry that produced it — the entry run, the logged entry
  confirmed, the raw write, the pending intent undone; none for a whole-state
  diff (a snapshot, a divergence, an open).
- ark-peer authors an intent under the **current span**: the one the app
  opened (a tap), or — while a job, a handler or a reconciler runs because
  of a change — the span of the entry whose change woke it. So a hand-off
  the server's house bridge makes because a browser pressed play is a child
  of the press, with no app code saying so.

The result is one tree per tap across every peer, joined where it happens,
not stitched by a query afterwards.

## One stream per peer

A peer emits events through `tracing` (the crate), at the places it already
funnels through — never inside the kernel:

- **`Peer::route`**, the one door every machine input and output passes:
  each input (an intent, a frame, open, connect, a tick) and each output
  (a frame sent, a verdict, changes, a refusal), with fixed fields — peer,
  space, entry id, origin, `local`, func, connection, verdict, span.
- **The link**: dial, up, down, closed and why.
- **Jobs and reconcilers**: a span around each run, so the Rust inside a job
  function is a child of it, and of the entry that made the row due.
- **The bridge** (`ark-ffi`): a span per call, and `span(name, fields)` /
  `event(...)` for the app, so a tap in Kotlin or Swift opens the root span
  in the same pipeline. No OpenTelemetry SDK on a phone.

The machine steps in that stream carry their input bytes, so **the stream is
also the recording**: replaying a peer to a moment (`ark_dev::trace`'s player)
is a query over it, not another system.

## Where it goes

- **Over the sync socket.** One new frame, `Telemetry { events }`, from a
  peer to its server: the connection, the login and the offline buffering a
  peer already has. A peer keeps a bounded buffer while it is away.
- **The server collects its system**: its own events and every peer's, in a
  bounded store in its data directory, with a live tail.
- **In a test**, an in-memory collector shared by every peer of a simulator
  or fleet run answers the same queries, so a test asserts on a trace: *this
  tap produced a hand-off and nothing was refused*.
- **Optionally**, a `tracing-opentelemetry` layer exports the same spans to
  Tempo or Jaeger for dashboards. Nothing depends on it.

## `arkdb`, the one command

`ark-dev` becomes `arkdb`, one binary of subcommands — `gen` and `fuzz` as
today, and:

- `arkdb tail` — the server's events as they come, filtered (peer, func,
  refused only).
- `arkdb trace <trace|entry>` — everything a tap or an entry caused, as one
  indented timeline across peers.
- `arkdb peer <origin|session>` — one peer's stream; `--at <seq|time>`
  replays it there and prints its rows.

It reads a live server (over loopback, so `ssh ark arkdb tail` works) or a
test's collector dumped to disk. Its output is text, for a person and for an
agent alike.

## Order of work

1. Delete `ark-explorer` (the admin page, harken's in-app explorer, the
   flake's `ark-admin`).
2. `Entry.cause`, `Ctx`'s span, changes tagged by span; the vectors written
   again.
3. ark-peer: the current span, authoring under it, jobs and reconcilers run
   under the span of what woke them; events at `route`, the link and jobs.
4. The bridge: a span per call, the app's `span`/`event`.
5. The `Telemetry` frame and the server's collector.
6. `arkdb`: the rename, `tail`, `trace`, `peer`.
7. The test collector, and a harken test from a press to the speaker's
   hand-off.
