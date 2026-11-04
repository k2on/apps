#!/usr/bin/env bash
# A server's data directory, written by main before the restructure, made
# one this build opens: its rows exported by that build (export.rs, compiled
# in a checkout of MAIN), imported by this one (`arkdb import`) under the
# schema this build's server runs, its files and sign-in key copied. The
# log starts afresh; devices refill from it. FROM is only read. The export
# is kept beside TO (TO.export.json), and the rows TO then holds are
# checked against it, row for row.
#
#   tools/from-main/migrate.sh FROM TO SCHEMA [OLD_SCHEMA]
#
# SCHEMA: the schema the new server runs, as JSON (harken's is
# harken/domain/tests/schema.json). OLD_SCHEMA: the one the old server ran,
# for a disk that keeps none beside its log.
set -euo pipefail
MAIN=${MAIN:-e680901}  # the commit the server ran; MAIN=… to say another
[ $# -ge 3 ] || { sed -n '2,13p' "$0"; exit 2; }
from=$(realpath "$1"); to=$2; schema=$(realpath "$3"); old=${4:+$(realpath "$4")}
[ ! -e "$to/log/kv" ] || { echo "$to already holds rows" >&2; exit 1; }
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'git -C "$repo" worktree remove -f -f "$work/main" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
git -C "$repo" worktree add -q --detach "$work/main" "$MAIN"
cp "$repo/tools/from-main/export.rs" "$work/main/rust/arkdb/src/bin/export-main.rs"
export=$(realpath -m "$to.export.json")
(cd "$work/main/rust" && cargo run -q --release -p arkdb --bin export-main -- "$from" $old) > "$export"
mkdir -p "$to/log"
(cd "$repo/rust" && cargo run -q --release -p arkdb -- import "$export" "$schema" --to "$(realpath "$to")")
(cd "$repo/rust" && cargo run -q --release -p arkdb -- export --from "$(realpath "$to")") > "$work/check.json"
python3 - "$export" "$work/check.json" <<'PY'
import json, sys
a, b = (json.load(open(f))["tables"] for f in sys.argv[1:])
rows = lambda t: sorted(json.dumps(r, sort_keys=True) for r in t)
bad = [t for t in sorted(set(a) | set(b)) if rows(a.get(t, [])) != rows(b.get(t, []))]
if bad:
    sys.exit(f"the rows imported are not the rows exported: {', '.join(bad)}")
print(f"{sum(map(len, b.values()))} rows in {sum(1 for v in b.values() if v)} tables, as exported")
PY
[ ! -d "$from/log/files" ] || cp -a "$from/log/files" "$to/log/"
for f in "$from"/*; do
  case $(basename "$f") in log) ;; *) cp -a "$f" "$to/" ;; esac
done
echo "$from is now $to (the export kept: $export)"
