# apps

ArkDB — a local-first sync engine whose domain is written once, in Rust,
and emitted as one IR every peer applies — and harken, the first app on
it.

- `docs/arkdb.md` — the design: a survey of Petros and how harken used it,
  what compiling the domain to FFI cost, and the architecture of the
  successor — a domain written once and emitted as one IR; one intent log
  with facts retained beside it; snapshots; authority as a role; views
  maintained from a query's plan; and a conformance suite every runtime is
  held to. `docs/plan-v4.md` is the design of the current spec version:
  every query a plan, and Rust the specification.
- `rust/` — `ark`, the runtime and the specification (`spec/README.md`
  indexes its modules by section), with `ark::authoring`, the vocabulary a
  domain is written in, and three binaries: `ark-vectors`, `arkc` (verify,
  hash, check, vectors) and `gen-unicode`. Beside it, what every app
  shares: `ark-client` (a peer: persistence, the link, live rooms,
  maintained views, signing in later), `ark-server` (the authority over a
  socket, live-room relay, the web build), `ark-auth` (dev auth and OpenID
  Connect) and `arkui` (the iced components, the vim keyboard, theming).
- `spec/` — the conformance vectors `rust/ark` writes and reads back, the
  index of the specification (`spec/README.md`), and
  `spec/AUTHORING.md`, the contract a domain is written against: routers
  and middleware like tRPC's, input schemas whose checks are also a form's
  validation, and a query that is a plan.
- `harken/` — the app: `domain/`, `server/`, `iced/` (desktop and browser).
  `harken/README.md` says what each is and how to run them together; the
  browser demo is published at <https://k2on.github.io/apps/harken/>.
- **Frozen at spec v3**: `swift/` and `kotlin/`, the Swift and Kotlin
  runtimes and vocabularies, with harken's `domain/gen/{swift,kotlin}`,
  `ios/` and `android/`. They stay in the tree unchanged and out of
  `nix flake check` until the spec settles; `swift/FROZEN.md` and
  `kotlin/FROZEN.md` say where they were last green and what would bring
  them back. The Android app is still assembled by `nix build
  .#harken-apk`, over the frozen Kotlin.

One command holds all of it:

    nix flake check                 # the vectors are what rust/ark writes; fmt,
                                    # clippy and every test of the Rust workspace;
                                    # harken.ark is what harken's domain emits and
                                    # verifies; the NixOS module
    nix build .#harken-server       # or harken-iced, harken-web, harken-web-server,
                                    # harken-domain, harken-apk, ark
    nix run .#arkc -- verify harken/domain/harken.ark
    nix run .#vectors -- spec/vectors
    nix develop .#rust

What is not verified from here: no desktop window has been opened, and the
browser client has been drawn in headless Chromium only. The frozen phone
apps have not been run on a device; the Android one is assembled by nix, so
its code compiles, and of the iOS one only the bridge ever met a compiler.
Each app's `README.md` says exactly where the seen part ends.
