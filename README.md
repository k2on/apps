# apps

ArkDB — a local-first sync engine whose domain is written once, in Rust,
as one typed language every peer runs — and harken, the first app on it.

- `docs/arkdb.md` — the architecture's history: a survey of Petros and
  how harken used it, what compiling the domain to FFI cost, and the
  successor's architecture. `docs/plan-v5.md` is the design of the current
  spec version, v5 — bytes under types, one language, one machine — and
  the record of how it landed and replaced v4 (Part 3 measures the two);
  where the two documents disagree, the plan is newer.
- `rust/` — `ark`, the runtime and the specification (`spec/README.md`
  indexes its modules by section): types and the order-preserving codec,
  the schema and the store, the chained log, the one peer machine, the
  frames, the language (IR, checker, interpreter, standard library,
  modules), maintained views, and `ark::author`, which the attribute
  macros of `ark-macros` (`#[ark::mutator]`, `#[ark::query]`, …,
  `ark::module!`) write a domain through. `ark-dev` is the development
  tools: the simulator over the real hub and client, the trace recorder
  and player, the fuzzer (`ark-fuzz`), the vector writer and `arkc`, which
  verifies module files. Beside them, what every app shares:
  `ark-client` (the client driver: storage, the link, maintained views,
  the live space, signing in later), `ark-server` (the hub on its thread,
  the sync socket, persistence, the admin page's listener, the web
  build), `ark-auth` (dev auth and OpenID Connect), `ark-explorer` (the
  admin page and the explorer over a replica) and `arkui` (the iced
  components, the vim keyboard, theming).
- `spec/` — the conformance vectors `rust/ark` and `rust/ark-dev` write
  and read back, the index of the specification (`spec/README.md`),
  `spec/RUNNERS.md` (what a runtime in another language does with each
  vector) and `spec/AUTHORING.md`, the Rust subset the macros accept, row
  by row.
- `harken/` — the app: `domain/` (written with `#[ark::*]`; `harken.ark`
  is the module file it writes), `server/`, `iced/` (desktop and browser).
  `harken/README.md` says what each is and how to run them together; the
  browser demo is published at <https://k2on.github.io/apps/harken/>.
- `kotlin/`, `swift/` — the Kotlin and Swift runtimes, and the phones on
  them (`harken/android`, `harken/ios`), **frozen at spec v4**
  (`kotlin/FROZEN.md`, `swift/FROZEN.md`: the last green commit, and what
  a v5 port carries instead). Their checks have left the flake.

One command holds all of it:

    nix flake check                 # rust: fmt, clippy and every test of the Rust
                                    #   workspace (and harken-iced's demo feature);
                                    # vectors: spec/vectors is what the writer writes;
                                    # traces: spec/vectors/trace/ and a fuzzed trace
                                    #   corpus replay; fuzz-smoke: 25 fuzzed sessions;
                                    # versions: harken/server/tests/versions.rs over
                                    #   the pinned revisions (none pinned now);
                                    # harken-domain: harken.ark is what the domain
                                    #   writes, and arkc verifies it;
                                    # harken-module: the NixOS module evaluates
    nix build .#harken-server       # or harken-iced, harken-web, harken-web-server,
                                    # harken-domain, ark-admin, ark, vectors, trace-corpus
    nix run .#arkc -- verify harken/domain/harken.ark
    nix run .#fuzz -- 1000 1 traces
    nix develop .#rust

What is not verified from here: no desktop window has been opened, and the
browser client was drawn in headless Chromium under v4 and has not been
drawn on v5 (`docs/plan-v5.md`, D1). The frozen phone apps have
not been run on a device. Each app's `README.md` says exactly where the
seen part ends.
