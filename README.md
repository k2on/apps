# apps

ArkDB — a local-first sync engine whose domain is written once and runs
natively in Rust, Swift and Kotlin — and harken, the first app on it.

- `docs/arkdb.md` — the design: a survey of Petros and how harken used it,
  what compiling the domain to FFI cost, and the architecture of the
  successor — a domain written once, in one vocabulary spelt three ways,
  emitted as one IR and run natively by every peer; one intent log
  with facts retained beside it; snapshots; authority as a role;
  and a conformance suite every runtime is held to.
- `spec/` — the specification, as a Haskell program: one module per
  section, each normative, the pinned Unicode tables, `arkc` (verify,
  print, hash, check, and `gen`/`roundtrip`, which print a module back as
  the source that emits it), and the vectors `ark-vectors` emits.
  `spec/README.md` is the index; `spec/AUTHORING.md` is the contract a
  domain is written against: routers and middleware like tRPC's, input
  schemas whose checks are also a form's validation, and one vocabulary in
  Rust, Swift and Kotlin that round-trips through the IR.
- `rust/` — `ark`, the Rust runtime, with `ark::authoring`, the vocabulary a
  domain is written in.
- `swift/` — `ArkDB`, the Swift runtime; `ArkAuthoring`, the vocabulary; and
  `ArkDBClient`, a session over them.
- `kotlin/` — `ark-runtime` (with `dev.arkdb.authoring`) and `ark-client`,
  the same in Kotlin.
- `rust/` also holds what every app shares: `ark-client` (a peer:
  persistence, the link, live rooms, maintained views, signing in later),
  `ark-server` (the authority over a socket, live-room relay, the web
  build), `ark-auth` (dev auth and OpenID Connect) and `arkui` (the iced
  components, the vim keyboard, theming).
- `harken/` — the app: `domain/`, `server/`, `iced/` (desktop and browser),
  `ios/`, `android/`. `harken/README.md` says what each is and how to run
  them together; the browser demo is published at
  <https://k2on.github.io/apps/harken/>.

Every runtime passes the same vectors and runs every procedure natively,
held to the interpreter; harken's domain is written once in Rust and its
Swift and Kotlin are `arkc`'s print of it; and one command holds all of it:

    nix flake check                 # the vectors, the three runtimes, the Rust
                                    # workspace, harken's module and the round
                                    # trip of its three domains
    nix build .#harken-server       # or harken-iced, harken-web, harken-web-server,
                                    # harken-domain, harken-apk, arkdb-swift,
                                    # arkdb-kotlin, ark-spec
    nix run .#arkc -- verify harken/domain/harken.ark
    nix develop .#spec              # or .#rust, .#swift, .#kotlin

What is not verified from here: neither phone app has been run on a device,
no desktop window has been opened, and the browser client has been drawn in
headless Chromium only. The Android
app is assembled by nix, so its code compiles. Of the iOS app, the printed
domain and the bridge the screens call through compile and run on Linux as
a test target; the SwiftUI views have not met a compiler. The runtimes, the
client libraries and harken's domain under all three have been exercised,
and each app's `README.md` says exactly where the seen part ends.
