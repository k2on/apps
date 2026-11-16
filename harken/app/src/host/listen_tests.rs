//! This device's player as the host drives it: alone, a play loads the
//! media player module (its fake) and the player draws it; part of the
//! session, a play is a live call the server takes, the player draws the
//! session from the device's live view, and the module plays what the
//! session says once this device is its output.

use std::sync::Arc;

use ark::format::Encode;
use ark_app::host::{Hosting, Place, Session};
use ark_app::Modules;
use ark_media_player::Fake;
use ark_peer::store::Store;
use ark_peer::{dev, Config, Deliver, Live, Memory, Module, Peer};
use harken_domain::{AddSong, Device, HarkenChange, HarkenView};

use super::*;
use crate::testing::{demo_peer, NOW};

/// A place over a peer: the demo's library alone, or a device of a hub's.
struct Here {
    peer: Peer,
    store: Store<HarkenView>,
    modules: Modules,
    /// The server, and this device's connection to it.
    hub: Option<(Peer, u64)>,
}

impl Here {
    /// Every frame between the device and its server carried, and the
    /// views settled.
    fn carry(&mut self) {
        if let Some((hub, c)) = &mut self.hub {
            let mut q: Vec<Deliver> = vec![];
            loop {
                self.peer.flush();
                for f in self.peer.take_frames() {
                    q.extend(hub.frame(*c, f, 0));
                }
                q.extend(hub.release());
                if q.is_empty() {
                    break;
                }
                for (to, b) in std::mem::take(&mut q) {
                    if to == *c {
                        self.peer.receive(b);
                    }
                }
            }
        }
        self.store.settle(&mut self.peer);
    }
}

impl Place<HarkenView, HarkenChange> for Here {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<HarkenView>)) {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, change: HarkenChange) -> Result<(), String> {
        let r = self.store.sent(self.peer.change(change.name(), change.input()));
        self.carry();
        r
    }
    fn hold(&mut self, key: HarkenView) {
        self.store.hold(&mut self.peer, key);
        self.store.settle(&mut self.peer);
    }
    fn release(&mut self, key: &HarkenView) {
        self.store.release(&mut self.peer, key);
    }
    fn modules(&self) -> Modules {
        self.modules.clone()
    }
    fn session(&self) -> Session {
        let st = self.peer.status();
        Session {
            demo: self.hub.is_none(),
            server: "http://music.local".into(),
            linked: Some(st.linked),
            device: if self.hub.is_some() {
                self.peer.me().session.clone()
            } else {
                String::new()
            },
            epoch: st.epoch as i64,
            ..Session::default()
        }
    }
}

fn alone(fake: &Fake) -> Here {
    let mut modules = Modules::new();
    fake.register(&mut modules);
    Here {
        peer: demo_peer(),
        store: Store::new(),
        modules,
        hub: None,
    }
}

fn domain() -> Arc<ark::domain::Domain> {
    harken_domain::shared()
}

/// A phone of alice's, connected to a server, the library a song she put
/// there (her login the library's).
fn joined(fake: &Fake) -> Here {
    let get = |n: &str| Some(format!("listening.{n}"));
    let hub_config = Config {
        live: Some(Live {
            on_join: get("on_join"),
            on_leave: get("on_leave"),
            on_restore: get("on_restore"),
            arrive: Arc::new(harken_domain::host::arrive),
            ..Live::new(Module::of(&domain()))
        }),
        ..Config::of(domain())
    };
    let mut hub = Peer::serve(hub_config, Box::new(Memory::default()), 0);
    let said = Device {
        can_play: true,
        id: String::new(),
        kind: "phone".into(),
        name: "Phone".into(),
        user: String::new(),
    };
    let config = Config {
        live: Some(Live {
            device: Some(said.to_bytes()),
            ephemeral: ["listening.scrub".to_string()].into(),
            ..Live::new(Module::of(&domain()))
        }),
        ..Config::of(domain())
    };
    let mut who = dev("alice@phone").unwrap();
    who.roles = vec![harken_domain::LIBRARY.into()];
    let mut peer = Peer::open(config, who, Box::new(Memory::default()), false);
    let c = hub.accept(peer.me().clone());
    peer.linked = true;
    peer.connected();
    let mut modules = Modules::new();
    fake.register(&mut modules);
    let mut here = Here {
        peer,
        store: Store::new(),
        modules,
        hub: Some((hub, c)),
    };
    here.carry();
    for (title, file) in [("Aria", "bach/aria.flac"), ("Variatio 1", "bach/v1.flac"), ("Variatio 2", "bach/v2.flac")] {
        let song = AddSong {
            title: title.into(),
            artist: "Bach".into(),
            album: "Goldberg Variations".into(),
            file: file.into(),
            duration_ms: 180_000,
            ..AddSong::default()
        };
        here.peer.mutate("library.add_song", &song).unwrap();
        here.carry();
    }
    here
}

