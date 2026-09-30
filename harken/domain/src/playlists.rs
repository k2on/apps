//! The playlists router: what a person does on any device, offline or not.
//!
//! A playlist is a list somebody made; "Favorites" is one of these and
//! nothing special. It is ordered, so adding reads `MAX(pos) + 1`, which is
//! what makes the rebase visible: add while offline and it lands after
//! whatever arrived while you were away.
//!
//! Anybody may make one, including nobody: a peer used before anyone has
//! signed in authors as `Ctx::nobody()` (user `""`), its playlists are
//! nobody's and `owned` lets nobody at them, and signing in makes that
//! work the signer's (`Replica::sign_in`). A server hears no one who has
//! not signed in, so there is no guard: it could refuse nothing that ever
//! reaches the log, and would only refuse a person their own work offline.
use ark::authoring::*;

use crate::library::library_entry;
use crate::schema::*;

/// What `owned` reads of a procedure's input: the playlist it is about.
pub struct Owned {
    pub playlist_id: Id<Playlist>,
}
impl Input for Owned {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name").max(120))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("media_id", id::<Media>())
    }
}

pub struct AddAllToPlaylist {
    pub playlist_id: Id<Playlist>,
}
impl Input for AddAllToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists())
    }
}

pub struct RemoveFromPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}
impl Input for RemoveFromPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("media_id", id::<Media>())
    }
}

pub struct PlaylistsOf {
    pub media_id: Id<Media>,
}
impl Input for PlaylistsOf {
    fn schema() -> Object<Self> {
        object().field("media_id", id::<Media>())
    }
}

pub struct PlaylistInput {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistInput {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists())
    }
}

/// A name with a number after it: "Favorites (1)".
pub fn numbered(name: Text, n: Int) -> Text {
    helper("numbered", (("name", name), ("n", n)), |name: Text, n: Int| {
        concat(list([name, " (".into(), n.to_text(), ")".into()]))
    })
}

/// The smallest n from 1 whose "name (n)" is none of these names. The
/// names are one person's, so distinct: each one's rank among them is one
/// of 1..len, and at most len of the numbers are taken, so the answer is
/// len + 1 or among the ranks — whichever names the list holds besides
/// the numbered ones, which is what lets `create_playlist` hand it only
/// the name and its numbered siblings (`docs/plan-perf.md` R6).
///
/// Quadratic in the list, and run only for a name they already have
/// (`playlist_name`, R1): per name its rank is counted twice (natively
/// `pick` computes both arms) and the numbered name looked for. Over every
/// playlist of a person's that was 340 ms a call natively at a thousand;
/// over the siblings it is the square of how many times they have reused
/// this one name, not of how many playlists they have.
pub fn free_number(names: List<Text>, name: Text) -> Int {
    helper("free_number", (("names", names), ("name", name)), |names: List<Text>, name: Text| {
        names.fold(names.len().add(1), |acc: Int, x| {
            pick(
                names.contains(numbered(name, names.filter(|x_2| x_2.le(x)).len())),
                acc,
                acc.min(names.filter(|x_2| x_2.le(x)).len()),
            )
        })
    })
}

/// What a new playlist is called, among a person's other playlists: the
/// name asked for, or when they already have one of that name, the first
/// "name (n)" they do not have.
///
/// The branch is an option's match rather than `pick`, because `pick`
/// computes both arms natively and `free_number` is the expensive one
/// (`docs/plan-perf.md` R1): `filter` keeps the name only when it is
/// taken, and `map_or` numbers it only then — each an `EMatch` whose other
/// arm the interpreter never evaluates, over a closure the native build
/// calls only when it is taken. `if_else` is the vocabulary's other lazy
/// branch, but a statement: it chooses between effects and has no value
/// to give, and a helper is an expression. A fresh name costs one
/// `contains`.
pub fn playlist_name(names: List<Text>, name: Text) -> Text {
    helper("playlist_name", (("names", names), ("name", name)), |names: List<Text>, name: Text| {
        some(name)
            .filter(|n| names.contains(n))
            .map_or(name, |n| numbered(n, free_number(names, n)))
    })
}

