#!/usr/bin/env python3
"""The app's icon, drawn with nothing but the standard library: a 1024 x 1024
PNG, opaque (App Store Connect refuses an icon with alpha), a dark ground in
a soft vertical gradient and a waveform of seven rounded gold bars on it —
harken's mark, the "listen" of its name. Run from harken/ios:

    python3 tools/icon.py Harken/Assets.xcassets/AppIcon.appiconset/icon-1024.png
"""
import struct
import sys
import zlib

N = 1024
GROUND_TOP = (0x26, 0x24, 0x20)
GROUND_FOOT = (0x12, 0x11, 0x0F)
GOLD = (0xE9, 0xBB, 0x45)
GOLD_DEEP = (0xB8, 0x8A, 0x22)
# The bars: their heights as shares of the mark's height, middle tallest.
BARS = [0.30, 0.52, 0.78, 1.00, 0.78, 0.52, 0.30]
BAR_W = 68
GAP = 40
MARK_H = 560


def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def coverage(x, y, x0, y0, x1, y1, r):
    """How much of the pixel at (x, y) a rounded rectangle covers, 4x4 samples."""
    hit = 0
    for sy in range(4):
        for sx in range(4):
            px = x + (sx + 0.5) / 4
            py = y + (sy + 0.5) / 4
            if px < x0 or px > x1 or py < y0 or py > y1:
                continue
            cx = min(max(px, x0 + r), x1 - r)
            cy = min(max(py, y0 + r), y1 - r)
            if (px - cx) ** 2 + (py - cy) ** 2 <= r * r:
                hit += 1
    return hit / 16


def main(out):
    total_w = len(BARS) * BAR_W + (len(BARS) - 1) * GAP
    left = (N - total_w) / 2
    rects = []
    for i, h in enumerate(BARS):
        x0 = left + i * (BAR_W + GAP)
        hh = MARK_H * h
        y0 = (N - hh) / 2
        rects.append((x0, y0, x0 + BAR_W, y0 + hh))
    rows = []
    for y in range(N):
        ground = mix(GROUND_TOP, GROUND_FOOT, y / (N - 1))
        gold = mix(GOLD, GOLD_DEEP, max(0.0, (y - (N - MARK_H) / 2) / MARK_H))
        row = bytearray([0])
        for x in range(N):
            c = 0.0
            for (x0, y0, x1, y1) in rects:
                if x0 - 1 <= x <= x1 + 1 and y0 - 1 <= y <= y1 + 1:
                    c = coverage(x, y, x0, y0, x1, y1, BAR_W / 2)
                    break
            row += bytes(mix(ground, gold, c))
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    main(sys.argv[1])
