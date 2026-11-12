"""The release channels' marks (docs/releasing.md), the same for every app.

An app is published in two channels: stable (a `v*` tag's build, under the
app's own id) and unstable (every build of main, a second app beside it:
its id with `.unstable` after it, its name with " Unstable" after it, and
its icon banded in the theme's caution colours). Nothing here knows any
app: each command is handed the app's own files and derives the rest from
them, so an app that joins the flake's `publish` list has both channels
(nix/channels.nix; an app opts out with app.toml's `[channels]`).

    channel.py android RES MANIFEST APPID THEME
        an Android project's resources and manifest made the unstable
        app's, in place: the application's label (its string resource)
        with " Unstable" after it; each adaptive launcher icon's
        foreground and monochrome layers with the band over them; and each
        content provider's authority under the app's own id moved under
        APPID.unstable, so that the two apps install side by side.
    channel.py badge SVG THEME OUT
        an icon (its SVG) with the band over it, at the same place.
    channel.py fdroid-yml YML OUT
        an app's F-Droid metadata made the unstable app's: its Name with
        " Unstable" after it and a paragraph saying what the channel is.
    channel.py web DIR OUT BASE THEME [unstable]
        a browser page (index.html beside pkg/) served under BASE (such as
        /apps/unstable/<app>/): its module, wasm and icon fetched from
        there and its router told so (`data-ark-base`); and with
        `unstable`, its title marked and its icon banded.
    channel.py check NAMES FILE...
        that none of the comma-separated NAMES (each app's directory, id
        and name) is written in any FILE: the channel code stays the same
        for every app (flake.nix's `checks.channels`).
"""

import os
import re
import shutil
import sys
import tomllib

SUFFIX = ".unstable"
LABEL = " Unstable"

# The band: across the icon below its middle, where every launcher's mask
# still shows it (the adaptive icon's safe zone is the 66dp circle in the
# middle of its 108). On the 108 canvas from 70 to 82; on an icon drawn
# whole (F-Droid's, the favicon), the same place in its visible 72.
ADAPTIVE = (108, 70, 82)
WHOLE = (0.72, 0.89)


def fail(why):
    print(f"channel.py: {why}", file=sys.stderr)
    sys.exit(1)


def colours(theme):
    """The band's colours: the theme's caution and what is drawn on it
    (ui/theme.toml's [signal.dark], the brighter of the two)."""
    with open(theme, "rb") as f:
        signal = tomllib.load(f)["signal"]["dark"]
    return signal["caution"], signal["on-caution"]


def stripes(x0, x1, y0, y1):
    """The band's stripes as one path: parallelograms leaning right, each
    half the band's height wide, from before x0 to past x1 (the band clips
    them)."""
    h = y1 - y0
    w = h / 2
    out = []
    x = x0 - h
    while x < x1 + h:
        out.append(f"M{x:.2f} {y1:.2f}L{x + w:.2f} {y1:.2f}L{x + w + h:.2f} {y0:.2f}L{x + h:.2f} {y0:.2f}Z")
        x += 2 * w
    return "".join(out)


def band_vector(theme):
    """The band as an Android vector drawable on the adaptive canvas."""
    ground, ink = colours(theme)
    size, y0, y1 = ADAPTIVE
    rect = f"M0 {y0}H{size}V{y1}H0Z"
    return f"""<?xml version="1.0" encoding="utf-8"?>
<!-- The unstable channel's band (nix/channel.py), generated. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="{size}dp" android:height="{size}dp"
    android:viewportWidth="{size}" android:viewportHeight="{size}">
    <path android:fillColor="{ground}" android:pathData="{rect}" />
    <group>
        <clip-path android:pathData="{rect}" />
        <path android:fillColor="{ink}" android:pathData="{stripes(0, size, y0, y1)}" />
    </group>
</vector>
"""


