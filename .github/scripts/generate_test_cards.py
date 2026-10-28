"""Generate deterministic 1920x1080 broadcast tuning patterns, without AI colour/geometry drift.

These are utility test patterns, not certified SMPTE/EBU measurement signals.
The PNGs contain exact RGB patches, no JPEG artefacts, gradients or artwork.
"""
from pathlib import Path
import math
import struct
import zlib

W, H = 1920, 1080
OUT = Path(__file__).resolve().parents[2] / "crates/lb-engine/assets/backgrounds/stills/maintenance"


class Card:
    def __init__(self, color):
        self.data = bytearray(bytes(color) * (W * H))

    def rect(self, x, y, w, h, c):
        x0, x1 = max(0, int(x)), min(W, int(x + w))
        row = bytes(c) * max(0, x1 - x0)
        for yy in range(max(0, int(y)), min(H, int(y + h))):
            a = (yy * W + x0) * 3
            self.data[a:a + len(row)] = row

    def save(self, name):
        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
        raw = b"".join(b"\0" + self.data[y * W * 3:(y + 1) * W * 3] for y in range(H))
        png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0))
        png += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
        (OUT / name).write_bytes(png)


def generate():
    OUT.mkdir(parents=True, exist_ok=True)
    colors = [(191, 191, 191), (191, 191, 0), (0, 191, 191), (0, 191, 0), (191, 0, 191), (191, 0, 0), (0, 0, 191)]
    bars = Card((0, 0, 0))
    for i, c in enumerate(colors):
        x, end = round(W * i / 7), round(W * (i + 1) / 7)
        bars.rect(x, 0, end - x, 720, c)
        bars.rect(x, 720, end - x, 90, colors[6-i] if i % 2 == 0 else (0, 0, 0))
    for i in range(12):
        v = round(255 * i / 11)
        bars.rect(i * 160, 810, 160, 180, (v, v, v))
    for i, v in enumerate((0, 4, 8, 12, 16, 20, 24, 32, 64, 128, 192, 255)):
        bars.rect(i * 160, 990, 160, 90, (v, v, v))
    bars.save("test-color-bars.png")

    grid = Card((96, 96, 96))
    for x in range(0, W, 60): grid.rect(x, 0, 2, H, (210, 210, 210))
    for y in range(0, H, 60): grid.rect(0, y, W, 2, (210, 210, 210))
    cx, cy, radius = W // 2, H // 2, 450
    for y in range(cy-radius, cy+radius+1):
        half = int(math.sqrt(max(0, radius * radius - (y-cy) ** 2)))
        grid.rect(cx-half, y, 2*half+1, 1, (238, 238, 238))
    for i, c in enumerate(colors): grid.rect(540+i*120, 300, 120, 180, c)
    for i in range(10):
        v = round(255 * i / 9)
        grid.rect(540+i*84, 480, 84, 120, (v, v, v))
    for i, period in enumerate((24, 16, 12, 8, 4, 2)):
        for x in range(540+i*140, 680+i*140):
            v = 0 if (x // period) % 2 == 0 else 255
            grid.rect(x, 600, 1, 160, (v, v, v))
    grid.rect(cx-1, 80, 2, H-160, (0, 0, 0))
    grid.rect(120, cy-1, W-240, 2, (0, 0, 0))
    for margin in (48, 96):
        grid.rect(margin, margin, W-2*margin, 3, (255, 255, 255))
        grid.rect(margin, H-margin-3, W-2*margin, 3, (255, 255, 255))
        grid.rect(margin, margin, 3, H-2*margin, (255, 255, 255))
        grid.rect(W-margin-3, margin, 3, H-2*margin, (255, 255, 255))
    grid.save("test-geometry.png")


if __name__ == "__main__":
    generate()
