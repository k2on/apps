# Running the vectors D1 writes

What a runtime in another language does with each file under
`vectors/protocol/`, `vectors/trace/` and `vectors/rebase/`: what it
reads, what it does and what it compares. `rust/ark5-dev/tests/vectors.rs`
writes them (`ARK5_WRITE_VECTORS=1`) and is the reference reading of this
file; where the two disagree, the code is right and this file is fixed.
The other directories are `README.md`'s.

## Every runner

**It reports by name**: each vector it ran and each it skipped, by path,
with why. A directory holds exactly what the writer wrote; a file it did
not write is a stale vector.

**The JSON here is plain**: bytes are lowercase hex strings, an `Int` a
JSON number (every one fits a 64-bit integer), an object's keys in
code-point order. A vector under `falsify/` carries `"expect": "fail"`:
it is a right vector with one thing changed, and a runner must fail it.

**The demo** is `README.md`'s (`vectors/module/demo.json`), with two
functions beside its four: the query `demo.items` and the scope
`demo.top_two`. **The room** is live state over volatile tables of its own
(`rust/ark5-dev/src/demo.rs`): `here(session, name, since)` and
`pointer(session, x)`; `room.on_join` and `room.on_leave` take
`Arrival(session, name)`; `room.on_restore` lets every pointer go;
`room.rename(name)` and `room.point(x)` are a device's calls, `point` the
ephemeral one. A trace carries the bytes of every function it needs, so a
runner needs neither module to replay one.

## protocol — client

`{bytes, frame, name}`: `bytes` a frame's encoding (§1.11, `frames.rs`),
`frame` the same value as JSON read under the frame type `frame_ty()` —
an `Int` a number, `Bytes` hex, a list or a tuple an array, a sum
`{"variant": i, "fields": [...]}` — and `name` who writes it, its kind
and, for a frame of a space's log, its space (`client-hello-volatile`).
**Do and compare**: `frame` written under the frame type is `bytes`;
`bytes` decodes to a frame that encodes to `bytes`; `bytes` read under the
frame type is `frame`. Every kind is here, each in each space a session
sent it in; `Held` and a `Reject` are built by hand from an entry, since
no session sends them. No frame carries a function's body: an authority
runs only the functions its operator loaded, and `Held` tells a device
its intent names another, to keep pending and push again on its next
connection.

## trace — client

One client's life in one session: the inputs its two machines (§1.7, the
log's and the live one, §1.9) were given and what each said. A client is
a pure function of these: a runtime replays them into fresh machines and
must say the same, byte for byte, standing where the client stood. The
driver's policies — when it is linked, its relay, its disk, its views —
are not in a trace; what they decided is (the frames it received, the
calls it made, the state an open was given).

**The file**: `{device, from, live, log, steps}`. `log` and `live` are each
space's module as the machines hold it: `schema`, the schema's bytes
(`Node` encoding of `Schema`), and `functions`, the bytes of every
function body, each loaded under its hash (`FnHash = sha256(bytes)`);
`live` is `null` for a client without live state. `device` is the hex of
what the live machine says it is in its `Hello`, or `null`. `from` names
the session and the client. `steps` holds one step per line, each one of:

| `t` | fields | the runner |
|---|---|---|
| `open` | `who` (`{actor, session, roles}`), `origin` (hex, 16 bytes), `fork` (`[seq, hash]` or `null`), `joining` (hex or `null`) | both machines afresh, replicas of their module as `who` from `origin` (§1.6: the bytes the client drew once and keeps), the live one saying `device`; with a `fork`, the log's machine an authority alone since it (§1.7); with `joining`, the chain it had handed over and not had answered, `((seq, hash), [Logged])` encoded |
| `sign_in` | `who`, `origin` | the log's machine authors as `who`; when `origin` is not the one it had (another actor signed in), from it and from `local` 0; the handed-over chain forgotten when the actor or roles changed; the live machine afresh as `who` from `origin`, its `local` kept when the origin is |
| `step` | `space` (`durable`, `volatile`), `in`, `out`, `after` | `in` given to that space's machine; what it says must be `out`, and it must stand at `after` |

`in` is `{"t": …}`: `frame` (`bytes`, from upstream), `mutate` (`fn`,
`input`, `now`), `connected`, `alone`, `join`, `compact` (`seq`), `tick`
(`now`), `persisted` (`seq`), or `open` (`durable`: the hex of `((Snapshot,
[Logged]), ([Entry], Int))`, what the machine is opened from: the snapshot,
the chain on it, its pending intents and the last `local`). `out` is the
list of outputs in order, each `{"t": …}`: `send`, `fanout` (`bytes`),
`to` (`session`, `bytes`), `changes` (the hex of the `[Change]`),
`persist` (`bytes`), `refused` (`id`, `why`), `confirmed` (`id`). `after`
is `{hash, head, pending, state}`: the head's sequence and entry hash, the
store's state hash and the pending intents' ids in order.

**Compare**: replay every step into fresh machines; on the first that says
other than `out`, or stands other than `after`, report the trace, the step
and both sides. `trace/falsify/` holds a trace with one byte of one frame
changed, which must fail.

The files are every client of the scripted sessions
(`ark5_dev::vectors::sessions`): `the-demo`, `a-restart` (the hub keeps
one entry above its horizon and restarts from its disk), `a-reopen`, `a-sign-in`,
`alone-then-join`, `the-room` (live state: a rename, three pointer moves
inside one relay interval, a device leaving) and `fleet-seed-7`. The
corpus beyond them — every client of every fleet session and of every fuzz
session — is replayed by `ark5-dev`'s tests and by `ark5-fuzz … traces`,
not checked in.

## rebase — Rust's

`fleet-seed-7.json`: `{clients, hash, head, playlist, script, seed}`. Three
clients and the hub; client 0 creates `playlist`, everything is settled,
the `script` runs (`mutate` with `client`, `fn`, `input`; `partition`,
`heal`; `step`, one delivery chosen by the seeded scheduler), everything
is settled; the hub stands at `head` with state hash `hash`, K1's pin over
a bare machine. `fleet-fuzz-seed-N.json`: `{found, hash, head, seed,
state}`, a session the fuzzer (`ark5_dev::fuzz::session`) found a bug with,
what it was, and where it stands once fixed. Both are rust-only: to run
one is to reimplement the simulator's scheduler and the fuzzer's
generator exactly, which is not a client. What they do to a client
reaches every runtime through its traces (§ trace).
