# Pebble

Pebble is an arkui toolkit beside SwiftUI, Compose, iced and the browser:
a screen on the watch is an ordinary `#[view]` of arkui's primitives and
native parts, and the same view is drawn by every other toolkit. The
first screen is the Pebble host's demo, standing in for harken's now
playing (`ui/pebble/host/demo/view`, step 5 of `docs/north-star.md`).

```rust
#[view]
pub fn screen() -> View<Act> {
    let link = cell(STATUS);
    let now = cell(NOW);
    let p = row(playback());
    let next = list(up_next(SHOWN));
    let status = show::status_line(link);
    let title = show::title(p);
    let at = show::at(p, now);
    …
    action_bar(
        vstack((
            vstack((
                hstack((glyph(wifi).style(TextStyle::Caption).ink(Ink::Quiet), text(status)…)).spacing(Space::S),
                text(title).style(TextStyle::Title).lines(1),
                …
                vstack((meter(progress, "accent"), hstack((text(at), spacer(), text(length))))),
            ))
            .align(Align::Start)
            .padding(Edges::Horizontal, Space::Page),
            card_section(for t in next {
                let name = text!("{}", t.title);
                let line = text!("{}  {}", t.artist, Clock(u64::from(t.ms)));
                list_row(name).subtitle(line)
            })
            .header(heading),
        ))
        .spacing(Space::S),
    )
    .select(play, say, Act::Toggle)
    .down("skip-forward", "Skip", Act::Skip)
    .into()
}
```

## How It Is Drawn: Written at Build Time

The watch is a Cortex-M3 with 128 KB for an app's code and heap, no_std.
The view machinery (`ark_app::view`, `#[view]`, the recorder) is std and
stays on the build machine, as it does for the phones: **ark-ui-gen's
`pebble` emitter** writes the recorded view out as no_std Rust over
ark-pebble's toolkit (`ark_pebble::ui`), exactly as `swift.rs` and
`kotlin.rs` write SwiftUI and Compose. The watch crate's `build.rs` runs it
(`now_playing_view::watch()`) and includes what it writes from `OUT_DIR`;
nothing generated is committed, and the view's tests hold it to its
snapshot (`demo/view/tests/snapshots/now_playing_watch.rs.snap`, beside the
same view's `NowPlayingView.swift` and `NowPlayingScreen.kt`).

The other way, a small no_std renderer walking a view tree on the watch,
would need a second view model (`View` holds `String`s, `Rc`s and boxed
closures) and its layout and diffing linked into the app. Written at build
time there is one view definition, the layout is worked out once, and the
watch links only the layers it uses.

The view reads sources, never a record: the host's two cells (`STATUS`,
`NOW`), what plays (a one-row plan) and the queue's first few (a plan),
each a `let`, and what it shows of them is worked out in `let`s by plain
functions of values (`now_playing::show`), the same on the watch as on
every toolkit. What it writes, for a view sending `Act`:

- `GLYPHS`: each glyph the view names, and those its paths may name (the
  `glyphs` of its `pebble::Screen`), arkui's Lucide drawings flattened into
  line segments in eighths of their 24 units;
- `build(ui)`: each layer made once, at a frame that is an expression of
  the room the window gives and the tokens, never a number; a literal's
  glyph or label set here;
- a function for each layer that reads a `let`, written from the `let`s
  its path reads, in order, each the view's own Rust: a cell's value
  (`ark_kernel::source::cell`), a keyed watch's row or rows (`row`,
  `rows`, `count`, and a row's item by the row, `item`), a value worked
  out, and a `text!`'s template written straight into the layer's line,
  hole by hole. A `let` more than one layer reads (what plays) is worked
  out once a settle and shared (`ark_kernel::source::shared`);
- `BINDINGS`: a table, in rodata, binding each layer to its function,
  and each `if`'s branches to the flag that shows them;
- `PRESS`: the message a button sends.

`Ui::bind` gives each binding a kernel reader; the host's `Node::settle`
hands `Ui` (the sink) the readers whose recorded reads changed, and their
layers are marked dirty; as PebbleOS draws one, the shim asks Rust for
its words (`ark_pull`, `ark_pull_meter`), and the binding runs through
the node, its reads recorded against the layer again. A keyed watch a
binding asks for that the node does not keep yet is made, and the
binding read again. A layer is drawn only when what it read changed, and
its words live only on the stack while it is drawn. Which reads a layer
depends on is recorded, not worked out by the emitter: only running a
binding says what it reads.

Text in a view is `Text`, never `String`: a literal converts, and `text!`
builds a template whose holes are kept apart, so a toolkit that pulls
(Pebble) or tracks (the phones, later) can tell the words from what fills
them. There is no `From<String>`; logging keeps `format!`. The rule is
every view's (`ark_kernel::show::Text`, which `ark_app::view` names).

It paints a glyph into a 1-bit bitmap the first time it is drawn at a size
and colour. The app has no resources.

## What It Supports

