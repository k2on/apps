//! What arkui is made of.
//! The pages are by component (`components.rs`); a component's desktop half
//! is one or more of these modules.
//!
//! **The one place this is written down.** The page draws it, `--json`
//! prints it (`catalogue.json` beside this crate is that output, committed,
//! for a site that wants it without running anything), and the tests hold it
//! to the code: every `pub mod` of arkui has an entry here. Change a
//! component in arkui and you change its entry here in the same commit; the
//! tests say when you forgot.
use serde::Serialize;

use crate::components::{Component, COMPONENTS};

/// One module of arkui.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// The module, as `arkui::<module>` names it. Also the page's fragment.
    pub module: &'static str,
    pub name: &'static str,
    /// What it is, in a sentence or two.
    pub summary: &'static str,
    /// Its public items, as a caller writes them.
    pub api: &'static [&'static str],
    /// The rules worth knowing before changing it.
    pub rules: &'static [&'static str],
}

impl Entry {
    pub fn source(&self) -> String {
        format!("rust/arkui/src/{}.rs", self.module)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Catalogue {
    /// The docs' pages: each component, its desktop modules and its phones.
    pub components: &'static [Component],
    /// arkui's modules, each in one component.
    pub entries: &'static [Entry],
}

pub const CATALOGUE: Catalogue = Catalogue {
    components: COMPONENTS,
    entries: ENTRIES,
};

pub fn find(module: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.module == module)
}

pub const ENTRIES: &[Entry] = &[
    // Foundations.
    Entry {
        module: "theme",
        name: "Theme",
        summary: "An app's palette, installed once: light and dark iced palettes and the derived square's gradient pairs for each. Every style closure asks `of(theme)` rather than writing a colour down, which is why dark mode works.",
        api: &["Palette { light, dark, light_art, dark_art }", "Palette::NEUTRAL", "install(palette) -> bool", "palette()", "of(&Theme) -> &Extended", "is_dark(&Theme)", "art(&Theme)"],
        rules: &[
            "Install before the first frame; a second install is refused, so one window has one answer to \"what colour is this\".",
            "NEUTRAL (AppKit greys, system blue) is the only colour arkui owns. The docs' pictures are drawn in it.",
        ],
    },
    Entry {
        module: "style",
        name: "Style",
        summary: "The style closures every widget is handed: text tones, buttons, scrollbars, the page's ground and the surface a step off it, hairlines, and the rows a cursor lights.",
        api: &[
            "dim, faint(alpha), accent (text)",
            "action, quiet (buttons)",
            "seek (slider)",
            "bars (scrollable: never the accent)",
            "page, surface, SURFACE_STEP (containers)",
            "edge, edge_color, EDGE_STEP, hairline, outline",
            "row(theme, on_cursor, focused, odd)",
            "entry(theme, on_cursor, focused), entry_text(theme, lit)",
            "on_row(theme, on_cursor, accent, dim)",
        ],
        rules: &["A scrollbar is never the accent.", "A row under the cursor is marked whether or not its pane has the keys, and lit only while it has."],
    },
    Entry {
        module: "glyphs",
        name: "Glyphs",
        summary: "The table of Lucide drawings, as SVG bytes. `LICENSE.lucide` sits beside it.",
        api: &["ADD_TO, ALBUM, ARTIST, BACK, CHEVRON, CLOSE, COMPOSER, DEBUG, DEVICES, DOWN, HOME, LAPTOP, LIBRARY, LINK, MINUS, MORE, MUTED, NEXT, NOTE, OFFLINE, ONLINE, PAUSE, PHONE, PLAY, PLAYING, PLAYLIST, PLUS, PREVIOUS, PULPIT, SEARCH, SHOW, SHUFFLE, SIGN_OUT, SKIP_BACK, SKIP_FORWARD, SPEAKER, STOP, TICK, TICKED, TRANSCRIPT, UNTICK, VOLUME"],
        rules: &["Drawn with currentColor; the colour is the icon's tint, never the file's."],
    },
    Entry {
        module: "icon",
        name: "Icon",
        summary: "A glyph drawn at a size in a colour asked of the palette. The named ones are the tones a row, a panel or a control needs.",
        api: &["SIZE (15)", "tinted(shape, size, |palette| colour)", "plain(shape, dim)", "accent(shape, on_cursor)", "line(shape, lit)", "faint(shape, on_cursor)", "tick(on_cursor)", "more(on_cursor)", "chevron(lit)"],
        rules: &["The colour goes through the svg style filter, so one drawing serves every tone."],
    },
    Entry {
        module: "format",
        name: "Format",
        summary: "Words for numbers: a clock, a count spelt out, a plural, a span of time, a day.",
        api: &["clock(secs)", "spell(ms)", "plural(n, word)", "span(born, died)", "day(ms, now)"],
        rules: &["A length past an hour is h:mm:ss."],
    },
    Entry {
        module: "fit",
        name: "Fit",
        summary: "Cutting text to a width: from the end or through the middle, measured in pixels, and the characters the typeface cannot draw spelt in ones it can.",
        api: &["Cut::{End, Middle}", "tail(s, chars)", "middle(s, chars)", "within(body, px, cut, measure)", "drawable(s)"],
        rules: &["A name loses its middle, a heading its end."],
    },
    Entry {
        module: "url",
        name: "URL",
        summary: "Percent-encoding for a fragment, so a `/`, a `#` or an accent survives the address bar.",
        api: &["encode(s, &mut out)", "encoded(s)", "decode(s)"],
        rules: &[],
    },
    // Components.
    Entry {
        module: "sidebar",
        name: "Sidebar",
        summary: "A column of places down the left of a window, grouped under headings; the one under the cursor is the page beside it.",
        api: &["Item { glyph, label, heading, count, press }", "WIDTH (200)", "view(items, cursor, focused, width, padding, id)"],
        rules: &[
            "One highlight, not two: the cursor is what the page shows, marked always, lit only while the sidebar has the keys.",
            "A heading is drawn where the group changes, not stored as a row, so row n is the cursor's n.",
            "Its pixel of edge is part of its width.",
        ],
    },
    Entry {
        module: "panel",
        name: "Panel",
        summary: "The shape every context window is: a bordered panel, a title, and fixed-height entries with a glyph column that is there whether or not it has a glyph.",
        api: &["panel(body, width)", "entry(glyph, label, trailing)", "title(label)", "label(body, lit)", "width_for(labels, min, max)", "chars(width, chrome)", "titled_height(rows)", "hang_above(right, bottom, w, h)", "PADDING, ENTRY, TITLE, ENTRY_CHAR, ENTRY_CHROME, EDGE, RADIUS"],
        rules: &["Heights are declared constants: a panel is placed before iced lays it out."],
    },
    Entry {
        module: "table",
        name: "Table",
        summary: "Rows of cells cut to their columns at layout, headings over them, section labels and a gutter for a row's mark.",
        api: &["cell(body, width, on_cursor, accent, dim)", "cell_cut(…, cut, …)", "heading(label, width)", "section(label, indent)", "gutter()", "row(line, on_cursor, focused, odd)", "GUTTER, ROW_PADDING, SPACING, CELL_SIZE, HEADING_SIZE"],
        rules: &["A cell loses its middle and a heading its end, each to its own column.", "Columns are portions, so no text column pushes another off a narrow window."],
    },
    Entry {
        module: "line",
        name: "Line",
        summary: "One line of text cut, at layout, to the width it is given; `gives_way` lays it out after the rest of its row.",
        api: &["Line::new(body, size, cut, colour)", "Line::gives_way()"],
        rules: &["A row lays out in order, so a line first in it would take everything; giving way puts it last."],
    },
    Entry {
        module: "cards",
        name: "Cards",
        summary: "A grid of pictures with a name and a line under each, the arithmetic of how many fit, and a page's header of picture, name and facts.",
        api: &["Shelf { card, gap, padding }, Shelf::DEFAULT", "Shelf::columns(pane), side(pane), room(pane), grid(cells, pane)", "Card { seed, title, under, round, picture, open }", "view(shelf, cards, pane, cursor, empty, on_hover, scroll, on_scroll)", "view_card(side, card, on_cursor, hover)", "header(picture, kind, name, under, open_under, facts, padding)", "empty_page(message, padding)"],
        rules: &["A person is a circle and a thing a square.", "The keyboard and the view divide the same pane width, so `l` lands where it looks."],
    },
    Entry {
        module: "hero",
        name: "Hero",
        summary: "The wide banner a page can open with, the bold face, section headings, and a listing of entries in columns with a \"more\" after them.",
        api: &["banner(picture, kind, name, width)", "height(width), ASPECT, TALLEST", "heading(title)", "Entry { over, title, under, count, open }", "listing(entries, columns, cursor, on_hover)", "grid(entries, columns)", "entry(picture, e, on_cursor, hover)", "more(label, lit)", "columns(room), COLUMN", "BOLD, FIRA_SANS_BOLD"],
        rules: &["Fira Sans Bold is embedded: a browser has no system fonts, and with none bold is not bold."],
    },
    Entry {
        module: "art",
        name: "Art",
        summary: "The derived square: a gradient picked from a name, the same on every device, and the real picture over it once it arrives.",
        api: &["hash(s) (FNV-1a over UTF-16)", "pair(seed)", "square(seed, size, corner)", "picture(seed, handle, side, corner)"],
        rules: &["The hash is the phones' too, so a name has one square everywhere."],
    },
    Entry {
        module: "account",
        name: "Account",
        summary: "Who is signed in as a circle — their picture, or initials on a colour picked from their name — and the menu that hangs off it.",
        api: &["Who { name, id }, Who::full(), Who::initials()", "Account { who, header, entries }", "Account::signed_in, turned_away, alone", "Account::menu(right, bottom, window)", "face(who, picture, side)", "button(account, picture, press)", "origin(…)", "HEIGHT (28)"],
        rules: &["Nobody goes offline by hand: the menu signs out, and nothing else."],
    },
    // Overlays.
    Entry {
        module: "menu",
        name: "Menu",
        summary: "The dropdown: a title and entries, as wide as its longest entry, opened at the pointer or lined up under a control, with submenus beside it.",
        api: &["Menu::open(title, entries, cursor, window, anchor)", "Entry::run(glyph, label, msg), Entry::submenu(…)", "Anchor::{Pointer, RightEdge(x)}", "view(menu, focused, at, activate)", "Menu::submenu_origin, origin_for, height, width_for", "fit, right_aligned, submenu_height", "MIN_WIDTH, MAX_WIDTH, SUBMENU_OVERLAP, DWELL"],
        rules: &[
            "A right click opens at the pointer; a control's menu opens at the control.",
            "As wide as its longest entry, within 198..=420: AppKit's rule.",
            "A submenu laps the parent by exactly its padding, so the lit row touches its edge.",
            "A submenu opens after a 200ms rest, not on the way past.",
        ],
    },
    Entry {
        module: "picker",
        name: "Picker",
        summary: "A list of choices each ticked or not, with an optional last row that names a new one — on its own in the middle, or as a menu's submenu.",
        api: &["Picker::new(title, choices, new_row)", "Choice { value, name, on }", "Picker::activate() -> Picked::{Toggled, Naming}", "Picker::land, rows, grid, width_for", "view(picker, focused, Messages { at, activate, name, create })", "focus_naming()", "MIN_WIDTH, MAX_WIDTH, MAX_HEIGHT"],
        rules: &["A submenu draws no header: the menu it hangs off is still up with the title."],
    },
    Entry {
        module: "reorder",
        name: "Reorder",
        summary: "A list whose rows are dragged into a new order: the row lifts and follows the pointer, and the rows it passes slide out of its way.",
        api: &["Reorder::press(from, rows, row, y, now)", "drag(y, now), drop(now) -> Drop::{Click, Moved { from, to, settle }}", "list(rows, row, drag, settle, now)", "moved(items, from, to)", "THRESHOLD, SLIDING, SETTLING, EASE_OUT"],
        rules: &[
            "Under THRESHOLD pixels a press is a click, so a row still does what a click on it did.",
            "Where a row lands is arithmetic on how far the pointer moved, the rows being one height: nothing laid out is measured.",
            "The app owns the order: it moves its row on a drop, and draws the new order with the Settle until its own order says so.",
        ],
    },
    Entry {
        module: "context",
        name: "Context",
        summary: "A menu, its submenu, or a picker alone, and the rules for them together: which one has the keys, the dwell, `l` in and `h` out, `<Esc>`.",
        api: &["Context::new()", "focus() -> Option<Layer>", "open_menu, close_menu, open_picker, close_picker", "land(i), picker_at(i), tick(), chosen(), activate_picker()", "cancel(), travel(motion)", "Layer::{Menu, Picker}"],
        rules: &["Closing a menu takes its submenu with it.", "An app refuses its rows' hover while a layer is up: a backdrop cannot swallow a hover."],
    },
    Entry {
        module: "layer",
        name: "Layer",
        summary: "What goes under and around an overlay: a backdrop that closes on a click and swallows the wheel, a dimmed one, and pinned or centred placement.",
        api: &["backdrop(close, swallow)", "dimmed(close, swallow)", "pinned(content, at)", "centred(content)"],
        rules: &["A submenu gets no backdrop of its own: a second one over the menu would eat its clicks."],
    },
    Entry {
        module: "dialog",
        name: "Dialog",
        summary: "A titled window fading in and growing from 95% over a page as the page dims, as shadcn/ui's does without the blur.",
        api: &["shown(now) -> Shown", "open(&shown, now), dimmed(&shown, now) -> 0..=1", "view(title, close, body, width, height, open)", "backdrop(close, swallow, dim)", "TIMING (200 ms, ease), DIMMING (150 ms), FROM (0.95)"],
        rules: &[
            "Closed, it fades and shrinks back the way it came: the app keeps its Shown, hidden, and draws it until Shown::gone.",
            "Keep a clock running while Shown::moving.",
        ],
    },
    Entry {
        module: "sheet",
        name: "Sheet",
        summary: "A panel the window's height sliding in from its right-hand edge over the page dimmed, as shadcn/ui's sheet does without the blur.",
        api: &["shown(now) -> Shown", "open(&shown, now) -> 0..=1", "view(title, description, close, body, width, open)", "width(window) -> three quarters, at most MAX_WIDTH", "body_height(window, described)", "TIMING (500 ms in, 300 ms out, ease-in-out)"],
        rules: &["The page under it dims at the dialog's pace (dialog::backdrop with Shown::at_pace(dialog::DIMMING, now))."],
    },
    Entry {
        module: "motion",
        name: "Motion",
        summary: "What moves a while and stops: something shown and hidden with a move each way, a slide, the curves they go along, and a widget that draws what it holds moved, scaled and veiled.",
        api: &["Ease(x1, y1, x2, y2).at(t)", "EASE, EASE_IN_OUT", "Timing { opening, closing, ease }", "Shown::new(timing, now), at(now), hide(now), show(now), moving(now), gone(now)", "Slide::new(from, to, now, taking, ease), at(now), toward(...)", "transformed(content, by, scale, veil, radius)"],
        rules: &[
            "Nothing ticks at rest: how far along a move is, is worked out from the time whenever it is drawn.",
            "iced has no opacity for a group of widgets, so a fade is a veil of the ground over what fades, thinning.",
        ],
    },
    // Behaviour.
    Entry {
        module: "vim",
        name: "Keys",
        summary: "The keyboard grammar — counts, gg/G, ^d/^u, / and n/N, <Tab>, <Esc> — and the grids it moves a cursor over.",
        api: &["Keys::new(), press(key, mods) -> Option<Action>, pending(), mode()", "Action::{Move, Activate, Toggle, Search, Match, Cycle, Cancel, Jump, Key}", "Motion::{Down, Up, Left, Right, First, Last, To}", "Grid { cells, columns }, column, row, step, progress", "Section, Page", "presses()", "find(labels, needle, at, delta)", "PAGE"],
        rules: &["`step` returns None only at that edge, which means \"not mine\": the app says what lies past it."],
    },
    Entry {
        module: "route",
        name: "Route",
        summary: "Places as URL fragments, and the address bar kept in step: reconciled, not pushed, and a scrub one history entry. Each page here has one.",
        api: &["trait Route { fragment(), parse() }", "join(kind, name), split(fragment)", "Router::new(), sync(&place), replace_next(), poll(ready), go(by), can_go(by)", "AddressBar, Browser, Nav"],
        rules: &["`parse` never fails: an unknown fragment is home.", "Call `sync` after every update."],
    },
    Entry {
        module: "scroll",
        name: "Scroll",
        summary: "Keeping a keyboard cursor on screen.",
        api: &["reveal(id, grid, at)", "reveal_to(id, progress)"],
        rules: &[],
    },
    Entry {
        module: "swipe",
        name: "Swipe",
        summary: "Two fingers sideways as back and forward, with the arrow drawn over the page as they go.",
        api: &["Swipe::feed, tick, allow, active, offset, arrow", "band(…)", "view(page, swipe, now)", "FAR, LET_GO, QUIET, BACK, STRETCH"],
        rules: &["It goes when the fingers lift, and only where there is somewhere to go.", "In a browser the browser's own back does this instead."],
    },
    Entry {
        module: "wake",
        name: "Wake",
        summary: "What wakes a window that runs no loop: a bell anything can ring, one timer at a deadline, and the browser's address bar moving.",
        api: &["Bell::new(), ring(), rung(), subscription()", "at(key, delay)", "page_changes(f)"],
        rules: &["With nothing playing, dwelling or arriving, a window runs no timer."],
    },
    Entry {
        module: "images",
        name: "Images",
        summary: "A picture cache: fetched off the render thread, decoded and bounded, kept on disk natively and in the Cache API in a browser. A failure is remembered.",
        api: &["Images::new(name)", "want(url) -> Option<Task<Loaded>>", "loaded(l), handle(url), bounded(url)", "Loaded, Rgba", "BOUND (384), WIDE (2400)"],
        rules: &["The renderer is handed pixels at the size they are drawn, not a megapixel JPEG to decode inside a frame."],
    },
];

/// The catalogue as JSON, pretty: what `--json` prints and `catalogue.json`
/// holds.
pub fn json() -> String {
    serde_json::to_string_pretty(&CATALOGUE).expect("the catalogue is plain data") + "\n"
}