fn hosted(mut place: Here) -> (Hosting<Harken>, Here) {
    let h = Hosting::open(&place.session(), &mut place);
    place.carry();
    (h, place)
}

/// Alone, a song played from Home loads this device's media player and
/// plays it, and the player draws it as its own; a toggle pauses it.
/// Falsified by the host keeping the play (as it did before the player
/// was its own): nothing was loaded.
#[test]
fn alone_a_play_is_this_devices_own_sound() {
    let fake = Fake::new();
    let (mut h, mut place) = hosted(alone(&fake));
    let first = h.app().home.page(&place, NOW).newest[0].clone();
    h.send::<Home>(HomeMsg::PlayNew { id: first.key.clone() }, &mut place);
    let asked = fake.take();
    assert!(asked.iter().any(|a| a.starts_with("load ")), "{asked:?}");
    assert!(asked.iter().any(|a| a == "play"), "{asked:?}");
    let p = h.app().player.page(&place, NOW);
    assert!(p.has && !p.joined, "{p:?}");
    assert_eq!(p.now.title, first.title);
    h.send::<Player>(PlayerMsg::Toggle, &mut place);
    assert!(fake.take().iter().any(|a| a == "pause"));
    assert!(!h.app().player.page(&place, NOW).now.playing);
}

/// Alone and joined, the queue is what plays and what follows: alone the
/// host tells the player its sound's queue, joined the session's; a row
/// jumped to plays, a row taken out is gone. Falsified by the host not
/// telling the player its own queue (alone it was empty) or a jump that
/// goes nowhere.
#[test]
fn the_queue_is_this_devices_alone_and_the_sessions_joined() {
    for joined_ in [false, true] {
        let fake = Fake::new();
        let (mut h, mut place) = hosted(if joined_ { joined(&fake) } else { alone(&fake) });
        let newest = h.app().home.page(&place, NOW).newest.clone();
        h.send::<Home>(HomeMsg::PlayNew { id: newest[0].key.clone() }, &mut place);
        h.moved(&mut place);
        let p = h.app().player.page(&place, NOW);
        assert_eq!(p.joined, joined_);
        assert_eq!(p.queue.len(), newest.len(), "joined {joined_}: {p:?}");
        assert!(p.queue[0].now && p.queue[0].title == newest[0].title);
        assert!(newest.len() >= 3, "joined {joined_}: three songs at least");
        h.send::<Player>(PlayerMsg::Jump { at: "2".into() }, &mut place);
        h.moved(&mut place);
        let p = h.app().player.page(&place, NOW);
        assert_eq!(h.app().refused, None);
        assert_eq!(p.now.title, newest[2].title, "joined {joined_}");
        assert!(p.queue[2].now);
        h.send::<Player>(PlayerMsg::Remove { at: "0".into() }, &mut place);
        h.moved(&mut place);
        let p = h.app().player.page(&place, NOW);
        assert_eq!(p.queue.len(), newest.len() - 1, "joined {joined_}");
        assert_eq!(p.now.title, newest[2].title);
    }
}

/// A player registered after the session opened (a phone's screen hands
/// the process its player once it shows) is the one a play then loads.
/// Falsified by the host taking the place's player only at its open: the
/// play found none, and nothing here played.
#[test]
fn a_player_registered_after_the_open_is_the_one_played() {
    let fake = Fake::new();
    let mut place = alone(&fake);
    place.modules = Modules::new();
    let (mut h, mut place) = hosted(place);
    fake.register(&mut place.modules);
    let first = h.app().home.page(&place, NOW).newest[0].clone();
    h.send::<Home>(HomeMsg::PlayNew { id: first.key.clone() }, &mut place);
    let asked = fake.take();
    assert!(asked.iter().any(|a| a.starts_with("load ")), "{asked:?}");
    assert!(h.app().player.page(&place, NOW).has);
}

