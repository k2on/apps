# Pebble host

The watch side of step 5 in `docs/north-star.md`: the new kernel
(`rust/kernel`) running as a full node on a Pebble Time 2, with its own
store, its own pending entries and its own queries, offline, and the phone
as its upstream.

```
host/
  rust/       ark-pebble: the host crate (no_std), linking ark-kernel
  c/          ark_pebble.c: the thin C shim (the SDK's loop, window, layers)
  demo/       a watchapp standing in for harken's now playing
    app/        its domain (entries, handlers, the query) and its screen's
                record, linked by both sides
    view/       its screen: a `#[view]` of arkui's, written out as Pebble's
                layers at build time (ui/docs/pebble.md)
    watch/      the watch's Rust, a staticlib for thumbv7m-none-eabi: the
                record worked out, the screen its build wrote
    src/pkjs/   the relay: a PebbleKit JS script piping frames to a WebSocket
  upstream/   a test server: the kernel's Authority over a WebSocket, and a
              player node beside it, standing in for harken's phone
```

## The host

One kernel `Node` per app, its stores `Rows` (the kernel's `Memory`, noting
its tables), set up from the disk at start and driven by PebbleOS's
callbacks, which the shim passes to Rust (`ark_on_*`):

| callback | becomes |
|---|---|
| a button | the app's entries (`App::button`), routed by the node: stepped, pushed or sent |
| AppMessage in | chunks of the upstream's frames, put back together, each `Down` into `Node::receive` |
| the relay's `LINK` | `Node::connect`: subscriptions, hello (with the watch's time), every pending entry again; `LINK 0` is `Node::disconnect` |
| `connection_service` | the link down while the phone is away (`Node::disconnect`); the watch says `READY` again when it returns |
| the app's timer | `App::wake` at the time the app asked: for playback, `Anchor::next` (no tick) |
| AppMessage sent or failed | the next chunk, or the same one again |

After each, the host **settles**: it sets its status cell (`STATUS`: the
link, the subscriptions, what waits and what was refused), lets the app set
its own (`App::settled`: the demo's time), and calls `Node::settle` with
the screen as its sink: each layer whose recorded reads changed is marked
dirty, and PebbleOS draws it again, pulling its words from its source as
it draws. A change to
what the disk keeps arms a flush 400 ms later, so a burst of frames is
written once; the host flushes at exit too.

**The store** (`disk.rs`). Persistent storage is 1 MiB per app with at most
256 bytes per value, no files, and about 13 ms a write in the emulator, so
the rows live in memory and the disk holds a copy in 256-byte pages: the
device and shared spaces' rows in path order, whole rows per page, each
page holding the rows from its first path to the next page's. A flush packs
every page again and writes only those whose bytes changed (a row changed:
its page, the directory and the superblock, three writes). Beside the pages
are the pending records in chunks, and a superblock with the origin, the
cursor, the actor and the count of entries made. Writes are copy on write,
committed by the superblock, so a watch that dies mid-flush comes back to
the flush before. The count is kept as a lease 32 ahead, flushed before it
is reached, so an entry id is never used twice even after a crash between a
push and a flush. The session and local spaces are memory only, as the
kernel says: playback comes back with the upstream's hello.

**The link** (`link.rs`). The kernel's frames are a byte stream, cut into
AppMessages the size of the outbox (1024 bytes here, of the 8200 allowed:
the buffers come out of the app heap) and put back together from the inbox
(2048) by the kernel's `Frames`. The relay numbers each connection it opens
upstream (its epoch) and every chunk carries it, so nothing of a connection
that has ended is read as part of the next. Frames made while the link is
down are dropped: the pending entries and subscriptions are said again at
the next connection.

```
watch -> phone  READY, INBOX, ORIGIN   at start, when the phone returns, on HELLO
                EPOCH, DATA            a chunk of the frames going up
phone -> watch  HELLO                  the script started: say READY
                LINK                   its connection upstream opened (epoch) or closed (0)
                EPOCH, DATA            a chunk of the frames coming down
