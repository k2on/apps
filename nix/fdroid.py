"""What apps-fdroid (nix/fdroid.nix) asks of an APK and of an index.

    fdroid.py apkid APK              the application id and the version code
    fdroid.py listed INDEX APPID N   the newest N of APPID's APKs in an
                                     index-v2.json: version code, file name
                                     and sha256, one per line
    fdroid.py check DIR [FINGERPRINT]
                                     the chain the F-Droid client walks:
                                     entry.jar signed (by FINGERPRINT,
                                     if given), entry.json naming
                                     index-v2.json by its hash, and every
                                     APK the index lists by its hash

The shell around it does the fetching, the layout and fdroid itself.
"""

import hashlib
import json
import logging
import re
import shutil
import sys
import zipfile
from pathlib import Path

# A file name from an index someone else wrote, before it is a path here.
APK_NAME = re.compile(r"^[A-Za-z0-9._]+_[0-9]+\.apk$")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def apkid(path):
    logging.disable(logging.CRITICAL)
    from androguard.core.bytecodes.apk import APK

    apk = APK(path)
    print(apk.get_package(), apk.get_androidversion_code())


def listed(index, appid, n):
    data = json.loads(Path(index).read_text())
    versions = data.get("packages", {}).get(appid, {}).get("versions", {})
    rows = []
    for v in versions.values():
        name = v["file"]["name"].lstrip("/")
        if not APK_NAME.match(name):
            continue
        rows.append((int(v["manifest"]["versionCode"]), name, v["file"]["sha256"]))
    for code, name, digest in sorted(rows, reverse=True)[: int(n)]:
        print(code, name, digest)


def fail(why):
    print(f"fdroid.py check: {why}", file=sys.stderr)
    sys.exit(1)


def check(repo, fingerprint=None):
    repo = Path(repo)
    jar = repo / "entry.jar"
    # fdroidserver's own reading of an index JAR, the one `fdroid` uses on
    # a mirror: jarsigner -strict, signed exactly once, and the signer's
    # certificate against the fingerprint, which is what `?fingerprint=`
    # pins.
    from fdroidserver import common
    from fdroidserver import index as fdroid_index
    from fdroidserver.exception import VerificationException

    common.config = {"jarsigner": shutil.which("jarsigner")}
    try:
        _, _, signer = fdroid_index.get_index_from_jar(str(jar), fingerprint)
    except VerificationException as e:
        fail(f"{jar}: {e}")

    with zipfile.ZipFile(jar) as z:
        entry = json.loads(z.read("entry.json"))
    if entry != json.loads((repo / "entry.json").read_text()):
        fail("entry.json beside entry.jar is not the one inside it")
    named = entry["index"]
    index = repo / named["name"].lstrip("/")
    if sha256(index) != named["sha256"]:
        fail(f"{index.name} is not the file entry.json names")

    apks = 0
    for pkg in json.loads(index.read_text())["packages"].values():
        for v in pkg["versions"].values():
            f = repo / v["file"]["name"].lstrip("/")
            if sha256(f) != v["file"]["sha256"]:
                fail(f"{f.name} is not the file the index lists")
            apks += 1
    print(f"signed by {signer}; index and {apks} APK(s) match")


if __name__ == "__main__":
    {"apkid": apkid, "listed": listed, "check": check}[sys.argv[1]](*sys.argv[2:])
