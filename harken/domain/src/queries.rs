//! The reads. A screen joins items to tracks itself, because the two are in
//! different scopes and a query reads one store.
use ark_builder::*;

pub fn queries(m: &mut ModuleBuilder) {
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
}
