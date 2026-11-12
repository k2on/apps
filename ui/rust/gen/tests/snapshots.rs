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

use ark_app::shell::{command, command_menu, CommandMenu, MenuPlace};
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

#[view]
pub fn card(p: &CardPage) -> View<ListMsg> {
    detail_page((
        header_card(p.name, p.figure, p.tint)
            .glyph("target")
            .line(p.attention)
            .actions(list_row("Add Thing").glyph("circle-plus").accent().on_tap(ListMsg::Add)),
        card_section(goal_progress(p.fraction, p.tint, p.note)).header("Goal"),
        if p.empty {
            card_section(empty_state("list", "No Things", "Add one with the button above."))
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
    for (name, text) in files(&app(), "dev.things.android") {
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
