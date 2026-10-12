"""The page `packages.harken-android-screens` opens on: every screenshot in
the directory given, one row per screen and state, light beside dark.

The names are Paparazzi's, less its prefix: `<order>_<screen>_<state>-<side>.png`,
so sorting them sorts the screens the way `Screens.kt` lists them.
"""

import html
import os
import sys

out = sys.argv[1]
shots = {}
for name in sorted(os.listdir(out)):
    if not name.endswith(".png"):
        continue
    stem = name[: -len(".png")]
    base, _, side = stem.rpartition("-")
    if side not in ("light", "dark"):
        continue
    shots.setdefault(base, {})[side] = name

rows = []
section = None
for base in sorted(shots):
    order, _, rest = base.partition("_")
    screen, _, state = rest.rpartition("_") if rest.count("_") else (rest, "", "")
    if screen != section:
        section = screen
        rows.append(f'<h2>{html.escape(screen.replace("_", " "))}</h2>')
    cells = "".join(
        f'<figure><a href="{html.escape(shots[base][side])}"><img src="{html.escape(shots[base][side])}" loading="lazy" alt="{html.escape(base)} {side}"></a>'
        f"<figcaption>{side}</figcaption></figure>"
        for side in ("light", "dark")
        if side in shots[base]
    )
    rows.append(f'<section><h3>{html.escape(state or screen)}</h3><div class="pair">{cells}</div></section>')

print(
    """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Harken screens</title>
<style>
  :root { color-scheme: light dark; --bg: #ffffff; --fg: #262626; --quiet: #737373; --line: #e6e6e6; }
  @media (prefers-color-scheme: dark) { :root { --bg: #1e1e1e; --fg: #dddddd; --quiet: #9a9a9a; --line: #343434; } }
  body { background: var(--bg); color: var(--fg); font: 15px/1.4 system-ui, sans-serif; margin: 0; padding: 16px; }
  h1 { font-size: 22px; margin: 0 0 4px; }
  p { color: var(--quiet); margin: 0 0 16px; }
  h2 { font-size: 18px; text-transform: capitalize; border-top: 1px solid var(--line); padding-top: 16px; margin: 24px 0 8px; }
  main { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 8px 24px; }
  h2 { grid-column: 1 / -1; }
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  .pair { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  figure { margin: 0; }
  img { width: 100%; height: auto; border: 1px solid var(--line); border-radius: 8px; display: block; }
  figcaption { color: var(--quiet); font-size: 12px; text-align: center; margin-top: 2px; }
</style></head><body>
<h1>Harken for Android, every screen</h1>
<p>Rendered on the JVM by Paparazzi from a fixed library, at a Pixel 5's size; light beside dark. Covers are the derived squares: nothing here talks to a server.</p>
<main>
"""
    + "\n".join(rows)
    + "\n</main></body></html>"
)
