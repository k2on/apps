"""Whether two runs took the same shots: the determinism check (ui/docs/shots.md),
never the stored shots' comparison (compare.py is that).

    python3 -I same.py <run A> <run B> [--tolerance N] [--glob 'app-screens-android-*']

Each directory is a run's artifacts downloaded one per directory (`gh run
download <id> -n <name> -D <name>` for each `app-screens-*`). Every PNG in A
is held to the same path in B and counted as byte-identical, the same within
the tolerance (no channel of any pixel more than N levels apart: the
emulator's GPU antialiases a rounded edge one to three levels differently
from one boot to the next, which nothing in the app can hold still; 3 by
default), or different; each different one is listed with the box its
differences fall in, the largest, and how many pixels are past the
tolerance. A shot only one run has is listed as such. Exits 0 always: it
says, the reader decides.

Pillow from the flake's nixpkgs (`nix build .#screens-python`).
"""
import argparse
import glob
import os

from PIL import Image, ImageChops

p = argparse.ArgumentParser()
p.add_argument("a")
p.add_argument("b")
p.add_argument("--tolerance", type=int, default=3)
p.add_argument("--glob", default="app-screens-*")
args = p.parse_args()

same = close = 0
differ, only = [], []
for pa in sorted(glob.glob(os.path.join(args.a, args.glob, "**", "*.png"), recursive=True)):
    rel = os.path.relpath(pa, args.a)
    if os.path.basename(rel).startswith(("stuck", "signin-stuck")):
        continue
    pb = os.path.join(args.b, rel)
    if not os.path.exists(pb):
        only.append(rel)
        continue
    with open(pa, "rb") as fa, open(pb, "rb") as fb:
        if fa.read() == fb.read():
            same += 1
            continue
    ia, ib = Image.open(pa).convert("RGB"), Image.open(pb).convert("RGB")
    if ia.size != ib.size:
        differ.append(f"{rel}: {ia.size} against {ib.size}")
        continue
    d = ImageChops.difference(ia, ib).convert("L")
    h = d.histogram()
    largest = max(i for i in range(256) if h[i])
    past = sum(h[args.tolerance + 1 :])
    if past == 0:
        close += 1
    else:
        differ.append(f"{rel}: in {d.getbbox()}, up to {largest} levels, {past} pixels past {args.tolerance}")

total = same + close + len(differ)
print(f"{same} of {total} byte-identical; {close} more within {args.tolerance} levels; {len(differ)} different")
for line in differ:
    print("  different:", line)
for rel in only:
    print("  only in", args.a + ":", rel)
