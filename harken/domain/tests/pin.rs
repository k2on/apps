//! The port pin: every function of harken's domain, by its name, at the
//! hash the attribute macros built it at, and the module's hash
//! (`fixtures/functions.txt`, written from the macro-built domain before
//! the macros were deleted). The builder port is held to it byte for byte.
//! Falsified by editing the first hash in the fixture: the test failed.
use harken_domain::library::*;
use harken_domain::listening::*;
use harken_domain::playlists::*;
use std::collections::BTreeMap;

fn walk(b: &'static ark::author::Built, out: &mut BTreeMap<String, String>) {
    out.insert(b.name.to_string(), b.hash.to_string());
    for c in &b.calls { walk(c, out); }
}

#[test]
fn every_function_at_its_pinned_hash() {
    let mut out = BTreeMap::new();
    for b in [
        add_song::built(), describe_work::built(), describe_recording::built(), describe_person::built(), credit_recording::built(), remove_media::built(),
        library::built(), search::built(), albums::built(), artists::built(), track_details::built(), album::built(), artist::built(), composers::built(), works::built(), work::built(), recordings::built(),
        credits::built(), recording::built(), work_key::built(), movement_key::built(), recording_key::built(), key_part::built(), playlist_name::built(),
        create_playlist::built(), add_to_playlist::built(), add_all_to_playlist::built(), remove_from_playlist::built(), playlists::built(), playlists_of::built(),
        playlist::built(),
        start::built(), play::built(), pause::built(), seek::built(), skip::built(), jump::built(), transfer::built(), stop::built(), enqueue::built(), remove::built(), move_track::built(), clear_upcoming::built(),
        started::built(), advance::built(), stalled::built(), report_position::built(), release::built(), equip::built(), set_volume::built(), nudge_volume::built(), set_mute::built(),
        report_volume::built(), join::built(), unjoin::built(), report_group::built(), set_group_volume::built(), nudge_group_volume::built(), set_group_mute::built(),
        scrub::built(), on_join::built(), on_leave::built(), on_restore::built(), record_play::built(), last_played::built(), recently_played::built(),
    ] { walk(b, &mut out); }
    let d = harken_domain::domain();
    let mut s = String::new();
    for (n, h) in &out { s += &format!("{n} {h}\n"); }
    s += &format!("module {}\n", d.module.hash());
    let pinned = include_str!("fixtures/functions.txt");
    assert_eq!(s, pinned);
}
