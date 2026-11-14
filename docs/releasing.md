# Releasing

Every app here is published in two channels.

| | stable | unstable |
|---|---|---|
| what | a `v*` tag's build: a release | every push to main |
| Android app | the app's own id (`us.koon.<app>`), its own name | a second app beside it: `us.koon.<app>.unstable`, "&lt;Name&gt; Unstable", its launcher icon banded in the theme's caution colours |
| F-Droid | https://k2on.github.io/apps/fdroid/repo | https://k2on.github.io/apps/fdroid/unstable/repo |
| browser demo | https://k2on.github.io/apps/&lt;app&gt;/?demo | https://k2on.github.io/apps/unstable/&lt;app&gt;/?demo |
| iPhone | TestFlight | nothing yet (a hook, below) |
| version name | the tag without its `v` (`v1.2` is `1.2`) | `<last tag>-unstable+<commit>`, such as `1.2-unstable+3f9c2ab` |
| GitHub release | the stable APKs attached | none |

The docs (https://k2on.github.io/apps/docs/) follow main. A branch or a
pull request deploys nothing: a pull request builds the stable APKs whose
inputs moved, for its screenshots.

Nothing of this names an app. An app in the flake's `publish` list has
both channels; one that should not opts out in its `app.toml`:

```toml
[channels]
unstable = false      # no .unstable app, no unstable page
# stable = false      # nor a release
# ios-unstable = true # the unstable TestFlight app (below): off by default
```

What does it: `nix/channels.nix` and `nix/channel.py` (the `.unstable` id,
the name, the banded icon, the F-Droid metadata, the pages' paths),
`nix/fdroid.nix` (one repository per channel), and `.github/channels.sh`
(what a run builds, its version name, and the deploy). `checks.channels`
fails if any of them names an app.

## Cutting a release

1. Main is where it should be, and green.
2. If the release's apps speak a schema the servers do not yet, deploy the
   server for the same commit first (k2on/os), so the stable apps never
   meet an older server.
3. Tag it and push the tag:

       git tag v1.2
       git push origin v1.2

   A tag is `vX.Y` or `vX.Y.Z`: App Store Connect takes one to three
   numbers as a version, and the iPhone's archive is given the tag's.

CI then, from the tag's commit:

- `build.yaml` builds every APK under its own id, with version name `1.2`
  and the commit's time as its version code, attaches them to a GitHub
  release, and deploys the site: the stable F-Droid repository gains the
  release (it keeps the newest three releases of each app) and each
  browser app's stable demo is the tag's. The unstable channel is carried
  forward as it was.
- `ios.yaml` archives each iPhone app, gives it the tag's version and the
  commit's time as its build number, signs it and sends it to TestFlight.

A push to main does the other half: its APKs go to the unstable
repository (the newest three of each app), its pages to `unstable/`, and
the stable channel is carried forward as it was. Main's iPhone archives
are signed to check the signing, and sent nowhere.

**Once, in the repository's settings:** Settings → Environments →
`github-pages` → Deployment branches and tags must allow the tags `v*`
beside `main`, or a tag's deploy is refused. Not checked from here.

### Carrying the other channel forward

GitHub Pages deploys the site whole, so each deploy lays out its own
channel and carries the other one forward from the live site: its pages
by the `files.sha256` every page is published with, each file checked
against it, and its F-Droid repository by its index, each APK checked
against its hash and the index signed again over the same APKs. A channel
that has never been published is simply absent. One that is published
and cannot be fetched fails the deploy, rather than take it off the site.

## On the phone

Add either repository, or both, in F-Droid (Settings → Repositories → +):

    https://k2on.github.io/apps/fdroid/repo?fingerprint=E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD
    https://k2on.github.io/apps/fdroid/unstable/repo?fingerprint=E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

Both indexes are signed with the same committed key, so the fingerprint
is the same (harken/android/fdroid/README.md says why the key is public).

The stable app and the unstable app are two apps: they install side by
side, each with its own data, its own sign-in and its own server
connection, and one never updates into the other. The APKs of both are
signed with the same committed key, but because the ids differ that only
means either installs over a hand-installed copy of itself.

A phone that added the repository before the channels existed keeps it:
it is the stable repository now, and offers what it offered until the
first release replaces it.

## The iPhone's unstable app: a hook, off

`ios.yaml` sends only tags to TestFlight. It can also send main's builds
as a second TestFlight app, `<bundle>.unstable` named "&lt;Name&gt; Unstable",
for an app whose `app.toml` says `[channels] ios-unstable = true`
(`.github/ios-archive.sh` rewrites the archive's bundle ids and name
before it is signed). No app says so, and before one does:

- the App Store Connect record for `<bundle>.unstable` must exist, with
  its extensions' ids and its capabilities (app groups, push);
- the band is not drawn on the iPhone's icon yet: the asset catalog is
  compiled into the archive, so it belongs in `ui/prebuild.py`'s asset
  catalog for the unstable build;
- none of it has been run.

## What is known not to be ready

- harken's Android cover provider builds its `content://` URIs from a
  fixed authority (`CoverProvider.kt`); the unstable app's provider is
  moved under its own id so that both install, and its covers in a car
  stay missing until that code asks for its own package's authority.
- A browser page under a path (`/apps/<app>/`) is served with
  `data-ark-base` set; a deep link reloaded there is GitHub Pages' 404,
  as there is no fallback page.
