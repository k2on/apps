"""An Android screen's accessibility tree, from `uiautomator dump`'s XML: where
to tap, or the tree itself to read.

    adb exec-out uiautomator dump /dev/tty | python3 -I ui.py <text> [role] [bottom]
    ... | python3 -I ui.py --field
    ... | python3 -I ui.py --tree

<text> is what a node says (its text or content description) or its id (a
resource id, whole or after its `/`, which a Compose testTag is when the
app exposes tags as ids). Prints the centre of the best match as `x y`;
nothing, and exit 1, when there is none. Exact matches come before ones
that only contain <text>; among them, one of the `role` asked for (`tab`,
`button`, `field`, `text`), then one that can be pressed (it, or what holds
it, is clickable), since a page's heading and its button often say the
same. A role is a preference, never a filter: a step that names one still
finds what it found without. With `bottom`, of all that match, the one
lowest on the screen: a mini player saying the title a row above it says.

--field is the first text field. --tree prints the screen as an outline,
a node a line, indented by depth, with what a step can name it by: what
it says, its id, its role, and whether it can be pressed. It is what each
shot's tree beside its picture is (app-android.sh), to write steps from.
"""
import re
import sys
import xml.etree.ElementTree as ET

raw = sys.stdin.read()
start = raw.find("<?xml")
if start < 0:
    sys.exit(1)
end = raw.rfind(">")
root = ET.fromstring(raw[start : end + 1])
nodes = list(root.iter("node"))
parent = {c: p for p in root.iter() for c in p}


def pressable(n):
    while n is not None:
        if n.get("clickable") == "true":
            return True
        n = parent.get(n)
    return False


def role(n):
    """What kind of thing a node is, as a step names it."""
    cls = n.get("class") or ""
    if "EditText" in cls:
        return "field"
    if "Tab" in cls or n.get("selected") == "true" and pressable(n) and _in_bar(n):
        return "tab"
    if "Button" in cls or n.get("clickable") == "true":
        return "button"
    if n.get("text"):
        return "text"
    return ""


def _in_bar(n):
    """In the bottom eighth of the screen: where a tab bar is."""
    b = bounds(n)
    r = bounds(root[0]) if len(root) else None
    return bool(b and r and b[1] > r[3] - (r[3] - r[1]) / 8)


def bounds(n):
    v = re.findall(r"\d+", n.get("bounds", ""))
    return tuple(map(int, v)) if len(v) == 4 else None


def centre(n):
    x1, y1, x2, y2 = bounds(n)
    return f"{(x1 + x2) // 2} {(y1 + y2) // 2}"


def shown(n):
    return n.get("bounds") not in (None, "[0,0][0,0]")


def names(n):
    rid = n.get("resource-id") or ""
    return [n.get("text") or "", n.get("content-desc") or "", rid, rid.rsplit("/", 1)[-1]]


def tree():
    def depth(n):
        d = 0
        while n in parent:
            n = parent[n]
            d += 1
        return d

    for n in nodes:
        said = [s for s in (n.get("text"), n.get("content-desc")) if s]
        rid = (n.get("resource-id") or "").rsplit("/", 1)[-1]
        parts = [repr(s) for s in said] + ([f"#{rid}"] if rid else []) + [f"[{role(n)}]" if role(n) else ""]
        if pressable(n) and n.get("clickable") == "true":
            parts.append("pressable")
        line = " ".join(p for p in parts if p)
        if line:
            print("  " * (depth(n) - 1) + line)


want = sys.argv[1] if len(sys.argv) > 1 else ""
if want == "--tree":
    tree()
    sys.exit(0)
if want == "--field":
    found = [n for n in nodes if n.get("class") == "android.widget.EditText" and shown(n)]
else:
    asked = sys.argv[2] if len(sys.argv) > 2 else ""
    lowest = len(sys.argv) > 3 and sys.argv[3] == "bottom"
    exact = [n for n in nodes if want in names(n) and shown(n)]
    found = exact or [n for n in nodes if any(want in s for s in names(n) if s) and shown(n)]
    found.sort(key=lambda n: (bool(asked) and role(n) != asked, not pressable(n)))
    if lowest:
        found.sort(key=lambda n: -bounds(n)[3])
if not found:
    sys.exit(1)
print(centre(found[0]))
