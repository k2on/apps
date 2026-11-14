#!/usr/bin/env bash
# The release channels in CI (docs/releasing.md), the same for every app:
# what build.yaml builds and where its deploy puts it. Nothing here names
# an app (flake.nix's `checks.channels`); the apps, pages and APKs are the
# flake's `publish` list, and an app's channels its app.toml's.
#
#   stable     a `v*` tag: each app under its own id, its APK in
#              fdroid/repo, its page at /apps/<app>/.
#   unstable   a push to main: each app's `.unstable` twin, its APK in
#              fdroid/unstable/repo, its page at /apps/unstable/<app>/,
#              and the pages that have one channel only (the docs).
#
# Pages deploys the site whole, so each deploy lays out its own channel
# from this run and carries the other forward from the live site as it
# is: the other channel's pages by the manifest every page is published
# with (files.sha256), and its F-Droid repository by its index
# (apps-fdroid --keep --strict). A channel never published (no manifest,
# no index: 404) is simply absent; anything else that keeps it from being
# carried forward fails the deploy rather than take it off the site.
#
#   channels.sh channel                 the run's channel, from GITHUB_*
#                                       (a run by hand: CHANNEL_INPUT)
#   channels.sh plan PUBLISH.json       the run's channel, its pages and its
#                                       APKs (each the channel's package
#                                       and key), as GITHUB_OUTPUT lines
#   channels.sh version-name            the APKs' version name
#   channels.sh deploy SITE_DIR         the whole site in SITE_DIR: needs
#                                       CHANNEL, PAGES (every page of the
#                                       plan), PAGES_DIR (this run's
#                                       page-<name>/), APK_DIR (this run's
#                                       APKs) and SITE_URL (the live site)
set -euo pipefail

# The run's channel: a tag's is stable, main's push unstable; a pull
# request builds the stable apps (the screenshots install them), and a
# run by hand builds what it is asked (stable when not).
channel() {
  case "${GITHUB_REF:-}" in
    refs/tags/v*) echo stable; return ;;
  esac
  if [ "${GITHUB_EVENT_NAME:-}" = push ] && [ "${GITHUB_REF:-}" = refs/heads/main ]; then echo unstable; return; fi
  if [ "${GITHUB_EVENT_NAME:-}" = workflow_dispatch ]; then
    case "${CHANNEL_INPUT:-stable}" in
      stable | unstable) echo "${CHANNEL_INPUT:-stable}"; return ;;
      *) echo "channels.sh: no channel ${CHANNEL_INPUT}" >&2; exit 2 ;;
    esac
  fi
  echo stable
}

plan() {
  local publish=$1 c
  c=$(channel)
  echo "channel=$c"
  echo "pages=$(jq -c --arg c "$c" '[.pages[] | select(.channel == $c)]' "$publish")"
  echo "all-pages=$(jq -c '.pages' "$publish")"
  # Each APK as the channel builds it: the unstable twin's package and key
  # in place of the app's own, and an app not in the channel left out.
  echo "apks=$(jq -c --arg c "$c" '[.apks[]
    | if $c == "unstable" then select(.unstable.package) | .package = .unstable.package | .key = .unstable.key
      else select(.stable) end
    | del(.stable, .unstable)]' "$publish")"
}

# A tag's own version (`v1.2` is 1.2); anything else the last tag's,
# marked unstable with its commit: `<last tag>-unstable+<commit>`. The
# last tag is the highest, asked of the remote, not of a shallow
# checkout's history.
version_name() {
  if [[ "${GITHUB_REF:-}" == refs/tags/v* ]]; then
    echo "${GITHUB_REF_NAME#v}"
    return
  fi
  local last
  last=$(git ls-remote --tags --refs origin 'v*' | sed 's|.*refs/tags/v||' | sort -V | tail -n 1)
  echo "${last:-0.0}-unstable+$(git rev-parse --short=7 HEAD)"
}

