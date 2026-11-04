"""A harken media root made from the demo's catalogue, for its screenshots (server.sh).

    python3 -I harken/screens/media.py <catalogue.json> <media root> [albums]

Every album of harken's demo (harken/iced/src/catalogue.json) as a folder
under <root>/music/<performer>/<album>/, a FLAC of silence for each track
as long as the catalogue says, tagged as the catalogue says (title,
composer, performer, album, track, year, work), and the album's cover
beside it as cover.jpg: fetched from the catalogue's address, or, when that
cannot be had, a gradient. The real server's scanner reads these as it
reads anyone's library (harken/server/src/library.rs), so the phone shows
what a server with this library would show. [albums] keeps only the first
that many (all, unset).

Needs ffmpeg and curl on the PATH.
"""
import hashlib
import json
import os
import subprocess
import sys


def safe(name):
    """A path segment: no slashes, nothing hidden."""
    s = "".join("-" if c in '/\\:*?"<>|' else c for c in name).strip().lstrip(".")
    return s[:120] or "untitled"


def run(*args):
    subprocess.run(args, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def cover(url, to, seed):
    """The album's cover at `to`: the catalogue's, or a gradient of its own."""
    if url:
        try:
            run("curl", "-fsSL", "--max-time", "30", "-o", to, url)
            if os.path.getsize(to) > 0:
                return
        except subprocess.CalledProcessError:
            pass
    h = hashlib.sha256(seed.encode()).hexdigest()
    run(
        "ffmpeg", "-nostdin", "-y", "-f", "lavfi",
        "-i", f"gradients=s=600x600:c0=0x{h[:6]}:c1=0x{h[6:12]}:x0=0:y0=0:x1=600:y1=600:d=1",
        "-frames:v", "1", to,
    )


def track(t, album, year, to):
    seconds = max(1, min(int(t.get("ms") or 0) // 1000, 3600))
    performer = t.get("performer") or ""
    tags = {
        "title": t["title"],
        "artist": performer or t.get("composer", ""),
        "album_artist": performer,
        "album": album,
        "composer": t.get("composer", ""),
        "track": str(t.get("track") or ""),
        "date": str(year or ""),
    }
    meta = [x for k, v in tags.items() if v for x in ("-metadata", f"{k}={v}")]
    run(
        "ffmpeg", "-nostdin", "-y", "-f", "lavfi", "-i", "anullsrc=r=8000:cl=mono",
        "-t", str(seconds), *meta, "-c:a", "flac", to,
    )


def main():
    catalogue, root = sys.argv[1:3]
    keep = int(sys.argv[3]) if len(sys.argv) > 3 else None
    albums = json.load(open(catalogue))["albums"][:keep]
    for a in albums:
        performer = next((t.get("performer") for t in a["tracks"] if t.get("performer")), "") or "Various"
        at = os.path.join(root, "music", safe(performer), safe(a["name"]))
        os.makedirs(at, exist_ok=True)
        cover(a.get("art"), os.path.join(at, "cover.jpg"), a["name"])
        for t in a["tracks"]:
            name = f"{int(t.get('track') or 0):02d} {safe(t['title'])}.flac"
            track(t, a["name"], a.get("year"), os.path.join(at, name))
        print(f"{performer} / {a['name']}: {len(a['tracks'])} tracks")


main()
