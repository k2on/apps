"""A pull request's app screenshots compared with main's, as the PR's comment.

    python3 -I compare.py <main apps dir> <PR dir> <owner/repo> <main ref> <PR branch> <commit>

<main apps dir> is the `screens` branch's apps/ and <PR dir> the PR's
images, both laid out <app>/<phone>/<theme>/<id>.png. A shot differs when
more than a thousandth of its pixels are more than a few percent off
(ImageMagick's `compare`, 6 or 7, which must be on the PATH: the flake's): enough that the
clock or a cursor's blink is not a change and a moved label is. Prints the
comment's Markdown: each shot that differs or is new, main's beside the
PR's, and how many are the same. Images are linked at <main ref> (the
commit main's were read at) and the PR's branch, so an old comment still
shows what it compared.

A row is one screen — an app's shot in a theme — with main's and the PR's
on each phone side by side, Android's then the iPhone's, so a change on one
can be held against the other; it is shown when either phone's differs or
is new. A phone with nothing of that screen says so in its cell.
"""
import os
import subprocess
import sys

main, pr, repo, main_ref, branch, commit = sys.argv[1:7]
raw = f"https://raw.githubusercontent.com/{repo}"


def shots(root):
    for app in sorted(os.listdir(root)) if os.path.isdir(root) else []:
        for phone in sorted(os.listdir(os.path.join(root, app))):
            at = os.path.join(root, app, phone)
            if not os.path.isdir(at):
                continue
            for theme in ("light", "dark"):
                d = os.path.join(at, theme)
                for name in sorted(os.listdir(d)) if os.path.isdir(d) else []:
                    if name.endswith(".png"):
                        yield app, phone, theme, name[:-4]


def differs(a, b):
    """Whether b is not a, by enough to say."""
    r = subprocess.run(
        ["compare", "-metric", "AE", "-fuzz", "4%", a, b, "null:"],
        capture_output=True, text=True,
    )
    # How many pixels are off: ImageMagick 7 says it in brackets, after the
    # same scaled by its quantum ("6.5535e+06 (100)"); 6 said it alone.
    said = r.stderr.split()
    try:
        off = float(said[1].strip("()")) if len(said) > 1 and said[1].startswith("(") else float(said[0])
    except (IndexError, ValueError):
        return True  # not comparable: a different size, or unreadable
    size = subprocess.run(["identify", "-format", "%w %h", b], capture_output=True, text=True).stdout.split()
    pixels = int(size[0]) * int(size[1]) if len(size) == 2 else 1
    return off / pixels > 0.001


# Each screen (app, theme, id), and on each phone: "same", "changed" or "new".
screens = {}
for app, phone, theme, id in shots(pr):
    path = f"{app}/{phone}/{theme}/{id}.png"
    before = os.path.join(main, path)
    if not os.path.isfile(before):
        state = "new"
    elif differs(before, os.path.join(pr, path)):
        state = "changed"
    else:
        state = "same"
    screens.setdefault((app, theme, id), {})[phone] = state

PHONES = (("android", "Android"), ("ios", "iPhone"))


def img(url):
    return f'<img src="{url}" width="160">'


def cells(app, theme, id, phone, state):
    """main's and the PR's of one screen on one phone."""
    path = f"{app}/{phone}/{theme}/{id}.png"
    before = img(f"{raw}/{main_ref}/apps/{path}") if os.path.isfile(os.path.join(main, path)) else "—"
    # A phone this PR did not rebuild (its app's inputs for it did not move) is main's.
    after = img(f"{raw}/{branch}/{path}") if state else ("not rebuilt: as main's" if before != "—" else "—")
    return [before, after]


shown = {k: v for k, v in screens.items() if any(s != "same" for s in v.values())}
same = sum(1 for v in screens.values() for s in v.values() if s == "same") - sum(
    1 for v in shown.values() for s in v.values() if s == "same"
)

out = ["<!-- app-screens -->", "### App screenshots", ""]
out.append(
    f"Taken at {commit[:7]} from this PR's builds, signed in to each app's own server "
    "with sample data (`.github/screens/apps.md`), and compared with main's."
)
out.append("")
if not shown:
    out.append(f"No screen changed ({same} the same as main's)." if same else "No screens were taken.")
else:
    out.append("| Screen | Android, main | Android, this PR | iPhone, main | iPhone, this PR |")
    out.append("| --- | --- | --- | --- | --- |")
    for (app, theme, id), phones in sorted(shown.items()):
        said = ", ".join(f"{label} {phones[p]}" for p, label in PHONES if phones.get(p, "same") != "same")
        row = [f"**{app}** {id} ({theme})<br>{said}"]
        for p, _ in PHONES:
            row += cells(app, theme, id, p, phones.get(p))
        out.append("| " + " | ".join(row) + " |")
    if same:
        out += ["", f"{same} other shot{' is' if same == 1 else 's are'} the same as main's."]
out += ["", "Money's sample household is dated today, so a screen showing dates can differ from main's for that alone."]
print("\n".join(out))
