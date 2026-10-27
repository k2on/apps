"""The `screens` branch's manifest.json, from what is on it.

    python3 manifest.py <dir> <version> <commit>

<dir> is the branch: a directory per phone (`android`, `ios`) with its
device.json and, per theme, `<id>.png` for a screenshot and `<id>/NNN.jpg`
for a recording's frames. A phone whose capture failed this run keeps the
directory it had, so it is listed as it was. The docs read this
(docs/site/src/screens.rs).
"""
import datetime
import json
import os
import sys

FPS = int(os.environ.get("FPS", "10"))


def platform(path):
    device = json.load(open(os.path.join(path, "device.json")))
    shots = {}
    for theme in sorted(os.listdir(path)):
        at = os.path.join(path, theme)
        if not os.path.isdir(at):
            continue
        for name in sorted(os.listdir(at)):
            full = os.path.join(at, name)
            if os.path.isdir(full):
                frames = len([f for f in os.listdir(full) if f.endswith(".jpg")])
                if frames:
                    s = shots.setdefault(name, {"id": name, "kind": "recording", "themes": [], "frames": frames, "fps": FPS})
                    s["frames"] = min(s["frames"], frames)
                    s["themes"].append(theme)
            elif name.endswith(".png"):
                id = name[: -len(".png")]
                shots.setdefault(id, {"id": id, "kind": "screenshot", "themes": [], "frames": 1, "fps": 0})["themes"].append(theme)
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
