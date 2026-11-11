"""An app's hero for the README: its logo and name over its latest
screenshots, the desktop window in a laptop and the phones' in phones,
one picture for a light page and one for a dark.

    hero.py <app>/demo/shots.json <branch>/apps/<app> <font.ttc>

Reads the app's `hero` in shots.json: its name, a line under it, its
logo (a path from the repository's root) and the screen to show, the
same on every device unless `screens` names one per device. Writes
hero-light.png and hero-dark.png beside the app's captures. A device with
no capture of that screen is left out and the rest are laid out without
it; with none at all, nothing is written.

The devices are drawn here, rounded bodies and bezels from the
captures' own device.json (the corner radius and the cutout the docs
site draws too), not anybody's artwork. Run from the repository's root.
"""

import json
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

# The picture is drawn at twice the size it is shown and shrunk at the end,
# so every curve is smooth without drawing anything antialiased by hand.
SS = 2
# How wide the picture is, in the pixels it is written at.
WIDTH = 2400

# Each theme's colours: the ground (top to foot), the name and the line
# under it, a phone's frame, a laptop's lid and its base.
THEMES = {
    "light": {
        "ground": ((246, 246, 248), (232, 233, 237)),
        "name": (29, 29, 31),
        "line": (110, 110, 115),
        "frame": (58, 58, 62),
        "lid": (36, 36, 38),
        "base": ((222, 223, 227), (184, 185, 190)),
        "lip": (150, 151, 156),
        "shadow": 0.28,
    },
    "dark": {
        "ground": ((26, 26, 30), (10, 10, 12)),
        "name": (245, 245, 247),
        "line": (152, 152, 160),
        "frame": (92, 92, 98),
        "lid": (20, 20, 22),
        "base": ((120, 121, 127), (78, 79, 84)),
        "lip": (58, 59, 63),
        "shadow": 0.6,
    },
}


def px(v):
    """A length in the final picture's pixels, in the drawing's."""
    return int(round(v * SS))


def rounded_mask(size, radius):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, size[0] - 1, size[1] - 1), radius=radius, fill=255)
    return m


def gradient(size, top, foot):
    """A vertical gradient from `top` to `foot`."""
    w, h = size
    col = Image.new("RGB", (1, h))
    for y in range(h):
        t = y / max(h - 1, 1)
        col.putpixel((0, y), tuple(int(a + (b - a) * t) for a, b in zip(top, foot)))
    return col.resize((w, h))


