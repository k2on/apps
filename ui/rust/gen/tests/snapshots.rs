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
    pub can_save: bool,
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
                    form_section(name_field("Name", draft.name, draft.problem, |name| ListMsg::Name { name })),
                )
                .confirm("Add", draft.can_save, ListMsg::Save)
            },
        ),
    )
    .refresh(ListMsg::Refresh)
    .into()
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
    assert_eq!(names, ["ListView.swift", "ListScreen.kt", "ThingsCommands.swift", "thingsCommands.kt"]);
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
