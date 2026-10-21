# Running the vectors

What `rust/ark-dev` does with `vectors/protocol/`, `vectors/trace/` and
`vectors/rebase/` (`tests/vectors.rs` is the runner; where it and this
file disagree, the code is right). A port of the kernel — the wasm build,
the FFI library — runs the same code, so it is held by the same runner;
the forms below are what a reader of a failure needs.

**Every file is JSON** (§1.1's human form): hashes, ids and bytes are
lowercase hex; a row inside a frame is its JSON under `values-json` and
its hex under `values-postcard`. A vector under `falsify/` carries
`"expect": "fail"`: a right vector with one thing changed, which the
runner must fail. The runner reports each file by name; a file the writer
did not write is stale.

**The domains** are `rust/ark-dev/src/demo.rs`: the demo (`playlist`,
`item`; `demo.create_playlist`, `demo.add_to_playlist`,
`demo.rename_playlist`, `demo.remove_from_playlist`, the view
`demo.items`, the scope `demo.top_two`) and the room, live state over
volatile tables (`here`, `pointer`; `room.on_join`, `room.on_leave`,
`room.on_restore`, `room.rename`, `room.point`, the ephemeral one). An
entry names its function; a trace is replayed with these domains, found
by the name the trace gives.

## protocol

`{bytes, frame, name}`: one frame (`frames.rs`), `name` who writes it, its
kind and its space (`client-hello-volatile`). The runner decodes `bytes`
as a frame, encodes it again to the same bytes, and checks its JSON is
`frame`. The kinds: `Hello`, `Page` (a client's intents at sequence 0, a
client's chain made alone, an authority's log), `Snapshot`, `Verdict`
(`Ok`, `Refused`, `Held` built by hand: no session sends them).

## trace

One client's life in one session: `{device, from, live, log, steps}`.
`log` and `live` name each space's domain, its schema's bytes (hex) and
the functions its machine runs; `steps`, one per line, are `open` (who,
the origin drawn once, a fork when alone, a chain handed over and not
answered), `sign_in`, and `step` — `space`, `in` (a frame, a mutate, a
lifecycle input, an open's `Durable`), `out` (every output, bytes as hex)
and `after` (head, hash, state hash, pending ids). The runner replays
every step into fresh machines (`ark_dev::trace`) and fails at the first
that says other than `out` or stands other than `after`. The corpus
beyond these files — every client of every fleet and fuzz session — is
replayed by `ark-dev`'s tests and by `ark-fuzz N FIRST traces DIR`, not
checked in.

## rebase

`fleet-seed-7.json`: three clients and the hub under the simulator's
seeded scheduler (`ark_dev::sim`): client 0 creates `playlist`, the
`script` runs (`mutate`, `partition`, `heal`, `step`), all settle; the hub
stands at `head` with state `hash`. `fleet-fuzz-seed-N.json`: a fuzzer
session (`ark_dev::fuzz::session`) once a bug, and where it stands
settled. To run one is to run the simulator and the fuzzer themselves.
