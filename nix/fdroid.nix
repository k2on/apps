# The phone's F-Droid repository, https://k2on.github.io/apps/fdroid/repo
# (harken/android/fdroid/README.md): add that URL in the F-Droid client and
# each APK CI builds arrives as an update. One repository for every app of
# this tree, so a phone adds it once.
#
#   fdroid-repo     the repository for the APKs this flake builds, signed
#                   and checked, laid out exactly as it is served
#   apps-fdroid     what build.yaml's deploy runs: the same layout over the
#                   new APKs and the newest of those already published
#   fdroid-keygen   a private repository key, for the day there is one
#
# `apps` is the list of them, each
#
#   { appid; apk; apkName; metadata; icon; }
#
# its application id, the derivation that builds it and the APK's name in
# it, its F-Droid metadata (a .yml) and its icon (an SVG).
#
# **The index is signed with a key committed beside the metadata**
# (harken/android/fdroid/repo.keystore, `harken` / `harken`), public on
# purpose, the same trade the APKs' own committed key makes. The client refuses an
# unsigned index, and a key nobody may read would be a secret for CI to
# hold before the first repository could exist. What a key that anyone can
# read does not protect is the index's origin, so that is protected
# elsewhere: by HTTPS, and by who can publish to k2on.github.io. Moving to
# a private key later is `fdroid-keygen` and three repository secrets, and
# costs one thing on the phone — the repository re-added, because its
# fingerprint is the key's. The APKs keep their own key, so no app is
# uninstalled for it.
{ lib, pkgs, apps }:
let
  py = pkgs.python3Packages;

  # nixpkgs' fdroidserver is 2.2 GB, almost none of it what `fdroid update`
  # runs: libvirt, samba and xen for the build server, and scipy, pandas and
  # matplotlib for androguard's analysis tools. Pages fetches it on every
  # deploy (.github/workflows/build.yaml), so it is built without them — two
  # pure Python packages, a minute — and comes to under a gigabyte, more
  # than half of it the JDK that verifies the APKs and signs the index.
  androguard = py.androguard.overridePythonAttrs (old: {
    dependencies = with py; [ asn1crypto click colorama future lxml pygments ];
    pythonRemoveDeps = [ "networkx" "matplotlib" "pydot" "ipython" ];
    # The two bytes after a type spec's id were padding when 3.4 was
    # written and are its `typesCount` since aapt2 for Android 13; ours
    # (nix/aapt2.nix, platform 35) fills them in, and 3.4 refuses the APK
    # as malformed.
    postPatch = (old.postPatch or "") + ''
      substituteInPlace androguard/core/bytecodes/axml/__init__.py --replace-fail \
        'raise ResParserError("res1 must be zero!")' 'pass'
    '';
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
  python = pkgs.python3.withPackages (_: [ (py.toPythonModule fdroidserver) ]);

  appids = map (a: a.appid) apps;
  # How many versions the repository offers of each app: the new one and
  # two to go back to. Each of harken's is ~14 MB, against Pages' 1 GB site
  # and 100 MB file.
  keep = 3;
  repoKey = {
    file = ../harken/android/fdroid/repo.keystore;
    alias = "harken";
    pass = "harken";
    # SHA-256 of its certificate: what `?fingerprint=` pins, and what the
    # repository below is checked against before it is a package.
    fingerprint = "E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD";
  };

  # Each app's icon from its SVG. An APK's own is an adaptive icon, XML and
  # nothing else (minSdk 26), and fdroidserver takes an icon out of an APK
  # only as a PNG of the same name, so F-Droid takes this from the metadata.
  icon = a: pkgs.runCommand "${a.appid}-icon.png" { nativeBuildInputs = [ pkgs.librsvg ]; } ''
    rsvg-convert -w 512 -h 512 ${a.icon} -o $out
  '';
  # The repository's own is the first app's: harken's trumpet, which phones
  # have shown for the repository since it began.
  repoIcon = icon (lib.head apps);

  # Per app, once the APKs are in repo/: all but its newest `keep`
  # dropped, and its metadata and icon laid out if any APK is left. An app
  # with no APK has no metadata either, so fdroid is never asked to list an
  # app with nothing to install.
  perApp = a: ''
    mapfile -t codes < <(find "$work/repo" -maxdepth 1 -name '${a.appid}_*.apk' -printf '%f\n' \
      | sed -n 's/^${lib.escapeRegex a.appid}_\([0-9]*\)\.apk$/\1/p' | sort -n -r)
    for code in "''${codes[@]:${toString keep}}"; do
      rm "$work/repo/${a.appid}_$code.apk"
      echo "apps-fdroid: dropping ${a.appid} $code, older than its newest ${toString keep}"
    done
    if [ ''${#codes[@]} -gt 0 ]; then
      offered=$((offered + 1))
      mkdir -p "$work/metadata/${a.appid}/en-US"
      cp ${a.metadata} "$work/metadata/${a.appid}.yml"
      cp ${icon a} "$work/metadata/${a.appid}/en-US/icon.png"
    fi
  '';

  helper = "${python}/bin/python3 -I ${./fdroid.py}";

  apps-fdroid = pkgs.writeShellApplication {
    name = "apps-fdroid";
    runtimeInputs = [ fdroidserver pkgs.jdk_headless pkgs.curl pkgs.coreutils pkgs.findutils ];
    text = ''
      usage() {
        cat >&2 <<'EOF'
      apps-fdroid repo [--keep URL] OUT [APK...]
          A signed repository in OUT (which must not exist): the APKs given,
          each one of ${lib.concatStringsSep ", " appids}
          by its application id, and with --keep, those the repository at
          URL already offers; of each app the newest ${toString keep} by version
          code. URL unreachable or empty is not an error; nothing at all to
          offer is (exit 3).
          Signed with FDROID_KEYSTORE (base64 PKCS12), FDROID_KEYSTORE_PASS
          and FDROID_KEY_ALIAS when FDROID_KEYSTORE is set, and otherwise with
          the committed key (nix/fdroid.nix says why).
      apps-fdroid check DIR [FINGERPRINT]
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
        [ ! -e "$out" ] || { echo "apps-fdroid: $out exists" >&2; exit 2; }

        # Everything with a password in it lives here, and goes with it.
        work=$(mktemp -d -p "''${RUNNER_TEMP:-''${TMPDIR:-/tmp}}" apps-fdroid.XXXXXX)
        trap 'rm -rf "$work"' EXIT
        mkdir -p "$work/repo" "$work/metadata" "$work/fetched"
        # Relative to where fdroid runs; it copies it into repo/icons itself.
        cp ${repoIcon} "$work/apps.png"

        # An APK is the app its manifest names, and one this repository
        # does not know is refused rather than listed without metadata.
        local apk id code
        for apk in "$@"; do
          read -r id code < <(${helper} apkid "$apk")
          case $id in
            ${lib.concatStringsSep "|" appids}) ;;
            *) echo "apps-fdroid: $apk is $id, which is none of ${lib.concatStringsSep ", " appids}" >&2; exit 2 ;;
          esac
          cp "$apk" "$work/repo/''${id}_$code.apk"
          echo "apps-fdroid: offering $id $code ($apk)"
        done

        # Pages deploys the site whole, so what was offered before is
        # fetched back from the live index, app by app. Best effort: a first
        # deploy, a site down or an index that does not parse is the new
        # APKs alone, and an app the index does not list yet keeps nothing.
        if [ -n "$url" ]; then
          url=''${url%/}
          if curl -fsSL --max-time 60 --retry 2 -o "$work/index-v2.json" "$url/index-v2.json"; then
            local name digest
            local ids=(${lib.concatStringsSep " " appids})
            for id in "''${ids[@]}"; do
              if ! ${helper} listed "$work/index-v2.json" "$id" ${toString keep} > "$work/listed"; then
                echo "apps-fdroid: the index at $url does not parse; offering only what was given" >&2
                break
              fi
              while read -r code name digest; do
                if [ -e "$work/repo/$name" ]; then continue; fi
                if curl -fsSL --max-time 300 --retry 2 -o "$work/fetched/$name" "$url/$name" \
                  && echo "$digest  $work/fetched/$name" | sha256sum -c --quiet; then
                  mv "$work/fetched/$name" "$work/repo/$name"
                  echo "apps-fdroid: keeping $id $code from $url"
                else
                  echo "apps-fdroid: could not fetch $name from $url; leaving it out" >&2
                fi
              done < "$work/listed"
            done
          else
            echo "apps-fdroid: no index at $url; offering only what was given" >&2
          fi
        fi

        local codes offered=0
        ${lib.concatMapStrings perApp apps}
        if [ "$offered" -eq 0 ]; then
          echo "apps-fdroid: no APK to offer" >&2
          exit 3
        fi

        local alias pass
        if [ -n "''${FDROID_KEYSTORE:-}" ]; then
          printf %s "$FDROID_KEYSTORE" | base64 -d > "$work/keystore.p12"
          alias=''${FDROID_KEY_ALIAS:?FDROID_KEYSTORE is set and FDROID_KEY_ALIAS is not}
          pass=''${FDROID_KEYSTORE_PASS:?FDROID_KEYSTORE is set and FDROID_KEYSTORE_PASS is not}
          echo "apps-fdroid: signing with the key in FDROID_KEYSTORE"
        else
          cp ${repoKey.file} "$work/keystore.p12"
          alias=${repoKey.alias} pass=${repoKey.pass}
          echo "apps-fdroid: signing with the committed key"
        fi
        chmod 600 "$work/keystore.p12"
        (
          umask 077
          cat > "$work/config.yml" <<EOF
      repo_url: https://k2on.github.io/apps/fdroid/repo
      repo_name: apps
      repo_description: >-
        A personal repository for the apps of https://github.com/k2on/apps:
        Harken, a music library synced from your own server, and Revelation,
        the Bible as one scroll zoomed like a map. Each APK is the release
        build of a commit, signed with a key that is public.
      repo_icon: apps.png
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
        # fdroid's own log of the run, naming the machine and its paths.
        rm -rf "$out/status"
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
          echo "fdroid-keygen: $dir is inside a git work tree (from $up up); a private key is not written where it could be committed" >&2
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

  # Over every app's APK, and checked against the committed key's
  # fingerprint: the same key as ever, so a phone that added the repository
  # keeps it.
  fdroid-repo = pkgs.runCommand "fdroid-repo" {
    nativeBuildInputs = [ apps-fdroid ];
    passthru = { inherit fdroidserver; fingerprint = repoKey.fingerprint; };
  } ''
    export HOME=$TMPDIR
    apps-fdroid repo $out ${lib.concatMapStringsSep " " (a: "${a.apk}/${a.apkName}") apps}
    apps-fdroid check $out ${repoKey.fingerprint}
  '';
in
{
  inherit apps-fdroid fdroid-repo fdroid-keygen;
}
