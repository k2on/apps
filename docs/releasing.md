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
- `app-screens.yaml` takes each Android app's shots from the stable APKs
  the tag's `build` made, the release's own, and keeps them as that run's
  artifacts: a check that the release's APKs walk, published nowhere (the
  `screens` branch is main's, and main has taken the tag's commit already,
  as its `.unstable` twins).

A push to main does the other half: its APKs go to the unstable
repository (the newest three of each app), its pages to `unstable/`, and
the stable channel is carried forward as it was. Main's iPhone archives
are signed to check the signing, and sent nowhere. Main's screenshots (the
`screens` branch, .github/screens/apps.md) are of the commit pushed: on
Android the `.unstable` APKs its `build` made (or, for an app whose inputs
had not moved, the unstable repository's, the same inputs' build), never
the stable repository's, which holds the last release.

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

## The servers already deployed

The unstable clients — every push to main: the phones, the browser page,
the desktop — work against the money and harken servers the owner runs,
which are built from k2on/apps 77da9807 (the revision k2on/os pins; for
money, the same server as main before the app restructure, 7a4f5634).
ui/docs/app.md, Servers Already Deployed, lists every difference and how a
client gets by; `.github/compat-check.sh` runs this build's money phone
host and harken's headless client against servers built from that
revision. Run it before a release the deployed servers will meet.

**The push table is off** until the servers are deployed again. To bring
it back:

1. Turn on the `push` features: money-core's and harken-domain's (the
   table appended to the schema, `session.push` registered), ark-peer's on
   the servers (the hub's wakes; `Server::push` with `Apns::sender`),
   ark-ffi's in the phones' libraries (the endpoint written as the
   session's push row) and ark-app's where "Notify Me" should register
   for push. The table is appended, empty: no migration step.
2. Deploy the servers built with it first — money's and harken's, from
   the commit that turns it on — and move k2on/os's pin to that commit.
   A server with the table serves clients without it (a schema grows,
   §1.2), so the phones can follow.
3. Then move `tests/deployed.json` (money-core's, harken-domain's) to that
   server's catalog and the revision in `.github/compat-check.sh`, so the
   tests hold the clients to the server now deployed. A client with the
   table against a server without it would send `session.push` that the
   server holds for ever, and its state hashes would not be the server's.

## What is known not to be ready

- harken's Android cover provider builds its `content://` URIs from a
  fixed authority (`CoverProvider.kt`); the unstable app's provider is
  moved under its own id so that both install, and its covers in a car
  stay missing until that code asks for its own package's authority.
- A browser page under a path (`/apps/<app>/`) is served with
  `data-ark-base` set; a deep link reloaded there is GitHub Pages' 404,
  as there is no fallback page.
