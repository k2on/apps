# docs

The documentation site: for now arkui's, and ArkDB's can join it.

An iced app that is a page in a browser (`nix build .#docs`, published at
https://k2on.github.io/apps/docs/) and a window on the desktop
(`cargo run` here). It is organised by component — a menu, cards, a
list — and each page says how to use it:

- **Desktop**: arkui's modules for it, a working example drawn by arkui
  itself in the renderer the apps use, each module's API and rules.
- **Mobile**, with a switch between **Android** and **iOS**: where the
  Compose or SwiftUI version is and how to call it, and the latest capture
  from a real emulator or simulator — a screenshot, or a recording played as
  frames — inside a frame of that device at its own dimensions.

It works at a phone's width: below 760 pixels the sidebar becomes a dropdown
in the top bar, examples wrap, and a finger scrolls the page.

## A workspace of its own

`docs/site` is a Cargo workspace on its own, with its own `Cargo.lock`, and
not a member of `rust/`'s. So the docs and the apps never share a build: an
edit here is no input to ArkDB's kernel or to any app, and the kernel moving
does not rebuild the docs. The docs reach the rest by path — `arkui` now,
and ArkDB's crates when their docs join — so they rebuild exactly when
something they name changes. Its checks are `nix build .#checks.<system>.docs`
(format, clippy, tests); the page is `nix build .#docs`.

## The one place the components are described

`src/components.rs` lists the components: each one's desktop modules, its
Android and iOS halves, and the captures the phones take of it. `src/catalogue.rs`
describes every arkui module. The page draws both, and `catalogue.json`
beside this file is the same data for any site or agent that wants it without
running anything (it is published beside the page).

**If you change a component, change its entry here in the same commit**, then
regenerate the JSON:

    cargo run -- --json > catalogue.json        # in docs/site

`cargo test` here fails until you have. It checks that:

- every `pub mod` of arkui has an entry, and every entry is a module;
- every module is in exactly one component;
- no two captures share an id;
- `catalogue.json` is what the catalogue prints;
- the Icons page draws every glyph.

## The captures

`src/screens.rs` reads `manifest.json` from the `screens` branch through
raw.githubusercontent.com, which any page may fetch, so a capture appears on
the next load with nothing redeployed. To try the page against captures of
your own, serve a directory laid out like the branch and build with
`DOCS_SCREENS=http://localhost:8000/` set.

`src/examples.rs` is one example per component; `src/pages.rs` lays out the
pages; `src/main.rs` is the window, its sidebar (`arkui::sidebar`) and its
theme dropdown (`arkui::menu`); `web/index.html` is the page around the
canvas, with the touch scrolling.
