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
    app/        its domain (entries, handlers, the query), linked by both sides
    watch/      its screen, a staticlib for thumbv7m-none-eabi
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
| the relay's `LINK` | `Node::connect`: subscriptions, hello, every pending entry again |
| `connection_service` | the link down while the phone is away; the watch says `READY` again when it returns |
| the app's timer | `App::wake` at the time the app asked: for playback, `Anchor::next` (no tick) |
| AppMessage sent or failed | the next chunk, or the same one again |

After each, the host **settles**: it takes each space's changes and the
refusals from the node and hands them to the app (`App::update`), whose
queries (`Live::push`) and screen redraw only what changed. A change to
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

**The renderer** (`ui.rs`, the shim's screen) is a first cut: a fixed screen
of five text slots, a progress bar, a list of drawn rows in a `MenuLayer`
and an `ActionBarLayer` whose glyphs are painted into 1-bit bitmaps at
start, so the app has no resources. `Screen` keeps what it last showed and
tells the shim only what changed. The arkui backend proper comes later and
replaces it, writing an app's `(glance)` views as Pebble's layers:

| here | arkui |
|---|---|
| a text slot (`TextLayer`) | the text primitive, its font a token |
| the slots' fixed frames | a vertical stack and spacers, laid out by `[layout]` tokens |
| the progress bar (a drawn `Layer`) | a primitive (swatch with a fraction), or a control like the scrubber |
| the list (`MenuLayer`, drawn rows) | the list container, its rows a component of the app's |
| the action bar and its glyphs | a native control, its glyphs arkui's glyphs rasterised |

## The demo

Standing in for harken's now playing (`demo/app`):

- **Playback** is a session row (`PLAYBACK`) the player owns and sets whole:
  a track and an `Anchor` the watch extrapolates, redrawn once a second
  through `Anchor::next`.
- **The queue** is a shared table (`QUEUE`) keyed by place. The watch
  subscribes to its first five rows (`NEXT`) and shows three, a take over
  its own view kept live by its changes.
- **Select** toggles play: a session entry at the position now. **Down**
  skips: a shared `Pop` of the next track, pending until the server
  confirms it (kept through a restart), and a session entry playing it.
  Offline only the `Pop` is made: a session entry is neither kept nor sent
  again, and an upstream's hello does not say a node's own latest back to
  it, so one made with no link would show what the player never heard.
- A `Pop` of a track the player already took is refused (`GONE`), and the
  status line counts it.

## Building and running

With the Pebble SDK (4.44, `pebble-tool`), Rust and
`rustup target add thumbv7m-none-eabi`:

    cd demo && pebble build                 # cargo builds watch/ first
    cargo run --release -p now-playing-upstream    # from host/: ws://127.0.0.1:9010/
    pebble install --emulator emery --logs

The server takes lines on standard input: `toggle`, `skip`, `add TITLE`,
`offline` (closes every connection and refuses new ones, a phone that lost
the server), `online` and `state`. It logs every frame either way.

    cargo test -p ark-pebble -p now-playing  # the disk, the link, the domain

On a real phone the relay's upstream is the localStorage key `upstream`
(by default `ws://127.0.0.1:9010/`, the emulator's host); a phone reaching a
dev machine needs its LAN address there, and the app declares the
`LocalNetwork` permission for it.

## Measured in the emulator

On the Time 2's emulator (emery, SDK 4.44), a run of subscription, live
playback, pause and resume either side, skips online and offline, a
restart with two pending skips, a refusal, a reconnect and confirmations:

- **Image** 51.7 KB (with bss 52.6 KB), of the 128 KB an app gets for code
  and heap. By symbol: the kernel's own 11.8 KB, and about 4 KB more inlined
  into the host's callbacks (`Node::receive` in `chunk`, the store's B-tree
  in `Rows::put`); the host about 13 KB; `core`, `alloc` and the compiler's
  builtins 13.6 KB; the demo 2.4 KB; the C shim about 2.5 KB.
- **Heap** at most 10.0 KB in use, by PebbleOS's own count at exit, of
  78.4 KB: AppMessage's buffers 4.9 KB, and Rust at most 4.9 KB (the node,
  its stores and pending records, the query, the disk's page table).
- **Stack**: a watchapp gets 4 KB. Large values are kept on the heap
  (the disk's key map, log lines); the deepest Rust frames are the host's
  start (1.1 KB, once) and a chunk received (1 KB, `Node::receive` inlined).
- **Writes**: a flush after a confirmed row is three values; one with
  nothing changed writes none. The host's flushes took 0 to 5 ms in this
  emulator, against the probe's 13 ms a write: still to measure on a watch.

## Not yet

- Seen only in the emulator, never on a Pebble Time 2 or with a phone.
- The watch's clock is taken as the player's: an anchor's `wall` is the
  owner's, and no offset is measured yet.
- A row larger than a page (about 250 bytes with its key) is not kept.
- Keys let go by a flush are left as they are and used again, never deleted.
- Nothing of the host is in CI yet; the probe's `.pbw` is not either.
