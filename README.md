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
  indexes its modules by section): values as JSON and keys memcomparable,
  the schema and the store, the chained log, the one peer machine, the
  frames, a domain in plain Rust (derives, the `Domain` registry, the
  typed `Db`), and maintained views over typed plans. `ark-dev` is the
  development tools: the demo, the simulator over real peers in both
  roles, the trace recorder and player, the fuzzer (`ark-fuzz`) and the
  vector writer. Beside them, what every app shares:
  `ark-peer` (the one peer: a device that dials — storage, the link with
  redial, the views a UI drives and the functions it calls by name, the
  live relay, signing in later — and a server that listens — the
  connections routed, the log written before delivered, the sync socket,
  the admin page's listener, the web build), `ark-auth` (dev auth and OpenID Connect), `ark-explorer` (the
  admin page and the explorer over a replica), `arkui` (the iced
  components, the vim keyboard, theming) and `ark-ffi` (the peer for an
  app in another language: `ark-peer`'s app kit through UniFFI, every value
  its JSON; `ark-dev gen kotlin|swift` writes a domain's types and stubs
  for it).
- `spec/` — the conformance vectors `rust/ark` and `rust/ark-dev` write
  and read back, the index of the specification (`spec/README.md`),
  `spec/RUNNERS.md` (what a runtime in another language does with each
  vector) and `spec/AUTHORING.md`, writing a domain in Rust.
- `harken/` — the app: `domain/` (its domain, a crate of plain Rust),
  `server/` and `iced/` (desktop and browser), which link it, and the
  phones — `ffi/` (the kernel with the domain linked in, one native
  library) under `android/` (Compose) and `ios/` (SwiftUI), each calling
  the domain through the types `ark-dev gen` writes. `harken/README.md`
  says what each is and how to run them together; the browser demo is
  published at <https://k2on.github.io/apps/harken/>. The Kotlin and Swift
  runtimes the phones ran at v4 are gone (`docs/plan-phones.md`; their last
  green commit is `a983b3c`).

One command holds all of it:

    nix flake check                 # rust: fmt, clippy and every test of the Rust
                                    #   workspace (and harken-iced's demo feature);
                                    # vectors: spec/vectors is what the writer writes;
                                    # traces: spec/vectors/trace/ and a fuzzed trace
                                    #   corpus replay; fuzz-smoke: 25 fuzzed sessions;
                                    # versions: harken/server/tests/versions.rs over
                                    #   the pinned revisions (none pinned now);
                                    # harken-domain: the domain crate builds and
                                    #   its schema hash is the pinned one;
                                    # harken-module: the NixOS module evaluates;
                                    # ffi: ark-ffi and harken/ffi, their tests, and
                                    #   UniFFI's Kotlin and Swift for the library
    nix build .#harken-server       # or harken-iced, harken-web, harken-web-server,
                                    # harken-domain, ark-admin, ark, vectors, trace-corpus
    nix build .#harken-apk          # the phone: the kernel for three ABIs and the app
                                    # over it, its JVM tests run on the real kernel
    nix run .#fuzz -- 1000 1 traces
    nix develop .#rust

What is not verified from here: no desktop window has been opened, and the
browser client was drawn in headless Chromium under v4 and has not been
drawn on v5 (`docs/plan-v5.md`, D1). No phone has run either app: the
APK has not been installed, and the iOS app has not been built (it needs
a Mac). Each app's `README.md` says exactly where the
seen part ends.
