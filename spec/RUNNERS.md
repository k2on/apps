# Running the vectors

What `rust/ark-dev` does with `vectors/protocol/`, `vectors/trace/`,
`vectors/rebase/` (`tests/vectors.rs` is the runner) and `vectors/views/`
(`ark_dev::vectors::run_view`, run by `tests/views.rs`); where it and this
file disagree, the code is right. A port of the kernel — the wasm build,
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
by the name the trace gives. The `views/` vectors' are
`rust/ark-dev/src/library.rs`: `library` (the demo's two tables, `person`,
`work`, `movement`, `song`, `track`, and a view per v4 scenario), `colors`
and `pairs` (a fuzz finding each).

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

## views

`{batches, ctx, domain, input, rows_before, schema, steps, store_before,
view}`: the view `view` of the reference domain `domain`, read as `ctx`
with `input` (its input type's JSON; `null` is `()`), over the store
`store_before` (each table's rows as JSON, in key order). Its `schema`
is the domain's, which the runner checks first. The runner makes the view,
checks its rows are `rows_before`, then for each batch — changes `add`
`{row}`, `remove` `{row}`, `edit` `{old, new}` under a `table` — applies
it to the store, pushes it to the view as one, and checks the patches
(`insert {at, node}`, `remove {at}`, `update {at, node}`, positions in
the list as each is applied) and the rows are that step's. After every
push the rows are the view's answer afresh and a view made again's, and
the patches splice the rows before into them. A second implementation
runs the view of that name it implements: the plan is the Rust in
`library.rs`, a few lines each.
