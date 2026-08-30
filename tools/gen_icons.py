"""Generate caffeinate's two tray icons (a solid coffee bean).

Usage: python tools/gen_icons.py

Pure Python, no dependencies. The icons are drawn in code, so there is no
third-party asset licensing to worry about.
"""
import math
import os
import struct

SIZE = 32
SS = 4  # supersampling factor per axis, used to antialias the edges

ACTIVE = (0xD9, 0x9A, 0x5B)  # active: warm amber
IDLE = (0x8C, 0x8C, 0x8C)    # idle: grey

# Bean geometry, expressed as fractions of SIZE so changing the size needs no
# recalculation.
TILT_DEG = -45.0      # tilt of the bean
SEMI_LONG = 0.44      # semi-major axis
SEMI_SHORT = 0.30     # semi-minor axis
GROOVE_WAVE = 0.075   # amplitude of the groove's S curve
GROOVE_HALF = 0.045   # half width of the groove


def in_bean(x, y):
    """(x, y) is relative to the icon centre. True if the point is on the bean
    body, with the groove excluded."""
    cos_t = math.cos(math.radians(TILT_DEG))
    sin_t = math.sin(math.radians(TILT_DEG))
    # Rotate into the bean's own frame: u along the major axis, v across it
    u = x * cos_t + y * sin_t
    v = -x * sin_t + y * cos_t

    a = SEMI_LONG * SIZE
    b = SEMI_SHORT * SIZE
    if (u / a) ** 2 + (v / b) ** 2 > 1.0:
        return False

    # Cut out the S-shaped groove down the middle
    groove_center = GROOVE_WAVE * SIZE * math.sin(math.pi * u / a)
    return abs(v - groove_center) > GROOVE_HALF * SIZE


def render(rgb):
    """Render a SIZE x SIZE BGRA image, bottom-up as BMP expects."""
    r, g, b = rgb
    center = (SIZE - 1) / 2.0
    samples = SS * SS
    rows = []
    for py in range(SIZE - 1, -1, -1):
        row = bytearray()
        for px in range(SIZE):
            hits = 0
            for sy in range(SS):
                for sx in range(SS):
                    x = px + (sx + 0.5) / SS - 0.5 - center
                    y = py + (sy + 0.5) / SS - 0.5 - center
                    if in_bean(x, y):
                        hits += 1
            if hits:
                row += bytes([b, g, r, int(round(255 * hits / samples))])
            else:
                row += b"\x00\x00\x00\x00"
        rows.append(bytes(row))
    return b"".join(rows)


def write_ico(path, rgb):
    xor_mask = render(rgb)
    and_mask = b"\x00" * (4 * SIZE)  # 1bpp, 32 bits per row = 4 bytes, all opaque

    # BITMAPINFOHEADER: the height field must be doubled (XOR + AND masks)
    header = struct.pack(
        "<IiiHHIIiiII", 40, SIZE, SIZE * 2, 1, 32, 0, len(xor_mask), 0, 0, 0, 0
    )
    image = header + xor_mask + and_mask

    ico = struct.pack("<HHH", 0, 1, 1)  # reserved, type=icon, count=1
    ico += struct.pack("<BBBBHHII", SIZE, SIZE, 0, 0, 1, 32, len(image), 22)
    ico += image

    with open(path, "wb") as f:
        f.write(ico)


def main():
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    assets = os.path.join(root, "assets")
    os.makedirs(assets, exist_ok=True)
    write_ico(os.path.join(assets, "active.ico"), ACTIVE)
    write_ico(os.path.join(assets, "idle.ico"), IDLE)
    print("wrote assets/active.ico and assets/idle.ico")


if __name__ == "__main__":
    main()
