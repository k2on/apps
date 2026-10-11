# Frozen at spec v3, and brought back

This tree was ArkDB for Swift at **spec version 3**, frozen while v4 moved
(`docs/plan-v4.md`) and last green at **`4bacc8f`** — `git show
4bacc8f:swift` is that runtime, its authoring vocabulary and its client, and

    nix build "git+file://$PWD?rev=$(git rev-parse 4bacc8f)#checks.x86_64-linux.swift"

builds the check as it passed. `docs/plan-phones.md` rounds 5–7 bring it to
v4 in place, as a client: the authoring vocabulary, the domains written in
it and `harken/domain/gen/swift` are deleted, not ported, and `README.md`
describes what is here now.
