//! What arkui is made of, and what harken draws with and without it.
//!
//! **The one place this is written down.** The page draws it, `--json`
//! prints it (`catalogue.json` beside this crate is that output, committed,
//! for a site that wants it without running anything), and the tests hold it
//! to the code: every `pub mod` of arkui has an entry here, and an entry says
//! harken's desktop uses it exactly when harken's source names it. Change a
//! component in arkui and you change its entry here in the same commit; the
//! tests say when you forgot.
use arkui::glyphs;
use serde::Serialize;

/// The sidebar's headings, in the order they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    /// The colours, the styles, the glyphs and the words.
    Foundations,
    /// What a page is built of.
    Components,
    /// What goes over a page.
    Overlays,
    /// The rules with no drawing of their own: the keys, the address bar,
    /// waking, pictures.
    Behaviour,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Foundations, Group::Components, Group::Overlays, Group::Behaviour];

    pub fn heading(self) -> &'static str {
        match self {
            Group::Foundations => "Foundations",
            Group::Components => "Components",
            Group::Overlays => "Overlays",
            Group::Behaviour => "Behaviour",
        }
    }
}

/// One module of arkui.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// The module, as `arkui::<module>` names it. Also the page's fragment.
    pub module: &'static str,
    pub name: &'static str,
    pub group: Group,
    #[serde(skip)]
    pub glyph: &'static [u8],
    /// What it is, in a sentence or two.
    pub summary: &'static str,
    /// Its public items, as a caller writes them.
    pub api: &'static [&'static str],
    /// The rules worth knowing before changing it.
    pub rules: &'static [&'static str],
    /// Where harken's desktop and browser client draws with it. Empty when
    /// it does not (a test holds this to harken's source).
    pub harken: &'static [&'static str],
    /// What the phones have for the same job. Theirs live in the apps
    /// (`harken/android/.../ui/`, `harken/ios/Harken/`), not in arkui.
    pub phones: &'static [&'static str],
}

impl Entry {
    pub fn source(&self) -> String {
        format!("rust/arkui/src/{}.rs", self.module)
    }
}

/// Something harken draws that arkui does not have.
#[derive(Debug, Clone, Serialize)]
pub struct HarkenOnly {
    pub name: &'static str,
    /// `desktop` (harken/iced), `android` or `ios`.
    pub platform: &'static str,
    /// Where it is: a file, and the function or type in it.
    pub at: &'static str,
    pub what: &'static str,
    /// The arkui modules it is built from, if any.
    pub built_on: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct Catalogue {
    pub entries: &'static [Entry],
    pub harken_only: &'static [HarkenOnly],
}

pub const CATALOGUE: Catalogue = Catalogue {
    entries: ENTRIES,
    harken_only: HARKEN_ONLY,
};

pub fn find(module: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.module == module)
}