| arkui | On Pebble |
|---|---|
| `vstack` | its content one under another, `spacing` apart; what is left shared by its spacers and lists (and anything `.fill_height()`) |
| `hstack` | its glyphs at their side, its words and spacers sharing the rest; words before its first spacer at the leading edge, after its last at the trailing edge, between them centred; each thing at the row's `align` across it |
| `zstack` | its content in one frame |
| `spacer`, `spacer_min` | what is left along its stack (the least is not kept yet) |
| `text` | a layer drawn with `graphics_draw_text`: Gothic at its style's size (`FONT_*`), its ink, `lines(n)` lines of it tall (one unless it says), cut short with an ellipsis; `.centred()`; leading or trailing by its stack. An empty one keeps its line |
| `glyph` | a `BitmapLayer` of the glyph painted at its style's size (`SIDE_*`), stroked, in its ink; a name read from a source must be among the screen's `glyphs` |
| `meter` | a drawn layer: a bar `PROGRESS_HEIGHT` tall over a `TRACK_HEIGHT` track (`CONTAINER_HIGH`), in its tint (`neutral` the positive's) |
| `card_section` of `list_row`s | a `MenuLayer`: its heading (`FONT_HEADER`, quiet) over rows of a title (`FONT_HEADLINE`) over the line under it (`FONT_FOOTNOTE`, quiet), `ROW_HEIGHT` each, inset by the page margin; a `for` of rows, or fixed rows |
| `action_bar(content).up/select/down` | the `ActionBarLayer` on `CONTAINER_HIGH`, its glyphs `ACTION_GLYPH` a side, solid (a closed shape filled, as the system's are); the content laid out in what is left. A screen's root only |
| `if`, `if let Some` | both branches made in one frame, each shown while it holds |
| `.padding`, `.background`, `.corner` | an inset; a filled rectangle behind, rounded |
| `.fill_width`, `.fill_height`, `.grow`, `.combine` | everything is as wide as it is given; there is no screen reader |
| `Ink::Ink`, `Quiet`, `Accent`, a tint by name | `INK`, `QUIET`, the screen's accent's `*_ACCENT_TEXT`, `POSITIVE`/`CAUTION`/`ERROR` |

Not yet, each a panic as the screen is written, so the watch's build fails
rather than its screen: `scroll`, `image`, `swatch`, `grid`, a `for`
outside a list, `.width`, `.side`, `.square`, `.max_width`, `.bleed`, a
tint read from a source, a list row's tap, and every other part (pages,
sheets, forms, fields, the scrubber, the shell). A `for` of more rows than
the shim keeps (six) shows the first six.

On every other toolkit `action_bar` is a column of glyph buttons at the
trailing edge beside its content, top, middle and bottom, each pressed as a
give and said by its label (SwiftUI's and Compose's `ActionBar`, iced's and
the browser's `Part::ActionBar`), so the watch's view is not a dialect.

## Tokens

Pebble's values are `ui/theme.toml`'s `[layout]` `pebble-*` keys, written
by `nix run .#update-theme` into `ui/pebble/host/rust/src/tokens.rs`
(`checks.theme` holds it):

- the steps, `SPACE_XS` … `SPACE_XXL`, `PAGE_MARGIN`, `SECTION_GAP`, and
  the corners, in pixels: a watch's own rhythm, each the role of the
  phones' step of its name;
- each type style's Gothic size (`FONT_*`, bold for the headline, the
  titles and the figure, as `TextStyle::bold` says), a line of it
  (`LINE_*`, `pebble-line-height` times the size) and a glyph set in it
  (`SIDE_*`);
- a row, a heading, a meter, the action bar's glyph and a glyph's stroke;
- the colours: every role of the dark side and each accent's, as the
  nearest of the Time 2's 64 (`GColor8`, two bits a channel).

## Motion

Nothing on the watch moves yet: a new track, a pause or a skip redraws in
place, and the list's rows are replaced, not slid. PebbleOS has property
animations; when one is wanted it takes `[motion]`'s tokens, as every
toolkit does, and goes through the toolkit, never a number in a view.

## Routes

The demo is one screen and no ark app (no `app.toml`, no `ark_router`), so
it has no routes. When harken reaches the watch its screens go in the
`(glance)` route group (`docs/north-star.md`, Steps): `src/app/(glance)/
_layout.rs` and its pages, the smallest windows; ark_router will hand the
Pebble emitter the group's views as it hands the phones' generator the
app's, and an app with no `(glance)` group has no watch app.

## What Is Verified

The emitter's output by `ui/rust/gen/tests/snapshots` (`glance_watch.rs.snap`,
with the same view's `GlanceView.swift` and `GlanceScreen.kt`) and the
demo's by its own snapshots, run by `checks.pebble-host`; the glyphs'
painting by ark-pebble's tests; the `.pbw` built and held under 96 KB by
`checks.pebble-now-playing` and `checks.pebble-size`. Seen in the emulator
(emery, SDK 4.44) against the test upstream: playing, paused, a skip
offline pending, a refusal and a reconnect. Not seen on a Pebble Time 2.
