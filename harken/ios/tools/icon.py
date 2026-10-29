#!/usr/bin/env python3
"""The app's icon, drawn with nothing but the standard library: a 1024 x 1024
PNG, opaque (App Store Connect refuses an icon with alpha), harken's mark on
the dark scheme's page — the trumpet the Android launcher, the desktop's
favicon and the docs carry (Pictogrammers' Material Design Icons, Apache
2.0), turned 18 degrees, in the dark scheme's gold — placed as the favicon
places it (harken/iced/web/favicon.svg), at twice its size. Rasterized here
from the path itself: its curves flattened to lines, each pixel's coverage
counted by scanlines, four to a pixel, each span's ends to the fraction, and
the fill the nonzero winding's, as an SVG's is. Run from harken/ios:

    python3 tools/icon.py Harken/Assets.xcassets/AppIcon.appiconset/icon-1024.png
"""
import math
import re
import struct
import sys
import zlib

N = 1024
GROUND = (0x1E, 0x1E, 0x1E)
GOLD = (0xE9, 0xBB, 0x45)
# The trumpet, in its 24-unit box (mdi-trumpet), and where the favicon puts it:
# translate(248 262) rotate(-18) scale(13.1) translate(-12 -12) on 512, doubled.
TRUMPET = (
    "M22 6C21 11 15 11 15 11H4C3 11 2 10 2 10H1V14H2C2 14 3 13 4 13H4.3C4.1 13.3 4 13.6 4 14V16C4 17.1 4.9 18 6 18H7V19"
    "H9V18H10V19H12V18H13V19H15V18H16C17.1 18 18 17.1 18 16V14C18 13.9 18 13.7 17.9 13.6C19.6 14.2 21.4 15.4 22 18H23V6H22"
    "M6 16.5C5.7 16.5 5.5 16.3 5.5 16V14C5.5 13.7 5.7 13.5 6 13.5H7V16.5H6M9 16.5V13.5H10V16.5H9M12 16.5V13.5H13V16.5H12"
    "M16.5 16C16.5 16.3 16.3 16.5 16 16.5H15V13.5H16C16.3 13.5 16.5 13.7 16.5 14V16M9 10H7V9H9V10M12 10H10V9H12V10M15 10H13V9H15V10Z"
)
CENTRE = (496.0, 524.0)
TURN = -18.0
SCALE = 26.2
SUB = 4


def placed(x, y):
    """A point of the trumpet's box on the icon: the favicon's transform, doubled."""
    x, y = (x - 12) * SCALE, (y - 12) * SCALE
    a = math.radians(TURN)
    return (CENTRE[0] + x * math.cos(a) - y * math.sin(a), CENTRE[1] + x * math.sin(a) + y * math.cos(a))


def segments(path):
    """The path's outline as line segments on the icon: M, C, H, V and Z, absolute, as the trumpet is spelt."""
    out = []
    tokens = re.findall(r"[MCHVZ]|-?[\d.]+", path)
    i = 0
    start = at = (0.0, 0.0)

    def line(a, b):
        out.append((placed(*a), placed(*b)))

    while i < len(tokens):
        c = tokens[i]
        i += 1
        if c == "M":
            start = at = (float(tokens[i]), float(tokens[i + 1]))
            i += 2
        elif c == "C":
            p1, p2, p3 = [(float(tokens[i + k]), float(tokens[i + k + 1])) for k in (0, 2, 4)]
            i += 6
            prev = at
            for k in range(1, 17):
                t = k / 16
                u = 1 - t
                p = (
                    u * u * u * at[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t * t * t * p3[0],
                    u * u * u * at[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t * t * t * p3[1],
                )
                line(prev, p)
                prev = p
            at = p3
        elif c == "H":
            to = (float(tokens[i]), at[1])
            i += 1
            line(at, to)
            at = to
        elif c == "V":
            to = (at[0], float(tokens[i]))
            i += 1
            line(at, to)
            at = to
        elif c == "Z":
            line(at, start)
            at = start
        if (i >= len(tokens) or tokens[i] == "M") and at != start:
            # A subpath left open is closed, as a fill closes it.
            line(at, start)
            at = start
    return out


def coverage(segs):
    """How much of each pixel the outline encloses, 0 to 1, row by row."""
    rows = [[0.0] * N for _ in range(N)]
    for sy in range(N * SUB):
        y = (sy + 0.5) / SUB
        crossings = []
        for (x0, y0), (x1, y1) in segs:
            if (y0 <= y < y1) or (y1 <= y < y0):
                crossings.append((x0 + (y - y0) * (x1 - x0) / (y1 - y0), 1 if y1 > y0 else -1))
        if not crossings:
            continue
        crossings.sort()
        row = rows[sy // SUB]
        winding = 0
        for k, (x, d) in enumerate(crossings):
            was = winding
            winding += d
            if was == 0 and winding != 0:
                xa = x
            elif was != 0 and winding == 0:
                xa, xb = max(xa, 0.0), min(x, float(N))
                if xb <= xa:
                    continue
                a, b = int(xa), int(xb)
                if a == b:
                    row[a] += (xb - xa) / SUB
                else:
                    row[a] += (a + 1 - xa) / SUB
                    for px in range(a + 1, b):
                        row[px] += 1 / SUB
                    if b < N:
                        row[b] += (xb - b) / SUB
    return rows


def main(out):
    rows = coverage(segments(TRUMPET))
    raw = bytearray()
    for row in rows:
        raw.append(0)
        for c in row:
            c = min(c, 1.0)
            raw += bytes(round(g + (t - g) * c) for g, t in zip(GROUND, GOLD))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    main(sys.argv[1])
