"""Generate caffeinate's artwork: a solid coffee bean.

Usage: python tools/gen_icons.py

Writes the two tray icons the build embeds as Win32 resources, plus a larger
PNG for the README. Pure Python, no dependencies, so there is no third-party
artwork to license and the whole thing is reproducible from source.
"""
import math
import os
import struct
import zlib

ICON_SIZE = 32   # what Windows loads from the executable's resources
README_SIZE = 256

SS = 4  # supersampling factor per axis, used to antialias the edges

ACTIVE = (0xD9, 0x9A, 0x5B)  # active: warm amber
IDLE = (0x8C, 0x8C, 0x8C)    # idle: grey

# Bean geometry, expressed as fractions of the canvas so any size works.
TILT_DEG = -45.0      # tilt of the bean
SEMI_LONG = 0.44      # semi-major axis
SEMI_SHORT = 0.30     # semi-minor axis
GROOVE_WAVE = 0.075   # amplitude of the groove's S curve
GROOVE_HALF = 0.045   # half width of the groove


def in_bean(x, y, size):
    """(x, y) is relative to the canvas centre. True if the point is on the bean
    body, with the groove excluded."""
    cos_t = math.cos(math.radians(TILT_DEG))
    sin_t = math.sin(math.radians(TILT_DEG))
    # Rotate into the bean's own frame: u along the major axis, v across it
    u = x * cos_t + y * sin_t
    v = -x * sin_t + y * cos_t

    a = SEMI_LONG * size
    b = SEMI_SHORT * size
    if (u / a) ** 2 + (v / b) ** 2 > 1.0:
        return False

    # Cut out the S-shaped groove down the middle
    groove_center = GROOVE_WAVE * size * math.sin(math.pi * u / a)
    return abs(v - groove_center) > GROOVE_HALF * size


def coverage(size):
    """Alpha for every pixel, row 0 at the top, as a flat list of 0..255."""
    center = (size - 1) / 2.0
    samples = SS * SS
    out = []
    for py in range(size):
        for px in range(size):
            hits = 0
            for sy in range(SS):
                for sx in range(SS):
                    x = px + (sx + 0.5) / SS - 0.5 - center
                    y = py + (sy + 0.5) / SS - 0.5 - center
                    if in_bean(x, y, size):
                        hits += 1
            out.append(round(255 * hits / samples))
    return out


def write_ico(path, rgb, size=ICON_SIZE):
    r, g, b = rgb
    alpha = coverage(size)

    # BMP stores rows bottom-up
    rows = []
    for py in range(size - 1, -1, -1):
        row = bytearray()
        for px in range(size):
            a = alpha[py * size + px]
            row += bytes([b, g, r, a]) if a else b"\x00\x00\x00\x00"
        rows.append(bytes(row))
    xor_mask = b"".join(rows)
    and_mask = b"\x00" * (4 * size)  # 1bpp, 32 bits per row = 4 bytes, all opaque

    # BITMAPINFOHEADER: the height field must be doubled (XOR + AND masks)
    header = struct.pack(
        "<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(xor_mask), 0, 0, 0, 0
    )
    image = header + xor_mask + and_mask

    ico = struct.pack("<HHH", 0, 1, 1)  # reserved, type=icon, count=1
    ico += struct.pack("<BBBBHHII", size, size, 0, 0, 1, 32, len(image), 22)
    ico += image

    with open(path, "wb") as f:
        f.write(ico)


def write_png(path, rgb, size=README_SIZE):
    r, g, b = rgb
    alpha = coverage(size)

    raw = bytearray()
    for py in range(size):
        raw.append(0)  # filter type 0 (none) for this scanline
        for px in range(size):
            a = alpha[py * size + px]
            raw += bytes([r, g, b, a]) if a else b"\x00\x00\x00\x00"

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")

    with open(path, "wb") as f:
        f.write(png)


def main():
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    assets = os.path.join(root, "assets")
    os.makedirs(assets, exist_ok=True)

    write_ico(os.path.join(assets, "active.ico"), ACTIVE)
    write_ico(os.path.join(assets, "idle.ico"), IDLE)
    write_png(os.path.join(assets, "icon.png"), ACTIVE)

    print("wrote assets/active.ico, assets/idle.ico, assets/icon.png")


if __name__ == "__main__":
    main()
