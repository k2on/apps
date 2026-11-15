//! The host over the demo library, as every platform runs it: its
//! runtimes side by side, a route pushed over the section it came from.

use ark_app::host::{Hosting, Place, Session};
use ark_peer::store::Store;
use ark_peer::Peer;
use harken_domain::{HarkenChange, HarkenView};

use super::*;
use crate::testing::{demo_peer, NOW};

/// The demo library as a host's place.
struct Demo {
    peer: Peer,
    store: Store<HarkenView>,
}

impl Place<HarkenView, HarkenChange> for Demo {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<HarkenView>)) {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, change: HarkenChange) -> Result<(), String> {
        let r = self.store.sent(self.peer.mutate_bytes(change.name(), change.input()));
        self.store.settle(&mut self.peer);
        r.map(|_| ())
    }
    fn hold(&mut self, key: HarkenView) {
        self.store.hold(&mut self.peer, key);
        self.store.settle(&mut self.peer);
    }
    fn release(&mut self, key: &HarkenView) {
        self.store.release(&mut self.peer, key);
    }
    fn session(&self) -> Session {
        Session {
            demo: true,
            server: "http://music.local".into(),
            ..Session::default()
        }
    }
}

fn hosted() -> (Hosting<Harken>, Demo) {
    let mut place = Demo {
        peer: demo_peer(),
        store: Store::new(),
    };
    let h = Hosting::open(&place.session(), &mut place);
    (h, place)
}

#[test]
fn a_tile_tapped_in_the_library_pushes_its_page_over_the_library() {
    let (mut h, mut place) = hosted();
    h.send::<HarkenShell>(ShellMsg::Select { tag: "library".into() }, &mut place);
    h.send::<Library>(LibraryMsg::Show { kind: "albums".into() }, &mut place);
    let a = h.app().library.page(&place, NOW).albums[0].clone();
    assert!(a.art.is_empty() || a.art.starts_with("http"), "{}", a.art);
    h.send::<Library>(
        LibraryMsg::OpenAlbum {
            key: a.key.clone(),
            title: a.title.clone(),
        },
        &mut place,
    );
    let shell = h.app().shell.page(&place, NOW);
    assert_eq!(
        (shell.section.as_str(), shell.showing, shell.page_title.as_str()),
        ("library", true, a.title.as_str())
    );
    let page = h.app().page.page(&place, NOW);
    assert_eq!(page.title, a.title);
    assert!(!page.tracks.is_empty());
    h.send::<HarkenShell>(ShellMsg::Back, &mut place);
    assert!(!h.app().shell.page(&place, NOW).showing);
}

#[test]
fn an_index_route_is_the_library_its_way() {
    let (mut h, mut place) = hosted();
    h.send::<Home>(HomeMsg::SeeAll { kind: "albums".into() }, &mut place);
    assert_eq!(h.app().shell.page(&place, NOW).section, "library");
    assert!(h.app().library.page(&place, NOW).on_albums);
}

#[test]
fn the_shells_view_records_both_sections() {
    let _ = crate::shell::view::harken_shell::record();
    let (h, place) = hosted();
    let _ = crate::shell::view::harken_shell::view(&h.app().shell.page(&place, NOW));
}
