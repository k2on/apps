# arkui-docs

arkui's documentation, drawn by arkui: an iced app that is a page in a
browser (`nix build .#arkui-docs`, published at
https://k2on.github.io/apps/arkui/) and a window on the desktop
(`cargo run -p arkui-docs`). Every example on it is the component itself, in
the renderer the apps use.

## The one place the components are described

`src/catalogue.rs` says, for every module of arkui, what it is, its API, its
rules, where harken's desktop uses it and what the phones have instead — and
lists what harken draws that arkui does not have yet. The page draws it, and
`catalogue.json` beside this file is the same data for any site or agent that
wants it without running anything (it is published beside the page).

**If you change a component in arkui, change its entry and its example here in
the same commit**, then regenerate the JSON:

    cargo run -p arkui-docs -- --json > rust/arkui-docs/catalogue.json

`cargo test -p arkui-docs` fails until you have. It checks that:

- every `pub mod` of arkui has an entry, and every entry is a module;
- an entry says harken uses it exactly when `harken/iced/src` imports it;
- `catalogue.json` is what the catalogue prints;
- the Glyphs page draws every glyph.

`src/examples.rs` is one example per module; `src/pages.rs` lays out the
pages; `src/main.rs` is the window, its sidebar (`arkui::sidebar`) and its
theme dropdown (`arkui::menu`).
