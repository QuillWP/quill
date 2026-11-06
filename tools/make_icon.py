#!/usr/bin/env python3
"""Draw Quill's document icon: a sheet of ruled paper with its corner
folded and a bold Q, in the flat grays and black of the classic Mac.

Usage: python3 tools/make_icon.py assets/quill-document.png [size]

Plain Python (no imaging library): the picture is drawn by testing each
pixel against a few shapes, four samples a pixel for smooth edges, and
written as a PNG with zlib.
"""

import struct
import sys
import zlib


def inside_poly(x, y, pts):
    """Even-odd test: whether (x, y) is inside the polygon."""
    c = False
    n = len(pts)
    for i in range(n):
        x1, y1 = pts[i]
        x2, y2 = pts[(i + 1) % n]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            c = not c
    return c


def stroke(x, y, a, b, w):
    """Whether (x, y) is within w/2 of the segment a-b."""
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
    px, py = ax + t * dx - x, ay + t * dy - y
    return px * px + py * py <= (w / 2) ** 2


# Shapes on a 1 x 1 square.
PAGE = [(0.20, 0.08), (0.66, 0.08), (0.82, 0.24), (0.82, 0.92), (0.20, 0.92)]
FOLD = [(0.66, 0.08), (0.66, 0.24), (0.82, 0.24)]
SHADOW = [(p[0] + 0.025, p[1] + 0.025) for p in PAGE]
Q_CENTER = (0.50, 0.52)
LINES = [0.30 + 0.07 * k for k in range(9)]


def color(x, y):
    """RGBA of the icon at (x, y) in 0..1."""
    dx, dy = x - Q_CENTER[0], y - Q_CENTER[1]
    outer = (dx / 0.17) ** 2 + (dy / 0.21) ** 2 <= 1
    inner = (dx / 0.105) ** 2 + (dy / 0.14) ** 2 < 1
    if (outer and not inner) or stroke(x, y, (0.53, 0.59), (0.67, 0.75), 0.055):
        return (0, 0, 0, 255)
    if inside_poly(x, y, FOLD):
        edge = any(stroke(x, y, FOLD[i], FOLD[(i + 1) % 3], 0.012) for i in range(3))
        return (0, 0, 0, 255) if edge else (192, 192, 192, 255)
    if inside_poly(x, y, PAGE):
        edge = any(stroke(x, y, PAGE[i], PAGE[(i + 1) % 5], 0.014) for i in range(5))
        if edge:
            return (0, 0, 0, 255)
        if 0.26 < x < 0.76 and any(abs(y - l) < 0.004 for l in LINES):
            return (96, 128, 200, 255)
        return (255, 255, 255, 255)
    if inside_poly(x, y, SHADOW):
        return (64, 64, 64, 160)
    return (0, 0, 0, 0)


def render(size):
    rows = []
    for py in range(size):
        row = bytearray([0])
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in (0.25, 0.75):
                for sx in (0.25, 0.75):
                    r, g, b, a = color((px + sx) / size, (py + sy) / size)
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            a = acc[3]
            if a:
                row += bytes((acc[0] // a, acc[1] // a, acc[2] // a, a // 4))
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    return b"".join(rows)


def png(size, pixels):
    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    head = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", head)
        + chunk(b"IDAT", zlib.compress(pixels, 9))
        + chunk(b"IEND", b"")
    )


def main():
    out = sys.argv[1]
    size = int(sys.argv[2]) if len(sys.argv) > 2 else 512
    with open(out, "wb") as f:
        f.write(png(size, render(size)))


if __name__ == "__main__":
    main()
