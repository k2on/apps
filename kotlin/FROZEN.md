# Frozen at spec v3

This tree is ArkDB for Kotlin (`ark-runtime`, with `dev.arkdb.authoring`, and
`ark-client`) as it stood at **spec version 3**, and it is
not built, tested or kept in step by anything in this repository now.
Spec v4 (`docs/plan-v4.md`) made every query a plan and made `rust/ark`
the specification; carrying Kotlin through that change would have meant
writing the plan algebra and its maintenance a second time while the
design was still moving, so the runtime, the vocabulary and the client
stop here, byte for byte, and come back when the spec settles.

Frozen with it, for the same reason and in the same state:

- `harken/domain/gen/kotlin` — `arkc gen kotlin`'s print of harken's v3
  domain, which the tests compiled and ran natively against the
  interpreter. There is no printer now (§18 left with the Haskell), and
  `harken/domain/harken.ark` is a v4 module.
- `harken/android` — the Compose app over `ark-client` and that print. It
  is the one frozen thing still built: `nix build .#harken-apk` assembles
  it (and `apk.yml` does on every push to `main`), because it reads no
  vectors and no v4 module. `kotlin/deps.json` stays beside it; nothing
  replays it now, and `harken/android/deps.json` is the graph the APK
  build uses.

## Where it was last green

**`4bacc8f`**, the last commit before the v4 work began. Found by
evaluating `checks.x86_64-linux.kotlin` at each commit and asking the store
whether that output exists — a check's output exists only if it built and
its runner passed:

- at `4bacc8f` the output is `/nix/store/ql2v3a7g…-arkdb-kotlin-0.1.0`,
  and it is in the store;
- the same store path, and so the same inputs, stands at every commit
  through `e18dae1`: nothing this check read (this tree, `spec/vectors`,
  `harken/domain/gen/kotlin`, `harken/domain/harken.ark`) moved in
  between;
- it moves at `e7b835f`, when `harken.ark` became a v4 module, and again
  at `561daf1`, when `spec/vectors` was regenerated at v4. Neither of those
  outputs was ever built. At the first the runner holds every procedure
  the phone carries to `harken.ark`'s hash, and every query's hash moved
  with v4; at the second the vectors carry spec-4 modules this runtime's
  verifier refuses. Both would fail, by reading; neither was run.

The last commit that touched this tree is `f2e12b2`. To run it as it
passed, build the check from that commit rather than from the working
tree, at the repository root:

    nix build "git+file://$PWD?rev=$(git rev-parse 4bacc8f)#checks.x86_64-linux.kotlin"

## What would bring it back

In the order the work would be done, each against `rust/ark` at the day
it is done:

1. **The vectors of that day.** `spec/vectors`, as `nix run .#vectors`
   writes them then; the runner in
   `ark-runtime/src/test/kotlin/dev/arkdb/conformance`
   walks the directories and must fail every `falsify/` case.
2. **IR v4**: `Function.plan`, the plan's wire form with its keys written
   only when present (`docs/plan-v4.md` §1.8), `normalize` over a plan's
   binders, and the verifier's plan rules (§1.10).
3. **The plan algebra** (§1.3) and **`pull`** as the one evaluator of a
   plan — source, filter, lookups, related plans, having, projection,
   order, limit — recording the dependencies each entry's subtree read,
   which is what maintenance is routed by.
4. **The projection expression evaluator**: `Eval`'s expressions under a
   binder environment, with the list functions over bound lists, and no
   read inside a plan.
5. **`push_all`** (§1.5): entries indexed by key and by plan-node
   dependency, a settle's changes rebuilding the entries they touch against
   the final store, patches under a limit window, and the contract that
   the answer equals a fresh hydrate.
6. **The query builder** (§1.9): `each`, `get`/`by`, `group_by`,
   `having`, `sort_by`, `map`, emit-only, with a query's closure run once
   in a plan context.
7. **harken's phone domain again.** With no `arkc gen`, it is written by
   hand against the v4 vocabulary (or a printer comes back first), and its
   hashes are held to `harken.ark` as before.

Nothing here has been attempted.