/// Part of the session, a song played is the session's: a live call the
/// server takes, read back through the device's live view and drawn by
/// the player — the sound here, since this device pressed it — and the
/// media player loaded with it by the reconciler. Falsified by
/// `Peer::change` sending the live call as an intent on the log: the play
/// was refused ("nothing here can play it"), and no session began.
#[test]
fn joined_a_play_starts_the_session_and_this_device_plays_it() {
    let fake = Fake::new();
    let (mut h, mut place) = hosted(joined(&fake));
    let first = h.app().home.page(&place, NOW).newest[0].clone();
    h.send::<Home>(HomeMsg::PlayNew { id: first.key.clone() }, &mut place);
    assert_eq!(h.app().refused, None);
    h.moved(&mut place);
    let p = h.app().player.page(&place, NOW);
    assert!(p.has && p.joined, "{p:?}");
    assert_eq!((p.now.title.as_str(), p.output.as_str()), (first.title.as_str(), "This Device"));
    assert_eq!(p.devices.len(), 1);
    let asked = fake.take();
    assert!(asked.iter().any(|a| a.starts_with("load ")), "{asked:?}");
    // A pause is the session's too, and the sound follows it.
    h.send::<Player>(PlayerMsg::Toggle, &mut place);
    h.moved(&mut place);
    assert!(!h.app().player.page(&place, NOW).now.playing);
    assert!(fake.take().iter().any(|a| a == "pause"));
}

/// The shell's accessory follows the player: nothing plays, no bar and
/// nothing to open; a play puts the bar up and it grows into the full
/// player; the bar's swipe skips through the host. Alone, the volume is
/// this device's own sound's. Falsified by the host not telling the shell
/// (the bar never up), or the swipe going nowhere.
#[test]
fn the_accessory_and_the_volume_follow_the_player() {
    let fake = Fake::new();
    let (mut h, mut place) = hosted(alone(&fake));
    h.send::<HarkenShell>(ShellMsg::OpenPlayer, &mut place);
    let shell = h.app().shell.page(&place, NOW);
    assert!(!shell.playing && !shell.player_open, "{shell:?}");
    let first = h.app().home.page(&place, NOW).newest[0].key.clone();
    h.send::<Home>(HomeMsg::PlayNew { id: first }, &mut place);
    assert!(h.app().shell.page(&place, NOW).playing);
    h.send::<HarkenShell>(ShellMsg::OpenPlayer, &mut place);
    assert!(h.app().shell.page(&place, NOW).player_open);
    h.send::<HarkenShell>(ShellMsg::ClosePlayer, &mut place);
    assert!(!h.app().shell.page(&place, NOW).player_open);
    fake.take();
    h.send::<HarkenShell>(ShellMsg::Skip { by: 1 }, &mut place);
    assert_eq!(h.app().refused, None);
    let p = h.app().player.page(&place, NOW);
    assert!(p.has_volume);
    h.send::<Player>(PlayerMsg::Volume { level: 30 }, &mut place);
    h.send::<Player>(PlayerMsg::Mute, &mut place);
    let p = h.app().player.page(&place, NOW);
    assert_eq!((p.now.volume, p.now.muted), (30, true));
}

/// A car browses the library through the media player: the root is the
/// library's kinds; Albums is held and answered at the next step; an
/// album's tracks carry where they were found, and choosing one plays the
/// album from it. Falsified by answering a branch before its view is held
/// (no albums), or a choice played from the top.
#[test]
fn a_car_browses_the_library_and_plays_from_it() {
    use ark_app::modules::media_player::{MediaEvent, ROOT};
    let fake = Fake::new();
    let (mut h, mut place) = hosted(alone(&fake));
    assert_eq!(fake.browse(ROOT), None, "asked, not answered yet");
    h.moved(&mut place);
    let root = fake.browse(ROOT).expect("the root answered");
    let ids: Vec<&str> = root.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["playlists", "albums", "artists", "podcasts", "sermons"]);
    assert_eq!(fake.browse("albums"), None);
    h.moved(&mut place);
    h.moved(&mut place);
    let albums = fake.browse("albums").expect("albums answered once held");
    assert!(!albums.is_empty() && albums.iter().all(|a| a.browsable), "{albums:?}");
    let album = albums[0].id.clone();
    assert_eq!(fake.browse(&album), None);
    h.moved(&mut place);
    h.moved(&mut place);
    let tracks = fake.browse(&album).expect("an album's tracks");
    assert!(tracks.len() >= 2 && tracks.iter().all(|t| t.playable && !t.browsable), "{tracks:?}");
    fake.take();
    fake.hear(MediaEvent::PlayItem { id: tracks[1].id.clone() });
    h.moved(&mut place);
    let p = h.app().player.page(&place, NOW);
    assert_eq!(p.now.title, tracks[1].title, "{p:?}");
}
