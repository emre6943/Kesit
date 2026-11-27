#!/usr/bin/env python3
"""Writes a 1024x1024 app icon PNG (stdlib only): a rounded dark square with
two red crop brackets. Usage: make-icon.py OUT.png"""
import struct
import sys
import zlib

S = 1024


def inside_rounded(x, y, x0, y0, x1, y1, r):
    if x < x0 or x >= x1 or y < y0 or y >= y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def pixel(x, y):
    # Squircle-ish background with the macOS icon grid margin (~100 px).
    if not inside_rounded(x, y, 100, 100, 924, 924, 185):
        return (0, 0, 0, 0)
    t = (y - 100) / 824
    bg = (int(44 - 14 * t), int(48 - 14 * t), int(58 - 12 * t), 255)
    red = (255, 69, 58, 255)
    w = 64  # bracket thickness
    # Top-left bracket.
    if 300 <= x < 300 + w and 200 <= y < 724:
        return red
    if 200 <= y - 100 < 200 + w and 200 <= x < 724:
        return red
    # Bottom-right bracket.
    if 660 <= x < 660 + w and 300 <= y < 824:
        return red
    if 660 <= y < 660 + w and 300 <= x < 824:
        return red
    return bg


def main():
    out = sys.argv[1]
    rows = bytearray()
    for y in range(S):
        rows.append(0)
        for x in range(S):
            rows.extend(pixel(x, y))

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows), 9))
    png += chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    main()
