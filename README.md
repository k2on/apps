# apps

ArkDB — a local-first sync engine with one runtime, Rust, whose domain is
a crate of plain Rust every peer links — and harken, the first app on it.

- `rust/ark` — the kernel and the specification (`spec/README.md`
  indexes its modules): the value format (postcard by default, JSON as a
  feature; JSON is the human form) and memcomparable keys, the schema and
  the store, the chained log, the one sans-io peer machine (replica or
  authority; alone is an authority), the frames, a domain in plain Rust
  (derives, a `Domain` of functions by name, the typed `Db`) and views
  maintained per operator. `ark-macros` holds its derives and attributes.
- `rust/ark-peer` — the one peer over one disk: a device that dials
  (storage, the link with redial, the app kit a UI calls by name, the live
  relay) and a server that listens (the connections, the log written
  before delivered, the sync socket, the web build).
- `rust/ark-ffi` — `ark-peer`'s app kit through UniFFI for an app in
  another language; `arkdb gen kotlin|swift` writes a domain's types
  and stubs for it. The phones run the Rust kernel; there is no Kotlin or
  Swift runtime.
- `rust/arkdb` — the demo, the simulator over real peers in both roles,
  the trace recorder and player, the fuzzer (`arkdb fuzz`), the vector
  writer, `gen`. Beside it `ark-auth` (dev sign-in and OpenID Connect)
  and `arkui` (the iced components).
- `spec/` — the vectors the kernel is held to (the bytes, the machine, the
  frames, the views), `RUNNERS.md` (how `arkdb` runs them) and
  `AUTHORING.md` (writing a domain).
- `harken/` — the app: `domain/` (a crate of plain Rust), `server/` and
  `iced/` (desktop and browser), and the phones: `ffi/` (the kernel with
  the domain linked in, one native library) under `android/` (Compose)
  and `ios/` (SwiftUI), each over the types `arkdb gen` writes.
  `harken/README.md` says how to run them together.
- `docs/plan-v5.md` — the design, how it landed and what it measures
  (Part 3); `docs/arkdb.md` is the history it superseded.

`revelation/` is a second app, and not on ArkDB: an Android app in plain
Kotlin, no Rust and no native library, that is the Bible as one scroll
zoomed like a map. `revelation/README.md` says how to build and run it and
`docs/plan-bible.md` is its plan. Both apps' APKs are published through one
F-Droid repository.

One command holds all of it:

    nix flake check   # rust: every test of the workspace
                      #   (and harken-iced's demo feature);
                      # clippy: the workspace lint-clean, -D warnings;
                      # fmt: the workspace formatted;
                      # vectors: spec/vectors is what the writer writes;
                      # traces: a fuzzed corpus replays (spec/vectors/trace/ is rust's);
                      # fuzz-smoke: 25 fuzzed sessions;
                      # versions: harken/server/tests/versions.rs over the
                      #   pinned revisions (none pinned now);
                      # harken-domain: the domain builds, its schema hash pinned;
                      # harken-module: the NixOS module evaluates;
                      # ffi: ark-ffi and harken/ffi, their tests, UniFFI's
                      #   Kotlin and Swift, the generated types held to the domain;
                      # harken-ios-typecheck: the iOS app's non-UI Swift
                      #   type-checked on Linux against the library
    nix build .#harken-server   # or harken-iced, harken-web, harken-domain,
                                # ark, vectors, trace-corpus
    nix build .#harken-apk      # the kernel for three ABIs and the APK over it,
                                # its JVM tests run on the real kernel
    nix build .#revelation-apk  # Revelation's APK, its JVM tests run first;
                                # *-android-screens are either app's screens
    nix build .#fdroid-repo     # both APKs as the F-Droid repository, signed
                                # and checked; apps-fdroid is what CI runs
    nix run .#fuzz -- 1000 1 traces

What is verified: the kernel, the peer, harken's domain and server by
their tests and the vectors; the fuzzer's sessions converge and every
trace replays; the phone library's surface in-process; the APK builds and
its JVM tests pass over the kernel. What is not: no desktop window has
been opened, the browser build has not been drawn on v5, the APK has not
been installed on a phone, and the iOS app has not been built (it needs a
Mac; its Swift is type-checked only). Each app's `README.md` says where
the seen part ends.