def android(res, manifest, appid, theme):
    with open(manifest) as f:
        man = f.read()
    # The label: the application's, a string resource.
    m = re.search(r"<application\b[^>]*?android:label=\"@string/([A-Za-z0-9_]+)\"", man, re.S)
    if not m:
        fail(f"{manifest}: the application's label is no @string resource")
    label = m.group(1)
    found = 0
    for d in sorted(os.listdir(res)):
        if not (d == "values" or d.startswith("values-")):
            continue
        for name in sorted(os.listdir(os.path.join(res, d))):
            p = os.path.join(res, d, name)
            with open(p) as f:
                text = f.read()
            text, n = re.subn(r'(<string name="' + label + r'"[^>]*>)(.*?)(</string>)', lambda m: m.group(1) + m.group(2) + LABEL + m.group(3), text, flags=re.S)
            if n:
                found += n
                with open(p, "w") as f:
                    f.write(text)
    if not found:
        fail(f"{res}: no string {label}, the application's label")

    # The launcher icons: each adaptive icon's foreground and monochrome
    # drawn with the band over them.
    os.makedirs(os.path.join(res, "drawable"), exist_ok=True)
    with open(os.path.join(res, "drawable", "ark_unstable_band.xml"), "w") as f:
        f.write(band_vector(theme))
    icons = 0
    layered = set()
    for d in sorted(os.listdir(res)):
        if not d.startswith("mipmap-anydpi"):
            continue
        for name in sorted(os.listdir(os.path.join(res, d))):
            p = os.path.join(res, d, name)
            with open(p) as f:
                text = f.read()
            if "<adaptive-icon" not in text:
                continue

            def layer(m):
                layered.add(m.group(2))
                return f'{m.group(1)}@drawable/ark_unstable_{m.group(2)}"'

            text, n = re.subn(r'(<(?:foreground|monochrome)\s+android:drawable=")@drawable/([A-Za-z0-9_]+)"', layer, text)
            if not n:
                fail(f"{p}: an adaptive icon with no @drawable foreground")
            icons += 1
            with open(p, "w") as f:
                f.write(text)
    if not icons:
        fail(f"{res}: no adaptive launcher icon (mipmap-anydpi*) to mark")
    for name in sorted(layered):
        with open(os.path.join(res, "drawable", f"ark_unstable_{name}.xml"), "w") as f:
            f.write(f"""<?xml version="1.0" encoding="utf-8"?>
<!-- {name} with the unstable channel's band over it (nix/channel.py), generated. -->
<layer-list xmlns:android="http://schemas.android.com/apk/res/android">
    <item android:drawable="@drawable/{name}" />
    <item android:drawable="@drawable/ark_unstable_band" />
</layer-list>
""")

    # A provider's authority is unique on the phone: one under the app's
    # id moves with it.
    man = re.sub(r'(android:authorities=")' + re.escape(appid) + r'(["\.;])', lambda m: m.group(1) + appid + SUFFIX + m.group(2), man)
    with open(manifest, "w") as f:
        f.write(man)
    print(f"channel.py: {appid}{SUFFIX}: label {label}, {icons} launcher icon(s) banded")


def viewbox(svg):
    m = re.search(r"<svg\b[^>]*>", svg, re.S)
    if not m:
        fail("no <svg> element")
    head = m.group(0)
    vb = re.search(r'viewBox="([^"]+)"', head)
    if vb:
        x, y, w, h = (float(v) for v in vb.group(1).replace(",", " ").split())
        return x, y, w, h
    w = re.search(r'\bwidth="([0-9.]+)', head)
    h = re.search(r'\bheight="([0-9.]+)', head)
    if not (w and h):
        fail("an <svg> with neither a viewBox nor a width and height")
    return 0.0, 0.0, float(w.group(1)), float(h.group(1))


