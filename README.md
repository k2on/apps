# apps

ArkDB — a local-first sync engine whose domain is written once and runs
natively in Rust, Swift and Kotlin — and harken, the first app on it.

- `docs/arkdb.md` — the design: a survey of Petros and how harken used it,
  what compiling the domain to FFI cost, and the architecture of the
  successor — a domain authored through builders, emitted as one IR,
  compiled to native source for every language; an intent log per scope
  with facts retained beside it; snapshots; authority as a role; and a
  conformance suite every runtime is held to.
- `spec/` — the specification, as a Haskell program: one module per
  section, each normative, the pinned Unicode tables, `arkc` (verify,
  print, hash, check, gen), and the vectors `ark-vectors` emits.
  `spec/README.md` is the index; `spec/GENERATED.md` is the contract every
  generated file and every runtime keeps.
- `rust/` — `ark`, the Rust runtime, and `ark-builder`, the frontend a
  domain is authored through.
- `swift/` — `ArkDB`, the Swift runtime, and `ArkDBClient`, a session over it.
- `kotlin/` — `ark-runtime` and `ark-client`, the same two in Kotlin.
- `harken/` — the app: `domain/`, `server/`, `desktop/`, `ios/`, `android/`.
  `harken/README.md` says what each is and how to run them together.

Every runtime passes the same vectors; every generated file is one emitter's
output in three spellings; and one command holds all of it:

    nix flake check                 # the vectors, the three runtimes, the Rust
                                    # workspace, harken's module and generated code
    nix build .#harken-server       # or harken-desktop, harken-domain, arkdb-swift,
                                    # arkdb-kotlin, ark-spec
    nix run .#arkc -- verify harken/domain/harken.ark
    nix develop .#spec              # or .#rust, .#swift, .#kotlin

What is not verified from here: the iOS and Android apps have not been
built for or run on a device — the runtimes, the client libraries and the
generated domain under them have, and each app's `README.md` says exactly
where the seen part ends.
