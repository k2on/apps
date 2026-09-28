# Plan: all of harken on ArkDB, and code every app shares

The request: port all of harken — the iced desktop client and its web build —
into `k2on/apps` on ArkDB; make `arkui`, the common iced components and the
vim keyboard logic; and have every app in this repository share the db, the
ui and the other shared code.

This file is the contract between the sessions and agents doing it. Read
`spec/AUTHORING.md` (how a domain is written), `docs/plan-authoring-v2.md`
(how the last port went) and the old harken's `CLAUDE.md`
(`/home/user/harken/CLAUDE.md`) — the last one describes nearly every
behaviour of the client being ported, and the reason for it. The old
harken repository at `/home/user/harken` and the engine at `/home/user/petros`
are **read-only references**: nothing is ever committed there.

## What "all of harken" means here

- **The domain**: every table of `harken/domain/schema.sql` (media, album,
  person, work, movement, recording, credit, song, playlist, playlist_item),
  every mutation of `domain/src/functions.rs` (`add_song`, `describe_work`,
  `describe_recording`, `describe_person`, `credit_recording`,
  `create_playlist`, `add_to_playlist`, `add_all_to_playlist`,
  `remove_from_playlist`, `remove_media`), every query the client reads
  (`library`, `albums`, `artists`, `track_details`, `album`, `artist`,
  `composers`, `works`, `work`, `recordings`, `credits`, `recording`,
  `playlists`, `playlists_of`, `playlist`), and the listening-session
  protocol (`domain/src/listening.rs`) — written in the ArkDB vocabulary.
- **The client**: `harken/iced/` — the desktop window and the same crate
  compiled to wasm for a browser, with the `demo` build (seeded library, no
  server) that GitHub Pages publishes. It replaces the ratatui
  `harken/desktop` and the TypeScript `harken/web`.
- **The server features the client talks to**: the scanner that reads tags
  and authors `add_song`, `/media`, the listening session relayed over live
  rooms (the device picker), the Home Assistant bridge (speakers as devices),
  sign-in (dev auth and OpenID Connect, what petros-auth does), and serving
  the web build with the caching rules the old CLAUDE.md explains.
- **The phones** keep working on the new domain: their Swift and Kotlin are
  `arkc gen … --only` of it, and their bridges follow the renamed tables.

## Layout: shared code, and apps that use it

```
rust/                       the shared Rust crates (one cargo workspace; apps join it)
  ark/                      the db: engine, authoring vocabulary (exists)
  ark-client/               a Rust peer: persistence, transport, live rooms,
                            maintained views, sign-in state — desktop and wasm
  ark-server/               axum: authorities per scope, persistence, live-room
                            relay, auth routes, /media, web serving
  ark-auth/                 who a peer is: sessions, OIDC, login/exchange, the
                            desktop's loopback flow and the page's redirect flow
  arkui/                    the ui: iced components, the vim keyboard, theming,
                            the glyph table and icon widget, routing
swift/, kotlin/             the shared phone runtimes (exist)
harken/                     an app: domain/, server/, iced/, ios/, android/
```

An app never copies shared code; if two apps would, it belongs in a shared
crate. What is harken's alone stays under `harken/`: its domain, its seed
data, its palette (the gold) and its screens.

## Phases and owners

Every agent owns paths; nobody edits another's paths without asking the
coordinator (the main session) first.

**Phase 1 (parallel)**
- **A — `rust/arkui`**: from `harken/iced/src/{vim,style,palette,icon,glyphs,art,covers,route}.rs`
  and the generic parts of `main.rs` (panels, menus, pickers, submenus,
  table rows/cells/headings, card grids, text fitting, overlays/backdrops,
  focus), made domain-free, with tests (the old ones ported). Owns `rust/arkui/`.
- **B — the domain**: `harken/domain/` rewritten as all of harken's domain;
  whatever `rust/ark/src/authoring/` needs to express it; its tests (the old
  `read_model.rs`, `converge.rs`, the agreement test). Owns `harken/domain/`,
  `rust/ark/src/authoring/`. Deletes `harken/desktop/` and `harken/web/`
  (replaced by `harken/iced`) and keeps `harken/server` compiling with the
  smallest edit that does it.
- **C — the shared runtime crates**: `rust/ark-client`, `rust/ark-server`,
  `rust/ark-auth`, extracted from `harken/{server,desktop,web}` and the
  petros equivalents (`petros::Client`, `transport::{ws,web}`, `petros-axum`,
  `petros-auth`), tested against the demo module (`spec/AUTHORING.md`
  Appendix B), not harken. Owns those three crates. Does not edit `harken/`.
- **Coordinator**: `spec/` (the printers learn whatever the vocabulary
  gains), `flake.nix`, docs, merges.

**Phase 2 (parallel, after phase 1 lands)**
- **D — `harken/iced`**: the client, on `arkui`, `ark-client` and the domain;
  desktop and wasm; the demo; its tests (the old ones ported).
- **E — `harken/server`**: on `ark-server` and `ark-auth`; scanner (lofty),
  listening desk, Home Assistant bridge, NixOS module; its tests.
- **F — the phones**: `harken/domain/gen/{swift,kotlin}` reprinted, the iOS
  bridge and the Android model on the new tables; Swift and Kotlin suites
  green; `nix build .#harken-apk`.

**Phase 3 — coordinator**: `flake.nix` (`harken-web` is the iced demo —
the Pages workflow builds that attribute and cannot be edited from here;
`harken-iced`, `harken-server`, checks), `nix flake check`, docs, push.

Icons and the typeface are out of scope (the user's call): arkui carries
harken's existing generated glyph table as it is, with no vendored icon set,
no normalisation and no geometry test; text is iced's bundled face (the
`fira-sans` feature, because a browser with no embedded font draws no text).

## Rules every agent keeps

- **Git**: commit your own paths only — `git add -- <paths>` then
  `git commit -m "…" -- <paths>` — and **never** push, reset, checkout,
  stash, rebase or clean; other agents' uncommitted work shares the tree.
  Retry on `index.lock`. Every commit message ends with exactly:

      Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
      Claude-Session: https://claude.ai/code/session_014Nz5smRBnkQqoe6xuVGUvk

  No model names anywhere else, and no personal names or emails anywhere.
- **Nix for everything**: `export PATH=/nix/var/nix/profiles/default/bin:$PATH
  NIX_SSL_CERT_FILE=/root/.ccr/ca-bundle.crt`, then `nix develop .#rust -c …`
  (from `/home/user/apps`). The disk is a fixed allowance: `rust/target` is
  shared by everyone; do not create per-agent target directories; if the
  disk fills, say so rather than deleting others' files.
- **No Rust macros in a domain** (the vocabulary is plain functions and
  closures), no wasm-interpreted domains, no SQL.
- **Falsify every new test once** by breaking what it checks.
- **Say what is not verified** (a window nobody saw, a phone nobody ran).
- When the contract does not answer a question, ask the coordinator with a
  concrete proposal; the answer goes into `spec/AUTHORING.md` or this file.