pub const ENTRIES: &[Entry] = &[
    // Foundations.
    Entry {
        module: "theme",
        name: "Theme",
        group: Group::Foundations,
        glyph: glyphs::SHUFFLE,
        summary: "An app's palette, installed once: light and dark iced palettes and the derived square's gradient pairs for each. Every style closure asks `of(theme)` rather than writing a colour down, which is why dark mode works.",
        api: &["Palette { light, dark, light_art, dark_art }", "Palette::NEUTRAL", "install(palette) -> bool", "palette()", "of(&Theme) -> &Extended", "is_dark(&Theme)", "art(&Theme)"],
        rules: &[
            "Install before the first frame; a second install is refused, so one window has one answer to \"what colour is this\".",
            "NEUTRAL (AppKit greys, system blue) is the only colour arkui owns. This page draws with it.",
        ],
        harken: &["palette.rs: HARKEN, AppKit greys and the gold", "every style closure in view.rs, through of(theme)"],
        phones: &["android ui/Theme.kt: Space, ArkTheme; Theme.kt: Palette, HarkenTheme", "ios Theme.swift: Space, Squares"],
    },
    Entry {
        module: "style",
        name: "Style",
        group: Group::Foundations,
        glyph: glyphs::LIBRARY,
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
        harken: &["everywhere in view.rs: the page, the sidebar and bar surfaces, every row and button"],
        phones: &["android ui/Theme.kt", "ios Theme.swift"],
    },
    Entry {
        module: "glyphs",
        name: "Glyphs",
        group: Group::Foundations,
        glyph: glyphs::ADD_TO,
        summary: "The table of Lucide drawings, as SVG bytes. `LICENSE.lucide` sits beside it.",
        api: &["ADD_TO, ALBUM, ARTIST, BACK, CHEVRON, CLOSE, COMPOSER, DEBUG, DEVICES, DOWN, HOME, LAPTOP, LIBRARY, MINUS, MORE, MUTED, NEXT, NOTE, OFFLINE, ONLINE, PAUSE, PHONE, PLAY, PLAYING, PLAYLIST, PLUS, PREVIOUS, PULPIT, SEARCH, SHOW, SHUFFLE, SIGN_OUT, SKIP_BACK, SKIP_FORWARD, SPEAKER, STOP, TICK, TICKED, UNTICK, VOLUME"],
        rules: &["Drawn with currentColor; the colour is the icon's tint, never the file's."],
        harken: &["the sidebar's places, the transport, every menu entry"],
        phones: &["android ui/Glyphs.kt: ArkGlyphs; Glyphs.kt", "ios: SF Symbols"],
    },
    Entry {
        module: "icon",
        name: "Icon",
        group: Group::Foundations,
        glyph: glyphs::TICKED,
        summary: "A glyph drawn at a size in a colour asked of the palette. The named ones are the tones a row, a panel or a control needs.",
        api: &["SIZE (15)", "tinted(shape, size, |palette| colour)", "plain(shape, dim)", "accent(shape, on_cursor)", "line(shape, lit)", "faint(shape, on_cursor)", "tick(on_cursor)", "more(on_cursor)", "chevron(lit)"],
        rules: &["The colour goes through the svg style filter, so one drawing serves every tone."],
        harken: &["the play/pause on the playing row, the speaker in the bar, the close on a dialog"],
        phones: &["android ui/Components.kt: Glyph"],
    },
    Entry {
        module: "format",
        name: "Format",
        group: Group::Foundations,
        glyph: glyphs::NOTE,
        summary: "Words for numbers: a clock, a count spelt out, a plural, a span of time, a day.",
        api: &["clock(secs)", "spell(ms)", "plural(n, word)", "span(born, died)", "day(ms, now)"],
        rules: &["A length past an hour is h:mm:ss."],
        harken: &["the bar's clocks, a page's facts line, an episode's day"],
        phones: &[],
    },
    Entry {
        module: "fit",
        name: "Fit",
        group: Group::Foundations,
        glyph: glyphs::MINUS,
        summary: "Cutting text to a width: from the end or through the middle, measured in pixels, and the characters the typeface cannot draw spelt in ones it can.",
        api: &["Cut::{End, Middle}", "tail(s, chars)", "middle(s, chars)", "within(body, px, cut, measure)", "drawable(s)"],
        rules: &["A name loses its middle, a heading its end."],
        harken: &["menu titles, track and album names in the table"],
        phones: &[],
    },
    Entry {
        module: "url",
        name: "URL",
        group: Group::Foundations,
        glyph: glyphs::SEARCH,
        summary: "Percent-encoding for a fragment, so a `/`, a `#` or an accent survives the address bar.",
        api: &["encode(s, &mut out)", "encoded(s)", "decode(s)"],
        rules: &[],
        harken: &["a picture's address on the server"],
        phones: &[],
    },
    // Components.
    Entry {
        module: "sidebar",
        name: "Sidebar",
        group: Group::Components,
        glyph: glyphs::LIBRARY,
        summary: "A column of places down the left of a window, grouped under headings; the one under the cursor is the page beside it. This page's own sidebar is one.",
        api: &["Item { glyph, label, heading, count, press }", "WIDTH (200)", "view(items, cursor, focused, width, padding, id)"],
        rules: &[
            "One highlight, not two: the cursor is what the page shows, marked always, lit only while the sidebar has the keys.",
            "A heading is drawn where the group changes, not stored as a row, so row n is the cursor's n.",
            "Its pixel of edge is part of its width.",
        ],
        harken: &["Music, Shows and the playlists down the left (view.rs: view_sidebar)"],
        phones: &["android: the tab bar (HarkenApp.kt: TabBar, ui/Tabs.kt: TabStack)", "ios Shell.swift: Shell, a TabView"],
    },
    Entry {
        module: "panel",
        name: "Panel",
        group: Group::Components,
        glyph: glyphs::PLAYLIST,
        summary: "The shape every context window is: a bordered panel, a title, and fixed-height entries with a glyph column that is there whether or not it has a glyph.",
        api: &["panel(body, width)", "entry(glyph, label, trailing)", "title(label)", "label(body, lit)", "width_for(labels, min, max)", "chars(width, chrome)", "titled_height(rows)", "hang_above(right, bottom, w, h)", "PADDING, ENTRY, TITLE, ENTRY_CHAR, ENTRY_CHROME, EDGE, RADIUS"],
        rules: &["Heights are declared constants: a panel is placed before iced lays it out."],
        harken: &["the queue and devices panels over the bar"],
        phones: &["android ui/Components.kt: SheetRow", "ios Shell.swift: SheetHost"],
    },
    Entry {
        module: "table",
        name: "Table",
        group: Group::Components,
        glyph: glyphs::PLAYLIST,
        summary: "Rows of cells cut to their columns at layout, headings over them, section labels and the transport's gutter.",
        api: &["cell(body, width, on_cursor, accent, dim)", "cell_cut(…, cut, …)", "heading(label, width)", "section(label, indent)", "gutter()", "row(line, on_cursor, focused, odd)", "GUTTER, ROW_PADDING, SPACING, CELL_SIZE, HEADING_SIZE"],
        rules: &["A cell loses its middle and a heading its end, each to its own column.", "Columns are portions, so no text column pushes another off a narrow window."],
        harken: &["Songs, an album's tracks, a show's episodes (view.rs: view_list, view_tracks)"],
        phones: &["android Components.kt: TrackRow, TrackNumber", "ios Components.swift: TrackRow, EpisodeRow"],
    },
    Entry {
        module: "line",
        name: "Line",
        group: Group::Components,
        glyph: glyphs::MINUS,
        summary: "One line of text cut, at layout, to the width it is given; `gives_way` lays it out after the rest of its row.",
        api: &["Line::new(body, size, cut, colour)", "Line::gives_way()"],
        rules: &["A row lays out in order, so a line first in it would take everything; giving way puts it last."],
        harken: &["a page's title before its count"],
        phones: &[],
    },
    Entry {
        module: "cards",
        name: "Cards",
        group: Group::Components,
        glyph: glyphs::ALBUM,
        summary: "A grid of pictures with a name and a line under each, the arithmetic of how many fit, and a page's header of picture, name and facts.",
        api: &["Shelf { card, gap, padding }, Shelf::DEFAULT", "Shelf::columns(pane), side(pane), room(pane), grid(cells, pane)", "Card { seed, title, under, round, picture, open }", "view(shelf, cards, pane, cursor, empty, on_hover, scroll, on_scroll)", "view_card(side, card, on_cursor, hover)", "header(picture, kind, name, under, open_under, facts, padding)", "empty_page(message, padding)"],
        rules: &["A person is a circle and a record a square.", "The keyboard and the view divide the same pane width, so `l` lands where it looks."],
        harken: &["Albums, Artists, Composers, Podcasts (view.rs: view_cards, view_header)"],
        phones: &["android Components.kt: CoverCard, PageHeader", "ios Components.swift: CoverShelf, CoverCard, PageHeader"],
    },
    Entry {
        module: "hero",
        name: "Hero",
        group: Group::Components,
        glyph: glyphs::HOME,
        summary: "The wide banner a page can open with, the bold face, section headings, and a listing of entries in columns with a \"more\" after them.",
        api: &["banner(picture, kind, name, width)", "height(width), ASPECT, TALLEST", "heading(title)", "Entry { over, title, under, count, open }", "listing(entries, columns, cursor, on_hover)", "grid(entries, columns)", "entry(picture, e, on_cursor, hover)", "more(label, lit)", "columns(room), COLUMN", "BOLD, FIRA_SANS_BOLD"],
        rules: &["Fira Sans Bold is embedded: a browser has no system fonts, and with none bold is not bold."],
        harken: &["an artist's banner and sections (view.rs: view_artist, view_work)"],
        phones: &["android Components.kt: Banner, Entry, MoreLine; ui/Motion.kt: Hero", "ios Hero.swift: Banner, EntryRow, MoreRow"],
    },
    Entry {
        module: "art",
        name: "Art",
        group: Group::Components,
        glyph: glyphs::ALBUM,
        summary: "The derived square: a gradient picked from a name, the same on every device, and the real picture over it once it arrives.",
        api: &["hash(s) (FNV-1a over UTF-16)", "pair(seed)", "square(seed, size, corner)", "picture(seed, handle, side, corner)"],
        rules: &["The hash is the phones' too, so a name has one square everywhere."],
        harken: &["every cover before its picture arrives"],
        phones: &["android ui/Art.kt: Art, Square, Cover", "ios Theme.swift: Square; Covers.swift: CoverView"],
    },
    Entry {
        module: "account",
        name: "Account",
        group: Group::Components,
        glyph: glyphs::ARTIST,
        summary: "Who is signed in as a circle — their picture, or initials on a colour picked from their name — and the menu that hangs off it.",
        api: &["Who { name, id }, Who::full(), Who::initials()", "Account { who, header, entries }", "Account::signed_in, turned_away, alone", "Account::menu(right, bottom, window)", "face(who, picture, side)", "button(account, picture, press)", "origin(…)", "HEIGHT (28)"],
        rules: &["Nobody goes offline by hand: the menu signs out, and nothing else."],
        harken: &["the account at the top right (view.rs: view_top)"],
        phones: &["android Components.kt: Face", "ios Components.swift: Avatar, AccountMenu"],
    },
    // Overlays.
    Entry {
        module: "menu",
        name: "Menu",
        group: Group::Overlays,
        glyph: glyphs::MORE,
        summary: "The dropdown: a title and entries, as wide as its longest entry, opened at the pointer or lined up under a control, with submenus beside it. The theme control at the top of this page is one.",
        api: &["Menu::open(title, entries, cursor, window, anchor)", "Entry::run(glyph, label, msg), Entry::submenu(…)", "Anchor::{Pointer, RightEdge(x)}", "view(menu, focused, at, activate)", "Menu::submenu_origin, origin_for, height, width_for", "fit, right_aligned, submenu_height", "MIN_WIDTH, MAX_WIDTH, SUBMENU_OVERLAP, DWELL"],
        rules: &[
            "A right click opens at the pointer; a control's menu opens at the control.",
            "As wide as its longest entry, within 198..=420: AppKit's rule.",
            "A submenu laps the parent by exactly its padding, so the lit row touches its edge.",
            "A submenu opens after a 200ms rest, not on the way past.",
        ],
        harken: &["a row's more button and right click, the account's menu (main.rs: Menu::open)"],
        phones: &["android Sheets.kt: TrackMenu; ui/Components.kt: MoreButton", "ios Components.swift: TrackActions"],
    },
    Entry {
        module: "picker",
        name: "Picker",
        group: Group::Overlays,
        glyph: glyphs::TICK,
        summary: "A list of choices each ticked or not, with an optional last row that names a new one — on its own in the middle, or as a menu's submenu.",
        api: &["Picker::new(title, choices, new_row)", "Choice { value, name, on }", "Picker::activate() -> Picked::{Toggled, Naming}", "Picker::land, rows, grid, width_for", "view(picker, focused, Messages { at, activate, name, create })", "focus_naming()", "MIN_WIDTH, MAX_WIDTH, MAX_HEIGHT"],
        rules: &["A submenu draws no header: the menu it hangs off is still up with the title."],
        harken: &["Add to playlist (main.rs: open_picker)"],
        phones: &["android Sheets.kt: AddToPlaylist, DevicesSheet", "ios Sheets.swift: AddToPlaylistSheet, DevicesSheet"],
    },
    Entry {
        module: "context",
        name: "Context",
        group: Group::Overlays,
        glyph: glyphs::CHEVRON,
        summary: "A menu, its submenu, or a picker alone, and the rules for them together: which one has the keys, the dwell, `l` in and `h` out, `<Esc>`.",
        api: &["Context::new()", "focus() -> Option<Layer>", "open_menu, close_menu, open_picker, close_picker", "land(i), picker_at(i), tick(), chosen(), activate_picker()", "cancel(), travel(motion)", "Layer::{Menu, Picker}"],
        rules: &["Closing a menu takes its submenu with it.", "An app refuses its rows' hover while a layer is up: a backdrop cannot swallow a hover."],
        harken: &["App::ctx, every menu and picker (main.rs)"],
        phones: &[],
    },
    Entry {
        module: "layer",
        name: "Layer",
        group: Group::Overlays,
        glyph: glyphs::PLUS,
        summary: "What goes under and around an overlay: a backdrop that closes on a click and swallows the wheel, a dimmed one, and pinned or centred placement.",
        api: &["backdrop(close, swallow)", "dimmed(close, swallow)", "pinned(content, at)", "centred(content)"],
        rules: &["A submenu gets no backdrop of its own: a second one over the menu would eat its clicks."],
        harken: &["under every menu, picker and panel (view.rs: view)"],
        phones: &["ios Shell.swift: SheetHost"],
    },
    Entry {
        module: "dialog",
        name: "Dialog",
        group: Group::Overlays,
        glyph: glyphs::CLOSE,
        summary: "A titled window rising over a page as the page dims.",
        api: &["view(title, close, body, width, height, open)", "backdrop(close, swallow, open)", "opened(since, now) -> 0..=1", "opening(since, now)", "OPENING, RISE"],
        rules: &["`open` is eased; ask `opening` whether to keep a clock running."],
        harken: &["Follow a podcast (view.rs: view_adding)"],
        phones: &["android Sheets.kt: FindShow, NewPlaylistCard", "ios Sheets.swift: FollowSheet, NewPlaylist"],
    },
    // Behaviour.
    Entry {
        module: "vim",
        name: "Keys",
        group: Group::Behaviour,
        glyph: glyphs::NEXT,
        summary: "The keyboard grammar — counts, gg/G, ^d/^u, / and n/N, <Tab>, <Esc> — and the grids it moves a cursor over. j and k move this page's sidebar.",
        api: &["Keys::new(), press(key, mods) -> Option<Action>, pending(), mode()", "Action::{Move, Activate, Toggle, Search, Match, Cycle, Cancel, Jump, Key}", "Motion::{Down, Up, Left, Right, First, Last, To}", "Grid { cells, columns }, column, row, step, progress", "Section, Page", "presses()", "find(labels, needle, at, delta)", "PAGE"],
        rules: &["`step` returns None only at that edge, which means \"not mine\": the app says what lies past it."],
        harken: &["every pane, menu and picker"],
        phones: &[],
    },
    Entry {
        module: "route",
        name: "Route",
        group: Group::Behaviour,
        glyph: glyphs::BACK,
        summary: "Places as URL fragments, and the address bar kept in step: reconciled, not pushed, and a scrub one history entry. Each page here has one.",
        api: &["trait Route { fragment(), parse() }", "join(kind, name), split(fragment)", "Router::new(), sync(&place), replace_next(), poll(ready), go(by), can_go(by)", "AddressBar, Browser, Nav"],
        rules: &["`parse` never fails: an unknown fragment is home.", "Call `sync` after every update."],
        harken: &["App::router, the Place of every page"],
        phones: &["android ui/Destination.kt: Destination", "ios Shell.swift: Destination"],
    },
    Entry {
        module: "scroll",
        name: "Scroll",
        group: Group::Behaviour,
        glyph: glyphs::DOWN,
        summary: "Keeping a keyboard cursor on screen.",
        api: &["reveal(id, grid, at)", "reveal_to(id, progress)"],
        rules: &[],
        harken: &["the sidebar, the table and the cards follow the cursor"],
        phones: &["android ui/Tabs.kt: ScrollsToTop"],
    },
    Entry {
        module: "swipe",
        name: "Swipe",
        group: Group::Behaviour,
        glyph: glyphs::SKIP_BACK,
        summary: "Two fingers sideways as back and forward, with the arrow drawn over the page as they go.",
        api: &["Swipe::feed, tick, allow, active, offset, arrow", "band(…)", "view(page, swipe, now)", "FAR, LET_GO, QUIET, BACK, STRETCH"],
        rules: &["It goes when the fingers lift, and only where there is somewhere to go.", "In a browser the browser's own back does this instead."],
        harken: &["the page beside the sidebar (view.rs: view)"],
        phones: &["android ui/Motion.kt: PageBack"],
    },
    Entry {
        module: "wake",
        name: "Wake",
        group: Group::Behaviour,
        glyph: glyphs::ONLINE,
        summary: "What wakes a window that runs no loop: a bell anything can ring, one timer at a deadline, and the browser's address bar moving.",
        api: &["Bell::new(), ring(), rung(), subscription()", "at(key, delay)", "page_changes(f)"],
        rules: &["With nothing playing, dwelling or arriving, a window runs no timer."],
        harken: &["the peer's socket and the page's events ring App::bell (wake.rs)"],
        phones: &["android ui/Splash.kt: holdSplashUntil"],
    },
    Entry {
        module: "images",
        name: "Images",
        group: Group::Behaviour,
        glyph: glyphs::ALBUM,
        summary: "A picture cache: fetched off the render thread, decoded and bounded, kept on disk natively and in the Cache API in a browser. A failure is remembered.",
        api: &["Images::new(name)", "want(url) -> Option<Task<Loaded>>", "loaded(l), handle(url), bounded(url)", "Loaded, Rgba", "BOUND (384), WIDE (2400)"],
        rules: &["The renderer is handed pixels at the size they are drawn, not a megapixel JPEG to decode inside a frame."],
        harken: &["covers, banners and faces"],
        phones: &["android ui/Art.kt: Cover", "ios Covers.swift: CoverView"],
    },
];

