//! The library scope's one mutator. Only the server's scanner authors it;
//! a client is generated without it and receives its entries as facts.
use ark_builder::*;

pub fn library(m: &mut ModuleBuilder) {
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
}
