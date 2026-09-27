//! harken's domain, as a program that emits it.
//!
//! Nothing here runs on a phone or a server. Run, it writes `harken.ark`:
//! the schema, four mutators and three queries as Ark IR in canonical CBOR,
//! which `arkc verify` checks and `arkc gen` turns into Rust, Swift and
//! Kotlin. The builder's types keep the IR well-formed as it is written —
//! an `Expr<Int>` cannot be compared to an `Expr<Text>`, a read is always
//! bound by a `let` — and the verifier is what holds it to the schema.
//!
//! Read beside docs/arkdb.md §3.4 and harken/README.md.

use ark_builder::*;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "harken.ark".to_string());
    let module = harken();
    module.write(&out).expect("write harken.ark");
    println!("{out}  {} functions", module.functions().len());
}

/// The whole domain.
pub fn harken() -> ModuleBuilder {
    let mut m = ModuleBuilder::new();

    // ---- schema -------------------------------------------------------
    // Two scopes: what the scanner authors, and what people make. A
    // playlist item names a track across the boundary, unchecked.
    m.scope("library", |s| {
        s.table("track", |t| {
            t.id("id");
            t.text("title");
            t.text("artist");
            t.text_opt("album");
            t.int("duration_ms");
            t.text("file");
            t.int("added_ms");
            t.text("user_id");
            t.key(&["id"]);
        });
    });
    m.scope("playlists", |s| {
        s.table("playlist", |t| {
            t.id("id");
            t.text("name");
            t.text("user_id");
            t.int("created_ms");
            t.key(&["id"]);
        });
        s.table("playlist_item", |t| {
            t.id_ref("playlist_id", "playlist");
            t.id_of("track_id", "track");
            t.int("pos");
            t.int("added_ms");
            t.text("user_id");
            t.key(&["playlist_id", "track_id"]);
        });
    });

    // ---- mutators -----------------------------------------------------

    // Put a track in the library. Refuses a blank title; a no-op if the id
    // is known or a non-empty file already is — inside apply, so a rescan
    // is idempotent wherever it happens.
    m.mutator("add_track", "library", |f| {
        let id = f.new_id("id", "track");
        let added_ms = f.now("added_ms");
        let title = f.arg("title", Ty::Text);
        let artist = f.arg("artist", Ty::Text);
        let album = f.arg("album", Ty::option(Ty::Text));
        let duration_ms = f.arg("duration_ms", Ty::Int);
        let file = f.arg("file", Ty::Text);
        let b = f.body();
        b.if_(title.trim().is_empty(), |b| b.refuse("a track needs a title"));
        let known = b.exists("track", [id.clone()]);
        b.if_(known, |b| b.ret());
        let file = b.let_("file", file.trim());
        let same = b.select(
            Plan::from("track")
                .filter(Pred::cmp("file", CmpOp::Eq, file.clone()))
                .limit(1),
        );
        b.if_(file.is_empty().not().and(same.len().gt(0)), |b| b.ret());
        b.put(
            "track",
            record([
                ("id", id),
                ("title", title.trim()),
                ("artist", artist.trim()),
                ("album", album),
                ("duration_ms", duration_ms),
                ("file", file),
                ("added_ms", added_ms),
                ("user_id", ctx_user()),
            ]),
        );
    });

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

    // ---- queries ------------------------------------------------------

    m.query("library", Ty::list(m.row_ty("track")), |f| {
        let b = f.body();
        let rows = b.select(
            Plan::from("track")
                .order_by("artist", Dir::Asc)
                .order_by("album", Dir::Asc)
                .order_by("title", Dir::Asc),
        );
        b.ret_value(rows);
    });

    m.query("playlists", Ty::list(m.row_ty("playlist")), |f| {
        let b = f.body();
        let rows = b.select(Plan::from("playlist").order_by("name", Dir::Asc));
        b.ret_value(rows);
    });

    m.query("playlist_items", Ty::list(m.row_ty("playlist_item")), |f| {
        let playlist_id = f.arg("playlist_id", Ty::id("playlist"));
        let b = f.body();
        let rows = b.select(
            Plan::from("playlist_item")
                .filter(Pred::cmp("playlist_id", CmpOp::Eq, playlist_id))
                .order_by("pos", Dir::Asc),
        );
        b.ret_value(rows);
    });

    m
}
