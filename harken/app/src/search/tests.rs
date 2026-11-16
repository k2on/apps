use ark_app::Runtime;

use super::*;
use crate::testing::Demo;

#[test]
fn nothing_typed_invites_and_a_word_answers_from_songs_albums_and_people() {
    let mut rt = Runtime::<Search>::new();
    let mut d = Demo::<Search>::new();
    let p = d.page(&mut rt);
    assert!(p.idle && !p.nothing);
    d.run(&mut rt, [SearchMsg::Typed { text: "bach".into() }]);
    let p = d.page(&mut rt);
    assert!(!p.idle && !p.nothing);
    assert!(p.has_songs || p.has_albums || p.has_artists);
    d.run(&mut rt, [SearchMsg::Typed { text: "zzqqxx".into() }]);
    let p = d.page(&mut rt);
    assert!(p.nothing && p.nothing_text.contains("zzqqxx"));
}
