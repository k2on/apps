# harken-desktop

A terminal peer for harken on ArkDB: ratatui over the `ark` runtime and the
Rust that `arkc gen rust` writes from `harken.ark`. It holds the `library`
and `playlists` scopes whole, shows the library and the playlists, and adds
to and removes from a playlist with the keyboard — against a server, or
alone with no network at all.

## Running it

    cd rust && nix develop ../#rust -c cargo build -p harken-desktop
    target/debug/harken-desktop                                  # alone
    target/debug/harken-desktop --server ws://127.0.0.1:8787/sync --user alice

    --server URL   the sync server (`harken-server` listens on 127.0.0.1:8787
                   and speaks on /sync); without one the peer is its own authority
    --user NAME    who you are; dev auth makes a name a login (default $USER)
    --data DIR     the database (default $XDG_DATA_HOME/harken-desktop/<user>)
    --module PATH  a .ark to load instead of the one the generated code embeds;
                   it must be the module the build was generated from

Take two peers to one server, kill the server or one peer's network, add to a
playlist on both sides, bring it back: the offline add lands *after* what
arrived meanwhile, because `add_to_playlist` reads `MAX(pos) + 1` when it
is applied and not when it is typed.

## Keys

Two panes. The left one is `Library` and then every playlist; the cursor
there decides what the right pane **shows**. `Enter` on a playlist makes it
the **target** — marked `*` — that `a` adds to, so the way to fill a
playlist is: `Enter` on it, `k` up to `Library`, `Tab`, `a` on a track.

| key | what |
|---|---|
| `j` `k` `↓` `↑` `g` `G` | move the cursor in the focused pane |
| `Tab` `h` `l` | switch panes |
| `Enter` | left pane: make this playlist the target (again: no target); right pane: back to the left |
| `n` | new playlist: type a name, `Enter` (`Esc` cancels) → `create_playlist` |
| `a` | on a library row: `add_to_playlist(target, track)` |
| `d` | on a playlist's item: `remove_from_playlist` |
| `t` | alone only: `title \| artist` → `add_track`, by intent through the module's own closure (see below) |
| `q` | quit |

The status line: the user, the server or `alone`, `linked`/`unlinked`
(or `denied: …`), the cursor per scope (`library@3 playlists@5`), how many
intents are pending, and the last refusal — the peer's own verdict on a
mutation, or the server's `Reject`.

## What path a mutation takes

Authoring runs the **generated code**: `domain.rs` includes
`harken/domain/gen/rust/harken_gen.rs` from one place, and every `n`, `a`
and `d` builds its arguments with the typed `create_playlist_args` /
`add_to_playlist_args` / `remove_from_playlist_args` and runs the generated
body as one transaction over the optimistic store, through
`Client::mutate_with` → `Replica::mutate_with` → `ark::gen::run_mutator`.
The entry it records names the closure's hash from `gen::FUNCTIONS`, with
the autos the module's function declares — a fresh random 16-byte id per
`NewId`, the clock per `Now` — drawn once here and frozen.

Every **replay** runs the interpreter: the rebase after a confirmed entry
lands, an authority sequencing an entry, a peer receiving one, all go
through `apply_closure` over the closure the module carries. So a mutation
is applied twice in its life, once by each path, which is the property the
`eval/` vectors hold; and in a debug build every authoring call also runs
the interpreter beside the generated body over copies of the store and
asserts the same verdict, the same changes and the same store
(`Peer::check_agreement`; `Peer::agreement_checks` counts them).

Reads run the generated queries — `gen::library`, `gen::playlists`,
`gen::playlist_items` — over each scope's optimistic store, re-run whenever
`take_changes` reports movement. The screen joins items to tracks itself,
because the two live in different scopes and a query reads one store.

`add_track` is the scanner's: the client is generated with `--only` and
carries no code for it, and tracks arrive from the server as entries the
peer holds the closure for (the module bytes are the whole module) and so
replays. A peer alone has no scanner, so `t` authors `add_track` **by
intent through the interpreter** (`Peer::author_by_intent`) — the one
mutation here that does not go through generated code, offered only to a
peer alone.

`mutate_with` on `Replica` and `Client`, and `From<Value> for String` (what
lets the emitter's `Fault::refuse(Value::text(…))` compile), are additive
changes to `rust/ark`, each with a test.

## Persistence

One canonical-CBOR file per scope under `--data`: `library.cbor`,
`playlists.cbor`, each `{ confirmed, cursor, pending }` — the confirmed
store as `store_value`, the cursor, and the pending intents as the
protocol's entry frames. That is exactly what `Replica::open` takes;
the optimistic view is recomputed from it. Written whole to a temporary
name and renamed after every change that moved a cursor or the pending
list; a scope that never moved has no file, and an absent file is an
empty scope at cursor 0. Nothing else is durable — not the replica's own
log, not the module.

## Alone

With no `--server` the peer holds an `ark::peer::Authority` per scope and
runs `local_commit` after every mutation: the intent is sequenced, its facts
recorded, and the acknowledgement delivered, so nothing stays pending and
the view is the confirmed store — the serverless peer of `docs/arkdb.md`
§3.10, working with no network at all. Reopened, the authority is rebuilt
from the file: its log is the confirmed store as a snapshot at the cursor
(the horizon) with nothing above it, so sequencing continues from where it
stopped. What is not built is handing that scope to a server later
(adoption, §3.10) — a data directory made alone should stay alone, and one
made against a server should stay with it.

## Transport

`net.rs` is a WebSocket client on a thread of its own (blocking
`tungstenite`, a 50 ms read slice), speaking to the engine's `Client` over
two channels: `connected()` on connect, `take_outgoing` frames sent as
binary canonical CBOR, incoming frames decoded through `ServerMsg::from_value`
into `recv`, and a reconnect with a backoff from 500 ms to 30 s when the
socket drops. Pings are answered by the library. The engine never sees the
socket: the tests run the same `Peer` against an in-process
`ark::protocol::Server`, carrying frames by hand.

## Tests

    cd rust && nix develop ../#rust -c cargo test -p harken-desktop

`tests/headless.rs` runs the peer with no terminal and no socket: alone, it
seeds two tracks, makes a playlist, adds to it, is refused a blank name,
is reopened from its files with the same rows, cursors and state hashes,
and goes on sequencing; then two peers through one in-process server, where
A goes offline, B adds a track meanwhile, A adds one alone (at position 2,
all it can see), is restarted from disk still offline with the intent
pending, reconnects, and its item lands at position 3 on both peers with
every hash agreeing with the authority's.
