#!/usr/bin/env bash
# The screens jobs' tools, from the flake: read from k2on.cachix.org, which
# the nix step trusts by its key, so no cachix program is installed just to
# download. Only when one had to be built here (the flake changed it) is it
# pushed, with cachix, at the job's end.
#
#   tools.sh get <package…>   fetch or build them; their bin/ on the PATH
#   tools.sh push             push what `get` built, if it built anything
set -euo pipefail
list=${RUNNER_TEMP:-/tmp}/tools
case $1 in
  get)
    shift
    targets=()
    for p in "$@"; do targets+=(".#$p"); done
    if nix build --dry-run "${targets[@]}" 2>&1 | grep -q 'will be built'; then
      echo "not all in the cache: building, and pushing at the end"
      touch "$list.built"
    fi
    nix build --no-link --print-out-paths --print-build-logs "${targets[@]}" | tee -a "$list" |
      while read -r out; do
        [ -d "$out/bin" ] && echo "$out/bin" >> "${GITHUB_PATH:-/dev/null}"
      done
    ;;
  push)
    [ -f "$list.built" ] || { echo "all from the cache: nothing to push"; exit 0; }
    [ -n "${CACHIX_AUTH_TOKEN:-}" ] || { echo "::warning::built here, but no CACHIX_AUTH_TOKEN to push it with"; exit 0; }
    nix run --inputs-from . nixpkgs#cachix -- push k2on < "$list"
    ;;
esac
