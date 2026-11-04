"""Where to tap on an Android screen, from `uiautomator dump`'s XML.

    adb exec-out uiautomator dump /dev/tty | python3 -I ui.py <text>
    ... | python3 -I ui.py --field

Prints the centre of the first node whose text or content description is
<text> (else the first that contains it), or with --field of the first
text field, as `x y`; nothing, and exit 1, when there is none.
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


def centre(n):
    x1, y1, x2, y2 = map(int, re.findall(r"\d+", n.get("bounds", "")))
    return f"{(x1 + x2) // 2} {(y1 + y2) // 2}"


def shown(n):
    return n.get("bounds") not in (None, "[0,0][0,0]")


want = sys.argv[1]
if want == "--field":
    found = [n for n in nodes if n.get("class") == "android.widget.EditText" and shown(n)]
else:
    said = lambda n: (n.get("text") or "", n.get("content-desc") or "")
    found = [n for n in nodes if want in said(n) and shown(n)] or [
        n for n in nodes if any(want in s for s in said(n)) and shown(n)
    ]
if not found:
    sys.exit(1)
print(centre(found[0]))
