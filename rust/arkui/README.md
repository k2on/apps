# arkui

The ui every app in this repository shares: iced 0.14 components, the vim
keyboard, theming, the glyph table, a picture cache and the address bar. It
knows nothing about any domain. An app hands over its palette, its places, its
messages and its screens; arkui supplies the parts of a window that are the
same in every app.

Most of it is lifted from harken's iced client, where each rule was paid for,
and the doc comments keep the reasons. Read them before "simplifying" a rule
back into the bug it fixed.

It builds for the desktop (`smol`) and for a browser (`webgl`). Text is iced's
bundled Fira Sans (the `fira-sans` feature), because with no embedded font a
browser draws no text at all.

## What is in it

| module | what it is |
|---|---|
| `vim` | `Keys` turns presses into an `Action` (counts, `gg`/`G`/`{n}G`, `^d`/`^u`, `/` search, `n`/`N`, `<Tab>`, `<Esc>`, anything else as `Action::Key(c)`). `Grid` turns a `Motion` into a cursor. `Grid::step` returns `None` only when the cursor is already at that edge, which means "not mine". `Grid::progress` is how far down to scroll. `presses()` is the keyboard subscription, and `find` is `/` and `n`/`N` over labels. |
| `theme` | `Palette { light, dark, light_art, dark_art }`. `install(palette)` once at startup; `of(theme)` is the app's `Extended` for whichever side iced picked. Before anything is installed it answers `Palette::NEUTRAL` (AppKit greys, system blue), which is the only colour arkui owns. |
| `style` | `dim`, `faint(alpha)`, `accent`, `seek`, `action`, `bars` (a scrollbar is never the accent), `page` (the root's ground), `row` (the zebra and the cursor), `entry`/`entry_text` (a lit row in a panel), `on_row` (text on a row that may be the cursor). |
| `glyphs` | Harken's generated table of Lucide drawings, copied as it is, with `LICENSE.lucide` beside it. |
| `icon` | `tinted(shape, size, |palette| colour)` plus `plain`, `accent`, `tick`, `line`, `faint`, `more`, `chevron`. `SIZE` is 15. The colour goes through the svg style filter. |
| `panel` | The shape every context window is: `panel`, `entry` (a fixed-height row with a glyph column that is there whether or not it has a glyph), `title`, `label`, the constants (`PADDING`, `ENTRY`, `TITLE`, `ENTRY_CHAR`, `EDGE`, …), `width_for`, `chars`, `titled_height`, and `hang_above` for a panel that opens off a button. |
| `menu` | `Menu<M>` with `Entry<M>` (`Entry::run` / `Entry::submenu`), `Anchor::{Pointer, RightEdge(x)}`, `Menu::open` (sized to its longest entry, then placed), `origin_for`, `submenu_origin` (beside the parent, lapped by `SUBMENU_OVERLAP`, slid rather than flipped), `fit`, `right_aligned`, `submenu_height`, `DWELL`, and `view`. |
| `picker` | `Picker<T>` of `Choice { value, name, on }` and an optional trailing "new" row. It has `width_for`, `activate` (which returns `Picked::Toggled` or `Picked::Naming`), `view` taking `Messages`, and `focus_naming`. |
| `context` | `Context<M, T>`: a menu, its submenu, or a picker on its own, and the rules for them. `focus()` says which `Layer` has the keys. It also has `open_menu`, `close_menu` (which takes its submenu with it), `open_picker`, `land`, `picker_at`, `tick` (the dwell), `chosen`, `activate_picker`, `cancel` (`<Esc>` closes both), and `travel` (`l` into a submenu, `h` back out). |
| `layer` | `backdrop` and `dimmed` close on a click and swallow the wheel. Also `pinned` and `centred`. |
| `table` | `cell`, `heading`, `section`, `gutter`, `row`, `GUTTER`. |
| `cards` | `Shelf` (`columns`, `grid`, `room`, where the scrollbar is subtracted), `Card<M>`, `view`, `view_card`, `header`, `empty_page`. |
| `art` | `hash` (FNV-1a over UTF-16, the same as the phone's), `pair`, `square` (the derived square), and `picture` (the real one if it has arrived, the square until then). |
| `images` | `Images::new(name)` with `want(url) -> Option<Task<Loaded>>`, `loaded`, `handle`. Pictures are decoded and bounded (`BOUND` is 384) off the render thread. The cache is a disk directory natively and the Cache API plus `createImageBitmap` in a browser. A failure is remembered. |
| `route` | `trait Route { fragment, parse }`, `join`/`split`, `Router<R>` with `sync` (reconciled, not pushed), `replace_next` (a scrub is one entry) and `poll` (the back button, once there is something to resolve against), `AddressBar`, `Browser`. |
| `scroll` | `reveal(id, grid, at)` keeps a keyboard cursor on screen. |
| `fit`, `format`, `url` | `tail`/`middle`/`to_width`/`PER_PORTION`; `clock`/`spell`/`plural`/`span`; `encode`/`encoded`/`decode`. |

## Using it

Theming. Install the palette once, before the first frame. Every closure asks
for colours rather than writing them down:

```rust
arkui::theme::install(arkui::theme::Palette {
    light: palette::LIGHT, dark: palette::DARK,
    light_art: &palette::LIGHT_ART, dark_art: &palette::DARK_ART,
});
container(page).style(arkui::style::page);
text("4:21").style(arkui::style::dim);
```

The keyboard over a `Grid`. The grammar says which way, the shape says
whether it can go, and the app says what lies past a refused edge:

```rust
fn subscription(&self) -> Subscription<Message> {
    arkui::vim::presses().map(|(key, mods)| Message::Key(key, mods))
}

Message::Key(key, mods) => match self.keys.press(&key, mods) {
    Some(vim::Action::Move(motion)) => {
        if self.ctx.focus().is_some() {
            if let Some(m) = self.ctx.travel(motion) { return self.update(m) }
            return Task::none();
        }
        let grid = self.shelf.grid(self.albums.len(), self.pane_width());
        match grid.step(self.at, motion) {
            Some(at) => { self.at = at; arkui::scroll::reveal("page", grid, at) }
            None if matches!(motion, vim::Motion::Left(_)) => { self.pane = Pane::Sidebar; Task::none() }
            None => Task::none(),
        }
    }
    Some(vim::Action::Cancel) => { self.ctx.cancel(); Task::none() }
    // …
    None => Task::none(), // still typing; show self.keys.pending()
}
```

A menu with a submenu, and a picker. The context holds both. The app draws
them as layers and routes their messages back:

```rust
Message::RowMenu(id, anchor) => {
    let entries = vec![
        Entry::run(glyphs::PLAY, "Play", Message::Play(id)),
        Entry::submenu(glyphs::ADD_TO, "Add to playlist", Message::OpenPicker),
    ];
    self.ctx.open_menu(Menu::open(title, entries, self.pointer, self.window, anchor));
}
Message::MenuAt(i) => self.ctx.land(i),
Message::MenuActivate => if let Some(m) = self.ctx.chosen() { return self.update(m) },
Message::OpenPicker => {
    let choices = lists.map(|(id, name, on)| Choice { value: id, name, on }).collect();
    self.ctx.open_picker(Picker::new(title, choices, Some("New playlist…".into())), self.window);
}
Message::PickerActivate => match self.ctx.activate_picker() {
    Some(Picked::Toggled { value, on }) => /* mutate */,
    Some(Picked::Naming) => return arkui::picker::focus_naming(),
    None => {}
},
Message::Tick => if let Some(m) = self.ctx.tick() { return self.update(m) }, // the dwell

// view
let mut layers = stack![mouse_area(base).on_move(Message::Pointer)];
if let Some(menu) = &self.ctx.menu {
    layers = layers
        .push(layer::backdrop(Message::CloseMenu, Message::Swallow))
        .push(layer::pinned(menu::view(menu, self.ctx.focus() == Some(Layer::Menu), Message::MenuAt, Message::MenuActivate), menu.origin));
}
if let Some(p) = &self.ctx.picker {
    let drawn = picker::view(p, self.ctx.focus() == Some(Layer::Picker), picker::Messages {
        at: Box::new(Message::PickerAt), activate: Message::PickerActivate,
        name: Box::new(Message::PickerName), create: Message::PickerCreate,
    });
    layers = match p.origin {
        Some(at) => layers.push(layer::pinned(drawn, at)), // a submenu: no backdrop of its own
        None => layers.push(layer::dimmed(Message::ClosePicker, Message::Swallow)).push(layer::centred(drawn)),
    };
}
```

A menu opened from a row's ⋯ takes `Anchor::RightEdge(x)`, where `x` is where
that column ends (for harken, the window width less the page padding and
`arkui::SCROLLBAR`). A right click takes `Anchor::Pointer`. An app refuses its
rows' hover while `ctx.focus().is_some()`, because a backdrop cannot swallow a
hover. It also closes the menu when an entry that goes somewhere is run
(`ctx.close_menu()`).

Routing. Call `router.sync(&where_the_app_is)` after every update. Call
`router.replace_next()` from a scrub. On the tick, call
`router.poll(ready)` and navigate to what it returns.

## Tests

`cargo test -p arkui` runs 45 tests, all passing. Each ported test was
rewritten against this API, and each new or ported test was falsified once
by breaking what it checks. The doc comment on each test says how.

## Not verified

No window has been opened. Nothing in the container that wrote this can show
one, so every view function has been type-checked for the host and for
`wasm32-unknown-unknown`, and none has been seen. The arithmetic behind the
placement, widths, card columns, dwell, focus and routing is tested. What it
looks like on screen is not. The browser halves of `route::Browser` and
`images` (the history API, the Cache API snippet, `createImageBitmap`) compile
for wasm and have never run. The native picture fetch has only been run as far
as a refused connection.
