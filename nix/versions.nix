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
[
  # The log gained its identity and the server its journal (Round 4).
  { name = "v4-journal"; rev = "abbf861feed6468baea238babc7baa8e1750d3bc"; }
  # The head when versions landed: the positional row (plan-perf R11).
  { name = "v4-rows"; rev = "71f7b0c40e81a226d980f52d20c259d73ee4e055"; }
]
