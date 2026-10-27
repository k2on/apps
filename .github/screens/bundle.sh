#!/usr/bin/env bash
# The macOS job's tools as one folder that needs no Nix: the binaries the
# flake built, each with every library it loads from /nix/store copied
# beside it and its load commands pointed there, then signed again (an
# arm64 binary will not run once changed unsigned). The job keeps the
# folder in GitHub's cache, keyed on the flake, so a run whose flake is
# unchanged restores it and installs no Nix at all.
#
#   bundle.sh <out> <bin dir…>
#
# Fails if anything in <out> still names /nix/store.
set -euo pipefail
out=$1
shift
mkdir -p "$out/bin" "$out/lib"

# What a Mach-O file loads from the store.
deps() { otool -L "$1" | awk 'NR > 1 && $1 ~ /^\/nix\/store\// {print $1}'; }

# Copy every store library `file` reaches into lib/, once each.
gather() {
  local dep name
  for dep in $(deps "$1"); do
    name=$(basename "$dep")
    [ -e "$out/lib/$name" ] && continue
    cp -L "$dep" "$out/lib/$name"
    chmod u+w "$out/lib/$name"
    gather "$out/lib/$name"
  done
}

# Point `file`'s store loads at lib/, by `prefix`.
relink() {
  local file=$1 prefix=$2 dep
  for dep in $(deps "$file"); do
    install_name_tool -change "$dep" "$prefix/$(basename "$dep")" "$file"
  done
}

for dir in "$@"; do
  for tool in "$dir"/*; do
    [ -f "$tool" ] && [ -x "$tool" ] || continue
    cp -L "$tool" "$out/bin/"
    chmod u+w "$out/bin/$(basename "$tool")"
    gather "$out/bin/$(basename "$tool")"
  done
  # A tool's own resources beside its bin/ (XcodeGen's share/xcodegen).
  [ -d "$dir/../share" ] && cp -RL "$dir/../share" "$out/"
done
chmod -R u+w "$out"

for lib in "$out"/lib/*.dylib; do
  install_name_tool -id "@loader_path/$(basename "$lib")" "$lib"
  relink "$lib" "@loader_path"
done
for tool in "$out"/bin/*; do
  relink "$tool" "@executable_path/../lib"
done
for file in "$out"/lib/*.dylib "$out"/bin/*; do
  codesign --force --sign - "$file" 2>/dev/null
done

left=$(for file in "$out"/lib/*.dylib "$out"/bin/*; do deps "$file"; done | sort -u)
if [ -n "$left" ]; then
  echo "::error::still loads from /nix/store: $left"
  exit 1
fi
echo "bundled: $(ls "$out/bin" | tr '\n' ' ')($(ls "$out/lib" | wc -l | tr -d ' ') libraries, $(du -sh "$out" | cut -f1))"
