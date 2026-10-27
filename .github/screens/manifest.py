"""The `screens` branch's manifest.json, from what is on it.

    python3 manifest.py <dir> <version> <commit>

<dir> is the branch: a directory per phone (`android`, `ios`) with its
device.json and, per theme, `<id>.png` for a screenshot and `<id>.mp4`
for a recording. A phone whose capture failed this run keeps the
directory it had, so it is listed as it was. The docs read this
(docs/site/web/site.js).
"""
import datetime
import json
import os
import sys

FPS = int(os.environ.get("FPS", "60"))


def platform(path):
    device = json.load(open(os.path.join(path, "device.json")))
    shots = {}
    for theme in ("light", "dark"):
        at = os.path.join(path, theme)
        if not os.path.isdir(at):
            continue
        for name in sorted(os.listdir(at)):
            for ext, kind in ((".png", "screenshot"), (".mp4", "recording")):
                if name.endswith(ext):
                    id = name[: -len(ext)]
                    shots.setdefault(id, {"id": id, "kind": kind, "themes": [], "fps": FPS if kind == "recording" else 0})["themes"].append(theme)
    return {"device": device, "shots": sorted(shots.values(), key=lambda s: s["id"])}


def main():
    root, version, commit = sys.argv[1:4]
    platforms = {
        p: platform(os.path.join(root, p))
        for p in sorted(os.listdir(root))
        if os.path.isfile(os.path.join(root, p, "device.json"))
    }
    made = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    json.dump({"version": version, "commit": commit, "made": made, "platforms": platforms}, open(os.path.join(root, "manifest.json"), "w"), indent=1)


main()
