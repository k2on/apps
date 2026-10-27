# docs

The documentation site: for now arkui's, and ArkDB's can join it.

A static site, generated: plain HTML and CSS with a few lines of script,
published at https://k2on.github.io/apps/docs/ (`nix build .#docs`, or
`cargo run --release -- out` here to write it into `out/`). Each page is
whole HTML, so a load is that page and nothing it does not show. It is
organised by component — a menu, cards, a list — and each page says how to
use it:

- **Desktop**: arkui's modules for it, pictures of it drawn by arkui itself,
  tables of what its functions answer (worked out when the site is built),
  and each module's API and rules.
- **Mobile**, with a switch between **Android** and **iOS**: where the
  Compose or SwiftUI version is and how to call it, and the latest capture
  from a real emulator or simulator — a screenshot, or a recording as an
  MP4 at 60 frames a second — inside a frame of that device at its own dimensions.

Below 700 pixels wide the sidebar becomes a dropdown in the top bar and the
pictures are the ones drawn at a phone's width. The theme follows the
system, or the dropdown at the top right.

## The pictures are the components

`src/render.rs` lays an iced element out at a width and draws it with
tiny-skia, iced's CPU renderer, with no window and no GPU, then cuts it to
its own height and writes it as lossless WebP. Every picture is drawn four
times: in iced's light and dark themes with arkui's palette, at 640 and at
320 logical pixels, all at twice that. The page fetches only the one it
shows. The stylesheet's colours are written from the same palette
(`site::colours`), so a picture sits on the page's own ground.

`src/examples.rs` is what each component shows: its pictures (overlays are
drawn open, over a page), its tables, and its notes. `src/site.rs` writes
the HTML; `web/style.css` and `web/site.js` are the page's style and its
script (the theme, the phone, j and k between pages, the captures).

## A workspace of its own

`docs/site` is a Cargo workspace on its own, with its own `Cargo.lock`, and
not a member of `rust/`'s. So the docs and the apps never share a build: an
edit here is no input to ArkDB's kernel or to any app, and the kernel moving
does not rebuild the docs. The docs reach the rest by path — `arkui` now,
and ArkDB's crates when their docs join — so they rebuild exactly when
something they name changes. Its checks are `nix build .#checks.<system>.docs`
(format, clippy, tests); the site is `nix build .#docs`.

## The one place the components are described

`src/components.rs` lists the components: each one's desktop modules, its
Android and iOS halves, and the captures the phones take of it. `src/catalogue.rs`
describes every arkui module. The pages say both, and `catalogue.json`
beside this file is the same data for any site or agent that wants it
(it is published beside the pages).

**If you change a component, change its entry here in the same commit**, then
regenerate the JSON:

    cargo run -- --json > catalogue.json        # in docs/site

`cargo test` here fails until you have. It checks that:

- every `pub mod` of arkui has an entry, and every entry is a module;
- every module is in exactly one component;
- no two captures, pictures or pages share a name;
- `catalogue.json` is what the catalogue prints;
- the Icons picture draws every glyph;
- every link on every page lands;
- a picture draws, and is cut to its own height.

## The captures

`web/site.js` reads `manifest.json` from the `screens` branch through
raw.githubusercontent.com, which any page may fetch, so a capture appears on
the next load with nothing redeployed. To try the site against captures of
your own, serve a directory laid out like the branch and build with
`DOCS_SCREENS=http://localhost:8000/` set.
