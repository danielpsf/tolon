#!/usr/bin/env python3
"""Draw Tolon's tray icons. Pure standard library, deterministic.

Usage: python3 draw.py [output_dir]   (default: this script's folder)

Placeholder until the logo is chosen: a ring with a dot in the middle, and a paused variant with
two bars instead of the dot. Four 44x44 RGBA PNGs, anti-aliased by 4x4 supersampling:
- template.png, template-paused.png: black and alpha only, for the macOS menu bar (template image,
  macOS tints it for light, dark and highlighted menu bars);
- color.png, color-paused.png: the Swiss Moss green, for the Windows and Linux trays.
"""
import struct
import sys
import zlib
from pathlib import Path

SIZE = 44
SAMPLES = 4
BLACK = (0, 0, 0)
MOSS = (0x4A, 0x7A, 0x52)  # --sm-moss (light)


def ring(x, y):
    d = ((x - 22) ** 2 + (y - 22) ** 2) ** 0.5
    return 15.5 <= d <= 20


def dot(x, y):
    return ((x - 22) ** 2 + (y - 22) ** 2) ** 0.5 <= 6


def bars(x, y):
    return 14 <= y <= 30 and (15.5 <= x <= 19.5 or 24.5 <= x <= 28.5)


def draw(inside, rgb):
    rows = []
    for py in range(SIZE):
        row = bytearray([0])  # filter type: none
        for px in range(SIZE):
            hits = sum(
                inside(px + (sx + 0.5) / SAMPLES, py + (sy + 0.5) / SAMPLES)
                for sx in range(SAMPLES)
                for sy in range(SAMPLES)
            )
            row += bytes(rgb) + bytes([round(255 * hits / SAMPLES**2)])
        rows.append(bytes(row))
    return png(b"".join(rows))


def png(raw):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
    shapes = {
        "": lambda x, y: ring(x, y) or dot(x, y),
        "-paused": lambda x, y: ring(x, y) or bars(x, y),
    }
    for suffix, shape in shapes.items():
        (out / f"template{suffix}.png").write_bytes(draw(shape, BLACK))
        (out / f"color{suffix}.png").write_bytes(draw(shape, MOSS))


if __name__ == "__main__":
    main()