# A published page's manifest: each file's sha256 and path.
manifest() {
  local listed
  listed=$(mktemp)
  (cd "$1" && find . -type f ! -name files.sha256 -print0 | sort -z | xargs -0 -r sha256sum) > "$listed"
  mv "$listed" "$1/files.sha256"
  chmod 644 "$1/files.sha256"
}

# A page as the live site has it, by its manifest, each file checked:
# exit 3 when the live site has no such page (its manifest 404s).
keep_page() {
  local url=$1 to=$2 status digest path listed
  listed=$(mktemp)
  status=$(curl -sSL --max-time 60 --retry 2 -w '%{http_code}' -o "$listed" "$url/files.sha256") || status=000
  if [ "$status" = 404 ]; then rm -f "$listed"; return 3; fi
  [ "$status" = 200 ] || { echo "channels.sh: $url/files.sha256 did not answer ($status)" >&2; return 1; }
  mkdir -p "$to"
  mv "$listed" "$to/files.sha256"
  while read -r digest path; do
    # A path from a file someone else served, before it is a path here.
    if ! [[ "$path" =~ ^\./[A-Za-z0-9._@+/-]+$ ]] || [[ "$path" == *..* ]] || ! [[ "$digest" =~ ^[0-9a-f]{64}$ ]]; then
      echo "channels.sh: $url/files.sha256 lists '$path', which is no path of a page" >&2
      return 1
    fi
    mkdir -p "$(dirname "$to/$path")"
    curl -fsSL --max-time 300 --retry 2 -o "$to/$path" "$url/${path#./}" || { echo "channels.sh: $url/${path#./} would not download" >&2; return 1; }
    echo "$digest  $to/$path" | sha256sum -c --quiet || return 1
  done < "$to/files.sha256"
}

repo_path() {
  case $1 in
    stable) echo fdroid/repo ;;
    unstable) echo fdroid/unstable/repo ;;
  esac
}

