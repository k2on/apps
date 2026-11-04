"""A pull request's app screenshots compared with main's, as the PR's comment.

    python3 -I compare.py <main apps dir> <PR dir> <owner/repo> <main ref> <PR branch> <commit>

<main apps dir> is the `screens` branch's apps/ and <PR dir> the PR's
images, both laid out <app>/<phone>/<theme>/<id>.png. A shot differs when
more than a thousandth of its pixels are more than a few percent off
(ImageMagick's `compare`, which must be on the PATH): enough that the
clock or a cursor's blink is not a change and a moved label is. Prints the
comment's Markdown: each shot that differs or is new, main's beside the
PR's, and how many are the same. Images are linked at <main ref> (the
commit main's were read at) and the PR's branch, so an old comment still
shows what it compared.
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
    try:
        off = float(r.stderr.split()[0])
    except (IndexError, ValueError):
        return True  # not comparable: a different size, or unreadable
    size = subprocess.run(["identify", "-format", "%w %h", b], capture_output=True, text=True).stdout.split()
    pixels = int(size[0]) * int(size[1]) if len(size) == 2 else 1
    return off / pixels > 0.001


changed, new, same = [], [], 0
for app, phone, theme, id in shots(pr):
    path = f"{app}/{phone}/{theme}/{id}.png"
    before = os.path.join(main, path)
    if not os.path.isfile(before):
        new.append((app, phone, theme, id, path))
    elif differs(before, os.path.join(pr, path)):
        changed.append((app, phone, theme, id, path))
    else:
        same += 1


def img(url):
    return f'<img src="{url}" width="220">'


out = ["<!-- app-screens -->", "### App screenshots", ""]
out.append(
    f"Taken at {commit[:7]} from this PR's builds, signed in to each app's own server "
    "with sample data (`.github/screens/apps.md`), and compared with main's."
)
out.append("")
if not changed and not new:
    out.append(f"No screen changed ({same} the same as main's)." if same else "No screens were taken.")
for title, rows, both in (("Changed", changed, True), ("New", new, False)):
    if not rows:
        continue
    out += ["", f"#### {title}", ""]
    out.append("| Shot | main | this PR |" if both else "| Shot | this PR |")
    out.append("| --- | --- | --- |" if both else "| --- | --- |")
    for app, phone, theme, id, path in rows:
        after = img(f"{raw}/{branch}/{path}")
        name = f"{app} · {phone} · {id} ({theme})"
        out.append(f"| {name} | {img(f'{raw}/{main_ref}/apps/{path}')} | {after} |" if both else f"| {name} | {after} |")
if (changed or new) and same:
    out += ["", f"{same} other shot{' is' if same == 1 else 's are'} the same as main's."]
out += ["", "Money's sample household is dated today, so a screen showing dates can differ from main's for that alone."]
print("\n".join(out))