```

**The screen** is arkui's (ui/docs/pebble.md). The demo's is a `#[view]`
(`demo/view`) of arkui's primitives and native parts: text, stacks, a
spacer, a glyph, a meter, a list and the action bar, the same view the
phones' generator and iced draw. The watch's build (`watch/build.rs`)
records it and ark-ui-gen's `pebble` emitter writes it out as no_std Rust:
the layers made once at frames worked out from the window and
ui/theme.toml's tokens (`rust/src/tokens.rs`, written by `nix run
.#update-theme`), a function for each layer written from the view's
`let`s over the node's sources (the host's status and time, the playback
row, the queue's watch) and a table binding each layer to its own, and
what each button sends. `ui.rs` is the toolkit it is written over: `Ui` paints
arkui's glyphs into 1-bit bitmaps, so the app has no resources, gives each
binding a kernel reader, and is the node's sink. The shim makes each layer
by id (a drawn text layer, a drawn meter, a `BitmapLayer`, a `MenuLayer`,
the `ActionBarLayer`) and, as PebbleOS draws one, asks Rust for its words
(`ark_pull`, `ark_pull_meter`) into a stack buffer: nothing a layer shows
is kept between draws, on either side.

## The demo

Standing in for harken's now playing (`demo/app`):

- **Playback** is a session row (`PLAYBACK`) the player owns and sets whole:
  a track and an `Anchor` the watch extrapolates, redrawn once a second
  through `Anchor::next`. Its key is the player's origin, so the kernel
  knows the row is the player's and the watch's entries for it are
  controls. The anchor's times are the server's: the player writes them at
  `Node::time`, and the watch extrapolates them at its clock corrected by
  the offset each hello measures (`Ctx::time`; about -0.3 s in the
  emulator).
- **The queue** is a shared table (`QUEUE`) keyed by place. The watch
  subscribes to its first five rows (`NEXT`) and shows three, a take over
  its own view kept live by its changes.
- **Select** toggles play: a session entry at the position now. **Down**
  skips: a shared `Pop` of the next track, pending until the server
  confirms it (kept through a restart), and a session entry playing it.
- **Live controls don't queue.** With no link the kernel refuses the
  watch's session entries for the player's row (`Refusal::OFFLINE`, which
  `Ctx::make` returns): nothing changes on the screen but the status line,
  "Can't Reach Player", until a hello is answered, and nothing fires
  later. Each hello's answer carries the player's latest (and the watch's
  own), so the screen agrees with the player once the link is back.
- A `Pop` of a track the player already took is refused (`GONE`), and the
  status line counts it.

## Building and running

In nix (`nix/pebble.nix`), from the repository's root, on Linux (x86_64 or
aarch64):

    nix build .#pebble-now-playing          # result/demo.pbw
    nix run .#pebble -- install --phone 192.168.1.42 result/demo.pbw
    nix run .#pebble -- logs --phone 192.168.1.42

The SDK's waf builds it with nixpkgs' arm-none-eabi GCC, and `watch/` is
built by cargo with the repository's watch toolchain (`watchRust`, for
thumbv7m-none-eabi); its build script, on the build machine, writes the
screen from `view/` first. `result/` also has the app's ELF (`emery.elf`), its
binary and the link's map. The phone's address is the Pebble app's LAN
developer connection (`ui/pebble/probe/README.md`, Install). CI builds it
and the probe, runs the tests below and fails the app past 96 KB with its
bss (`checks.pebble-size`, of the 128 KB an app gets):

    nix build .#checks.x86_64-linux.pebble-host .#checks.x86_64-linux.pebble-size

The nix pebble-tool has no emulator (that is the SDK's prebuilt
qemu-pebble). For the emulator, with the Pebble SDK (4.44, `pebble-tool`)
installed by hand, Rust and `rustup target add thumbv7m-none-eabi`:

    cargo fetch                             # once: the build is offline
    cd demo && pebble build                 # cargo builds watch/ (and its screen) first
    cargo run --release -p now-playing-upstream    # from host/: ws://127.0.0.1:9010/
    pebble install --emulator emery --logs  # or the nix build's result/demo.pbw

