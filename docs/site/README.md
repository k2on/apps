# docs

The documentation site: for now arkui's, and ArkDB's can join it.

An ArkUI app, written as static HTML: published at
https://k2on.github.io/apps/docs/ (`nix build .#docs`). Every page is a
path (`/`, `/all/`, `/menu/`) and a `#[view]` in ArkUI's own shell — the
sidebar of pages by section, each page's toolbar, the search field — drawn
by arkui-web as HTML (ui/docs/app.md, "The Browser's Modes") and right
without script at every width (arkui-web writes each size class's draw and
theme.css shows the one the window is in). With script, the page's module
(`docs-web`) takes it over: the sidebar walks the pages without a load, the
search finds words on any page, the live parts answer.

Each component's page says how to use it:

- **In the browser**: arkui-web's own parts, live — the same
  `ark_app::view` parts an app's screen is made of (`src/live.rs`).
- **On the desktop**: arkui's modules for it, pictures of it drawn by arkui
  itself, tables of what its functions answer (worked out when the site is
  built), and each module's API and rules.
- **On the phones**, Android and iOS: where the Compose or SwiftUI version
  is and how to call it, and the latest capture from a real emulator or
  simulator in a frame of that device; **Side by Side** shows Android's
  beside iOS's where both take one.

The words are long-form writing (`ark_app::writing`, Markdown), which any
app can use for its own help pages: `writing(md)` in a view.

## Pages, Paths and What Is the Reader's

A page's path is the page and nothing else: `DocsWeb::static_paths` is
every page, and each is written, with its `<title>`, its description, a
canonical link, and a line in `sitemap.xml`. The search (`?q=`), the phone
chosen and the appearance are the reader's: never a page, taken up by the
module once it has taken the page over. Without script the search field is
disabled and **All Pages** in the toolbar is the index of every page; the
live parts are drawn as they open.

## Sections, and Walking the Pages

The pages are in five sections, Foundations to Behaviour, each with a page
of its own (`/overlays/`) listing its pages. Wide, the sidebar has every
page under its section's heading, the heading the way to the section's
page. Short of a sidebar (`tab_section`), each section is a tab, its page
the list of its pages and each page pushed over it with the way back
named for the section, as Settings' are on an iPhone.

Every page ends with the pages before and after it in reading order (the
overview, then each section's page and its pages), as links. With
script, the keyboard is arkui-web's, every app's: `j` and `k` on the
sidebar walk the pages, as the desktop's sidebar cursor is the section
shown (on a page with live rows, the keys start in the rows: `h` is the
sidebar); `:` or ⌘K finds Next Page, Previous Page and the rest.

What goes over a page — a row's menu and its submenu, a sheet, a
question, a picker's menu, the command box, the keys panel — is drawn
open in its frame on its page (`arkui_web::specimen`, `menu::written`),
so it reads without script and takes no keys from the page; with script,
it answers.

## The Pictures Are the Components

`src/render.rs` lays an iced element out at a width and draws it with
tiny-skia, iced's CPU renderer, with no window and no GPU, then cuts it to
its own height and writes it as lossless WebP. Every picture is drawn four
times: in iced's light and dark themes with arkui's palette, at 640 and at
320 logical pixels, all at twice that. The page fetches only the one it
shows, by its theme and a media query.

`src/examples.rs` is what each component's pictures, tables and notes are.
What the browser cannot work out (the pictures' sizes, the tables) is
carried in each page as `#docs-built`, so the module draws the page it
takes over exactly as it was written.

## A Workspace of Its Own

`docs/site` is a Cargo workspace on its own, with its own `Cargo.lock`, and
not a member of `rust/`'s. So the docs and the apps never share a build: an
edit here is no input to ArkDB's kernel or to any app. The docs reach the
rest by path — arkui, arkui-web, ark-app — so they rebuild exactly when
something they name changes. One crate, two builds: `docs` (feature
`native`, the default) writes the site, `docs-web` (feature `hydrate`,
wasm32) is the module. Its checks are `nix build .#checks.<system>.docs`
(format, clippy, tests); the site is `nix build .#docs`.

By hand, in a small target directory:

    cargo run -- out --base /                     # the pages and pictures
    cargo build --release --target wasm32-unknown-unknown \
      --no-default-features --features hydrate --bin docs-web
    wasm-bindgen --target web --no-typescript --out-dir out/pkg \
      $CARGO_TARGET_DIR/wasm32-unknown-unknown/release/docs-web.wasm

## The One Place the Components Are Described

`src/components.rs` lists the components: each one's desktop modules, its
Android and iOS halves, and the captures the phones take of it.
`src/catalogue.rs` describes every arkui module. The pages say both, and
`catalogue.json` beside this file is the same data for any site or agent
that wants it (it is published beside the pages).

**If you change a component, change its entry here in the same commit**, then
regenerate the JSON:

    cargo run -- --json > catalogue.json        # in docs/site

`cargo test` here fails until you have. It checks that:

- no two captures, pictures or pages share a name;
- `catalogue.json` is what the catalogue prints;
- the Icons picture draws every glyph;
- every page has a title and a description, is in the sitemap, and every
  link on it lands;
- every page is a path, and the search is not;
- a picture draws, and is cut to its own height.

## The Captures

`web/docs.js` reads `manifest.json` from the `screens` branch through
raw.githubusercontent.com, which any page may fetch, so a capture appears on
the next load with nothing redeployed; a capture the module draws again is
filled again. To try the site against captures of your own, serve a
directory laid out like the branch and build with
`DOCS_SCREENS=http://localhost:8000/` set.
