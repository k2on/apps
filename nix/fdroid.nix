# The phone's F-Droid repository, https://k2on.github.io/apps/fdroid/repo
# (harken/android/fdroid/README.md): add that URL in the F-Droid client and
# each APK CI builds arrives as an update.
#
#   harken-fdroid-repo   the repository for the APK this flake builds,
#                        signed and checked, laid out exactly as it is served
#   harken-fdroid        what pages.yml runs: the same layout over the new
#                        APK and the newest of those already published
#   fdroid-keygen        a private repository key, for the day there is one
#
# **The index is signed with a key committed beside the metadata**
# (harken/android/fdroid/repo.keystore, `harken` / `harken`), public on
# purpose, the same trade the debug APK key makes. The client refuses an
# unsigned index, and a key nobody may read would be a secret for CI to
# hold before the first repository could exist. What a key that anyone can
# read does not protect is the index's origin, so that is protected
# elsewhere: by HTTPS, and by who can publish to k2on.github.io. Moving to
# a private key later is `fdroid-keygen` and three repository secrets, and
# costs one thing on the phone — the repository re-added, because its
# fingerprint is the key's. The APKs keep their own key, so no app is
# uninstalled for it.
{ lib, pkgs, harken-apk }:
let
  py = pkgs.python3Packages;

  # nixpkgs' fdroidserver is 2.2 GB, almost none of it what `fdroid update`
  # runs: libvirt, samba and xen for the build server, and scipy, pandas and
  # matplotlib for androguard's analysis tools. Pages caches 2 GB of store
  # (.github/workflows/pages.yml), so it is built without them — two pure
  # Python packages, a minute — and comes to under a gigabyte, more than half
  # of it the JDK that verifies the APKs and signs the index.
  androguard = py.androguard.overridePythonAttrs (old: {
    dependencies = with py; [ asn1crypto click colorama future lxml pygments ];
    pythonRemoveDeps = [ "networkx" "matplotlib" "pydot" "ipython" ];
    nativeCheckInputs = [ ];
    doCheck = false;
    pythonImportsCheck = [ "androguard.core.bytecodes.apk" ];
  });
  unused = [ "apache-libcloud" "libvirt" "paramiko" "python-vagrant" "mwclient" "pycountry" "biplist" "python3.12-file" ];
  fdroidserver = pkgs.fdroidserver.overridePythonAttrs (old: {
    dependencies = map (d: if lib.getName d == "androguard" then androguard else d)
      (lib.filter (d: !lib.elem (lib.getName d) unused) old.dependencies);
    pythonRemoveDeps = old.pythonRemoveDeps ++ [ "apache-libcloud" "paramiko" "python-vagrant" ];
    pythonImportsCheck = [ "fdroidserver.update" "fdroidserver.signindex" ];
  });
  python = pkgs.python3.withPackages (_: [ androguard ]);

  appid = "dev.harken.android";
  # How many versions the repository offers: the new one and two to go back
  # to. Each is ~14 MB, against Pages' 1 GB site and 100 MB file.
  keep = 3;
  repoKey = {
    file = ../harken/android/fdroid/repo.keystore;
    alias = "harken";
    pass = "harken";
    # SHA-256 of its certificate: what `?fingerprint=` pins, and what the
    # repository below is checked against before it is a package.
    fingerprint = "E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD";
  };

  # The icon the app already has: the trumpet the browser's tab draws.
  # The APK declares none, so F-Droid takes this from the metadata.
  icon = pkgs.runCommand "harken-fdroid-icon.png" { nativeBuildInputs = [ pkgs.librsvg ]; } ''
    rsvg-convert -w 512 -h 512 ${../harken/iced/web/favicon.svg} -o $out
  '';

  helper = "${python}/bin/python3 -I ${./fdroid.py}";

  harken-fdroid = pkgs.writeShellApplication {
    name = "harken-fdroid";
    runtimeInputs = [ fdroidserver pkgs.jdk_headless pkgs.curl pkgs.coreutils pkgs.findutils ];
    text = ''
      usage() {
        cat >&2 <<'EOF'
      harken-fdroid repo [--keep URL] OUT [APK...]
          A signed repository in OUT (which must not exist): the APKs given,
          and with --keep, those the repository at URL already offers, the
          newest ${toString keep} by version code. URL unreachable or empty
          is not an error; nothing at all to offer is (exit 3).
          Signed with FDROID_KEYSTORE (base64 PKCS12), FDROID_KEYSTORE_PASS
          and FDROID_KEY_ALIAS when FDROID_KEYSTORE is set, and otherwise with
          the committed key (nix/fdroid.nix says why).
      harken-fdroid check DIR [FINGERPRINT]
          What the client checks: entry.jar's signature, the index's hash,
          every APK's hash; and the signer, if a fingerprint is given.
      EOF
        exit 2
      }

      repo() {
        local url=""
        if [ "''${1:-}" = --keep ]; then url=''${2:?--keep wants a URL}; shift 2; fi
        [ $# -ge 1 ] || usage
        local out=$1; shift
        [ ! -e "$out" ] || { echo "harken-fdroid: $out exists" >&2; exit 2; }

        # Everything with a password in it lives here, and goes with it.
        work=$(mktemp -d -p "''${RUNNER_TEMP:-''${TMPDIR:-/tmp}}" harken-fdroid.XXXXXX)
        trap 'rm -rf "$work"' EXIT
        mkdir -p "$work/repo/icons" "$work/metadata/${appid}/en-US" "$work/fetched"
        cp ${../harken/android/fdroid/metadata}/${appid}.yml "$work/metadata/"
        cp ${icon} "$work/metadata/${appid}/en-US/icon.png"
        cp ${icon} "$work/repo/icons/harken.png"

        local apk id code
        for apk in "$@"; do
          read -r id code < <(${helper} apkid "$apk")
          [ "$id" = ${appid} ] || { echo "harken-fdroid: $apk is $id, not ${appid}" >&2; exit 2; }
          cp "$apk" "$work/repo/${appid}_$code.apk"
          echo "harken-fdroid: offering $code ($apk)"
        done

        # Pages deploys the site whole, so what was offered before is
        # fetched back from the live index. Best effort: a first deploy, a
        # site down or an index that does not parse is the new APK alone.
        if [ -n "$url" ]; then
          url=''${url%/}
          if curl -fsSL --max-time 60 --retry 2 -o "$work/index-v2.json" "$url/index-v2.json" \
            && ${helper} listed "$work/index-v2.json" ${appid} ${toString keep} > "$work/listed"; then
            local name digest
            while read -r code name digest; do
              if [ -e "$work/repo/$name" ]; then continue; fi
              if curl -fsSL --max-time 300 --retry 2 -o "$work/fetched/$name" "$url/$name" \
                && echo "$digest  $work/fetched/$name" | sha256sum -c --quiet; then
                mv "$work/fetched/$name" "$work/repo/$name"
                echo "harken-fdroid: keeping $code from $url"
              else
                echo "harken-fdroid: could not fetch $name from $url; leaving it out" >&2
              fi
            done < "$work/listed"
          else
            echo "harken-fdroid: no index at $url; offering only what was given" >&2
          fi
        fi

        local codes
        mapfile -t codes < <(find "$work/repo" -maxdepth 1 -name '${appid}_*.apk' -printf '%f\n' \
          | sed -n 's/^${lib.escapeRegex appid}_\([0-9]*\)\.apk$/\1/p' | sort -n -r)
        if [ ''${#codes[@]} -eq 0 ]; then
          echo "harken-fdroid: no APK to offer" >&2
          exit 3
        fi
        for code in "''${codes[@]:${toString keep}}"; do
          rm "$work/repo/${appid}_$code.apk"
          echo "harken-fdroid: dropping $code, older than the newest ${toString keep}"
        done

        local alias pass
        if [ -n "''${FDROID_KEYSTORE:-}" ]; then
          printf %s "$FDROID_KEYSTORE" | base64 -d > "$work/keystore.p12"
          alias=''${FDROID_KEY_ALIAS:?FDROID_KEYSTORE is set and FDROID_KEY_ALIAS is not}
          pass=''${FDROID_KEYSTORE_PASS:?FDROID_KEYSTORE is set and FDROID_KEYSTORE_PASS is not}
          echo "harken-fdroid: signing with the key in FDROID_KEYSTORE"
        else
          cp ${repoKey.file} "$work/keystore.p12"
          alias=${repoKey.alias} pass=${repoKey.pass}
          echo "harken-fdroid: signing with the committed key"
        fi
        chmod 600 "$work/keystore.p12"
        (
          umask 077
          cat > "$work/config.yml" <<EOF
      repo_url: https://k2on.github.io/apps/fdroid/repo
      repo_name: harken
      repo_description: >-
        A personal repository for the Harken app: the APK built from each
        commit of https://github.com/k2on/apps, debug-signed.
      repo_icon: harken.png
      archive_older: 0
      make_current_version_link: false
      keystore: $work/keystore.p12
      keystorepass: "$pass"
      keypass: "$pass"
      repo_keyalias: "$alias"
      EOF
        )
        (cd "$work" && fdroid update --pretty)

        mkdir -p "$out"
        cp -r "$work/repo/." "$out/"
        rm -rf "$work"
        trap - EXIT
        ${helper} check "$out"
      }

      case "''${1:-}" in
        repo) shift; repo "$@" ;;
        check) shift; ${helper} check "$@" ;;
        *) usage ;;
      esac
    '';
  };

  fdroid-keygen = pkgs.writeShellApplication {
    name = "fdroid-keygen";
    runtimeInputs = [ pkgs.jdk_headless pkgs.git pkgs.coreutils ];
    text = ''
      [ $# -eq 1 ] || { echo "usage: fdroid-keygen DIR   (a directory outside any git tree)" >&2; exit 2; }
      dir=$(realpath -m "$1")
      # The nearest directory that exists says whose tree this would land in.
      probe=$dir
      while [ ! -d "$probe" ]; do probe=$(dirname "$probe"); done
      # Asked of git, and of the directories themselves, because git
      # answers "not a repository" for one it merely distrusts.
      up=$probe
      while :; do
        if [ -e "$up/.git" ] || git -C "$up" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
          echo "fdroid-keygen: $dir is inside the git tree at $up; a private key is not written where it could be committed" >&2
          exit 1
        fi
        [ "$up" != / ] || break
        up=$(dirname "$up")
      done
      ks=$dir/harken-fdroid.p12
      [ ! -e "$ks" ] || { echo "fdroid-keygen: $ks exists" >&2; exit 1; }
      mkdir -p "$dir"
      alias=harken
      pass=$(head -c 24 /dev/urandom | base64 | tr -d '/+=')
      (umask 077; keytool -genkeypair -keystore "$ks" -storetype PKCS12 \
        -storepass "$pass" -alias "$alias" -keyalg RSA -keysize 4096 \
        -validity 36500 -dname "CN=harken" 2>/dev/null)
      fp=$(keytool -exportcert -keystore "$ks" -storepass "$pass" -alias "$alias" 2>/dev/null \
        | sha256sum | cut -d' ' -f1 | tr a-f A-F)
      cat <<EOF
      Wrote $ks. Keep it, and the password, somewhere other than this
      repository; GitHub only ever needs the three secrets below.

      Repository secrets (Settings → Secrets and variables → Actions):

      FDROID_KEYSTORE
      $(base64 -w0 "$ks")

      FDROID_KEYSTORE_PASS
      $pass

      FDROID_KEY_ALIAS
      $alias

      Repository fingerprint (SHA-256):
      $fp

      Once a deploy has signed with it, re-add the repository in F-Droid as:
      https://k2on.github.io/apps/fdroid/repo?fingerprint=$fp
      EOF
    '';
  };

  harken-fdroid-repo = pkgs.runCommand "harken-fdroid-repo" {
    nativeBuildInputs = [ harken-fdroid ];
    passthru = { inherit fdroidserver; fingerprint = repoKey.fingerprint; };
  } ''
    export HOME=$TMPDIR
    harken-fdroid repo $out ${harken-apk}/harken-debug.apk
    harken-fdroid check $out ${repoKey.fingerprint}
  '';
in
{
  inherit harken-fdroid harken-fdroid-repo fdroid-keygen;
}
