# The apps' seeds

Each app's demo data is a crate of its own beside the app's, `<app>/seed`:
what a demo window opens on, what the phones' demo opens on, and what a
server is seeded with for the screenshots (`.github/screens/apps.md`) or
for a person to try. Not in the app's code: the real clients and servers
are built without them. One crate per app, the same shape for each,
members of the workspace in `rust/`.

| crate | its library | its binary |
| --- | --- | --- |
| `money/seed` (`money-seed`) | the demo household: three months of a family's budget up to today, run through the domain's own mutations (`seeded_disk`, `log`, `open`, `seeded`, `Bank`, `ME`, …) | `money-seed DATA`: the household as `DATA/log`, the log a money server boots on |
| `harken/seed` (`harken-seed`) | the demo library: the classical catalogue (`catalogue.json`) and the shows (`shows.json`), embedded, and the library made of them (`catalogue`, `shelf`, `seeded_disk`) | `harken-seed [--covers DIR] DATA`: the catalogue as a media root, `DATA/media`: a FLAC of silence per track, tagged as the catalogue says, and each album's cover (from `--covers`, the flake's `harken-covers`, the covers kept in `harken/seed/covers`), which a harken server's scanner reads |

The binaries share one contract: `<app>-seed DATA` writes what that app's
server needs to boot seeded, with `DATA` its data directory, and refuses
a `DATA` that already has anything in it. From `rust/`:

```sh
cargo run -p money-seed -- /tmp/money
MONEY_DEV_AUTH=1 MONEY_DATA=/tmp/money cargo run -p money-server -- 127.0.0.1:8788

cargo run -p harken-seed -- /tmp/harken
HARKEN_DEV_AUTH=1 HARKEN_DATA=/tmp/harken HARKEN_MEDIA=/tmp/harken/media cargo run --bin harken-server -- 127.0.0.1:8787
```

The screenshots run the same from nix's builds (`nix build .#<app>-seed
.#<app>-server`, in `<app>/screens/server.sh`). For a person to try one,
`nix run .#money-demo [ADDR]` and `nix run .#harken-demo [ADDR]`
(`nix/demo.nix`) do it in one step on a fresh directory under the temp dir.
Dev auth lets anyone in as whoever they say; money's household is
`alex`'s.

The binaries are behind each crate's `cli` feature, on by default; the
demos depend with `default-features = false`, so a demo (money's and
harken's in a browser too) links the data and builds no binary.

`harken/seed/catalogue.json` is a copy of the flake's `classical` input:
`nix flake update classical`, then `nix run .#update-catalogue` writes it,
and `checks.catalogue` holds it to the input. Its covers are kept beside
it, each fetched once and made small (`harken/seed/covers`, 600 pixels at
most, about 1.8 MB for all; `nix run .#update-covers` after the catalogue
moves), and the flake's `harken-covers` is that folder: a seed
(`--covers`) fetches nothing, and every run has the same bytes. A cover
not there is fetched, and one that cannot be had is drawn in harken's own
gold squares; harken-seed runs nothing outside itself (no ffmpeg).
