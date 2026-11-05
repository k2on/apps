#!/usr/bin/env bash
# What an iPhone app's shots are taken from, as one hash (apps.md): the same
# hash, the same shots, so a Mac need not take them again. Said on Linux, by
# evaluating and building nothing, before any Mac is asked for
# (app-screens.yaml's `which`); kept beside the shots as
# <app>/ios/inputs, and compared with the hash a run is about to take.
#
#   ios-inputs.sh <scripts' checkout> <commit's checkout> <app>...
#
# Prints `<app> <hash>` for each, or `<app> -` for one it cannot say (the
# commit has no <app>-ios-simulator, or it does not evaluate): that one is
# taken. The hash is of:
#
# - the app's build at the commit, `<app>-ios-simulator`'s store path for
#   the Mac (aarch64-darwin), which moves only with what it is built from
#   (flake.nix's `only`: the app's ios/ and swift/ArkUI; its kernel's Rust;
#   the pinned Xcode);
# - of the scripts' checkout (main's, which a workflow_run runs), what the
#   shots are taken by: each package its <app>/screens/server.sh builds
#   (`.#<name>`, the server and its seed), its screens/ (shots.json and the
#   server's script), the picture its sign-in wears, and the iPhone's own
#   scripts here. Not the workflow, nor Android's or the desktop's scripts:
#   moving those takes no iPhone shots again.
set -uo pipefail
scripts=$(cd "$1" && pwd)
src=$(cd "$2" && pwd)
shift 2

# A path's git object in the scripts' checkout, or `none`.
object() { git -C "$scripts" rev-parse "HEAD:$1" 2>/dev/null || echo none; }
# A package's store path for the Mac, or `none` (and why, on stderr). The
# checkout by its path, as `.#` is: a shallow clone's, which a `git+file:`
# URL refuses.
out() {
  local err
  err=$(mktemp)
  nix eval --raw "$1#packages.aarch64-darwin.$2.outPath" 2> "$err" || { echo none; tail -n 3 "$err" >&2; }
  rm -f "$err"
}

shared=""
for p in .github/screens/app-ios.sh .github/screens/apps-ios.sh .github/screens/ios-build.sh \
  .github/screens/app-server.sh .github/screens/drive-ios .github/nix-xcode.sh; do
  shared="$shared$p $(object "$p")"$'\n'
done

for a in "$@"; do
  app=$(out "$src" "$a-ios-simulator")
  if [ "$app" = none ]; then
    echo "$a: the commit's $a-ios-simulator does not evaluate" >&2
    echo "$a -"
    continue
  fi
  said="app $app"$'\n'"$shared"
  said="$said$a/screens $(object "$a/screens")"$'\n'
  picture=$(jq -r '.server.picture // empty' "$scripts/$a/screens/shots.json" 2>/dev/null)
  if [ -n "$picture" ]; then said="$said$picture $(object "$picture")"$'\n'; fi
  if [ -f "$scripts/$a/screens/server.sh" ]; then
    while read -r p; do
      said="$said$p $(out "$scripts" "$p")"$'\n'
    done < <(grep -o '\.#[A-Za-z0-9_-]*' "$scripts/$a/screens/server.sh" | cut -c3- | sort -u)
  fi
  # What it is made of, in the log: what to read when an app is taken, or
  # not, against expectation.
  while read -r line; do echo "  $a: $line" >&2; done <<< "${said%$'\n'}"
  echo "$a $(printf '%s' "$said" | sha256sum | cut -c1-32)"
done
