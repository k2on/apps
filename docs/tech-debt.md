# Tech Debt

What we know we owe: each entry says what it is, why it was left, and what
paying it would take. An entry goes when it is paid. The last section is
known bugs, kept here so there is one list.

## Kernel

### Four Structures Where One Versioned Store Would Do

Pending entries, the overlay, the change-tracking layer and the cells'
old values are separate structures. One versioned store would serve as
all four, and as the "before" view plans read. Left because it saves an
estimated 300 bytes and changes the `Store` trait every host implements.

### Live State Could Heal More Simply

A reconnect's healing of session rows has several rules (owned rows taken
from no hello, rows the hello leaves out dropped, an owner re-saying its
own after the welcome). A simpler rule, re-sending owned rows on every
reconnect, saves about 100 bytes but costs traffic on each reconnect.

### The Cell Core Is Unsafe and Has Never Run Under Miri

Cells are type-erased slots (a pointer and a vtable), so their core is
`unsafe`. It is kept to one module behind typed handles, and its tests
check every value is dropped exactly once, but it has not run under Miri:
the build machine has only stable Rust. Run `cargo miri test -p ark-kernel
slot` on a nightly toolchain.

### A Sort No Index Provides Holds Its Input

A `sort` by a closure no index provides keeps its whole input, so a long
list sorted that way costs its whole length in memory. A `take` over an
index-ordered input stays bounded. Materialize and index the value if a
real list needs it.

### Min and Max Refold

Count and sum are running totals, constant time per change. Min and max
are refolded over the group's held numbers when the extreme row leaves,
so a group holds its rows' numbers.

### No Explicit Memo

A binding that mixes a fast input (`now`) with an expensive value
computed outside a plan re-runs the expensive part on every tick. Plans,
per-leaf bindings and `Text` templates cover nearly every case. An
explicit memo waits until a real screen measures slow.

### Partial to Full Sync Must Start Over

A device that goes from holding its queries' rows to holding everything
must start again from cursor 0. Nothing enforces it.

### The Server's State Is in Memory

The authority's log, its journal of refusals and the per-client records
are kept in memory. Persisting them, and collecting the records of clients
that never return, is the server host's work and not done.

### No Hold-Everything Downstream Through a Relay

A downstream node that subscribes to nothing holds nothing through a
relay; one that should hold everything is not supported there.

### Refusal Rows Without a Client Are Kept

A refusal's row goes to its origin's connection, or the one it came
through, by the client the host names when it opens the connection. A
connection opened with no client is sent the refusal rows of its actor
instead, and the upstream lets none of them go: a host should always name
the client. A phone keeps its watch's refusal rows, through its own whole
fills, until the watch says hello past them; a watch that never connects
again leaves them at the phone.

### Two Healing Rules Have Only Unit Tests

Removing "ignore the hello for owned rows" or "drop rows the hello left
out" does not fail the convergence property test; only unit tests catch
it. The property test should be extended until it does.

## Sync Edge Cases

### The Clock Offset Is Measured Only at a Hello

A node's offset to its upstream is measured from the hello and welcome, so
a watch connected for a long time drifts until it next reconnects.

### A Server's Control for a Phone-Owned Row Can Be Lost

A control made on the server for a row a phone owns, while that phone is
cut off from the server, is overridden when the phone reconnects: the
control is lost, not delayed.

### An Offline Phone Answers Its Watch Only After a Hello

A phone offline from the server can write its watch's rows only after the
watch has said hello to it.

### The Phone Can Read the Watch's Frames

The phone relays the watch's frames as bytes it does not need to read,
but can. They should be encrypted end to end between the watch and the
server.

## Watch

### Large Rows and Freed Keys in the Watch's Store

The watch's store packs rows into 256-byte values. A row over about 250
bytes is not persisted, and freed storage keys are reused but never
deleted.

### A View's Sources Are Lets, Not Yet `api`

A view reads sources in `let`s (`cell(C)`, `row(f(…))`, `list(f(…))`),
and what it works out of them in further lets: the routes macro's `api.*`
(docs/north-star.md, step 12) replaces the first, and `#[state]` cells
the page's own (step 13). Until then:

- a keyed watch's arguments are constants or the record's, never read
  from another source, and are encoded by their own type (a literal is
  written typed, `3u32`);
- a keyed watch is kept by its function's name, not yet a generated id
  (step 14), and once made it is kept while the node runs;
- a sub-view is handed the record only, not a source's binding;
- what is worked out goes in a `let`, never in a part's argument, and a
  view names each `let` once;
- the phones read a source view's lets as a record's fields (`p.title`),
  which nothing yet builds for a source view: only the snapshots are
  written from it. harken's and money's screens still read records, on
  every toolkit;
- a result is shared within a settle by the layer's key only on the
  watch; `Real` works each `let` out once a build.

### The Watch's Screen Grew With Its Sources

Reading sources in place of hand-written functions grew the demo by about
2.4 KB of code (61.1 KB in all) and its heap by about 0.9 KB: the keyed
watches, the shared results (each a box and its read set), and each
`Text`'s holes made before they are written. A binding could write its
words straight into its line, not through a `Text`, when the emitter can
see its function's template.

### The Pebble Generator's Gaps

The emitter refuses scroll, images, swatches, grids, a `for` outside a
list, row taps, `.width`, `.side`, `.square`, `.max_width` and `.bleed`, a
tint read from data, and every part but the screen's own. A list shows at
most six rows, an action bar can only be a screen's root, and nothing
animates.

### Restarts Skip Entry Ids

The watch keeps its entry count as a lease 32 ahead, so a restart skips up
to 32 ids. The server accepts the gaps.

### Nothing Has Run on a Real Watch

Beyond the start of the probe, everything on the watch has been seen only
in the emulator: storage write times, Bluetooth throughput, stack depth
and battery are unmeasured on hardware.

## Known Bugs

### Three Bugs in the Old Kernel

On main's old kernel (`rust/ark`, `ark-peer`), live rows reach every
account's connections, the server starts a thread per call with no
bound, and a call is matched by the last part of its name. Each is fixed
by a step of docs/north-star.md, not separately.

### money's iPad Toolbar Buttons Stay Dark After a Theme Switch

On the iPad, landscape and light, the toolbar's glass buttons stay dark
on tabs first shown after a theme switch. Two attempts were reverted
(ui/swift/Sources/ArkUI/Appearance.swift says what was tried). It needs a
simulator, not CI round trips.

### harken's iPad Walk Stalled Once

harken's iPad screenshot walk once timed out on an album cover loading.
It did not recur, and its cause is unknown.

### The Watch's Status Line and Progress Colour

The watch's status line is cut off ("Can't Reach Player, 1 Pe…"), and the
progress bar's accent comes out peach in the watch's 64 colours.
