//! The playlists router, built (`crate::playlists`'s functions, one for one).

use ark::author::{Id, Int, Text};
use ark::build::*;

use super::library::LIBRARY_ENTRY;
use crate::playlists::*;
use crate::schema::*;

fn db() -> HarkenDb {
    Harken::db()
}

pub static OWNED: Def1<Id<Playlist>, Playlist> = Def1 {
    name: "playlists.owned",
    kind: Kind::Helper,
    body: |playlist_id| {
        db().playlist.get(&playlist_id).map_or(refuse("not your playlist"), |p| {
            if_(p.f().user_id.eq(Ctx.user()), p.clone(), refuse("not your playlist"))
        })
    },
};

pub static NUMBERED: Def2<Text, Int, Text> = Def2 {
    name: "playlists.numbered",
    kind: Kind::Helper,
    body: |name, n| concat(vec![name, txt(" ("), n.to_text(), txt(")")]),
};

pub static FREE_NUMBER: Def2<Vec<Text>, Text, Int> = Def2 {
    name: "playlists.free_number",
    kind: Kind::Helper,
    body: |names, name| {
        block(|b| {
            let ranks = b.let_(names.map(|x| names.filter(|y| y.le(&x)).len()));
            ranks
                .filter(|n| !names.contains(NUMBERED.call(&name, &n)))
                .sort(|n| n)
                .first()
                .unwrap_or(names.len() + 1)
        })
    },
};

pub static PLAYLIST_NAME: Def2<Vec<Text>, Text, Text> = Def2 {
    name: "playlists.playlist_name",
    kind: Kind::Helper,
    body: |names, name| if_(names.contains(&name), NUMBERED.call(&name, FREE_NUMBER.call(&names, &name)), name),
};

/// One past the last `pos` among `rows`' first.
fn next(last: &Ex<Option<PlaylistItem>>) -> Ex<Int> {
    last.map_or(0, |i| i.f().pos) + 1
}

pub static CREATE_PLAYLIST: Def1<CreatePlaylist, ()> = Def1 {
    name: "playlists.create_playlist",
    kind: Kind::Mutator,
    body: |input| {
        let db = db();
        let name = input.f().name;
        block(|b| {
            let id = b.let_(Ctx.new_id::<Playlist>());
            if_(
                db.playlist.get(&id).is_none(),
                block(|b| {
                    let last = b.let_(db.playlist.sort(|p| desc(p.f().pos)).first());
                    let siblings = b.let_(db.playlist.filter(|p| {
                        let p = p.f();
                        p.user_id
                            .eq(Ctx.user())
                            .and(p.name.ge(&name))
                            .and(p.name.lt(concat(vec![name.clone(), txt(" )")])))
                    }));
                    db.playlist
                        .insert(PlaylistEx {
                            id: id.clone(),
                            name: PLAYLIST_NAME.call(siblings.map(|p| p.f().name), &name),
                            pos: last.map_or(0, |p| p.f().pos) + 1,
                            created_ms: Ctx.now(),
                            user_id: Ctx.user(),
                        })
                        .s()
                }),
                unit(),
            )
        })
    },
};

/// `playlist_item.insert`, `media_id` at the end of the playlist `playlist`.
fn append(playlist: &Ex<Playlist>, media_id: Ex<Id<Media>>) -> Ex<()> {
    let db = db();
    block(|b| {
        let last = b.let_(
            db.playlist_item
                .filter(|i| i.f().playlist_id.eq(playlist.f().id))
                .sort(|i| desc(i.f().pos))
                .first(),
        );
        db.playlist_item.insert(PlaylistItemEx {
            playlist_id: playlist.f().id,
            media_id,
            pos: next(&last),
            added_ms: Ctx.now(),
            user_id: Ctx.user(),
        })
    })
}

pub static ADD_TO_PLAYLIST: Def1<AddToPlaylist, ()> = Def1 {
    name: "playlists.add_to_playlist",
    kind: Kind::Mutator,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            let playlist = b.let_(OWNED.call(&i.playlist_id));
            let fresh = db
                .media
                .get(&i.media_id)
                .is_some()
                .and(db.playlist_item.get((playlist.f().id, i.media_id.clone())).is_none());
            if_(fresh, append(&playlist, i.media_id.clone()).s(), unit())
        })
    },
};

pub static ADD_ALL_TO_PLAYLIST: Def1<AddAllToPlaylist, ()> = Def1 {
    name: "playlists.add_all_to_playlist",
    kind: Kind::Mutator,
    body: |input| {
        let db = db();
        block(|b| {
            let playlist = b.let_(OWNED.call(input.f().playlist_id));
            db.media
                .sort(|m| m.f().pos)
                .map(|m| {
                    if_(
                        db.playlist_item.get((playlist.f().id, m.f().id)).is_none(),
                        append(&playlist, m.f().id),
                        unit(),
                    )
                })
                .s()
        })
    },
};

pub static REMOVE_FROM_PLAYLIST: Def1<RemoveFromPlaylist, ()> = Def1 {
    name: "playlists.remove_from_playlist",
    kind: Kind::Mutator,
    body: |input| {
        let i = input.f();
        block(|b| {
            let playlist = b.let_(OWNED.call(&i.playlist_id));
            db().playlist_item.delete((playlist.f().id, i.media_id.clone())).s()
        })
    },
};

pub static PLAYLISTS: Def0<Vec<Playlist>> = Def0 {
    name: "playlists.playlists",
    kind: Kind::Query,
    body: || db().playlist.filter(|p| p.f().user_id.eq(Ctx.user())).sort(|p| p.f().pos),
};

pub static PLAYLISTS_OF: Def1<PlaylistsOf, Vec<Playlist>> = Def1 {
    name: "playlists.playlists_of",
    kind: Kind::Query,
    body: |input| {
        let db = db();
        db.playlist
            .filter(|p| {
                p.f()
                    .user_id
                    .eq(Ctx.user())
                    .and(db.playlist_item.get((p.f().id, input.f().media_id)).is_some())
            })
            .sort(|p| p.f().pos)
    },
};

pub static PLAYLIST: Def1<PlaylistInput, Vec<crate::library::LibraryEntry>> = Def1 {
    name: "playlists.playlist",
    kind: Kind::Query,
    body: |input| {
        let db = db();
        block(|b| {
            let p = b.let_(OWNED.call(input.f().playlist_id));
            db.playlist_item
                .filter(|i| i.f().playlist_id.eq(p.f().id).and(db.media.get(i.f().media_id).is_some()))
                .sort(|i| i.f().pos)
                .map(|i| LIBRARY_ENTRY.call(db.media.get(i.f().media_id).unwrap(), some(i.f().pos)))
        })
    },
};
