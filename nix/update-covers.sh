# harken/seed/covers from the catalogue (harken/seed/catalogue.json): each
# album's cover fetched, made at most 600 pixels and a JPEG, and kept under
# the name harken-seed's `media::key` gives its address (the first sixteen
# hex digits of its SHA-256), for the flake's `harken-covers`. Run from the
# repository's root after the catalogue moves (`update-catalogue`).
set -euo pipefail
catalogue=harken/seed/catalogue.json
out=harken/seed/covers
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
mkdir -p "$out"
rm -f "$out"/*.jpg
# Named as Wikimedia's policy asks a program to be, and slowly: a host that
# throttles a quick run of fetches answers a slow one.
agent="harken-seed/0.1 (https://github.com/k2on/apps)"
jq -r '.albums[] | [.name, .art] | @tsv' "$catalogue" | while IFS=$'\t' read -r album url; do
  [ -n "$url" ] || continue
  key=$(printf '%s' "$url" | sha256sum | cut -c1-16)
  got=0
  for try in 1 2 3 4; do
    if curl -fsSL -A "$agent" --max-time 60 -o "$tmp" "$url"; then got=1; break; fi
    sleep $((try * 10))
  done
  [ "$got" = 1 ] || { echo "update-covers: $album: $url could not be fetched; left out" >&2; continue; }
  magick "$tmp" -auto-orient -strip -resize '600x600>' -quality 85 "$out/$key.jpg"
  echo "update-covers: $album" >&2
  sleep 2
done
echo "update-covers: $(find "$out" -name '*.jpg' | wc -l) covers in $out" >&2
