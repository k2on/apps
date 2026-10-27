//! The docs by component: one page each, with how to use it on the desktop
//! (arkui, its modules in [`crate::catalogue`]) and on each phone.
//!
//! A component is the thing an app is made of — a menu, a card, the player —
//! whatever each platform calls it. On the desktop it is arkui's, and its
//! page draws the live example. On the phones it is Compose or SwiftUI, and
//! its page says where and how, with the latest capture from a real emulator
//! and simulator: `shots` names them, the `screens` workflow takes them
//! (`.github/workflows/screens.yaml`), and the page fetches what it last
//! published.
use arkui::glyphs;
use serde::Serialize;

/// The sidebar's headings, in the order they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    /// The colours, the styles, the glyphs and the words.
    Foundations,
    /// What a page is built of.
    Components,
    /// What goes over a page.
    Overlays,
    /// What moves.
    Motion,
    /// The rules with no drawing of their own.
    Behaviour,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Foundations,
        Section::Components,
        Section::Overlays,
        Section::Motion,
        Section::Behaviour,
    ];

    pub fn heading(self) -> &'static str {
        match self {
            Section::Foundations => "Foundations",
            Section::Components => "Components",
            Section::Overlays => "Overlays",
            Section::Motion => "Motion",
            Section::Behaviour => "Behaviour",
        }
    }
}

/// One phone's half of a component.
#[derive(Debug, Clone, Serialize)]
pub struct Mobile {
    /// Where it is: a file and what in it.
    pub at: &'static [&'static str],
    /// How to use it, a line at a time: its signature, then what to know.
    pub how: &'static [&'static str],
}

/// A capture from a real device: a still, or frames played in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A screen held still: the point is what it looks like.
    Screenshot,
    /// A few seconds of frames: the point is how it moves.
    Recording,
}

/// One capture, taken on both phones under the same id.
#[derive(Debug, Clone, Serialize)]
pub struct Shot {
    /// The file it is published as, and the id each phone's showcase answers
    /// to (Showcase.kt on Android, Showcase.swift on iOS).
    pub id: &'static str,
    pub kind: Kind,
    /// What it shows.
    pub what: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    /// The page's fragment.
    pub id: &'static str,
    pub name: &'static str,
    pub section: Section,
    #[serde(skip)]
    pub glyph: &'static [u8],
    pub summary: &'static str,
    /// The arkui modules that are it on the desktop, in the order the page
    /// lists them. Every module is in exactly one component.
    pub desktop: &'static [&'static str],
    pub android: Option<Mobile>,
    pub ios: Option<Mobile>,
    pub shots: &'static [Shot],
}

pub fn find(id: &str) -> Option<&'static Component> {
    COMPONENTS.iter().find(|c| c.id == id)
}

/// The component an arkui module belongs to.
pub fn of_module(module: &str) -> Option<&'static Component> {
    COMPONENTS.iter().find(|c| c.desktop.contains(&module))
}

const fn shot(id: &'static str, what: &'static str) -> Shot {
    Shot {
        id,
        kind: Kind::Screenshot,
        what,
    }
}

const fn recording(id: &'static str, what: &'static str) -> Shot {
    Shot {
        id,
        kind: Kind::Recording,
        what,
    }
}

