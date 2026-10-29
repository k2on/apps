//! The docs by component: one page each, with how to use it on the desktop
//! (arkui, its modules in [`crate::catalogue`]) and on each phone.
//!
//! A component is the thing an app is made of — a menu, a card, a list —
//! whatever each platform calls it. Only what is shared: on the desktop it
//! is arkui's, and its page shows it drawn by arkui itself; on a phone it is the
//! shared Compose (`ui/`, ark-ui for Android) or SwiftUI (ArkUI for SwiftUI,
//! `swift/ArkUI`), and its page says where and how, with the latest capture
//! from a real emulator and simulator: `shots` names them, the showcase apps
//! draw them (`showcase/`), the `screens` workflow takes them
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
    /// to (showcase/android's Showcase.kt, showcase/ios's ShowcaseApp.swift).
    pub id: &'static str,
    pub kind: Kind,
    /// What it shows.
    pub what: &'static str,
    /// The phones it is captured on: `android`, `ios`.
    pub phones: &'static [&'static str],
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

const ANDROID: &[&str] = &["android"];
const BOTH: &[&str] = &["android", "ios"];

const fn shot(id: &'static str, what: &'static str, phones: &'static [&'static str]) -> Shot {
    Shot {
        id,
        kind: Kind::Screenshot,
        what,
        phones,
    }
}

