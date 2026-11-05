#!/usr/bin/env bash
# An Android app's Paparazzi screenshots from the working tree, quickly: the
# same gradle, SDK, JDK, flags and dependencies as `<app>-android-screens`
# (nix develop gives that derivation's own environment, its dependencies
# served from its recorded cache, offline), but run in place rather than in
# a fresh sandbox, so gradle's build directories and its own cache (kept
# under ~/.cache/ark-shots) carry over from one run to the next and only
# what changed is compiled again. The nix build stays what CI and the
# screens branch use; this is for seeing a change in seconds.
#
#   nix/android-shots.sh <app> [gradle test filter...]
#
#   nix/android-shots.sh harken                       # every shot
#   nix/android-shots.sh harken --tests '*d1_player*' # one
#
# The PNGs land in <app>/android/app/src/test/snapshots/images (ignored by
# git), named <appid>_Screens_<test>_<state>-<theme>.png.
set -euo pipefail
app=${1:?which app}
shift
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
home=${XDG_CACHE_HOME:-$HOME/.cache}/ark-shots/$app
mkdir -p "$home"
shots="$root/$app/android/app/src/test/snapshots/images"
# shellcheck disable=SC2016 # the script is for the inner shell to expand
nix develop "$root#$app-android-screens" \
  --option extra-substituters https://k2on.cachix.org \
  --option extra-trusted-public-keys "k2on.cachix.org-1:oI4Db0uW5ER+s4Te/MrXSmjWUWkOc9rmKdzsgUDxQP8=" \
  --command bash -c '
    # The setup of the derivation: its hooks are shell functions, which a
    # command run in its environment does not inherit.
    # shellcheck disable=SC1091
    source "$stdenv/setup"
    set -eo pipefail  # not -u: the hooks read what may be unset
    export GRADLE_USER_HOME=$1
    shift
    # The configure hooks of the derivation: the dependency cache served as a
    # proxy, and the flags of gradle pointed at it.
    runHook preConfigure
    cd "$1/$2/android"
    shift 2
    runHook preBuild
    # Kept warm between runs is the build output, not the daemon: each run
    # has its own proxy, which a daemon could not follow.
    gradle --build-cache :app:recordPaparazziDebug "$@"
  ' _ "$home/gradle" "$root" "$app" "$@"
echo "$(find "$shots" -name '*.png' | wc -l) shots in $shots"