def shadow(canvas, box, radius, strength, spread):
    """A soft shadow under `box` on `canvas`, as if lit from above."""
    x0, y0, x1, y1 = box
    pad = spread * 3
    layer = Image.new("L", (x1 - x0 + 2 * pad, y1 - y0 + 2 * pad), 0)
    ImageDraw.Draw(layer).rounded_rectangle((pad, pad + spread // 2, pad + x1 - x0, pad + y1 - y0 + spread // 2), radius=radius, fill=int(255 * strength))
    layer = layer.filter(ImageFilter.GaussianBlur(spread))
    black = Image.new("RGB", layer.size, (0, 0, 0))
    canvas.paste(black, (x0 - pad, y0 - pad), layer)


def phone(shot, device, height, theme):
    """A phone `height` tall holding `shot`: the body, its frame, the
    bezel, the screen rounded as the device's own and its cutout."""
    sw, sh = shot.size
    scale = height / sh
    bezel = max(px(9), int(sw * scale * 0.032))
    frame = px(3)
    screen = (int(sw * scale), int(height))
    corner = int(device.get("corner", 100) * scale)
    body = (screen[0] + 2 * (bezel + frame), screen[1] + 2 * (bezel + frame))
    out = Image.new("RGBA", body, (0, 0, 0, 0))
    d = ImageDraw.Draw(out)
    outer = corner + bezel + frame
    d.rounded_rectangle((0, 0, body[0] - 1, body[1] - 1), radius=outer, fill=theme["frame"] + (255,))
    d.rounded_rectangle((frame, frame, body[0] - 1 - frame, body[1] - 1 - frame), radius=outer - frame, fill=(8, 8, 9, 255))
    picture = shot.convert("RGB").resize(screen, Image.LANCZOS)
    out.paste(picture, (bezel + frame, bezel + frame), rounded_mask(screen, corner))
    # The cutout, where the camera is.
    cx = body[0] // 2
    top = bezel + frame
    if device.get("cutout") == "island":
        w, h = int(screen[0] * 0.31), int(screen[0] * 0.092)
        y = top + int(screen[0] * 0.027)
        d.rounded_rectangle((cx - w // 2, y, cx + w // 2, y + h), radius=h // 2, fill=(0, 0, 0, 255))
    elif device.get("cutout") == "hole":
        r = int(screen[0] * 0.022)
        y = top + int(screen[0] * 0.045)
        d.ellipse((cx - r, y - r, cx + r, y + r), fill=(0, 0, 0, 255))
    return out, outer


def laptop(shot, width, theme):
    """A laptop whose screen is `width` wide holding `shot`: the lid with
    its bezel and camera, and the base under it, wider, with the notch a
    thumb opens it by."""
    sw, sh = shot.size
    screen = (int(width), int(width * sh / sw))
    bezel, chin = px(14), px(14)
    lid = (screen[0] + 2 * bezel, screen[1] + bezel + chin)
    base_w, base_h = int(lid[0] * 1.14), px(22)
    out = Image.new("RGBA", (base_w, lid[1] + base_h), (0, 0, 0, 0))
    d = ImageDraw.Draw(out)
    left = (base_w - lid[0]) // 2
    # The lid, edged in the frame's colour so it stands off a dark ground.
    d.rounded_rectangle((left, 0, left + lid[0] - 1, lid[1] + px(10)), radius=px(22), fill=theme["frame"] + (255,))
    d.rounded_rectangle((left + px(2), px(2), left + lid[0] - 1 - px(2), lid[1] + px(10)), radius=px(20), fill=theme["lid"] + (255,))
    picture = shot.convert("RGB").resize(screen, Image.LANCZOS)
    out.paste(picture, (left + bezel, bezel), rounded_mask(screen, px(4)))
    cam = px(2.5)
    d.ellipse((base_w // 2 - cam, bezel // 2 - cam, base_w // 2 + cam, bezel // 2 + cam), fill=(48, 48, 52, 255))
    # The base: a slab, rounded underneath, its top edge lit.
    base = gradient((base_w, base_h), *theme["base"]).convert("RGBA")
    m = Image.new("L", (base_w, base_h), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, -base_h, base_w - 1, base_h - 1), radius=base_h, fill=255)
    out.paste(base, (0, lid[1]), m)
    notch = int(base_w * 0.13)
    d.rounded_rectangle((base_w // 2 - notch // 2, lid[1] - px(1), base_w // 2 + notch // 2, lid[1] + px(7)), radius=px(6), fill=theme["lip"] + (255,))
    d.line((px(6), lid[1], base_w - px(6), lid[1]), fill=tuple(min(255, c + 22) for c in theme["base"][0]) + (255,), width=px(1.5))
    return out, left


def icon(path, side):
    """The app's icon, `side` across, rounded as a home screen rounds it;
    an SVG (an Ark app's icon.svg) drawn by rsvg-convert first."""
    if path.endswith(".svg"):
        import io
        import subprocess
        png = subprocess.run(["rsvg-convert", "-w", str(side * 2), "-h", str(side * 2), path], check=True, capture_output=True).stdout
        path = io.BytesIO(png)
    img = Image.open(path).convert("RGB").resize((side, side), Image.LANCZOS)
    out = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    out.paste(img, (0, 0), rounded_mask((side, side), int(side * 0.225)))
    return out


def font(path, size, weight):
    """Inter at `size` in `weight`, by its name in the collection; the
    file's own face when it is not a collection."""
    for i in range(32):
        try:
            f = ImageFont.truetype(path, size, index=i)
        except OSError:
            break
        if f.getname()[1] == weight and f.getname()[0] == "Inter":
            return f
    return ImageFont.truetype(path, size)


def capture(app_dir, device, theme, screen):
    path = app_dir / device / theme / f"{screen}.png"
    if not path.is_file():
        return None, {}
    meta = app_dir / device / "device.json"
    return Image.open(path), (json.loads(meta.read_text()) if meta.is_file() else {})


def hero(conf, app_dir, theme_name, font_path):
    t = THEMES[theme_name]
    screens = {d: conf.get("screens", {}).get(d, conf["screen"]) for d in ("desktop", "android", "ios")}
    desk, _ = capture(app_dir, "desktop", theme_name, screens["desktop"])
    andr, andr_dev = capture(app_dir, "android", theme_name, screens["android"])
    ios, ios_dev = capture(app_dir, "ios", theme_name, screens["ios"])
    if not (desk or andr or ios):
        return None

    W = px(WIDTH)
    head = px(300)
    phone_h = px(760)
    parts = []
    if desk:
        lap, lap_left = laptop(desk, px(1560), t)
        parts.append(("laptop", lap))
    phones = [(n, phone(s, d, phone_h, t)) for n, s, d in (("android", andr, andr_dev), ("ios", ios, ios_dev)) if s]

    # The laptop in the middle, the phones standing in front of its lower
    # corners; with no laptop, the phones side by side.
    if desk:
        H = head + lap.size[1] + px(150)
    else:
        H = head + phone_h + px(200)
    canvas = gradient((W, H), *t["ground"])
    if desk:
        lx = (W - lap.size[0]) // 2
        ly = head
        shadow(canvas, (lx + px(30), ly + lap.size[1] - px(40), lx + lap.size[0] - px(30), ly + lap.size[1] + px(6)), px(20), t["shadow"], px(22))
        shadow(canvas, (lx + lap_left, ly, lx + lap.size[0] - lap_left, ly + lap.size[1] - px(20)), px(22), t["shadow"] * 0.5, px(40))
        canvas.paste(lap, (lx, ly), lap)
        foot = ly + lap.size[1] + px(40)
        spots = {
            "android": lx - px(110),
            "ios": lx + lap.size[0] + px(110),
        }
        for name, (img, corner) in phones:
            x = spots[name] if name == "android" else spots[name] - img.size[0]
            y = foot - img.size[1]
            shadow(canvas, (x, y, x + img.size[0], y + img.size[1]), corner, t["shadow"], px(28))
            canvas.paste(img, (x, y), img)
    else:
        gap = px(80)
        total = sum(img.size[0] for _, (img, _) in phones) + gap * (len(phones) - 1)
        x = (W - total) // 2
        for _, (img, corner) in phones:
            y = head + px(40)
            shadow(canvas, (x, y, x + img.size[0], y + img.size[1]), corner, t["shadow"], px(28))
            canvas.paste(img, (x, y), img)
            x += img.size[0] + gap

    # The logo and the name, centred over it all, and the line under.
    d = ImageDraw.Draw(canvas)
    side = px(112)
    name_font = font(font_path, px(76), "Bold")
    line_font = font(font_path, px(30), "Regular")
    name_w = d.textlength(conf["name"], font=name_font)
    gap = px(28)
    row_w = side + gap + name_w
    x0 = int((W - row_w) // 2)
    y0 = px(56)
    logo = icon(conf["logo"], side)
    shadow(canvas, (x0, y0, x0 + side, y0 + side), int(side * 0.225), t["shadow"] * 0.6, px(10))
    canvas.paste(logo, (x0, y0), logo)
    asc, desc = name_font.getmetrics()
    d.text((x0 + side + gap, y0 + side // 2), conf["name"], font=name_font, fill=t["name"], anchor="lm")
    if conf.get("line"):
        d.text((W // 2, y0 + side + px(52)), conf["line"], font=line_font, fill=t["line"], anchor="mm")
    return canvas.resize((W // SS, H // SS), Image.LANCZOS)


def main():
    shots, app_dir, font_path = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
    conf = json.loads(shots.read_text()).get("hero")
    if not conf:
        return
    for theme in ("light", "dark"):
        img = hero(conf, app_dir, theme, font_path)
        if img is None:
            print(f"{app_dir.name}: no captures of {conf['screen']}, no hero")
            return
        out = app_dir / f"hero-{theme}.png"
        img.save(out, optimize=True)
        print(f"{out} {img.size[0]}x{img.size[1]}")


if __name__ == "__main__":
    main()
