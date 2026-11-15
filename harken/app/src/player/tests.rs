use ark_app::Runtime;

use super::*;
use crate::testing::Demo;

fn own() -> Playing {
    Playing {
        key: "00".into(),
        title: "Aria".into(),
        under: "Bach".into(),
        at_ms: 5_000,
        since_ms: 1,
        playing: true,
        length_ms: 180_000,
        can_forward: true,
        ..Playing::default()
    }
}

/// Alone, the player draws what this device plays on its own, as the host
/// says it; nothing, no bar. Its presses are the host's.
#[test]
fn alone_the_player_draws_its_own_sound_and_asks_the_host() {
    let mut rt = Runtime::<Player>::new();
    let mut d = Demo::<Player>::new();
    assert!(!d.page(&mut rt).has, "nothing plays: no bar");
    d.run(&mut rt, [PlayerMsg::Own { own: Some(own()) }]);
    let p = d.page(&mut rt);
    assert!(p.has && !p.joined);
    assert_eq!(
        (p.now.title.as_str(), p.play_glyph.as_str(), p.output.as_str()),
        ("Aria", "pause", "This Device")
    );
    d.run(&mut rt, [PlayerMsg::Toggle, PlayerMsg::Next, PlayerMsg::Seek { at_ms: 9_000 }]);
    assert_eq!(
        d.requests,
        [HarkenRequest::Toggle, HarkenRequest::Skip { by: 1 }, HarkenRequest::Seek { at_ms: 9_000 }]
    );
    d.run(&mut rt, [PlayerMsg::Open, PlayerMsg::Devices]);
    let p = d.page(&mut rt);
    assert!(p.open && p.devices_open);
    d.run(&mut rt, [PlayerMsg::Close]);
    let p = d.page(&mut rt);
    assert!(!p.open && !p.devices_open);
}

#[test]
fn the_view_draws_and_records() {
    let mut rt = Runtime::<Player>::new();
    let mut d = Demo::<Player>::new();
    d.run(&mut rt, [PlayerMsg::Own { own: Some(own()) }, PlayerMsg::Devices]);
    let _ = view::player::view(&d.page(&mut rt));
    let _ = view::player::record();
}
