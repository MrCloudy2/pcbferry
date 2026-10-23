"""Draw the plugin icons (no dependencies): a white ferry on a teal tile."""
import struct
import sys
import zlib
from pathlib import Path


def png(path: Path, size: int) -> None:
    s = size / 48.0
    teal, white, clear = (0, 128, 128, 255), (255, 255, 255, 255), (0, 0, 0, 0)

    def pixel(x, y):
        x, y = (x + 0.5) / s, (y + 0.5) / s  # in 48-unit space
        r = 9
        cx, cy = min(max(x, r), 48 - r), min(max(y, r), 48 - r)
        if (x - cx) ** 2 + (y - cy) ** 2 > r * r:
            return clear
        if 26 <= y <= 34 and 8 + (y - 26) * 0.9 <= x <= 40 - (y - 26) * 0.9:   # hull
            return white
        if 17 <= y < 26 and 15 <= x <= 33:                                    # cabin
            return teal if 19 <= y <= 22 and (int(x) - 17) % 5 in (0, 1, 2) and x <= 31 else white
        if 10 <= y < 17 and 25 <= x <= 29:                                    # funnel
            return white
        if 37 <= y <= 39 and (int(x / 4) % 2 == 0) and 6 <= x <= 42:          # waves
            return white
        return teal

    rows = b"".join(b"\0" + b"".join(bytes(pixel(x, y)) for x in range(size)) for y in range(size))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


if __name__ == "__main__":
    out = Path(sys.argv[1])
    for n in (24, 48, 64):
        png(out / f"pcbferry-{n}.png", n)