def badged(svg, theme):
    ground, ink = colours(theme)
    x, y, w, h = viewbox(svg)
    y0, y1 = y + WHOLE[0] * h, y + WHOLE[1] * h
    rect = f"M{x:.2f} {y0:.2f}H{x + w:.2f}V{y1:.2f}H{x:.2f}Z"
    # Its colours inline, over any stylesheet the icon has for its own.
    band = f"""<!-- The unstable channel's band (nix/channel.py). -->
<defs><clipPath id="ark-unstable-band"><path d="{rect}"/></clipPath></defs>
<path d="{rect}" style="fill:{ground}"/>
<path d="{stripes(x, x + w, y0, y1)}" style="fill:{ink}" clip-path="url(#ark-unstable-band)"/>
"""
    end = svg.rfind("</svg>")
    if end < 0:
        fail("no </svg>")
    return svg[:end] + band + svg[end:]


def badge(src, theme, out):
    with open(src) as f:
        svg = f.read()
    with open(out, "w") as f:
        f.write(badged(svg, theme))


def fdroid_yml(src, out):
    with open(src) as f:
        lines = f.read().split("\n")
    name = None
    for i, line in enumerate(lines):
        if line.startswith("Name: "):
            name = line[len("Name: "):].strip()
            lines[i] = f"Name: {name}{LABEL}"
    if not name:
        fail(f"{src}: no Name")
    for i, line in enumerate(lines):
        if line.startswith("Description: |"):
            lines[i + 1:i + 1] = [
                f"    The unstable channel of {name}: every build of the main branch, installed beside {name} as an app of its own, with its own data and sign-in. {name} itself is the stable channel, the tagged releases.",
                "",
            ]
            break
    else:
        fail(f"{src}: no block Description")
    with open(out, "w") as f:
        f.write("\n".join(lines))


def web(src, out, base, theme, channel="stable"):
    if not (base.startswith("/") and base.endswith("/")):
        fail(f"base {base} does not start and end with /")
    shutil.copytree(src, out)
    for root, dirs, files in os.walk(out):
        for n in dirs + files:
            os.chmod(os.path.join(root, n), 0o755 if n in dirs else 0o644)
    os.chmod(out, 0o755)
    index = os.path.join(out, "index.html")
    with open(index) as f:
        page = f.read()

    def once(pattern, repl, what):
        nonlocal page
        page, n = re.subn(pattern, repl, page, flags=re.S)
        if n == 0:
            fail(f"{index}: no {what}")

    # The page's own URLs are from the site's root (ui/prebuild.py's
    # `web_index`), which here is BASE.
    once(r'(["(])/pkg/', lambda m: m.group(1) + base + "pkg/", "module under /pkg/")
    once(r'href="/favicon\.svg"', f'href="{base}favicon.svg"', "favicon")
    once(r"<html\b", f'<html data-ark-base="{base}"', "<html>")
    if channel == "unstable":
        once(r"<title>(.*?)</title>", lambda m: f"<title>{m.group(1)}{LABEL}</title>", "title")
        fav = os.path.join(out, "favicon.svg")
        if os.path.exists(fav):
            badge(fav, theme, fav)
    with open(index, "w") as f:
        f.write(page)


def check(names, files):
    words = sorted({n.strip() for n in names.split(",") if n.strip()})
    bad = []
    for path in files:
        with open(path) as f:
            for i, line in enumerate(f, 1):
                for w in words:
                    if re.search(r"(?<![A-Za-z0-9])" + re.escape(w) + r"(?![A-Za-z0-9])", line, re.I):
                        bad.append(f"{path}:{i}: names {w}: the channel code is every app's, and an app's own is its app.toml's or the flake's list")
    for b in bad:
        print(b, file=sys.stderr)
    if bad:
        sys.exit(1)
    print(f"channel.py: none of {len(words)} app names in {len(files)} file(s)")


if __name__ == "__main__":
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "android":
        android(*args)
    elif cmd == "badge":
        badge(*args)
    elif cmd == "fdroid-yml":
        fdroid_yml(*args)
    elif cmd == "web":
        web(*args)
    elif cmd == "check":
        check(args[0], args[1:])
    else:
        fail(f"no command {cmd}")
