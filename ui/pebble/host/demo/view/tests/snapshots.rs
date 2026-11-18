//! The demo's screen as each toolkit is handed it, held to the files beside
//! this one (`tests/snapshots/`), as ark-ui-gen's own snapshots are: the
//! watch's layers (what `watch/build.rs` writes) and, from the same
//! recording, the phones' SwiftUI and Compose — one view, not a watch's
//! dialect. And the view drawn from a record, as iced and the browser are
//! handed it.
//!
//! After a deliberate change to the view or an emitter:
//! `ARK_UPDATE_SNAPSHOTS=1 cargo test -p now-playing-view`.

use ark_app::view::{ActionButton, Part, View};
use ark_ui_gen::{kotlin, swift, Screen};
use now_playing::face::{Act, Face, Next};
use now_playing_view::screen;

fn phones() -> Vec<(&'static str, String)> {
    let names = Screen {
        name: "NowPlayingView",
        holder: "NowPlayingScreen",
        source: "ui/pebble/host/demo/view/src/lib.rs",
        package: "dev.harken.android",
        root: "p",
        imports: &[],
        screen: "now-playing",
    };
    let v = screen::record();
    vec![
        ("NowPlayingView.swift", swift::emit(&v, &names)),
        (
            "NowPlayingScreen.kt",
            kotlin::emit(
                &v,
                &Screen {
                    name: "NowPlayingScreen",
                    holder: "NowPlayingHolder",
                    ..names
                },
            ),
        ),
    ]
}

#[test]
fn every_file_is_its_snapshot() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let update = std::env::var_os("ARK_UPDATE_SNAPSHOTS").is_some();
    let mut files = phones();
    files.push(("now_playing_watch.rs.snap", now_playing_view::watch()));
    let mut stale = vec![];
    for (name, text) in files {
        let path = dir.join(name);
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
fn the_watch_names_only_tokens() {
    let rs = now_playing_view::watch();
    let build = rs.split("pub fn build").nth(1).unwrap().split("pub fn draw").next().unwrap();
    // A frame is the room and the theme's tokens: no number of pixels,
    // only a share's divisor and the room's own origin.
    for line in build.lines().filter(|l| l.contains("Frame {")) {
        let frame = line.split("Frame {").nth(1).unwrap().split('}').next().unwrap();
        for word in frame
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| w.starts_with(|c: char| c.is_ascii_digit()))
        {
            let shared = frame.contains(&format!("/ {word}"));
            assert!(word == "0" || shared, "{word} in {line}");
        }
    }
    assert!(build.contains("ui.action_bar(CONTAINER_HIGH, INK, ACTION_GLYPH);"), "{build}");
    assert!(rs.contains("Button::Select => Some(Act::Toggle)"), "{rs}");
    assert!(rs.contains("Button::Down => Some(Act::Skip)"), "{rs}");
    assert!(rs.contains("ui.set_rows("), "{rs}");
}

#[test]
fn the_same_view_is_drawn_from_a_record() {
    let face = Face {
        status: "Online".into(),
        link: "wifi".into(),
        title: "Second Summer".into(),
        artist: "The Quiet Fleet".into(),
        progress: 0.25,
        at: "0:15".into(),
        length: "0:52".into(),
        queue: "Up Next".into(),
        next: vec![Next {
            title: "Under Lamplight".into(),
            line: "Juniper Lane  0:36".into(),
        }],
        play_glyph: "pause".into(),
        play_label: "Pause".into(),
    };
    let View::Part(Part::ActionBar { items, content }) = screen::view(&face) else {
        panic!("the screen is an action bar beside its content")
    };
    let select = items.iter().find(|i| i.button == ActionButton::Select).unwrap();
    assert_eq!(
        (select.glyph.get(), select.label.get(), select.tap),
        ("pause".to_string(), "Pause".to_string(), Act::Toggle)
    );
    // Its queue is a section of the record's rows.
    fn section(v: &View<Act>) -> Option<&ark_app::view::Section<Act>> {
        match v {
            View::Part(Part::Section(s)) => Some(s),
            View::List(vs) => vs.iter().find_map(section),
            View::Layout(l) => l.slots().into_iter().find_map(section),
            _ => None,
        }
    }
    let queue = section(&content).expect("the queue is a section");
    let View::List(rows) = &*queue.rows else {
        panic!("its rows are the record's")
    };
    assert_eq!(rows.len(), 1);
    // And a screen walk finds its buttons by their labels, as on the phones.
    let walk = ark_app::shots::nodes(&screen::view(&face), std::rc::Rc::new(|_: Act| {}));
    let labels: Vec<String> = walk
        .iter()
        .filter_map(|n| match n {
            ark_app::shots::Node::Target(t) => Some(t.label.clone()),
            _ => None,
        })
        .collect();
    assert!(
        labels.contains(&"Pause".to_string()) && labels.contains(&"Skip".to_string()),
        "{labels:?}"
    );
}