pub const COMPONENTS: &[Component] = &[
    // Foundations.
    Component {
        id: "theme",
        name: "Theme",
        section: Section::Foundations,
        glyph: glyphs::SHUFFLE,
        summary: "An app's colours for light and dark, installed once, and the gradient pairs the derived square picks from. Nothing draws a colour it did not ask the theme for.",
        desktop: &["theme"],
        android: Some(Mobile {
            at: &["ui/Theme.kt: ArkTheme, Space", "Theme.kt: the app's MaterialTheme and Palette"],
            how: &[
                "The app's theme wraps the root in MaterialTheme(colorScheme for dark or light); read colours from MaterialTheme.colorScheme.",
                "Space.xs … Space.xl, Space.corner: the spacing scale every screen uses.",
                "ArkTheme(art) carries the derived square's pairs; an app hands it its own, as the desktop installs its palette.",
            ],
        }),
        ios: Some(Mobile {
            at: &["Theme.swift: Space, Squares"],
            how: &["Space.* for spacing; the system's colours, with the app's colour as the accent.", "Squares holds the derived square's pairs."],
        }),
        shots: &[],
    },
    Component {
        id: "style",
        name: "Style",
        section: Section::Foundations,
        glyph: glyphs::LIBRARY,
        summary: "Text tones, buttons, scrollbars, grounds and surfaces, and the rows a cursor lights.",
        desktop: &["style"],
        android: Some(Mobile {
            at: &["Theme.kt: Palette, the app's Typography"],
            how: &["Material 3's roles: onSurfaceVariant for a dim line, primary for the accent.", "The typography sets names and headings in bold, as on the desktop."],
        }),
        ios: Some(Mobile {
            at: &["Theme.swift"],
            how: &[".foregroundStyle(.secondary) for a dim line, .tint for the accent."],
        }),
        shots: &[],
    },
    Component {
        id: "icons",
        name: "Icons",
        section: Section::Foundations,
        glyph: glyphs::TICKED,
        summary: "The Lucide drawings, tinted from the theme.",
        desktop: &["glyphs", "icon"],
        android: Some(Mobile {
            at: &["ui/Glyphs.kt: ArkGlyphs", "Glyphs.kt: Glyphs", "ui/Components.kt: Glyph"],
            how: &["Glyph(icon, description, modifier, size = 24.dp)", "ArkGlyphs.* and Glyphs.* are ImageVectors; tint with LocalContentColor."],
        }),
        ios: Some(Mobile {
            at: &["SF Symbols; Assets.xcassets for the pulpit"],
            how: &["Image(systemName:) for everything SF Symbols has; Image(\"pulpit\") is a template vector."],
        }),
        shots: &[],
    },
    Component {
        id: "text",
        name: "Text",
        section: Section::Foundations,
        glyph: glyphs::MINUS,
        summary: "Words fitted to a width, cut where a reader loses least, and numbers said as words.",
        desktop: &["fit", "line", "format"],
        android: Some(Mobile {
            at: &["Compose Text"],
            how: &["maxLines = 1 and overflow = TextOverflow.Ellipsis cut the end; the platform has no middle cut."],
        }),
        ios: Some(Mobile {
            at: &["SwiftUI Text"],
            how: &[".lineLimit(1) and .truncationMode(.middle) for a name, .tail for a heading."],
        }),
        shots: &[],
    },
    Component {
        id: "art",
        name: "Art",
        section: Section::Foundations,
        glyph: glyphs::ALBUM,
        summary: "The derived square, the same on every device for one name, and the real picture over it once it arrives.",
        desktop: &["art", "images"],
        android: Some(Mobile {
            at: &["ui/Art.kt: Art, Square, Cover"],
            how: &["Square(seed, modifier)", "Cover(seed, art, side, modifier, round = false, corner = Space.corner): the square until the picture loads.", "Art.hash is FNV-1a over UTF-16, the desktop's."],
        }),
        ios: Some(Mobile {
            at: &["Theme.swift: Square", "Covers.swift: CoverView"],
            how: &["Square(seed:)", "CoverView(seed:url:side:round:corner:)"],
        }),
        shots: &[],
    },
    // Components.
    Component {
        id: "navigation",
        name: "Navigation",
        section: Section::Components,
        glyph: glyphs::LIBRARY,
        summary: "Where the places are: a sidebar on the desktop, a tab bar on a phone. The one under the cursor, or the tab that is lit, is the page.",
        desktop: &["sidebar"],
        android: Some(Mobile {
            at: &["ui/Tabs.kt: TabStack, Reselect", "the app's Chrome: the tab bar and the mini player"],
            how: &["Chrome(tab, player, controls, onTab) { page }: the tab bar and the mini player around a page.", "TabStack keeps each tab's page alive; tapping the lit tab again is Reselect."],
        }),
        ios: Some(Mobile {
            at: &["Shell.swift: Shell"],
            how: &["Shell is a TabView, Home, Library and Search, each tab its own NavigationStack; the mini player is its tabViewBottomAccessory.", ".tabBarMinimizeBehavior(.onScrollDown): the bar shrinks as a page scrolls."],
        }),
        shots: &[shot("library", "Library: the tab bar, the mini player, and the songs.")],
    },
    Component {
        id: "rows",
        name: "Rows",
        section: Section::Components,
        glyph: glyphs::PLAYLIST,
        summary: "A list of tracks or episodes: what it is, what is under it, and which one is playing.",
        desktop: &["table"],
        android: Some(Mobile {
            at: &["Components.kt: TrackRow, TrackNumber", "ui/Components.kt: ListRow, Chevron", "Pages.kt: EpisodeRow"],
            how: &[
                "TrackRow(track, under, playing, onClick, onMore, art = true) { leading }",
                "ListRow(onClick, title, under, trailing = { Chevron() }) { picture }",
                "TrackNumber(n, playing) is the leading slot on an album's page.",
            ],
        }),
        ios: Some(Mobile {
            at: &["Components.swift: TrackRow, EpisodeRow, TrackNumber"],
            how: &["TrackRow(track:under:from:onPlay:)", "Its context menu is TrackActions, the same entries as Android's sheet."],
        }),
        shots: &[shot("album", "An album's page: its header, then its tracks, one playing.")],
    },
    Component {
        id: "cards",
        name: "Cards",
        section: Section::Components,
        glyph: glyphs::ALBUM,
        summary: "Pictures with a name and a line under each, in a grid or a shelf. A record is a square and a person a circle.",
        desktop: &["cards"],
        android: Some(Mobile {
            at: &["ui/Components.kt: CoverCard"],
            how: &["CoverCard(seed, art, title, under, side, onClick, round = false, opens = false)", "A shelf is a LazyRow of them, side from the screen's width."],
        }),
        ios: Some(Mobile {
            at: &["Components.swift: CoverCard, CoverShelf"],
            how: &["CoverCard(seed:url:title:under:side:round:opens:onTap:)", "CoverShelf(items:…) is a horizontal ScrollView of them."],
        }),
        shots: &[shot("home", "Home: shelves of covers and people.")],
    },
    Component {
        id: "header",
        name: "Headers",
        section: Section::Components,
        glyph: glyphs::HOME,
        summary: "How a page opens: a banner, or a picture beside the name and its facts, with play and shuffle; then section headings and listings.",
        desktop: &["hero"],
        android: Some(Mobile {
            at: &["Components.kt: PageHeader, Banner, CoverHeader, SectionHeading, Entry, MoreLine, Featured, PlayPill"],
            how: &[
                "PageHeader(cover, kind, title, by, onBy, facts, canPlay, onPlay, onShuffle)",
                "Banner(seed, art, kind, name, onBack): an artist's page, under the status bar.",
                "Entry(title, over, under, count, onOpen, picture = null) and MoreLine(label, onOpen) for listings.",
            ],
        }),
        ios: Some(Mobile {
            at: &["Hero.swift: Banner, RecordHeader, SectionHeading, EntryRow, MoreRow, PlayShuffle", "Components.swift: PageHeader"],
            how: &["PageHeader(kind:title:by:onBy:facts:canPlay:onPlay:onShuffle:)", "Banner(url:kind:name:)", "EntryRow(over:title:under:count:onOpen:)"],
        }),
        shots: &[shot("artist", "An artist's page: the banner, then their works.")],
    },
    Component {
        id: "account",
        name: "Account",
        section: Section::Components,
        glyph: glyphs::ARTIST,
        summary: "Who is signed in, as a circle of their picture or their initials, and signing in.",
        desktop: &["account"],
        android: Some(Mobile {
            at: &["Components.kt: Face", "ui/Onboarding.kt: OpeningScreen, ServerScreen, SignInScreen, PermissionScreen"],
            how: &["Face(name, art, role, onOpen)", "SignInScreen(mark, host, error, phase, onSignIn, onChangeServer)", "The onboarding's screens take an AppIdentity: the app's name, mark and colours."],
        }),
        ios: Some(Mobile {
            at: &["Components.swift: Avatar, AccountMenu", "OnboardingViews.swift: OpeningScreen, ServerScreen, SignInScreen"],
            how: &["Avatar(name:picture:side:)", "SignInScreen(server:phase:error:)"],
        }),
        shots: &[shot("sign-in", "Signing in to a server.")],
    },
    // Overlays.
    Component {
        id: "menu",
        name: "Menu",
        section: Section::Overlays,
        glyph: glyphs::MORE,
        summary: "What can be done about one thing: a dropdown at the pointer or under a control on the desktop, a sheet from the foot of a phone.",
        desktop: &["menu", "context", "panel"],
        android: Some(Mobile {
            at: &["Sheets.kt: TrackMenu", "ui/Components.kt: MoreButton, SheetRow"],
            how: &["TrackMenu(track, album, from, actions)", "SheetRow(icon, label, onClick, trailing = {}, emphasis = false) is one entry.", "MoreButton(about, origin, onMore) opens it from a row."],
        }),
        ios: Some(Mobile {
            at: &["Components.swift: TrackActions"],
            how: &["TrackActions(track:from:) is a .contextMenu's content: long-press a row."],
        }),
        shots: &[shot("track-menu", "A track's menu.")],
    },
    Component {
        id: "picker",
        name: "Picker",
        section: Section::Overlays,
        glyph: glyphs::TICK,
        summary: "Choices each ticked or not, and a last row that names a new one.",
        desktop: &["picker"],
        android: Some(Mobile {
            at: &["Sheets.kt: AddToPlaylist, DevicesSheet"],
            how: &["AddToPlaylist(track, playlists, on, onToggle, onNew)", "DevicesSheet(rows, cursor, actions): the speakers, each with its volume."],
        }),
        ios: Some(Mobile {
            at: &["Sheets.swift: AddToPlaylistSheet, DevicesSheet"],
            how: &["AddToPlaylistSheet(track:)", "DevicesSheet()"],
        }),
        shots: &[shot("add-to-playlist", "Adding a track to playlists.")],
    },
    Component {
        id: "dialog",
        name: "Dialog",
        section: Section::Overlays,
        glyph: glyphs::CLOSE,
        summary: "A window over the page as the page dims, and what goes under every overlay.",
        desktop: &["dialog", "layer"],
        android: Some(Mobile {
            at: &["Sheets.kt: NewPlaylistCard, NewPlaylist, FindShow"],
            how: &["NewPlaylistCard(name, problem, adding, onName, onMake, onCancel)", "FindShow(kind, feeds, onFollow): following a show."],
        }),
        ios: Some(Mobile {
            at: &["Sheets.swift: NewPlaylist, FollowSheet", "Shell.swift: SheetHost"],
            how: &["NewPlaylist(then:)", "SheetHost raises every sheet from the model's state."],
        }),
        shots: &[shot("new-playlist", "Naming a new playlist.")],
    },
    // Motion.
    Component {
        id: "player",
        name: "Player",
        section: Section::Motion,
        glyph: glyphs::PLAY,
        summary: "What is playing: a bar at the foot that rises into the whole screen and falls back.",
        desktop: &[],
        android: Some(Mobile {
            at: &["ui/ExpandingCard.kt: ExpandingCard, ExpandingCardState", "PlayerScreens.kt: MiniPlayer, FullPlayer, QueueSheet", "PlayerSheet.kt: PlayerCard"],
            how: &[
                "ExpandingCard(state, stand, onOpen, collapsed, expanded, traveler): the bar's bounds to the screen's, on a spring.",
                "MiniPlayer(state, controls, position) and FullPlayer(state, controls, position) are its two faces.",
            ],
        }),
        ios: Some(Mobile {
            at: &["PlayerViews.swift: MiniPlayer, FullPlayer, Scrubber, Transport, QueueSheet"],
            how: &["MiniPlayer(open:) is the TabView's bottom accessory, its matchedTransitionSource.", "FullPlayer(close:) is a fullScreenCover that zooms out of it (navigationTransition(.zoom)): iOS's ExpandingCard."],
        }),
        shots: &[recording("player", "The mini player rising into the full player, and falling back.")],
    },
    Component {
        id: "pages",
        name: "Page motion",
        section: Section::Motion,
        glyph: glyphs::SKIP_BACK,
        summary: "Going in and coming back: a page slides over the last, and a swipe from the edge takes it away.",
        desktop: &["swipe"],
        android: Some(Mobile {
            at: &["ui/Motion.kt: PageFrame, Moving, PageBack, Hero"],
            how: &["PageBack(motion, blocked, toFirst): the edge swipe, following the finger.", "Hero carries a cover from the card to the page's header."],
        }),
        ios: Some(Mobile {
            at: &["Shell.swift: stack", "Components.swift: zoomed"],
            how: &["Pages are pushed on the tab's NavigationStack (environment \\.open appends a Route); its own edge swipe goes back.", "A page opened from a card zooms out of it: .zoomed(route, zoom), navigationTransition(.zoom)."],
        }),
        shots: &[recording("page", "Opening an album from Home, and swiping back.")],
    },
    Component {
        id: "scroll",
        name: "Scroll",
        section: Section::Motion,
        glyph: glyphs::DOWN,
        summary: "Keeping the cursor on screen, and back to the top when the lit tab is chosen again.",
        desktop: &["scroll"],
        android: Some(Mobile {
            at: &["ui/Tabs.kt: ScrollsToTop, Reselect"],
            how: &["ScrollsToTop(listState) inside a tab's page: a reselect scrolls it home."],
        }),
        ios: Some(Mobile {
            at: &["Shell.swift: Shell"],
            how: &["The system's own: tapping the lit tab pops its NavigationStack, then scrolls to the top."],
        }),
        shots: &[recording("scroll-to-top", "A long list scrolled down, then the lit tab tapped again.")],
    },
    // Behaviour.
    Component {
        id: "keys",
        name: "Keys",
        section: Section::Behaviour,
        glyph: glyphs::NEXT,
        summary: "The keyboard grammar and the grids it moves a cursor over. j and k walk this page's sidebar.",
        desktop: &["vim"],
        android: None,
        ios: None,
        shots: &[],
    },
    Component {
        id: "routes",
        name: "Routes",
        section: Section::Behaviour,
        glyph: glyphs::BACK,
        summary: "Places as addresses, kept in step with the address bar or the back stack.",
        desktop: &["route", "url"],
        android: Some(Mobile {
            at: &["ui/Destination.kt: Destination, Destinations"],
            how: &["Destination(owner, name) { content }: a page's own ViewModel store, kept while it is on the stack."],
        }),
        ios: Some(Mobile {
            at: &["Model.swift: Route", "Shell.swift: Destination"],
            how: &["Route is the NavigationStack's value; .navigationDestination(for: Route.self) draws Destination(route:)."],
        }),
        shots: &[],
    },
    Component {
        id: "waking",
        name: "Waking",
        section: Section::Behaviour,
        glyph: glyphs::ONLINE,
        summary: "What wakes a window that runs no loop, and what holds the splash until the first screen is ready.",
        desktop: &["wake"],
        android: Some(Mobile {
            at: &["ui/Splash.kt: holdSplashUntil"],
            how: &["holdSplashUntil(ceilingMs) { ready }: in onCreate, before setContent."],
        }),
        ios: None,
        shots: &[],
    },
];
