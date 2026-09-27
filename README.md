# apps

harken's native clients, and the design of the engine they will run on.

- `docs/arkdb.md` — ArkDB: a survey of Petros and how harken uses it, what
  compiling the domain to FFI costs, and the architecture of the successor —
  a domain authored through builders in Rust, Swift or Kotlin, emitted as
  one IR, compiled to native source for every language; an intent log per
  scope with facts retained beside it; snapshots; authority as a role; and a
  conformance suite every runtime is held to.
- `spec/` — the ArkDB specification, as a Haskell program: the value model,
  canonical encoding, schema, IR, store, standard library with pinned
  Unicode tables, evaluator, hashes and verifier, each normative, with the
  vectors `ark-vectors` emits for every other runtime to be held to.
  `spec/README.md` is the index.
