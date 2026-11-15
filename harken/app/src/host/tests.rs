//! The host over the demo library, as every platform runs it: its
//! runtimes side by side, a route pushed over the section it came from.

use ark_app::host::{Hosting, Place, Session};
use ark_peer::store::Store;
use ark_peer::Peer;
use harken_domain::{HarkenChange, HarkenView};

use super::*;
use crate::testing::{demo_peer, NOW};

/// The demo library as a host's place.
pub(crate) struct Demo {
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

pub(crate) fn hosted() -> (Hosting<Harken>, Demo) {
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

/// A song's Add to Playlist raises the sheets' playlists for it, and the
/// shows' Follow raises the follow sheet of the kind shown. Falsified by
/// the host keeping the ask (as it did before the sheets were a runtime).
#[test]
fn a_screens_sheet_is_raised_by_the_host() {
    let (mut h, mut place) = hosted();
    h.send::<Library>(LibraryMsg::Show { kind: "songs".into() }, &mut place);
    let song = h.app().library.page(&place, NOW).songs[0].id.clone();
    h.send::<Library>(LibraryMsg::AddTo { id: song }, &mut place);
    let p = h.app().sheets.page(&place, NOW);
    assert!(p.adding && !p.playlists.is_empty(), "{p:?}");
    h.send::<crate::sheets::Sheets>(SheetsMsg::Dismiss, &mut place);
    h.send::<Library>(LibraryMsg::Show { kind: "sermons".into() }, &mut place);
    h.send::<Library>(LibraryMsg::Follow, &mut place);
    let f = h.app().sheets.page(&place, NOW).following.expect("the follow sheet");
    assert_eq!(f.title, "Follow a Series");
}

/// Settings says the session as the host reads it: the demo's library on
/// this device alone, no device of a session; its panes chosen by title.
/// Falsified by the host not telling the shell (the facts empty).
#[test]
fn settings_say_the_session() {
    let (mut h, mut place) = hosted();
    let p = h.app().shell.page(&place, NOW);
    assert_eq!(
        (p.pane.as_str(), p.facts.link.as_str(), p.facts.device.as_str()),
        ("General", "On This Device", "None")
    );
    assert!(!p.facts.on_server);
    h.send::<HarkenShell>(ShellMsg::Pane { title: "Diagnostics".into() }, &mut place);
    let p = h.app().shell.page(&place, NOW);
    assert_eq!(p.pane, "Diagnostics");
    let _ = crate::shell::view::harken_settings::view(&p);
    let _ = crate::shell::view::harken_settings::record();
}
