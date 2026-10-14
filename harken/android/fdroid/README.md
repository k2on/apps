# harken's F-Droid repository

The Android app as an F-Droid repository, served by GitHub Pages:

    https://k2on.github.io/apps/fdroid/repo?fingerprint=E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

Add that in the F-Droid client and each APK built from `main` arrives as an
update. It is a personal repository: one app, for the one person running
the server it signs in to.

## Setting it up

The workflow that publishes it is a patch rather than a change to
`.github/workflows/`, because the change was written somewhere that could
not push one. From the repository root, on a machine whose credentials may
push workflow files:

    git apply harken/android/fdroid/pages-workflow.patch
    git commit -m "pages: the F-Droid repository" -- .github/workflows/pages.yml
    git push

If `pages.yml` has moved since the patch was written, `git apply --3way`
merges it. Then wait for two runs: `apk` builds the APK, and `pages`, which
now also runs when `apk` finishes, deploys it. The landing page at
https://k2on.github.io/apps/ links the repository once there is one.

On the phone: F-Droid → Settings → Repositories → **+**, and paste the
address above, or open it in the phone's browser and let F-Droid take it.
Then install Harken from F-Droid.

**A copy installed by hand keeps working.** Every APK since 59fd287 is
signed with the one committed debug key, and so is every APK this
repository offers, so F-Droid updates a hand-installed copy in place. A
copy installed before 59fd287 was signed with a key its own build made up;
that one needs uninstalling once.

No secret is needed for any of this.

## What it is

`nix build .#harken-fdroid-repo` is the repository for the APK this flake
builds, exactly as it is served: `dev.harken.android_<versionCode>.apk`,
the trumpet as the app's icon and the repository's, the metadata from
`metadata/dev.harken.android.yml`, and `entry.jar`, `index-v2.json` and
the v1 indexes, signed. The build ends by checking what the client checks —
`entry.jar`'s signature and signer, the index's hash in `entry.json`, and
every APK's hash in the index — so a repository that would be refused is
not a package. `nix/fdroid.nix` is all of it.

What `pages.yml` runs is the same layout with one difference. Pages deploys
the site whole, so a repository built from the newest APK alone would
offer nothing to go back to: `harken-fdroid repo --keep` fetches the APKs
the live repository already lists, checks each against the hash the index
gave it, and keeps the newest three by version code — about 14 MB each,
well under Pages' 1 GB site and 100 MB file. An unreachable or empty live
repository is the new APK alone, never a failed deploy.

On a laptop, the first of these does what CI does and the second checks
any repository against a fingerprint:

    nix run .#harken-fdroid -- repo --keep https://k2on.github.io/apps/fdroid/repo out result/harken-debug.apk
    nix run .#harken-fdroid -- check out E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

## The index key is public, on purpose

`repo.keystore`, beside this file, signs the index: alias `harken`,
password `harken`. Anyone can read it, so anyone can sign an index with
it — the same trade the debug APK key makes, and made for the same reason:
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

The next `pages` run signs with it; with the secrets absent, the committed
key's signature stands. Keep the keystore and its password somewhere other
than this repository: losing them means another key and re-adding the
repository again.

What it costs on the phone is one thing: the fingerprint is the key's, so
the repository is removed in F-Droid and added again at the new address.
The APKs keep their own key, so the app is not uninstalled.