deploy() {
  local site=$1 name path c status
  : "${CHANNEL:?}" "${PAGES:?}" "${PAGES_DIR:?}" "${SITE_URL:?}"
  mkdir -p "$site"
  echo "channels.sh: deploying the $CHANNEL channel; the other is carried forward from $SITE_URL"

  # The pages: this channel's from this run, the other's from the live site.
  while IFS=$'\t' read -r name path c; do
    if [ "$c" = "$CHANNEL" ]; then
      mkdir -p "$(dirname "$site/$path")"
      cp -r "$PAGES_DIR/page-$name" "$site/$path"
      chmod -R u+w "$site/$path"
      echo "channels.sh: $path/ ($c) from this run"
    else
      status=0
      keep_page "$SITE_URL/$path" "$site/$path" || status=$?
      case $status in
        0) echo "channels.sh: $path/ ($c) carried forward" ;;
        3) echo "::notice::$path/ ($c) has never been published; it stays off the site until the $c channel publishes it." ;;
        *) echo "::error::$path/ ($c) could not be carried forward from $SITE_URL; deploying without it would take it off the site."; exit 1 ;;
      esac
    fi
    [ ! -d "$site/$path" ] || manifest "$site/$path"
  done < <(jq -r '.[] | [.name, .path, .channel] | @tsv' <<< "$PAGES")

  # The F-Droid repositories (nix/fdroid.nix): this channel's over this
  # run's APKs and those it already offered; the other's as it was. An APK
  # of the wrong channel is refused by apps-fdroid itself.
  shopt -s nullglob
  local apks=("${APK_DIR:-/nonexistent}"/*.apk)
  shopt -u nullglob
  if [ -z "${FDROID_KEYSTORE:-}" ]; then
    echo "channels.sh: signing the F-Droid indexes with the committed key; FDROID_KEYSTORE is not set."
  fi
  local fdroid=(nix run .#apps-fdroid --)
  for c in stable unstable; do
    path=$(repo_path "$c")
    status=0
    if [ "$c" = "$CHANNEL" ]; then
      [ ${#apks[@]} -gt 0 ] || echo "::notice::No new APK in this run; the $c repository offers what it already had."
      "${fdroid[@]}" repo --channel "$c" --keep "$SITE_URL/$path" "$RUNNER_TEMP/fdroid-$c" ${apks[@]+"${apks[@]}"} || status=$?
      if [ "$status" != 0 ] && [ "$status" != 3 ]; then
        echo "::warning::The $c repository would not build over this run's APKs; carrying it forward as it was."
        rm -rf "$RUNNER_TEMP/fdroid-$c"
        status=0
        "${fdroid[@]}" repo --channel "$c" --keep "$SITE_URL/$path" --strict "$RUNNER_TEMP/fdroid-$c" || status=$?
      fi
    else
      "${fdroid[@]}" repo --channel "$c" --keep "$SITE_URL/$path" --strict "$RUNNER_TEMP/fdroid-$c" || status=$?
    fi
    case $status in
      0) mkdir -p "$(dirname "$site/$path")" && cp -r "$RUNNER_TEMP/fdroid-$c" "$site/$path" ;;
      3) echo "::notice::The $c F-Droid repository has nothing to offer yet; it stays off the site." ;;
      *) echo "::error::The $c F-Droid repository could not be carried forward; deploying without it would take it off the site."; exit 1 ;;
    esac
  done

  index "$site" > "$site/index.html"
  cat "$site/index.html"
}

# The site's front page: what this is, the pages by channel, and the
# repositories.
index() {
  local site=$1
  cat <<'HTML'
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>apps</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:40em;margin:4em auto;padding:0 1em;color-scheme:light dark}code{word-break:break-all}</style></head>
<body>
<h1>apps</h1>
<p>ArkDB is a local-first sync engine: one runtime in Rust, a domain
written in plain Rust, and the phones embedding the same kernel, with
the apps built on it. The source is at
<a href="https://github.com/k2on/apps">github.com/k2on/apps</a>.</p>
<p>Every app comes in two channels. <b>Stable</b> is the tagged
releases. <b>Unstable</b> is every build of main: on a phone a second
app beside the stable one (its name ends in &ldquo;Unstable&rdquo;, its
icon is banded, and it keeps its own data and sign-in), in the browser
a page under <code>unstable/</code>.</p>
HTML
  local c heading
  for c in stable unstable; do
    case $c in
      stable) heading="Stable: the latest release" ;;
      unstable) heading="Unstable: main" ;;
    esac
    echo "<h2>$heading</h2>"
    echo "<ul>"
    jq -r --arg c "$c" '.[] | select(.channel == $c) | [.path, .title, .about, (.demo | tostring)] | @tsv' <<< "$PAGES" |
      while IFS=$'\t' read -r path title about demo; do
        [ -d "$site/$path" ] || continue
        if [ "$demo" = true ]; then href="$path/?demo"; else href="$path/"; fi
        echo "<li><a href=\"$href\">$title</a>: $about</li>"
      done
    path=$(repo_path "$c")
    if [ -d "$site/$path" ]; then
      case $c in
        stable) echo "<li><a href=\"$path/\">$path</a>: the stable Android apps as an F-Droid repository. Add <code>$SITE_URL/$path</code> in F-Droid and each release arrives as an update.</li>" ;;
        unstable) echo "<li><a href=\"$path/\">$path</a>: the unstable Android apps as an F-Droid repository of its own. Add <code>$SITE_URL/$path</code> too, and each build of main arrives as an update of the &ldquo;Unstable&rdquo; apps, installed beside the stable ones.</li>" ;;
      esac
    fi
    echo "</ul>"
  done
  echo '</body>'
  echo '</html>'
}

case "${1:-}" in
  channel) channel ;;
  plan) plan "$2" ;;
  version-name) version_name ;;
  deploy) deploy "$2" ;;
  *) echo "usage: channels.sh channel | plan PUBLISH.json | version-name | deploy SITE_DIR" >&2; exit 2 ;;
esac
