//! The playlists scope: what a person does on any device, offline or not.
use ark_builder::*;

pub fn playlists(m: &mut ModuleBuilder) {
    // Make a playlist. Trims; refuses an empty name; a no-op if this person
    // already has one by that name — decided in apply, so a second device's
    // default playlist is not a duplicate.
    m.mutator("create_playlist", "playlists", |f| {
        let id = f.new_id("id", "playlist");
        let created_ms = f.now("created_ms");
        let name = f.arg("name", Ty::Text);
        let b = f.body();
        let name = b.let_("name", name.trim());
        b.if_(name.is_empty(), |b| b.refuse("a playlist needs a name"));
        let known = b.exists("playlist", [id.clone()]);
        b.if_(known, |b| b.ret());
        let mine = b.select(
            Plan::from("playlist")
                .filter(Pred::cmp("user_id", CmpOp::Eq, ctx_user()))
                .filter(Pred::cmp("name", CmpOp::Eq, name.clone()))
                .limit(1),
        );
        b.if_(mine.len().gt(0), |b| b.ret());
        b.put(
            "playlist",
            record([
                ("id", id),
                ("name", name),
                ("user_id", ctx_user()),
                ("created_ms", created_ms),
            ]),
        );
    });

    // Put a track on a playlist, after everything already on it — which is
    // what makes the rebase visible.
    m.mutator("add_to_playlist", "playlists", |f| {
        let added_ms = f.now("added_ms");
        let playlist_id = f.arg("playlist_id", Ty::id("playlist"));
        let track_id = f.arg("track_id", Ty::id("track"));
        let b = f.body();
        let has_playlist = b.exists("playlist", [playlist_id.clone()]);
        b.if_(has_playlist.not(), |b| b.ret());
        let already = b.exists("playlist_item", [playlist_id.clone(), track_id.clone()]);
        b.if_(already, |b| b.ret());
        let last = b.select(
            Plan::from("playlist_item")
                .filter(Pred::cmp("playlist_id", CmpOp::Eq, playlist_id.clone()))
                .order_by("pos", Dir::Desc)
                .limit(1),
        );
        let pos = b.let_(
            "pos",
            last.first()
                .map_some(|row| row.field("pos"))
                .unwrap_or(0),
        );
        b.put(
            "playlist_item",
            record([
                ("playlist_id", playlist_id),
                ("track_id", track_id),
                ("pos", pos.add(1)),
                ("added_ms", added_ms),
                ("user_id", ctx_user()),
            ]),
        );
    });

    // Take a track off a playlist. The track stays in the library.
    m.mutator("remove_from_playlist", "playlists", |f| {
        let playlist_id = f.arg("playlist_id", Ty::id("playlist"));
        let track_id = f.arg("track_id", Ty::id("track"));
        let b = f.body();
        b.delete("playlist_item", [playlist_id, track_id]);
    });
}
