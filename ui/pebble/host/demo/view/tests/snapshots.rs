//! The demo's screen as each toolkit is handed it, held to the files beside
//! this one (`tests/snapshots/`), as ark-ui-gen's own snapshots are: the
//! watch's layers (what `watch/build.rs` writes) and, from the same
//! recording, the phones' SwiftUI and Compose — one view, not a watch's
//! dialect. And the view drawn from a node's reader, as iced and the
//! browser will be handed it.
//!
//! After a deliberate change to the view or an emitter:
//! `ARK_UPDATE_SNAPSHOTS=1 cargo test -p now-playing-view`.

use ark_app::view::{ActionButton, Part, View};
use ark_kernel::{Anchor, Memory, Node, Origin, Space, Spaces, Status, NOW, STATUS};
use ark_ui_gen::{kotlin, swift, Screen};
use now_playing::{Act, Msg, Playing, Track, HANDLERS, VERSION};
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
    let build = rs.split("pub fn build").nth(1).unwrap().split("pub static BINDINGS").next().unwrap();
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
    assert!(rs.contains("(Button::Select, Act::Toggle),"), "{rs}");
    assert!(rs.contains("(Button::Down, Act::Skip),"), "{rs}");
    // Each layer bound to a source, as data: no code per layer.
    assert!(rs.contains("(2, words_3),"), "{rs}");
    assert!(
        rs.contains("(7, List { header: words_8, count: count_9, title: words_10, line: words_11 }),"),
        "{rs}"
    );
    assert!(!rs.contains("pub fn draw"), "{rs}");
    // Each binding is the view's lets, in order; what more than one layer
    // reads is shared within a settle, and a row's words are written
    // straight from its template's holes.
    assert!(
        rs.contains("let p = &::ark_kernel::source::shared(db, ") && rs.contains("::ark_kernel::source::row(db, "),
        "{rs}"
    );
    assert!(rs.contains("let title = &show :: title(p);"), "{rs}");
    assert!(rs.contains("out.s(\"  \".as_bytes());"), "{rs}");
}

fn track(title: &str, artist: &str, ms: u32) -> Track {
    Track {
        title: title.into(),
        artist: artist.into(),
        ms,
    }
}

/// A node holding the queue and what plays, and the host's two cells.
fn node() -> Node<Msg, Memory> {
    let spaces = Spaces {
        device: Memory::new(),
        session: Memory::new(),
        shared: Memory::new(),
    };
    let mut n = Node::new(Origin(1), 0, VERSION, &HANDLERS, spaces);
    let online = Status {
        online: true,
        complete: true,
        ..Status::default()
    };
    assert_eq!(n.cell(online), STATUS);
    assert_eq!(n.cell(15_000u64), NOW);
    let mut up = Vec::new();
    let e = n.make(
        Space::Shared,
        Msg::Add {
            at: 1,
            track: track("Under Lamplight", "Juniper Lane", 36_000),
        },
        0,
    );
    n.route(e, &mut up);
    let playing = Playing {
        track: track("Second Summer", "The Quiet Fleet", 52_000),
        anchor: Anchor {
            at: 0,
            wall: 0,
            rate: 1000,
            playing: true,
        },
    };
    let e = n.make(Space::Session, Msg::Play { player: Origin(1), playing }, 0);
    n.route(e, &mut up);
    n.settle(&mut ());
    n
}

#[test]
fn the_same_view_is_drawn_from_the_node() {
    let mut n = node();
    let r = n.reader();
    let view = n.read(r, |db| screen::view(db));
    let View::Part(Part::ActionBar { items, content }) = view.clone() else {
        panic!("the screen is an action bar beside its content")
    };
    let select = items.iter().find(|i| i.button == ActionButton::Select).unwrap();
    assert_eq!(
        (select.glyph.get(), select.label.get(), select.tap),
        ("pause".to_string(), "Pause".to_string(), Act::Toggle)
    );
    // Its queue is a section of the queue's rows, its words the node's.
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
        panic!("its rows are the queue's")
    };
    assert_eq!(rows.len(), 1);
    let words = format!("{view:?}");
    for w in [
        "Second Summer",
        "The Quiet Fleet",
        "0:15",
        "0:52",
        "Online",
        "Up Next",
        "Under Lamplight",
        "Juniper Lane  0:36",
    ] {
        assert!(words.contains(w), "{w}: {words}");
    }
    // And a screen walk finds its buttons by their labels, as on the phones.
    let walk = ark_app::shots::nodes(&view, std::rc::Rc::new(|_: Act| {}));
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