pub const HARKEN_ONLY: &[HarkenOnly] = &[
    // The desktop and browser client.
    HarkenOnly {
        name: "Now-playing bar",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_bar",
        what: "The bar along the bottom: cover, title and artist, the transport, the seek bar between two clocks, then volume, queue and devices.",
        built_on: &["style", "icon", "art", "format"],
    },
    HarkenOnly {
        name: "Transport",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_bar",
        what: "Previous, play/pause, next and the skips, as icon buttons.",
        built_on: &["icon", "glyphs"],
    },
    HarkenOnly {
        name: "Seek bar",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_bar, progress",
        what: "An iced slider in style::seek, and a thin progress line under a row.",
        built_on: &["style"],
    },
    HarkenOnly {
        name: "Volume slider and mute",
        platform: "desktop",
        at: "harken/iced/src/view.rs: volume_slider, mute_button",
        what: "0 to 100, sending as dragged and once on release; the speaker or the speaker struck through.",
        built_on: &["style", "icon"],
    },
    HarkenOnly {
        name: "Queue panel",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_queue, view_queue_button",
        what: "What plays next, a panel over the bar.",
        built_on: &["panel", "layer"],
    },
    HarkenOnly {
        name: "Devices picker",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_devices, view_output",
        what: "The speakers in the house, each with join, mute and volume, a panel over the bar.",
        built_on: &["panel", "layer", "icon"],
    },
    HarkenOnly {
        name: "Top bar",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_top",
        what: "A server address to type and connect when alone; the account at the right.",
        built_on: &["account", "style"],
    },
    HarkenOnly {
        name: "Status line",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_status",
        what: "The last thing worth saying and the keys half typed; whole under D.",
        built_on: &["vim", "style"],
    },
    HarkenOnly {
        name: "Debug view",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_debug",
        what: "Every number the session holds, as a table.",
        built_on: &["table"],
    },
    HarkenOnly {
        name: "Keymap",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_help",
        what: "The keys and what they do, under ?.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Settings page",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_settings",
        what: "The licences and the window's settings.",
        built_on: &["hero"],
    },
    HarkenOnly {
        name: "Cover with play button",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_cover, play_button",
        what: "A record's cover with play over it.",
        built_on: &["art", "icon"],
    },
    HarkenOnly {
        name: "Featured",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_featured",
        what: "The best-known work set apart over a list.",
        built_on: &["art", "hero"],
    },
    HarkenOnly {
        name: "Faces",
        platform: "desktop",
        at: "harken/iced/src/view.rs: view_faces, initials",
        what: "A row of people as circles.",
        built_on: &["art", "cards"],
    },
    HarkenOnly {
        name: "Mosaic",
        platform: "desktop",
        at: "harken/iced/src/view.rs: mosaic",
        what: "A playlist's picture made of its first four covers.",
        built_on: &["art"],
    },
    HarkenOnly {
        name: "Unread ring",
        platform: "desktop",
        at: "harken/iced/src/view.rs: ringed, view_unread",
        what: "A ring round a show with episodes not yet heard.",
        built_on: &["art"],
    },
    HarkenOnly {
        name: "Work heading and record foot",
        platform: "desktop",
        at: "harken/iced/src/view.rs: work_heading, view_foot",
        what: "A composer and work over their movements; the year and label under a record.",
        built_on: &["table", "format"],
    },
    HarkenOnly {
        name: "Linked names",
        platform: "desktop",
        at: "harken/iced/src/view.rs: names, opens",
        what: "Names in a line that each open their page.",
        built_on: &["style"],
    },
    // The phones.
    HarkenOnly {
        name: "Expanding player card",
        platform: "android",
        at: "harken/android/.../ui/ExpandingCard.kt: ExpandingCard; PlayerSheet.kt: PlayerCard",
        what: "The player as a bar that grows to the screen, swipes sideways and closes on back.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Tab stack",
        platform: "android",
        at: "harken/android/.../ui/Tabs.kt: TabStack",
        what: "Each tab kept alive; a tab tapped again scrolls to top.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Page motion",
        platform: "android",
        at: "harken/android/.../ui/Motion.kt: Hero, PageFrame, PageBack, Pull",
        what: "The page transitions and the back gesture.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Springs",
        platform: "android",
        at: "harken/android/.../ui/Springs.kt; ios Theme.swift: Springs",
        what: "The spring numbers every animation moves by.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Volume slider",
        platform: "android",
        at: "harken/android/.../ui/Volume.kt: VolumeSlider; ios PlayerViews.swift: VolumeSlider",
        what: "A volume slider.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Splash",
        platform: "android",
        at: "harken/android/.../ui/Splash.kt: holdSplashUntil",
        what: "The splash held until the app is ready, up to a limit.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Onboarding",
        platform: "android",
        at: "harken/android/.../ui/Onboarding.kt; ios OnboardingViews.swift",
        what: "The opening, server, sign-in and permission screens.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Fetching and offline",
        platform: "android",
        at: "harken/android/.../ui/Components.kt: Fetching, Offline; ios Components.swift",
        what: "A delayed \"fetching from host\" and the offline note.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Play and shuffle pill",
        platform: "android",
        at: "harken/android/.../Components.kt: PlayPill; ios Hero.swift: PlayShuffle",
        what: "Play and shuffle under a record's header.",
        built_on: &[],
    },
    HarkenOnly {
        name: "Mini and full player",
        platform: "ios",
        at: "harken/ios/Harken/PlayerViews.swift: MiniPlayer, FullPlayer, Scrubber, Transport, QueueSheet",
        what: "The bar and the full-screen player as separate views.",
        built_on: &[],
    },
];

/// The catalogue as JSON, pretty: what `--json` prints and `catalogue.json`
/// holds.
pub fn json() -> String {
    serde_json::to_string_pretty(&CATALOGUE).expect("the catalogue is plain data") + "\n"
}
