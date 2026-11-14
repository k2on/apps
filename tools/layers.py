#!/usr/bin/env python3
"""The layers held (ui/docs/app.md, Layers): every workspace member in one
layer of layers.toml, and each of its dependencies one its layer may use.

    cargo metadata --no-deps --offline --format-version 1 | layers.py layers.toml

Reads the metadata on stdin, prints each problem, and exits 1 if there is
one. Dev-dependencies are a test's and not held; build and normal ones
are, optional or not (a feature is still the crate's).
"""

import json
import sys
import tomllib


def check(table, meta):
    layers = table["layers"]
    order = list(layers)
    layer_of = {}
    problems = []
    for name, layer in layers.items():
        for c in layer["crates"]:
            if c in layer_of:
                problems.append(f"{c} is in two layers: {layer_of[c]} and {name}")
            layer_of[c] = name
        for u in layer.get("uses", []):
            if u not in layers or order.index(u) >= order.index(name):
                problems.append(f"layer {name} uses {u}, which is no layer below it")
    members = {p["name"] for p in meta["packages"]}
    for m in sorted(members - set(layer_of)):
        problems.append(f"{m} is in no layer of layers.toml")
    for c in sorted(set(layer_of) - members):
        problems.append(f"layers.toml names {c}, which is no workspace member")
    allowed = dict(table.get("allow", {}))
    seen = set()
    for p in sorted(meta["packages"], key=lambda p: p["name"]):
        mine = layer_of.get(p["name"])
        if mine is None:
            continue
        layer = layers[mine]
        uses = set(layer.get("uses", [])) | {mine}
        external = set(layer.get("external", []))
        for d in sorted({d["name"] for d in p["dependencies"] if d.get("kind") != "dev"}):
            if d in members:
                ok = layer_of.get(d) in uses
                what = f"{d} ({layer_of.get(d)})"
            else:
                ok = "*" in external or d in external
                what = f"{d} (external)"
            if ok:
                continue
            key = f"{p['name']} -> {d}"
            if key in allowed:
                seen.add(key)
                continue
            problems.append(f"{p['name']} ({mine}) depends on {what}, which its layer may not use")
    for key in sorted(set(allowed) - seen):
        problems.append(f'layers.toml allows "{key}", which is no longer a violation: delete the line')
    return problems


def main(argv):
    with open(argv[1], "rb") as f:
        table = tomllib.load(f)
    problems = check(table, json.load(sys.stdin))
    for p in problems:
        print(p, file=sys.stderr)
    if not problems:
        print(f"layers held: {sum(len(l['crates']) for l in table['layers'].values())} crates, {len(table.get('allow', {}))} allowed until fixed")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
