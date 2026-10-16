# The pinned previous revisions the fleet runs mixed against
# (docs/plan-db.md D1). Each is fetched by the flake input
# `harken-<name>` — the lockfile pins it — and its `harken-server` package
# (both binaries, the server and harken-peer) is what `checks.versions`
# and `packages.fleet-vm` run beside the current build. `rev` is written
# here as well as in the input's URL so that the two cannot drift: the
# flake refuses to evaluate when the locked revision is not this one.
#
# To add a release: append an entry, add the input `harken-<name>` to
# flake.nix with the same `rev`, run `nix flake lock`. The matrix grows by
# one: the fleet's version scenarios run once more, as `HARKEN_OLD_<n>_*`
# with `n` the entry's place here, counting from 1.
#
# The matrix is empty until a shipped revision is pinned: the two
# development revisions it held (v4-journal at abbf861, v4-rows at
# 71f7b0c) were dropped, so `checks.versions` runs only the scenarios that
# need no older binary, and fleet-vm leaves its mixed machines out.
[
]
