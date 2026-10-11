# Frozen at spec v3, and brought forward

This tree was ArkDB for Kotlin at spec v3 — `ark-runtime` with the
authoring vocabulary `dev.arkdb.authoring`, and `ark-client` — frozen while
v4 moved (`docs/plan-v4.md`), with harken's printed v3 domain
(`harken/domain/gen/kotlin`) and the Compose app over both
(`harken/android`). It was last green at **`4bacc8f`**: its check's output
is in the store at every commit through `e18dae1`, and
`nix build "git+file://$PWD?rev=$(git rev-parse 4bacc8f)#checks.x86_64-linux.kotlin"`
from the repository root builds it as it passed.

`docs/plan-phones.md` brings it forward as a client:

- **round 2** (done): the core at v4 — values, codec, order, hashes,
  modules, the verifier, the interpreter, the store, plans and maintained
  views — held to `spec/vectors` by `checks.kotlin` (`README.md`). The
  authoring vocabulary, the `demo` and `selfdemo` sources that exercised
  it and the printed domain were deleted, not ported: a phone does not
  author modules;
- **round 3** (done): the client — replica and rebase, the protocol
  client, alone and its journal, storage byte for byte with the Rust's —
  held to every client vector and every trace of `packages.trace-corpus`,
  and `ark-client` back in the build as the v4 peer;
- **round 4**: the JVM peer in the fleet, and `harken/android` rewired to
  load `harken.ark`. Until then `harken-apk` is out of the flake (its
  derivation is as it stood at `2874274`), and
  `.github/workflows/apk.yml` still names it.
