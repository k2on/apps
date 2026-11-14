# What the main thread of a process did, from macOS's `sample` of it
# (apps-ios.sh, SAMPLE): its call tree down to every frame that held at
# least one part in fifty of its samples, then the frames it was in itself
# (not in what they called) the most. What a slow walk is spending its time
# on, read in the log.
#
#   python3 sample.py <sample's file>
import re
import sys

LINE = re.compile(r"^(\s*[+!:| ]*?)(\d+) (.*)$")


def clean(name):
    name = re.sub(r"\s+\[0x[0-9a-f]+(,0x[0-9a-f]+)*\]", "", name)
    name = re.sub(r"\s+\+ \d+(,\d+)*$", "", name)
    name = re.sub(r"\s+\+ \d+(,\d+)*\s+\(", " (", name)
    return name.strip()


def main(path):
    lines = open(path, errors="replace").read().splitlines()
    try:
        start = lines.index("Call graph:") + 1
    except ValueError:
        print("    no call graph in the sample")
        return
    # Each thread's block: a line at the call graph's least depth, and
    # every line under it. The main thread's is the one so named, else
    # the first (sample lists it first).
    rows = []
    for line in lines[start:]:
        if not line.strip():
            if rows:
                break
            continue
        m = LINE.match(line)
        if m:
            rows.append((len(m.group(1)), int(m.group(2)), clean(m.group(3))))
        elif rows:
            break
    if not rows:
        print("    nothing in the call graph; the file begins:")
        for line in lines[:20]:
            print(f"    | {line[:200]}")
        return
    top = min(d for d, _, _ in rows)
    threads = [i for i, (d, _, _) in enumerate(rows) if d == top]
    named = [i for i in threads if re.search(r"main-thread|Main Thread", rows[i][2])]
    if not named:
        print("    no thread named the main one; the call graph begins:")
        for line in lines[start:start + 12]:
            print(f"    | {line[:200]}")
    first = (named or threads)[0]
    after = [i for i in threads if i > first]
    indent, total = rows[first][0], rows[first][1]
    block = rows[first + 1:after[0] if after else len(rows)]
    print(f"    {len(threads)} threads; this one: {rows[first][2][:120]}")
    print(f"    the main thread: {total} samples")
    least = max(1, total // 50)
    shown = 0
    for depth, n, name in block:
        if n >= least and shown < 160:
            print(f"    {' ' * min(depth - indent, 120)}{n} {name[:160]}")
            shown += 1
    # Each frame's own time: its count less its children's.
    own = {}
    for i, (depth, n, name) in enumerate(block):
        # Its children: the lines after it one level deeper, until a line
        # back at its own depth.
        kids, child = 0, None
        for d2, n2, _ in block[i + 1:]:
            if d2 <= depth:
                break
            child = d2 if child is None else child
            if d2 == child:
                kids += n2
        own[name] = own.get(name, 0) + n - kids
    print("    its own time, the most:")
    for name, n in sorted(own.items(), key=lambda kv: -kv[1])[:30]:
        print(f"      {n} {name[:160]}")


if __name__ == "__main__":
    main(sys.argv[1])
