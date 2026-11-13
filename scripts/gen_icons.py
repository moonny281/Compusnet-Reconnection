"""Generate app + tray icons as PNGs (and ICO) without external deps.

Draws a rounded square with a Wi-Fi arc motif; tray variants are tinted
solid-color discs (green/yellow/red/grey). Pure zlib+struct PNG writer.
"""
import struct, zlib, math, os

SIZE = 512

def png(path, w, h, pixels):
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    raw = b"".join(b"\x00" + bytes(v for px_ in row for v in px_) for row in pixels)
    hdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    out = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", hdr)
           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(out)

def canvas(size, fill=(0,0,0,0)):
    return [[tuple(fill)] * size for _ in range(size)]

def put(px, x, y, c):
    if 0 <= x < len(px[0]) and 0 <= y < len(px):
        px[y][x] = c

def blend(dst, src):
    a = src[3] / 255
    return tuple(int(dst[i] * (1 - a) + src[i] * a) for i in range(3)) + (255,)

def fill_rect(px, x0, y0, x1, y1, c):
    for y in range(y0, y1):
        for x in range(x0, x1):
            put(px, x, y, c)

def fill_round_rect(px, x0, y0, x1, y1, r, c, aa=1.2):
    for y in range(y0, y1):
        for x in range(x0, x1):
            # signed distance to rounded rect
            cx = max(x0 + r, min(x, x1 - r))
            cy = max(y0 + r, min(y, y1 - r))
            d = math.hypot(x - cx, y - cy) - r
            if d <= 0:
                put(px, x, y, c)

def fill_circle(px, cx, cy, r, c):
    for y in range(int(cy - r - 1), int(cy + r + 2)):
        for x in range(int(cx - r - 1), int(cx + r + 2)):
            if math.hypot(x - cx, y - cy) <= r:
                put(px, x, y, c)

def wifi_motif(px, cx, cy, base_r, color, lw):
    """Three arcs + a dot, centered around (cx, cy)."""
    radii = (base_r, base_r * 2.0, base_r * 3.0)
    for y in range(len(px)):
        for x in range(len(px[0])):
            d = math.hypot(x - cx, y - cy)
            ang = math.degrees(math.atan2(cy - y, x - cx))
            if not (-50 <= ang <= 130):  # upward-ish sector
                continue
            for rr in radii:
                if abs(d - rr) <= lw:
                    put(px, x, y, color)
    fill_circle(px, cx, cy - base_r * 0.1, lw, color)

def app_icon(path):
    px = canvas(SIZE)
    bg = (37, 99, 235, 255)  # blue rounded square
    fill_round_rect(px, 16, 16, SIZE - 16, SIZE - 16, 96, bg)
    wifi_motif(px, SIZE / 2, SIZE * 0.62, 52, (255, 255, 255, 255), 18)
    png(path, SIZE, SIZE, px)

def tray_icon(path, color):
    S = 128
    px = canvas(S)
    fill_circle(px, S / 2, S / 2, 60, (*color, 255))
    png(path, S, S, px)

def ico_from(pngs, path):
    """ICO wrapping multiple PNGs."""
    images = []
    for p in pngs:
        images.append(open(p, "rb").read())
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    body = b""
    for img in images:
        # width/height bytes: 0 means 256; our sizes are smaller.
        size = struct.unpack(">II", img[16:24])
        w, h = size[0], size[1]
        nplanes, bpp = 1, 32
        ilen = len(img)
        body += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0,
                            nplanes, bpp, ilen, offset)
        offset += ilen
    with open(path, "wb") as f:
        f.write(header + body + b"".join(images))

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")

app_icon(os.path.join(OUT, "icon.png"))
tray_icon(os.path.join(OUT, "tray-green.png"), (22, 163, 74))
tray_icon(os.path.join(OUT, "tray-yellow.png"), (217, 119, 6))
tray_icon(os.path.join(OUT, "tray-red.png"), (220, 38, 38))
tray_icon(os.path.join(OUT, "tray-grey.png"), (107, 114, 128))

# Bundle sizes: resize by nearest-neighbor from the 512 canvas would be crude;
# instead draw at each size directly.
def sized_icon(path, size):
    px = canvas(size)
    m = size / 512
    fill_round_rect(px, int(16 * m), int(16 * m), size - int(16 * m), size - int(16 * m),
                    int(96 * m), (37, 99, 235, 255))
    wifi_motif(px, size / 2, size * 0.62, 52 * m, (255, 255, 255, 255), 18 * m)
    png(path, size, size, px)

sized_icon(os.path.join(OUT, "32x32.png"), 32)
sized_icon(os.path.join(OUT, "128x128.png"), 128)
sized_icon(os.path.join(OUT, "128x128@2x.png"), 256)
sized_icon(os.path.join(OUT, "Square30x30Logo.png"), 30)
sized_icon(os.path.join(OUT, "Square44x44Logo.png"), 44)
sized_icon(os.path.join(OUT, "Square71x71Logo.png"), 71)
sized_icon(os.path.join(OUT, "Square150x150Logo.png"), 150)
sized_icon(os.path.join(OUT, "Square310x310Logo.png"), 310)
sized_icon(os.path.join(OUT, "StoreLogo.png"), 50)

# ICO needs its contained PNGs; use the sized ones.
ico_from([os.path.join(OUT, p) for p in ("32x32.png", "128x128.png", "128x128@2x.png")],
         os.path.join(OUT, "icon.ico"))

# Windows tray icons referenced by tauri.conf.json.
import shutil
shutil.copy(os.path.join(OUT, "tray-green.png"), os.path.join(OUT, "tray.png"))

print("icons written to", os.path.abspath(OUT))
