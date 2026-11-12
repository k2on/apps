# The F-Droid repositories

Every Android app of this tree (harken's beside the others) as F-Droid
repositories, one per release channel (docs/releasing.md), served by
GitHub Pages:

    stable, the tagged releases:
    https://k2on.github.io/apps/fdroid/repo?fingerprint=E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

    unstable, every build of main, as `.unstable` apps of their own:
    https://k2on.github.io/apps/fdroid/unstable/repo?fingerprint=E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

Add the first in the F-Droid client and each release arrives as an
update; add the second too and each build of main arrives as an update of
"Harken Unstable" and the others, installed beside the stable apps with
their own data and sign-in. They are personal repositories, for the one
person running the server the apps sign in to.

## Setting it up

`.github/workflows/build.yaml` publishes both (`.github/channels.sh`): a
`v*` tag builds every APK under its own id and deploys it to the stable
repository; a push to `main` builds each app's `.unstable` twin (skipped
when its inputs have not moved) and deploys it to the unstable one. Each
deploy carries the other repository forward as it was, beside the pages,
and the landing page at https://k2on.github.io/apps/ links both.

On the phone: F-Droid → Settings → Repositories → **+**, and paste an
address above, or open it in the phone's browser and let F-Droid take it.
Then install Harken (or Harken Unstable) from F-Droid.

**A copy installed by hand keeps working.** Every APK since 59fd287 is
signed with the one committed key, `harken/android/debug.keystore` (the
name is AGP's; it signs the release builds too), and so is every APK this
repository offers, so F-Droid updates a hand-installed copy in place. What
it offers now are release builds, not debuggable, and they install over the
debug builds offered before them like any update. A copy installed before
59fd287 was signed with a key its own build made up; that one needs
uninstalling once.

No secret is needed for any of this.

## What it is

`nix build .#fdroid-repo` is the stable repository for the APKs this
flake builds (every app's, this one's among them), exactly as it is served:
`us.koon.harken_<versionCode>.apk`, the trumpet as the app's icon and
the repository's, the metadata from
`metadata/us.koon.harken.yml`, and `entry.jar`, `index-v2.json` and
the v1 indexes, signed. The build ends by checking what the client checks —
`entry.jar`'s signature and signer, the index's hash in `entry.json`, and
every APK's hash in the index — so a repository that would be refused is
not a package. `nix/fdroid.nix` is all of it.

What `build.yaml`'s deploy runs is the same layout, per channel, with one
difference. Pages deploys the site whole, so a repository built from the
newest APK alone would offer nothing to go back to: `apps-fdroid repo
--keep` fetches the APKs the live repository already lists, checks each
against the hash the index gave it, and keeps the newest three by version
code — about 14 MB each, well under Pages' 1 GB site and 100 MB file. A
live repository with no index yet is the new APKs alone. The channel a
deploy does not publish is carried forward the same way with `--strict`:
then a live repository that cannot be fetched fails the deploy rather
than vanish from the site. An APK of the other channel's id is refused.

On a laptop, the first of these does what CI does and the second checks
any repository against a fingerprint:

    nix run .#apps-fdroid -- repo --channel unstable --keep https://k2on.github.io/apps/fdroid/unstable/repo out result/harken.apk
    nix run .#apps-fdroid -- check out E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

## The index key is public, on purpose

`repo.keystore`, beside this file, signs the index: alias `harken`,
password `harken`. Anyone can read it, so anyone can sign an index with
it — the same trade the APKs' committed key makes, and made for the same reason:
the client refuses an unsigned index, and a key nobody may read is a secret
CI would have to hold before the first repository could exist.

What protects the phone is not the signature, then, but where it fetches
from: HTTPS, and who can publish to k2on.github.io. The fingerprint in the
address still pins the key, so the client refuses an index signed by any
other.

## Later: a private key

When the index should be signed by a key only CI holds:

    nix run .#fdroid-keygen -- ~/keys/harken-fdroid

It writes a new keystore there — it refuses any directory inside a git
tree — and prints three repository secrets, the new fingerprint and the
new address. Add the secrets under Settings → Secrets and variables →
Actions:

- `FDROID_KEYSTORE`: the keystore, base64, as printed;
- `FDROID_KEYSTORE_PASS`: its password;
- `FDROID_KEY_ALIAS`: `harken`.

The next deploy signs both repositories with it; with the secrets absent, the committed
key's signature stands. Keep the keystore and its password somewhere other
than this repository: losing them means another key and re-adding the
repository again.

What it costs on the phone is one thing: the fingerprint is the key's, so
each repository is removed in F-Droid and added again at its new address.
The APKs keep their own key, so the app is not uninstalled.
