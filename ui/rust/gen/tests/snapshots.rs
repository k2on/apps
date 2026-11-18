//! (`#[view]` writes `cfg(feature = "iced")` and `"web"` for an app's crate,
//! which has them; this test crate has neither.)
#![allow(unexpected_cfgs)]

//! What the emitters write for a view, held to the files beside this one
//! (`tests/snapshots/`): the generator's own check, now that no app's
//! generated screens are committed. A view of no app's: a list page, its
//! rows from a `for`, a sheet behind an `if let`, a field whose message
//! carries what was typed, a message built from a row's field, and a menu
//! bar.
//!
//! After a deliberate change to an emitter:
//! `ARK_UPDATE_SNAPSHOTS=1 cargo test -p ark-ui-gen --test snapshots`.

use ark_app::shell::{adaptive_tabs, command, command_menu, hosted, section_tab, CommandMenu, MenuPlace};
use ark_app::view;
use ark_app::view::*;
use ark_ui_gen::app::{files, App, Generated, Names};

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub name: String,
    pub problem: Option<String>,
    pub kind: String,
    pub kinds: Vec<Kind>,
    pub can_save: bool,
}

/// A kind a thing can be, found by typing: under its group, a count at
/// its end in its tint.
#[derive(Clone, Debug, PartialEq)]
pub struct Kind {
    pub key: String,
    pub title: String,
    pub glyph: String,
    pub group: String,
    pub count: String,
    pub tint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListPage {
    pub items: Vec<Item>,
    pub adding: Option<Draft>,
    pub footer: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ListMsg {
    Open { id: String },
    Add,
    Name { name: String },
    Kind { key: String },
    Save,
    Dismiss,
    Refresh,
    Play,
    Shuffle,
    Toggle,
    Back,
    Forward,
    Seek { at_ms: i64 },
    Volume { level: i64 },
    Mute,
    OpenPlayer,
    ClosePlayer,
}

#[view]
pub fn list(p: &ListPage) -> View<ListMsg> {
    tab_page(
        "Things",
        (
            card_section(for item in p.items {
                list_row(item.title).glyph("list").pushes().on_tap(ListMsg::Open { id: item.id })
            })
            .footer(p.footer),
            card_section(list_row("Add Thing").glyph("circle-plus").accent().on_tap(ListMsg::Add)),
            if let Some(draft) = p.adding {
                form_sheet(
                    "Add Thing",
                    ListMsg::Dismiss,
                    form_section((
                        name_field("Name", draft.name, draft.problem, |name| ListMsg::Name { name }),
                        search_picker(
                            "Kind",
                            draft.kind,
                            |key| ListMsg::Kind { key },
                            for k in draft.kinds {
                                search_choice(k.title, k.glyph, k.key).group(k.group).detail(k.count, k.tint)
                            },
                        )
                        .unset("Needs a Kind")
                        .prompt("Search Kinds"),
                    )),
                )
                .confirm("Add", draft.can_save, ListMsg::Save)
            },
        ),
    )
    .refresh(ListMsg::Refresh)
    .into()
}

/// A detail of primitives (ui/docs/primitives.md): the header card and the
/// goal's bar laid out of them, an empty state, and a row of words, a
/// glyph and a spacer, each value read from the record.
#[derive(Clone, Debug, PartialEq)]
pub struct CardPage {
    pub name: String,
    pub figure: String,
    pub tint: String,
    pub attention: String,
    pub fraction: f64,
    pub note: String,
    pub empty: bool,
    pub tags: Vec<Tag>,
    pub picture: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub name: String,
}

/// A header card as an app composes one (money's `header_card`): a card
/// whose first row is its face, centred and read as one, then its action.
fn header<M>(
    title: impl Into<Val<String>>,
    figure: impl Into<Val<String>>,
    tint: impl Into<Val<String>>,
    line: impl Into<Val<String>>,
    action: impl Into<View<M>>,
) -> View<M> {
    let tint = tint.into();
    let face = vstack((
        tile("target", tint.clone()),
        text(title).style(TextStyle::Headline).centred().if_any(),
        text(figure).style(TextStyle::Figure).ink(Ink::Figure(tint.clone())).centred(),
        text(line).style(TextStyle::Subheadline).ink(Ink::Tint(tint)).centred().if_any(),
    ))
    .spacing(Space::Xs)
    .padding(Edges::Horizontal, Space::L)
    .padding(Edges::Vertical, Space::Xl)
    .fill_width()
    .combine();
    card_section((face, action.into())).into()
}

/// A goal's bar and its note under it (money's `goal_progress`).
fn goal<M>(fraction: impl Into<Val<f64>>, tint: impl Into<Val<String>>, note: impl Into<Val<String>>) -> View<M> {
    vstack((meter(fraction, tint), text(note).style(TextStyle::Footnote).ink(Ink::Quiet)))
        .spacing(Space::Xs)
        .align(Align::Start)
        .padding(Edges::Horizontal, Space::L)
        .padding(Edges::Vertical, Space::M)
        .fill_width_at(Align::Start)
        .into()
}

#[view]
pub fn card(p: &CardPage) -> View<ListMsg> {
    detail_page((
        header(
            p.name,
            p.figure,
            p.tint,
            p.attention,
            list_row("Add Thing").glyph("circle-plus").accent().on_tap(ListMsg::Add),
        ),
        card_section(goal(p.fraction, p.tint, p.note)).header("Goal"),
        if p.empty {
            card_section(empty_state("list", "No Things"))
        },
        card_section(
            hstack((
                glyph("clock").style(TextStyle::Footnote).ink(Ink::Quiet),
                text(p.name).style(TextStyle::Subheadline).lines(1),
                spacer_min(Space::S),
                text(p.figure).style(TextStyle::Subheadline).ink(Ink::Figure(p.tint)),
            ))
            .spacing(Space::S)
            .padding(Edges::Horizontal, Space::L)
            .padding(Edges::Vertical, Space::M),
        ),
        scroll_across(for tag in p.tags {
            text(tag.name)
                .style(TextStyle::Caption)
                .padding(Edges::All, Space::S)
                .background(Surface::Raised)
                .corner(Corner::Small)
        }),
        image(p.picture, p.name).fill_width().corner(Corner::Card),
    ))
}

/// A record of covers and a sound (ui/docs/primitives.md, "What Harken Lays Out").
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub by: String,
    pub art: String,
    pub length: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaPage {
    pub title: String,
    pub by: String,
    pub art: String,
    pub tracks: Vec<Track>,
    pub play_glyph: String,
    pub play_label: String,
    pub at_ms: i64,
    pub since_ms: i64,
    pub playing: bool,
    pub length_ms: i64,
    pub level: i64,
    pub muted: bool,
    pub open: bool,
}

/// Pictures and sounds of the primitives alone (no component of an app's):
/// a filled picture laid into its square over the derived square, clipped
/// to a corner or round; rows lit as they are pressed with their menus; a
/// lazy grid of tiles each its column's width; and the native controls a
/// player has, which stay ArkUI's (the scrubber, the hairline, the volume).
#[view]
pub fn media(p: &MediaPage) -> View<ListMsg> {
    detail_page((
        hstack((
            zstack((swatch(p.title), image(p.art, p.title).fill().decode(Measure::ArtHeader)))
                .side(Measure::ArtHeader)
                .corner(Corner::Small),
            title_stack(p.title).over("Album").under(p.by).under_ink(Ink::Accent),
        ))
        .spacing(Space::L)
        .padding(Edges::Horizontal, Space::Page),
        card_section(for t in p.tracks {
            press_row(
                t.title,
                ListMsg::Open { id: t.id },
                hstack((
                    zstack((swatch(t.title), image(t.art, t.title).fill().decode(Measure::ArtRow)))
                        .side(Measure::ArtRow)
                        .corner(Corner::Small),
                    title_stack(t.title).under(t.by).plain().one_line(),
                    spacer_min(Space::Xs),
                    text(t.length).style(TextStyle::Caption).ink(Ink::Quiet),
                ))
                .spacing(Space::M)
                .padding(Edges::Horizontal, Space::L)
                .fill_width_at(Align::Start),
            )
            .menu("Play Next", "list-start", ListMsg::Open { id: t.id })
        }),
        grid(
            Measure::ArtShelf,
            for t in p.tracks {
                pressable(
                    t.by,
                    ListMsg::Open { id: t.id },
                    vstack((
                        zstack((swatch(t.by), image(t.art, t.by).fill())).square().corner(Corner::Circle),
                        text(t.by).style(TextStyle::Subheadline).lines(1),
                    ))
                    .spacing(Space::Xs)
                    .align(Align::Start)
                    .fill_width_at(Align::Start),
                )
            },
        )
        .padding(Edges::Horizontal, Space::Page),
        vstack(for t in p.tracks {
            text(t.title).width(Measure::ArtShelf)
        }),
        hstack((
            wide_button("Play", true, true, false, ListMsg::Play),
            wide_button("Shuffle", false, true, false, ListMsg::Shuffle),
            pressable(p.play_label, ListMsg::Toggle, glyph(p.play_glyph).style(TextStyle::LargeTitle)),
        ))
        .spacing(Space::M),
        scrubber(p.at_ms, p.since_ms, p.playing, p.length_ms, |at_ms| ListMsg::Seek { at_ms }),
        hairline(p.at_ms, p.since_ms, p.playing, p.length_ms),
        volume("Volume", p.level, p.muted, |level| ListMsg::Volume { level }, ListMsg::Mute),
    ))
}

/// Sections with the now-playing accessory over them: its bar and what it
/// grows into hosted, shown while something plays, its swipe skipping.
#[view]
pub fn tabs(p: &MediaPage) -> View<ListMsg> {
    adaptive_tabs(p.title, |key| ListMsg::Kind { key }, section_tab("Home", "house", "home", hosted("home")))
        .accessory(hosted("bar"), hosted("full"), ListMsg::OpenPlayer, ListMsg::ClosePlayer)
        .showing(p.playing)
        .expanded(p.open)
        .skips(ListMsg::Forward, ListMsg::Back)
        .into()
}

/// The smallest window's view of a sound (ui/docs/pebble.md): a glyph and
/// words in a row, words read and literal, a bar of how far, the next few
/// in a list behind an `if let`, and the watch's buttons beside it all.
#[derive(Clone, Debug, PartialEq)]
pub struct GlancePage {
    pub link: String,
    pub status: String,
    pub title: String,
    pub progress: f64,
    pub play_glyph: String,
    pub playing: bool,
    pub next: Option<Queue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Queue {
    pub heading: String,
    pub tracks: Vec<Track>,
}

#[view]
pub fn glance(p: &GlancePage) -> View<ListMsg> {
    action_bar(
        vstack((
            hstack((
                glyph(p.link).style(TextStyle::Caption).ink(Ink::Quiet),
                text(p.status).style(TextStyle::Caption).lines(1),
            ))
            .spacing(Space::S),
            text(p.title).style(TextStyle::Title).lines(1),
            if p.playing {
                meter(p.progress, "accent")
            } else {
                text("Paused").style(TextStyle::Footnote).ink(Ink::Accent)
            },
            hstack((text("Now").style(TextStyle::Caption), spacer(), glyph("music").style(TextStyle::Caption))),
            if let Some(q) = p.next {
                card_section(for t in q.tracks {
                    list_row(t.title).subtitle(t.by)
                })
                .header(q.heading)
            },
        ))
        .spacing(Space::Xs)
        .align(Align::Start)
        .padding(Edges::Horizontal, Space::Page),
    )
    .up("skip-back", "Back", ListMsg::Back)
    .select(p.play_glyph, "Play or Pause", ListMsg::Toggle)
    .down("skip-forward", "Skip", ListMsg::Forward)
    .into()
}

/// What the watch is written with: the test's own record and messages.
const WATCH: ark_ui_gen::pebble::Screen = ark_ui_gen::pebble::Screen {
    source: "ui/rust/gen/tests/snapshots.rs",
    root: "p",
    sources: "things_app::glance",
    uses: &["things_app::ListMsg"],
    accent: "blue",
    glyphs: &["play", "pause"],
};

fn menus() -> Vec<CommandMenu<ListMsg>> {
    vec![command_menu(
        MenuPlace::File,
        vec![command("New Thing", "n", ListMsg::Add), command("Refresh", "r", ListMsg::Refresh).shift()],
    )]
}

fn app() -> App {
    let names = Names {
        swift: "ListView",
        swift_holder: "ListScreen",
        kotlin: "ListScreen",
        kotlin_holder: "ListHolder",
        source: "ui/rust/gen/tests/snapshots.rs",
        msg: "ListMsg",
        screen: "list",
    };
    App {
        namespace: "things_app",
        screens: vec![
            Generated::view(list::record, names),
            Generated::view(
                card::record,
                Names {
                    swift: "CardView",
                    swift_holder: "CardScreen",
                    kotlin: "CardScreen",
                    kotlin_holder: "CardHolder",
                    ..names
                },
            ),
            Generated::view(
                media::record,
                Names {
                    swift: "MediaView",
                    swift_holder: "MediaScreen",
                    kotlin: "MediaScreen",
                    kotlin_holder: "MediaHolder",
                    ..names
                },
            ),
            Generated::view(
                tabs::record,
                Names {
                    swift: "TabsView",
                    swift_holder: "TabsScreen",
                    kotlin: "TabsScreen",
                    kotlin_holder: "TabsHolder",
                    ..names
                },
            ),
            Generated::view(
                glance::record,
                Names {
                    swift: "GlanceView",
                    swift_holder: "GlanceScreen",
                    kotlin: "GlanceScreen",
                    kotlin_holder: "GlanceHolder",
                    ..names
                },
            ),
            Generated::commands(
                menus,
                Names {
                    swift: "ThingsCommands",
                    kotlin: "thingsCommands",
                    ..names
                },
            ),
        ],
    }
}

#[test]
fn every_file_is_its_snapshot() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let update = std::env::var_os("ARK_UPDATE_SNAPSHOTS").is_some();
    let mut stale = vec![];
    let mut all = files(&app(), "dev.things.android");
    all.push(("glance_watch.rs.snap".into(), ark_ui_gen::pebble::emit(&glance::record(), &WATCH)));
    for (name, text) in all {
        let path = dir.join(&name);
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "not what the emitters write (ARK_UPDATE_SNAPSHOTS=1 to write them): {stale:?}"
    );
}

#[test]
fn the_files_are_named_for_each_language() {
    let names: Vec<String> = files(&app(), "p").into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        [
            "ListView.swift",
            "ListScreen.kt",
            "CardView.swift",
            "CardScreen.kt",
            "MediaView.swift",
            "MediaScreen.kt",
            "TabsView.swift",
            "TabsScreen.kt",
            "GlanceView.swift",
            "GlanceScreen.kt",
            "ThingsCommands.swift",
            "thingsCommands.kt"
        ]
    );
}

#[test]
fn kotlin_imports_the_message_from_the_apps_namespace() {
    let kt = files(&app(), "dev.things.android")
        .into_iter()
        .find(|(n, _)| n == "ListScreen.kt")
        .unwrap()
        .1;
    assert!(kt.contains("package dev.things.android"), "{kt}");
    assert!(kt.contains("import uniffi.things_app.ListMsg"), "{kt}");
}

#[test]
fn a_message_from_a_rows_field_reads_the_row() {
    let swift = files(&app(), "p").into_iter().find(|(n, _)| n == "ListView.swift").unwrap().1;
    assert!(swift.contains(".open(id: item.id)"), "{swift}");
    assert!(swift.contains("ForEach"), "{swift}");
}

#[test]
fn primitives_are_written_as_each_toolkits_own_layout() {
    let all = files(&app(), "p");
    let file = |n: &str| all.iter().find(|(f, _)| f == n).unwrap().1.clone();
    let (swift, kotlin) = (file("CardView.swift"), file("CardScreen.kt"));
    // The header card is a card of its face (a centred stack read as one)
    // and its action's row: no hand-written part of its own.
    assert!(!swift.contains("HeaderCard") && !kotlin.contains("HeaderCard"), "{swift}\n{kotlin}");
    assert!(swift.contains("VStack(alignment: .center, spacing: Space.xs) {"), "{swift}");
    assert!(swift.contains(".accessibilityElement(children: .combine)"), "{swift}");
    assert!(swift.contains(".listRowInsets(EdgeInsets())"), "{swift}");
    assert!(kotlin.contains("Column(modifier = Modifier.semantics(mergeDescendants = true) {}.fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).padding(vertical = Space.xl).padding(horizontal = Space.l), verticalArrangement = Arrangement.spacedBy(Space.xs), horizontalAlignment = Alignment.CenterHorizontally) {"), "{kotlin}");
    // Words read from the record, nothing while empty.
    assert!(swift.contains("if !p.attention.isEmpty {"), "{swift}");
    assert!(kotlin.contains("if (p.attention.isNotEmpty()) {"), "{kotlin}");
    assert!(swift.contains(".foregroundStyle(Tint(named: p.tint).figure)"), "{swift}");
    assert!(kotlin.contains("color = ArkInk.figure(p.tint)"), "{kotlin}");
    // What is left, at least a step.
    assert!(swift.contains("Spacer(minLength: Space.s)"), "{swift}");
    assert!(kotlin.contains("Spacer(Modifier.weight(1f).widthIn(min = Space.s))"), "{kotlin}");
    // Clipped after it is filled, on both.
    assert!(
        kotlin.contains("Modifier.clip(RoundedCornerShape(ArkCorner.small)).background(ArkSurface.raised).padding(Space.s)"),
        "{kotlin}"
    );
    assert!(swift.contains("ScrollView(.horizontal, showsIndicators: false) {"), "{swift}");
    assert!(kotlin.contains("Row(Modifier.horizontalScroll(rememberScrollState())) {"), "{kotlin}");
    assert!(kotlin.contains("import androidx.compose.foundation.layout.Column"), "{kotlin}");
    // No number in either: every measure is a token's.
    for f in [&swift, &kotlin] {
        assert!(!f.contains(".dp") && !f.contains("CGFloat("), "{f}");
    }
}

#[test]
fn the_watch_is_its_layers_laid_out_by_tokens() {
    let rs = ark_ui_gen::pebble::emit(&glance::record(), &WATCH);
    // The action bar first, its literal glyphs set once, its read one drawn.
    assert!(rs.contains("ui.action_bar(CONTAINER_HIGH, INK, ACTION_GLYPH);"), "{rs}");
    assert!(rs.contains("ui.set_action(Button::Up, \"skip-back\".as_bytes());"), "{rs}");
    assert!(rs.contains("(Button::Select, things_app::glance::play_glyph),"), "{rs}");
    assert!(rs.contains("(Button::Down, ListMsg::Forward),"), "{rs}");
    // Every glyph it can draw is painted from arkui's drawings.
    for g in ["\"music\"", "\"pause\"", "\"play\"", "\"skip-back\"", "\"skip-forward\""] {
        assert!(rs.contains(&format!("Glyph {{ name: {g}.as_bytes()")), "{g}: {rs}");
    }
    // An `if` is both branches in one frame, each shown while its source
    // holds; an `if let`'s item's paths are its source's.
    assert!(rs.contains("(things_app::glance::playing, &[3], &[4]),"), "{rs}");
    assert!(rs.contains("(things_app::glance::next, &[7], &[]),"), "{rs}");
    assert!(
        rs.contains("(7, List { header: things_app::glance::next_heading, count: things_app::glance::next_tracks, title: things_app::glance::next_tracks_title, line: things_app::glance::next_tracks_by }),"),
        "{rs}"
    );
    // A glyph after a row's spacer at its end; the accent is the screen's.
    assert!(rs.contains("ui.glyph(6, Frame { x: PAGE_MARGIN + fill2 + fill2,"), "{rs}");
    assert!(rs.contains("BLUE_ACCENT"), "{rs}");
    // No number but a share's divisor: every measure is a token's.
    for line in rs.lines().filter(|l| l.contains("Frame {")) {
        let frame = line.split("Frame {").nth(1).unwrap().split('}').next().unwrap();
        for word in frame
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| w.starts_with(|c: char| c.is_ascii_digit()))
        {
            assert!(word == "0" || frame.contains(&format!("/ {word}")), "{word} in {line}");
        }
    }
    // The phones get the same view as their own action bar.
    let all = files(&app(), "p");
    let file = |n: &str| all.iter().find(|(f, _)| f == n).unwrap().1.clone();
    let (swift, kotlin) = (file("GlanceView.swift"), file("GlanceScreen.kt"));
    assert!(
        swift.contains("ActionBar(items: [ActionItem(\"skip-back\", label: \"Back\", button: .up, onTap: { screen.send(.back) })"),
        "{swift}"
    );
    assert!(
        kotlin.contains("ActionItem(p.playGlyph, label = \"Play or Pause\", button = ActionButton.Select"),
        "{kotlin}"
    );
}