pub fn playlists() -> Router<Harken> {
    let playlists = router::<Harken>("playlists");
    let owned = playlists.provide("owned", |ctx, db, input: &Owned| {
        db.playlist
            .get((input.playlist_id,))
            .filter(|row| row.user_id.eq(ctx.user))
            .or_refuse("not your playlist")
    });
    playlists.routes((
        // Make a playlist, after every other. Every client makes a default
        // playlist before it has seen the log — a second device, or one used
        // before anybody signed in — so a name that person already has is
        // not refused and not dropped: the new playlist keeps its id, and
        // what is on it, as "Favorites (1)". Decided here, from the rows at
        // apply time, so every peer replaying reaches the same name in log
        // order; by person, not by library; case is kept. The same entry
        // twice is one playlist: its id is the key. The last playlist is
        // read through `playlist (pos)` (R1). Of the person's, only the
        // names `playlist_name` can find taken are read: the name itself
        // and its numbered siblings, which are every name from `name` up
        // to `name )` — "name (" sorts right after "name" and ")" right
        // after "(" — a range of `(user_id, name)` under the person
        // (`docs/plan-perf.md` R6). Somebody with a thousand playlists
        // and a hundred "Favorites (n)" examines a hundred and one.
        playlists.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            let playlist = db.playlist.order_by(Playlist::pos.desc()).first();
            let playlist_2 = db
                .playlist
                .filter(
                    Playlist::user_id
                        .eq(ctx.user)
                        .and(Playlist::name.ge(input.name))
                        .and(Playlist::name.lt(concat(list([input.name, " )".into()])))),
                )
                .all();
            db.playlist.insert(Playlist {
                id: ctx.new_id("id"),
                name: playlist_name(playlist_2.map(|row| row.name), input.name),
                pos: playlist.map_or(0, |row| row.pos).add(1),
                created_ms: ctx.now("created_ms"),
                user_id: ctx.user,
            })
        }),
        // Put something on a playlist, at the end of it. A playlist holds an
        // item once, so adding one already there keeps its place; something
        // no longer in the library is a no-op. `MAX(pos)` is the first row
        // of `playlist_item (playlist_id, pos)` walked backwards (R1): one
        // row examined onto a playlist of 7,999, where it was 7,999.
        owned.input::<AddToPlaylist>().mutation("add_to_playlist", |ctx, db, input, playlist| {
            let media = db.media.exists((input.media_id,));
            when(media, || {
                let playlist_item = db
                    .playlist_item
                    .filter(PlaylistItem::playlist_id.eq(playlist.id))
                    .order_by(PlaylistItem::pos.desc())
                    .first();
                db.playlist_item.insert(PlaylistItem {
                    playlist_id: playlist.id,
                    media_id: input.media_id,
                    pos: playlist_item.map_or(0, |row| row.pos).add(1),
                    added_ms: ctx.now("added_ms"),
                    user_id: ctx.user,
                })
            })
        }),
        // Put everything in the library on a playlist, in library order. One
        // entry, an intent: a replica replaying it covers whatever the library
        // held by then, including what another peer added meanwhile.
        owned
            .input::<AddAllToPlaylist>()
            .mutation("add_all_to_playlist", |ctx, db, _input, playlist| {
                let media = db.media.order_by(Media::pos.asc()).all();
                for_each(media, |row| {
                    let playlist_item = db
                        .playlist_item
                        .filter(PlaylistItem::playlist_id.eq(playlist.id))
                        .order_by(PlaylistItem::pos.desc())
                        .first();
                    db.playlist_item.insert(PlaylistItem {
                        playlist_id: playlist.id,
                        media_id: row.id,
                        pos: playlist_item.map_or(0, |row_2| row_2.pos).add(1),
                        added_ms: ctx.now("added_ms"),
                        user_id: ctx.user,
                    })
                })
            }),
        // Take something off a playlist. The item stays in the library.
        owned
            .input::<RemoveFromPlaylist>()
            .mutation("remove_from_playlist", |_ctx, db, input, playlist| {
                db.playlist_item.delete((playlist.id, input.media_id))
            }),
        // The caller's playlists, in the order they were made.
        playlists.query("playlists", |ctx, db, _input: ()| {
            db.playlist.filter(Playlist::user_id.eq(ctx.user)).order_by(Playlist::pos.asc())
        }),
        // Which of the caller's playlists a track is on: what makes a
        // playlist sheet a toggle rather than a one-way door. Each playlist
        // with its entry for the track beneath it, kept when there is one.
        playlists.input::<PlaylistsOf>().query("playlists_of", |ctx, db, input| {
            db.playlist
                .filter(Playlist::user_id.eq(ctx.user))
                .order_by(Playlist::pos.asc())
                .each(|playlist, ()| {
                    db.playlist_item
                        .filter(PlaylistItem::media_id.eq(input.media_id))
                        .on(PlaylistItem::playlist_id.eq(playlist.id))
                })
                .having(|_playlist, (items,)| items.is_empty().not())
                .map(|playlist, _| playlist)
        }),
        // One playlist's contents, in playlist order, as the library's own
        // rows; an entry whose media has gone is dropped.
        owned.input::<PlaylistInput>().query("playlist", |_ctx, db, _input, playlist| {
            db.playlist_item
                .filter(PlaylistItem::playlist_id.eq(playlist.id))
                .order_by(PlaylistItem::pos.asc())
                .get(|item, ()| db.media.by((item.media_id,)))
                .having(|_item, (media,)| media.is_some())
                .map(|item, (media,)| library_entry(media.unwrap(), some(item.pos)))
        }),
    ))
}