const fn recording(id: &'static str, what: &'static str, phones: &'static [&'static str]) -> Shot {
    Shot {
        id,
        kind: Kind::Recording,
        what,
        phones,
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
            at: &["ui/Theme.kt: ArkTheme, LocalArkTheme, Space"],
            how: &[
                "Inside a MaterialTheme, provide LocalArkTheme with ArkTheme(art): the derived square's pairs, ArkTheme.NeutralLight and NeutralDark until an app gives its own.",
                "Space.xs … Space.xxl, Space.page, Space.corner: the spacing every shared component lays out by.",
            ],
        }),
        ios: Some(Mobile {
            at: &["swift/ArkUI: Space, Squares, \\.squares, Color(light:dark:)"],
            how: &[
                "import ArkUI. Space.xs … Space.xxl, Space.page, Space.corner: the spacing, Android's.",
                "Set the derived square's colours once at the root: .environment(\\.squares, Squares(light:dark:)); Squares.neutral until an app does.",
            ],
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
        android: None,
        ios: None,
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
            at: &["ui/Glyphs.kt: ArkGlyphs", "ui/Components.kt: Glyph"],
            how: &["Glyph(icon, description, modifier, size = 24.dp)", "ArkGlyphs.More, Back, Chevron, Volume, Muted are ImageVectors; tinted by LocalContentColor."],
        }),
        ios: None,
        shots: &[],
    },
    Component {
        id: "text",
        name: "Text",
        section: Section::Foundations,
        glyph: glyphs::MINUS,
        summary: "Words fitted to a width, cut where a reader loses least, and numbers said as words.",
        desktop: &["fit", "line", "format"],
        android: None,
        ios: None,
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
            at: &["swift/ArkUI: Square, Art (ArkUICore)"],
            how: &["Square(seed:): the same square for the same name as Android's and the desktop's.", "Art.hash and Art.pick are in ArkUICore, which needs only Foundation."],
        }),
        shots: &[shot("art", "Squares derived from twelve names.", BOTH)],
    },
    // Components.
    Component {
        id: "navigation",
        name: "Navigation",
        section: Section::Components,
        glyph: glyphs::LIBRARY,
        summary: "Where the places are: a sidebar on the desktop, tabs on a phone. The place under the cursor, or the tab that is lit, is the page.",
        desktop: &["sidebar"],
        android: Some(Mobile {
            at: &["ui/Tabs.kt: TabStack, Reselect, LocalReselect"],
            how: &["TabStack keeps each tab's content alive while another is shown, each with its Reselect in LocalReselect.", "Reselect.ask() is the lit tab tapped again."],
        }),
        ios: None,
        shots: &[],
    },
    Component {
        id: "lists",
        name: "Lists",
        section: Section::Components,
        glyph: glyphs::PLAYLIST,
        summary: "Rows of what a page holds: what each is, a line under it, and where it goes.",
        desktop: &["table"],
        android: Some(Mobile {
            at: &["ui/Components.kt: ListRow, Chevron"],
            how: &["ListRow(onClick, title, under, trailing = { Chevron() }) { picture }"],
        }),
        ios: None,
        shots: &[shot("lists", "Rows with a picture and a line under each.", ANDROID)],
    },
    Component {
        id: "reorder",
        name: "Reorderable list",
        section: Section::Components,
        glyph: glyphs::PLAYLIST,
        summary: "Rows dragged into a new order, the rest sliding out of the way: what a queue is rearranged with.",
        desktop: &["reorder"],
        android: Some(Mobile {
            at: &["ui/Reorder.kt: rememberReorder, ReorderableItem"],
            how: &[
                "A LazyColumn drawn in reorder.order(items, key), each row in ReorderableItem(reorder, key, keys): a long press lifts it, and onMove(from, to) is told on the drop.",
                "The rows that make way are the list's own animateItem on Springs.MOVE; a tap still does what a tap did.",
            ],
        }),
        ios: Some(Mobile {
            at: &["swift/ArkUI: Reorderable, ReorderHandle"],
            how: &[
                "Reorderable(items, key:, onMove:) { item, handle in … }: the row places the handle it is given, and is dragged by it, as the Music app's queue is.",
                "The rows that make way move on Springs.move; the row keeps its tap and its context menu.",
            ],
        }),
        shots: &[],
    },
    Component {
        id: "cards",
        name: "Cards",
        section: Section::Components,
        glyph: glyphs::ALBUM,
        summary: "Pictures with a name and a line under each, in a grid or a shelf; a square, or a circle.",
        desktop: &["cards"],
        android: Some(Mobile {
            at: &["ui/Components.kt: CoverCard"],
            how: &["CoverCard(seed, art, title, under, side, onClick, round = false, opens = false)", "A shelf is a LazyRow of them, `side` from the screen's width."],
        }),
        ios: None,
        shots: &[shot("cards", "Cards, square and round.", ANDROID)],
    },
    Component {
        id: "headers",
        name: "Headers",
        section: Section::Components,
        glyph: glyphs::HOME,
        summary: "How a page opens: a banner or a bar with its title and back, then headings over its sections.",
        desktop: &["hero"],
        android: Some(Mobile {
            at: &["ui/Components.kt: PageBar, TabTitle, Heading"],
            how: &["PageBar(title, onBack, actions = {})", "TabTitle(title, actions = {})", "Heading(title, modifier, action = null, onAction = {})"],
        }),
        ios: None,
        shots: &[shot("headers", "A page's bar, a tab's title, a heading.", ANDROID)],
    },
    Component {
        id: "empty",
        name: "Empty states",
        section: Section::Components,
        glyph: glyphs::MINUS,
        summary: "What a page says while it has nothing to show, or is still asking for it.",
        desktop: &[],
        android: Some(Mobile {
            at: &["ui/Components.kt: Empty, Fetching"],
            how: &["Empty(icon, title, sentence, modifier, action = null, onAction = {})", "Fetching(host, delayMs): said only after a beat, so a quick answer never flashes it."],
        }),
        ios: None,
        shots: &[shot("empty", "Nothing here yet, and still asking.", ANDROID)],
    },
    Component {
        id: "account",
        name: "Account and sign-in",
        section: Section::Components,
        glyph: glyphs::ARTIST,
        summary: "Who is signed in, as a circle of their picture or their initials, and the screens that get them signed in.",
        desktop: &["account"],
        android: Some(Mobile {
            at: &["ui/Onboarding.kt: OpeningScreen, ServerScreen, SignInScreen, PermissionScreen, OnboardingPage"],
            how: &["SignInScreen(mark, host, error, phase, onSignIn, onChangeServer)", "OnboardingPage is the frame each screen is drawn in: the mark, a title, a sentence, then its own."],
        }),
        ios: None,
        shots: &[shot("sign-in", "Signing in to a server.", ANDROID)],
    },
    Component {
        id: "slider",
        name: "Slider",
        section: Section::Components,
        glyph: glyphs::VOLUME,
        summary: "A level to drag, and a button beside it to mute it.",
        desktop: &[],
        android: Some(Mobile {
            at: &["ui/Volume.kt: VolumeSlider"],
            how: &["VolumeSlider(level, muted, onLevel, onDone, onMute, modifier, enabled = true, about = \"Volume\")"],
        }),
        ios: None,
        shots: &[shot("slider", "A level and its mute.", ANDROID)],
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
            at: &["ui/Components.kt: SheetRow, MoreButton"],
            how: &["SheetRow(icon, label, onClick, trailing = {}, emphasis = false) is one entry of a sheet.", "MoreButton(about, origin, onMore) opens it from a row."],
        }),
        ios: None,
        shots: &[shot("sheet", "A sheet's entries.", ANDROID)],
    },
    Component {
        id: "picker",
        name: "Picker",
        section: Section::Overlays,
        glyph: glyphs::TICK,
        summary: "Choices each ticked or not, and a last row that names a new one.",
        desktop: &["picker"],
        android: None,
        ios: None,
        shots: &[],
    },
    Component {
        id: "dialog",
        name: "Dialog",
        section: Section::Overlays,
        glyph: glyphs::CLOSE,
        summary: "A window over the page as the page dims, a sheet along its edge, and what goes under every overlay.",
        desktop: &["dialog", "sheet", "layer"],
        android: None,
        ios: None,
        shots: &[],
    },
    // Motion.
    Component {
        id: "expanding-card",
        name: "Expanding card",
        section: Section::Motion,
        glyph: glyphs::PLAY,
        summary: "A bar at the foot of the screen that rises into the whole screen and falls back, following a finger or on its own.",
        desktop: &[],
        android: Some(Mobile {
            at: &["ui/ExpandingCard.kt: ExpandingCard, ExpandingCardState, rememberExpandingCard"],
            how: &[
                "ExpandingCard(state, stand, onOpen, collapsed, expanded, traveler): `collapsed` is the bar's face, `expanded` the screen's with a `slot` where `traveler`, the picture, lands.",
                "state.open() and state.close() move it on its springs; ExpandingCardState.held(p, density) holds it at p for a screenshot.",
            ],
        }),
        ios: Some(Mobile {
            at: &["swift/ArkUI: View.expandingCard(isPresented:bar:expanded:) (iOS 26)"],
            how: &[
                "On a TabView: tabView.expandingCard(isPresented: $open) { bar } expanded: { screen }. The bar sits on glass above the tab bar (tabViewBottomAccessory) and zooms into the screen and back (navigationTransition(.zoom)).",
                "The system does the motion, a swipe down on the screen included; `isPresented` going false zooms it back into the bar.",
            ],
        }),
        shots: &[recording("expanding-card", "The bar rising to the screen, and falling back.", BOTH)],
    },
    Component {
        id: "pages",
        name: "Page motion",
        section: Section::Motion,
        glyph: glyphs::SKIP_BACK,
        summary: "Going in and coming back: a page grows over the last, and a swipe from the edge takes it away.",
        desktop: &["swipe"],
        android: Some(Mobile {
            at: &["ui/Motion.kt: PageFrame, Moving, PageBack, Hero"],
            how: &["PageBack(motion, blocked, toFirst): the edge swipe, following the finger.", "Hero carries a picture from the card to the page's header."],
        }),
        ios: None,
        shots: &[],
    },
    Component {
        id: "springs",
        name: "Springs",
        section: Section::Motion,
        glyph: glyphs::SHUFFLE,
        summary: "The numbers and curves everything moves by, the same on both phones; on the desktop, the curves and what moves along them.",
        desktop: &["motion"],
        android: Some(Mobile {
            at: &["ui/Springs.kt: Springs"],
            how: &["Springs.settle() and the rest: spring specs with a cap on how long any move takes."],
        }),
        ios: Some(Mobile {
            at: &["swift/ArkUI: Springs"],
            how: &["Springs.move, Springs.size, Springs.fade: Android's springs converted exactly (docs/plan-arkui-swift.md)."],
        }),
        shots: &[],
    },
    Component {
        id: "scroll",
        name: "Scroll",
        section: Section::Motion,
        glyph: glyphs::DOWN,
        summary: "Keeping the cursor on screen, and back to the top when the lit tab is chosen again.",
        desktop: &["scroll"],
        android: Some(Mobile {
            at: &["ui/Tabs.kt: ScrollsToTop"],
            how: &["ScrollsToTop(listState) or ScrollsToTop(gridState) inside a tab's page: a Reselect scrolls it home."],
        }),
        ios: Some(Mobile {
            at: &["The system's: TabView with a NavigationStack per tab"],
            how: &[
                "Nothing to add: the shown tab, tapped again, takes its stack back to its root and then its list to the top.",
                "To do it from code, ScrollViewReader's scrollTo(first, anchor: .top) inside withAnimation(Springs.move). The recording sets a ScrollPosition a frame at a time along Springs.move's curve, which plays the same.",
            ],
        }),
        shots: &[recording("scroll-to-top", "A long list scrolled down, then asked to go to its top.", BOTH)],
    },
    // Behaviour.
    Component {
        id: "keys",
        name: "Keys",
        section: Section::Behaviour,
        glyph: glyphs::NEXT,
        summary: "The keyboard grammar and the grids it moves a cursor over.",
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
        ios: None,
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
