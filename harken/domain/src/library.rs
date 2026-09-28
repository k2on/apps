//! The library scope: what the scanner authors, and what every peer reads.
use ark::authoring::*;

use crate::schema::*;

pub struct AddTrack {
    pub title: Text,
    pub artist: Text,
    pub album: Opt<Text>,
    pub duration_ms: Int,
    pub file: Text,
}
impl Input for AddTrack {
    fn schema() -> Object<Self> {
        object()
            .field("title", text().trim().min(1, "a track needs a title"))
            .field("artist", text().trim())
            .field("album", opt(text().trim()))
            .field("duration_ms", int().at_least(0))
            .field("file", text().min(1))
    }
}

pub fn library() -> Router<Library> {
    let library = router::<Library>("library");
    library.routes((
        // Authored by the scanner for each file it finds. A second scan of
        // the same file is a no-op inside apply, whichever peer scanned it.
        library.input::<AddTrack>().mutation("add_track", |ctx, db, input| {
            db.track
                .insert(Track {
                    id: ctx.new_id("id"),
                    title: input.title,
                    artist: input.artist,
                    album: input.album,
                    duration_ms: input.duration_ms,
                    file: input.file,
                    added_ms: ctx.now("added_ms"),
                    user_id: ctx.user,
                })
                .on((Track::file,))
        }),
        library.query("library", |_ctx, db, _input: ()| {
            db.track
                .order_by((Track::artist.asc(), Track::album.asc(), Track::title.asc()))
                .all()
        }),
    ))
}
