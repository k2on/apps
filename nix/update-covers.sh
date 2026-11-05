# harken/seed/covers.json from the catalogue (harken/seed/catalogue.json):
# each album's cover address, the hash of its bytes and the kind of picture
# they are, for the flake's `harken-covers` to fetch each once by. Run from
# the repository's root after the catalogue moves (`update-catalogue`).
set -euo pipefail
catalogue=harken/seed/catalogue.json
out=harken/seed/covers.json
tmp=$(mktemp)
echo "[" > "$tmp"
first=1
while IFS=$'\t' read -r album url; do
  [ -n "$url" ] || continue
  key=$(printf '%s' "$url" | sha256sum | cut -c1-16)
  got=""
  for try in 1 2 3 4; do
    if got=$(nix store prefetch-file --json --name "cover-$key" "$url" 2>/dev/null); then break; fi
    got=""
    sleep $((try * 5))
  done
  [ -n "$got" ] || { echo "update-covers: $album: $url could not be fetched; left out" >&2; continue; }
  path=$(jq -r .storePath <<< "$got")
  case $(file --brief --mime-type "$path") in
    image/jpeg) ext=jpg ;;
    image/png) ext=png ;;
    image/webp) ext=webp ;;
    *) echo "update-covers: $album: $url is not a picture; left out" >&2; continue ;;
  esac
  [ $first = 1 ] || echo "," >> "$tmp"
  first=0
  jq -n --arg album "$album" --arg url "$url" --arg hash "$(jq -r .hash <<< "$got")" --arg ext "$ext" \
    '{album: $album, url: $url, hash: $hash, ext: $ext}' >> "$tmp"
  echo "update-covers: $album ($ext)" >&2
done < <(jq -r '.albums[] | [.name, .art] | @tsv' "$catalogue")
echo "]" >> "$tmp"
jq . "$tmp" > "$out"
rm -f "$tmp"
echo "update-covers: $(jq length "$out") covers in $out" >&2
