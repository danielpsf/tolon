#!/usr/bin/env python3
"""Build Tolon's tray icons from the Peek app icon. Pure standard library, deterministic.

Usage: python3 draw.py [output_dir]   (default: this script's folder)

Source art (the poll's Peek, 2026-10-22), read from the folder above:
- icon.png: the 512 px Peek icon, area-averaged down for the 44 px macOS icon (22 pt at 2x);
- 32x32.png: the poll's hand-made 32 px menu bar mark, for the Windows and Linux trays.

Colour, never a template image, so it keeps its look on every bar. A thin cream edge (the paper
colour) outside the 44 px mark, and inside the 32 px one, keeps the dark moss square readable
on a dark bar. The paused variant has a small badge with two bars at the bottom right (AC7.3).
Writes peek-44.png, peek-44-paused.png, peek-32.png, peek-32-paused.png (RGBA).
"""
import struct
import sys
import zlib
from pathlib import Path

PAPER = (0xFA, 0xF7, 0xEE)
INK = (0x1A, 0x1A, 0x1A)


def read_png(path):
    data = path.read_bytes()
    pos, idat, w, h = 8, b"", 0, 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype = struct.unpack(">IIBB", body[:10])
            assert depth == 8 and ctype == 6, "RGBA 8-bit only"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw, stride, rows = zlib.decompress(idat), w * 4, []
    prev = bytearray(stride)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b, c = prev[i], (prev[i - 4] if i >= 4 else 0)
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line)
        prev = line
    return [[tuple(r[x * 4 : x * 4 + 4]) for x in range(w)] for r in rows]


def write_png(path, px):
    h, w = len(px), len(px[0])
    raw = b"".join(b"\0" + bytes(c for p in row for c in p) for row in px)

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def shrink(px, size):
    """Area average of premultiplied colour: exact for fractional scale factors."""
    n, out = len(px), []
    s = n / size
    for oy in range(size):
        row = []
        for ox in range(size):
            acc, wsum = [0.0] * 4, 0.0
            y0, y1, x0, x1 = oy * s, (oy + 1) * s, ox * s, (ox + 1) * s
            for y in range(int(y0), min(n, int(y1) + 1)):
                wy = min(y + 1, y1) - max(y, y0)
                for x in range(int(x0), min(n, int(x1) + 1)):
                    wgt = wy * (min(x + 1, x1) - max(x, x0))
                    if wgt <= 0:
                        continue
                    r, g, b, a = px[y][x]
                    acc[0] += r * a * wgt
                    acc[1] += g * a * wgt
                    acc[2] += b * a * wgt
                    acc[3] += a * wgt
                    wsum += wgt
            a = acc[3]
            row.append(
                (round(acc[0] / a), round(acc[1] / a), round(acc[2] / a), round(a / wsum))
                if a
                else (0, 0, 0, 0)
            )
        out.append(row)
    return out


def blend(top, bottom):
    """top over bottom, straight alpha."""
    ta, ba = top[3] / 255, bottom[3] / 255
    a = ta + ba * (1 - ta)
    if a == 0:
        return (0, 0, 0, 0)
    return tuple(round((top[i] * ta + bottom[i] * ba * (1 - ta)) / a) for i in range(3)) + (round(a * 255),)


def outer_edge(px, size, pad):
    """Centre px on a transparent canvas and put a 1 px cream edge around the opaque shape."""
    n = len(px)
    canvas = [[(0, 0, 0, 0)] * size for _ in range(size)]
    for y in range(n):
        for x in range(n):
            canvas[y + pad][x + pad] = px[y][x]
    out = [[(0, 0, 0, 0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            near = max(
                canvas[j][i][3]
                for j in range(max(0, y - 1), min(size, y + 2))
                for i in range(max(0, x - 1), min(size, x + 2))
            )
            out[y][x] = blend(canvas[y][x], (*PAPER, near * 0.85 // 1))
    return out


def rounded(px, radius):
    """Round the corners of a full-bleed square (anti-aliased) and trace a 1 px cream edge inside."""
    n = len(px)
    out = []
    for y in range(n):
        row = []
        for x in range(n):
            dx = max(radius - (x + 0.5), (x + 0.5) - (n - radius), 0)
            dy = max(radius - (y + 0.5), (y + 0.5) - (n - radius), 0)
            d = (dx * dx + dy * dy) ** 0.5
            cover = min(1.0, max(0.0, radius + 0.5 - d))
            edge = min(1.0, max(0.0, d - (radius - 1.5))) if d > 0 else 0.0
            edge = max(edge, 1.0 if x in (0, n - 1) or y in (0, n - 1) else 0.0)
            r, g, b, _ = px[y][x]
            base = blend((*PAPER, round(255 * 0.55 * edge)), (r, g, b, 255))
            row.append((*base[:3], round(255 * cover)))
        out.append(row)
    return out


def badge(px, cx, cy, r):
    """Pause badge: ink disc, cream ring, two cream bars. Anti-aliased by 4x4 supersampling."""
    n = len(px)
    for y in range(n):
        for x in range(n):
            hits = [0, 0, 0]  # ring, disc, bars
            for sy in range(4):
                for sx in range(4):
                    fx, fy = x + (sx + 0.5) / 4 - cx, y + (sy + 0.5) / 4 - cy
                    d = (fx * fx + fy * fy) ** 0.5
                    if d <= r:
                        hits[0] += 1
                    if d <= r - 1.0:
                        hits[1] += 1
                        if abs(fy) <= r * 0.42 and (r * 0.16 <= abs(fx) <= r * 0.16 + r * 0.26):
                            hits[2] += 1
            for k, rgb in enumerate((PAPER, INK, PAPER)):
                if hits[k]:
                    px[y][x] = blend((*rgb, round(255 * hits[k] / 16)), px[y][x])
    return px


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
    src = Path(__file__).parent.parent
    big = outer_edge(shrink(read_png(src / "icon.png"), 42), 44, 1)
    small = rounded(read_png(src / "32x32.png"), 7)
    write_png(out / "peek-44.png", big)
    write_png(out / "peek-32.png", small)
    write_png(out / "peek-44-paused.png", badge([r[:] for r in big], 37, 37, 6.5))
    write_png(out / "peek-32-paused.png", badge([r[:] for r in small], 26.5, 26.5, 5))


if __name__ == "__main__":
    main()
