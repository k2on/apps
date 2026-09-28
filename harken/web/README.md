# harken-web

harken's browser peer: the `ark` runtime compiled to wasm, holding the
`library` and `playlists` scopes as an ordinary `ark::protocol::Client`,
with a small page over it. It is a demo peer, not a product — one Rust file,
one TypeScript file, a page and a stylesheet — and it is exact: the same
replica the desktop and the phones are, in a tab. Published at
<https://k2on.github.io/apps/harken/> by `.github/workflows/pages.yml`.

## How it applies entries today

The crate embeds `harken/domain/harken.ark` (`include_bytes!`) and applies
**every** entry through the interpreter over the module's closures —
`ark::hash::closures` for the bodies, `ark::eval::apply_closure` inside the
engine's `Replica` for every replay and rebase — which is exactly how the
server applies them. Authoring is by name through that same closure, the
way the desktop's `author_by_intent` does it: `mutate("add_to_playlist", …)`
finds the closure whose function has that name, draws the autos (`NewId`
from `crypto.getRandomValues`, `Now` from `Date.now()`), applies it to the
optimistic view, and records the entry under the closure's hash. Queries run
through the interpreter too, over the optimistic views.

So nothing is generated for this peer at all: a module with a new verb is a
rebuild away, with no code to write. What it costs is speed nobody notices
at harken's size.

**The follow-up is switching authoring to native procedures** — the
`Native` half of `spec/AUTHORING.md` §3 — once the domain lands on the new
authoring API (`harken/domain` is being rewritten against it). The engine's
`Client::mutate_with` is already the seam, and the desktop uses it; replay
stays interpreted either way, since what the log names is the closure.

Alone (no server URL) the peer is an `ark::peer::Authority` for each of its
scopes and sequences its own intents with `local_commit`, as the desktop
does: nothing stays pending, and the cursor moves. There is no scanner
alone, so the library pane offers **add demo tracks**, which authors five
`add_track` entries by intent, as the scanner would.

## The JS API (`harken_web.js`, wasm-bindgen `--target web`)

```
init(): Promise                                       load the wasm (default export)
scopes(): string[]                                    the module's scopes, before a peer exists
Peer.open(user, server | undefined, load): Peer       load(scope) → Uint8Array | undefined
peer.mutate(name, inputJson): string | undefined      the verdict: undefined = accepted, else the refusal
peer.query(name, inputJson): string                   the result, as JSON
peer.frame(bytes: Uint8Array)                         a binary frame from the server, in
peer.takeOutgoing(): Uint8Array[]                     frames to send, oldest first
peer.connected() / peer.disconnected()                the socket opened / closed
peer.tick()                                           housekeeping, on any cadence (alone: sequence)
peer.takeChanges(): boolean                           did any view move since last asked
peer.persist(): [scope, Uint8Array][]                 scopes that moved, to store; handed back to load
peer.status(): string                                 JSON: user, server, alone, linked, denied,
                                                      cursors {scope: seq}, pending, diverged,
                                                      lastRefusal, module (its hash)
peer.free()
```

`mutate` and `query` throw only for a mistake in the call — an unknown
name, a query passed to `mutate`, an argument of the wrong shape. A refusal
is a verdict, returned as text.

JSON ↔ `Value`: text is a string; an int is a number, or a decimal string
beyond 2⁵³ (either is accepted inbound); an id is its `8-4-4-4-12` text;
bool, null, list and struct are themselves; bytes are hex. Inbound
arguments are converted at the type the function declares, so `"x"` for an
`Id(playlist)` throws before anything is applied.

A persisted scope is the desktop's `storage.rs` shape — `{ confirmed,
cursor, pending }` as canonical CBOR — which is what `Replica::open` takes.

The page (`src/app.ts`) owns what the wasm cannot: the WebSocket (binary
frames, reconnect with backoff from half a second to thirty) and IndexedDB
(database `harken-web`, one store, keyed `user@where/scope`, where `where`
is the server URL or `alone` — a peer alone sequences its own log, and its
cursors mean nothing to a server, so the two are separate replicas). Every
URL is relative, so the same files serve at `/` or under `/apps/harken/`.

## Building and running

    nix build .#harken-web        # index.html app.js harken_web.js harken_web_bg.wasm style.css
    python3 -m http.server -d result 8080

Open <http://localhost:8080/>, type a name, press **alone** — or start a
server and press **connect** (the server box defaults to `/sync` beside
the page, or `ws://127.0.0.1:8787/sync` on GitHub Pages):

    cd rust && cargo run -p harken-server -- --module ../harken/domain/harken.ark --media /some/dir

Two tabs as two users show the sync; stopping the server, adding on both
sides and starting it again shows the rebase.

By hand, in the build's own shell (the workspace toolchain with the wasm
target, `wasm-bindgen` at the locked version, `wasm-opt`, `esbuild`, `tsc`):

    nix develop .#harken-web
    cd rust && cargo build --release -p harken-web --target wasm32-unknown-unknown && cd ..
    out=$(mktemp -d)
    wasm-bindgen --target web --out-dir $out --out-name harken_web \
        rust/target/wasm32-unknown-unknown/release/harken_web.wasm
    cp -r harken/web $out/page && cp $out/harken_web.d.ts $out/harken_web.js $out/page/src/
    (cd $out/page && tsc -p .)    # the page calls exports the module has
    esbuild harken/web/src/app.ts --bundle --format=esm --external:./harken_web.js --outfile=$out/app.js
    cp harken/web/index.html harken/web/style.css $out/

**The wasm-bindgen CLI must match the `wasm-bindgen` crate in
`rust/Cargo.lock` exactly**, or the glue it writes does not match the
module's imports. `flake.nix` reads the version out of the lockfile and
builds the CLI from crates.io at it; its two hashes live in
`wasmBindgenHashes`, and when the crate moves the build stops naming the
version it has no hashes for — add an entry with fake hashes and nix prints
the right ones.

`nix flake check`'s Rust check runs this crate's unit tests natively; the
wasm build itself is `nix build .#harken-web`.

## What has been run

Against the engine and domain as committed before the authoring rewrite:
`nix build .#harken-web`; the wasm through Node, alone (create, refuse,
add, remove, persist, reopen) and against a real `harken-server` over a
WebSocket (two users, one offline, the offline add landing after the
other's on reconnect); and the page itself in headless Chromium, served
under `/apps/harken/` — alone with the demo tracks, a refusal shown inline,
add and remove, a reload restoring from IndexedDB, then connect to the
server. Not run: the GitHub workflows, and a real (non-headless) browser.