The server takes lines on standard input: `toggle`, `skip`, `add TITLE`,
`offline` (closes every connection and refuses new ones, a phone that lost
the server), `online` and `state`. It logs every frame either way.

    cargo test -p ark-pebble -p now-playing -p now-playing-view  # the disk, the link, the domain, the screen

On a real phone the relay's upstream is the localStorage key `upstream`
(by default `ws://127.0.0.1:9010/`, the emulator's host); a phone reaching a
dev machine needs its LAN address there, and the app declares the
`LocalNetwork` permission for it.

## Measured in the emulator

On the Time 2's emulator (emery, SDK 4.44), a run of subscription, live
playback, pause and resume either side, skips online and offline, a
restart with two pending skips, a refusal, a reconnect and confirmations:

- **Image** 59.8 KB (with bss 61.2 KB; 61,284 and 62,680 bytes as
  `checks.pebble-size` reads nix's build), of the 128 KB an app gets for
  code and heap. The hand-written screen arkui's replaced was 53.2 KB
  (54.1 KB) in the same build; before the session's owners, its hello and
  the clock, 51.7 KB. arkui's screen added about 7.3 KB with its bss, by
  symbol: the toolkit (`ui.rs`: the layer table, the glyphs' painting)
  1.7 KB, the shim's layers 1.2 KB and their text buffers 1.2 KB (less the
  old screen's 0.65 KB), the generated screen inlined into the demo's
  callbacks 1.3 KB, a meter's fraction in soft float 0.8 KB, and the
  record's strings about 0.9 KB.
  By symbol at 51.7 KB: the kernel's own 11.8 KB, and about 4 KB more
  inlined into the host's callbacks (`Node::receive` in `chunk`, the
  store's B-tree in `Rows::put`); the host about 13 KB; `core`, `alloc`
  and the compiler's builtins 13.6 KB; the demo 2.4 KB; the C shim about
  2.5 KB.
- **Heap** at most 10.4 KB in use, by PebbleOS's own count at exit, of
  75.5 KB: AppMessage's buffers 4.9 KB, and Rust at most 5.8 KB (the node,
  its stores, session rows and pending records, the query, the disk's page
  table). With arkui's screen, at most 12.4 KB of 66.6 KB in a run of play,
  pause, an offline skip and a reconnect (the list's rows, the record, the
  painted glyphs and what each layer last showed). With the shim's
  buffers the only copy of what each layer shows (no table of it in
  Rust), in the scratch walk below of play, pause, an offline refusal, an
  offline skip and a reconnect, the host's peak went from 11,884 to 11,328
  bytes and the image from 61,468 to 60,348 bytes (a local build; nix's
  arm GCC differs by about 180 bytes).
- **Stack**: a watchapp gets 4 KB. Large values are kept on the heap
  (the disk's key map, log lines); the deepest Rust frames are the host's
  start (1.1 KB, once) and a chunk received (1 KB, `Node::receive` inlined).
- **Writes**: a flush after a confirmed row is three values; one with
  nothing changed writes none. The host's flushes took 0 to 5 ms in this
  emulator, against the probe's 13 ms a write: still to measure on a watch.

## Not yet

- Seen only in the emulator, never on a Pebble Time 2 or with a phone.
- The offset is measured at each hello only, so a watch that stays
  connected for long drifts with its clock until the next.
- A row larger than a page (about 250 bytes with its key) is not kept.
- Keys let go by a flush are left as they are and used again, never deleted.
- The emulator is not in nix or CI: no screenshot walk of the watch yet.
- The screen animates nothing (ui/docs/pebble.md, Motion), and what arkui
  lays out on Pebble is only what ui/docs/pebble.md lists.
