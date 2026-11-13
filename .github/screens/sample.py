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
    # The main thread's block: from its line to the next thread's.
    block, total, indent = [], 0, None
    for line in lines[start:]:
        if not line.strip():
            if block:
                break
            continue
        m = LINE.match(line)
        if not m:
            if block:
                break
            continue
        depth = len(m.group(1))
        if indent is None or depth <= indent:
            if block:
                break
            if "com.apple.main-thread" in m.group(3):
                indent, total = depth, int(m.group(2))
                continue
            continue
        block.append((depth, int(m.group(2)), clean(m.group(3))))
    if not total:
        print("    no main thread in the sample")
        return
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
