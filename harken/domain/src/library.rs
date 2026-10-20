//! The library router: what the scanner authors, and what every peer reads
//! (`harken/domain/src/library.rs`, function for function).
//!
//! Every playable thing (`media`), what only a song has (`song`), and the
//! rows a track belongs to — `album`, `person`, `work`, `movement`,
//! `recording`, `credit` — keyed by something derived from what they *are*
//! rather than by a minted id, because two peers naming "BWV 988" offline
//! must land on one row. The keys are the helpers below, permanent.
//!
//! Every mutation here is the library's to make: `is_library` runs first
//! and refuses an author without the `library` role. The queries are
//! everybody's.
//!
//! Where v4 said `insert` it meant "unless a row has this key" and `upsert`
//! "this row, whatever was there": v5's `Insert` refuses an existing key,
//! so each is spelled with the lookup it is. v4's list `fold`s are
//! `Std::Sum`, a sort, or a `split_once`; [`slug`] is the one that cannot
//! be: it does not collapse a run of separators, since nothing in v5's
//! language sees two neighbouring characters (a key with one is not v4's).

use ark::author::*;

use crate::schema::*;

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct AddSong {
    #[ark(trim, non_empty, why = "a song needs a title")]
    pub title: Text,
    #[ark(trim)]
    pub artist: Text,
    #[ark(trim)]
    pub album: Text,
    pub duration_ms: Int,
    #[ark(trim)]
    pub file: Text,
    pub track: Int,
    #[ark(trim)]
    pub part: Text,
    #[ark(trim)]
    pub catalogue: Text,
    #[ark(trim)]
    pub performer: Text,
    pub bpm: Int,
    #[ark(trim)]
    pub album_art: Text,
    #[ark(trim)]
    pub artist_art: Text,
    pub disc: Int,
    #[ark(trim)]
    pub work_title: Text,
    pub movement_no: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct DescribeWork {
    pub id: Text,
    #[ark(trim)]
    pub opus: Text,
    #[ark(trim)]
    pub key_sig: Text,
    #[ark(trim)]
    pub form: Text,
    #[ark(trim)]
    pub period: Text,
    pub composed: Int,
    #[ark(trim)]
    pub art: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct DescribeRecording {
    pub id: Text,
    pub recorded: Int,
    #[ark(trim)]
    pub venue: Text,
    #[ark(trim)]
    pub label: Text,
    #[ark(trim)]
    pub licence: Text,
    #[ark(trim)]
    pub art: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct DescribePerson {
    #[ark(trim, non_empty, why = "a person needs a name")]
    pub name: Text,
    #[ark(trim)]
    pub sort_name: Text,
    pub born: Int,
    pub died: Int,
    #[ark(trim)]
    pub art: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct CreditRecording {
    pub recording_id: Text,
    #[ark(trim, non_empty, why = "a credit needs somebody to credit")]
    pub person_name: Text,
    #[ark(trim)]
    pub role: Text,
    #[ark(trim)]
    pub instrument: Text,
    pub pos: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct RemoveMedia {
    pub id: Id<Media>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Library {
    pub playlist_id: Id<Playlist>,
}

/// `search`: the playlist each row's position is read against, and what to
/// look for.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Search {
    pub playlist_id: Id<Playlist>,
    pub needle: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct AlbumInput {
    pub playlist_id: Id<Playlist>,
    pub name: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Artist {
    pub playlist_id: Id<Playlist>,
    pub name: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Works {
    pub composer: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct WorkInput {
    pub id: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Recordings {
    pub work_id: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Credits {
    pub recording_id: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct RecordingInput {
    pub playlist_id: Id<Playlist>,
    pub id: Text,
}

/// Something playable as a list renders it, and where it sits on the
/// playlist the list was read against, if it is on it.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct LibraryEntry {
    pub added_ms: Int,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub kind: Text,
    pub playlist_pos: Option<Int>,
    pub pos: Int,
    pub title: Text,
    pub user_id: Text,
}

/// A record, as the albums page lists it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct AlbumsEntry {
    pub art: Text,
    pub creator: Text,
    pub name: Text,
    pub tracks: Int,
}

/// Whoever made something, as the artists page lists them.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct ArtistsEntry {
    pub art: Text,
    pub name: Text,
    pub tracks: Int,
}

/// What is true of a track as a song.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct TrackDetailsEntry {
    pub album: Text,
    pub bpm: Int,
    pub catalogue: Text,
    pub licence: Text,
    pub media_id: Id<Media>,
    pub part: Text,
    pub performer: Text,
    pub track: Int,
}

/// Somebody some work is by, as the composers index lists them.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct ComposersEntry {
    pub art: Text,
    pub born: Int,
    pub died: Int,
    pub name: Text,
    pub sort_name: Text,
    pub tracks: Int,
    pub works: Int,
}

/// A composition, as a composer's page lists them and a work's page heads.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct WorkSummary {
    pub art: Text,
    pub catalogue: Text,
    pub composer: Text,
    pub form: Text,
    pub id: Text,
    pub period: Text,
    pub recordings: Int,
    pub title: Text,
    pub tracks: Int,
}

/// One performance, as a work's page lists them.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct RecordingsEntry {
    pub art: Text,
    pub id: Text,
    pub label: Text,
    pub licence: Text,
    pub performers: Text,
    pub recorded: Int,
    pub tracks: Int,
}

/// One person on a recording, and what they did on it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct CreditsEntry {
    pub instrument: Text,
    pub name: Text,
    pub pos: Int,
    pub role: Text,
}

// * The keys

/// The tables, as a body reads them.
fn db() -> Harken {
    Harken::db()
}

// * The keys

/// The work an entry names: an entry without `work_title` names one anyway
/// when it carries a catalogue number or a part, and then it is the album's.
pub static WORK_TITLE: Def4<Text, Text, Text, Text, Text> = Def4 {
    name: "library.work_title",
    kind: Kind::Helper,
    body: |work_title, album, catalogue, part| if_(work_title.is_empty().and((!catalogue.is_empty()).or(!part.is_empty())), album, work_title),
};

/// One name, reduced to something that can stand in a key: letters and
/// digits lowercased, anything else a dash, none leading or trailing.
/// Non-ASCII letters are kept. (v4 made a run of separators one dash.)
pub static SLUG: Def1<Text, Text> = Def1 {
    name: "library.slug",
    kind: Kind::Helper,
    body: |text| {
        block(|b| {
            let spaced = b.let_(concat_of(&text.chars().map(|c| if_(c.is_alnum(), c.lower(), txt(" ")))).trim());
            concat_of(&spaced.chars().map(|c| if_(c.eq(" "), txt("-"), c)))
        })
    },
};

/// …and for a name with no letters at all, `x` and its hash: "!!!" is a
/// band, and every such name in one key would be one band.
pub static KEY_PART: Def1<Text, Text> = Def1 {
    name: "library.key_part",
    kind: Kind::Helper,
    body: |text| {
        block(|b| {
            let s = b.let_(SLUG.call(&text));
            if_(s.is_empty(), concat(vec![txt("x"), text.trim().fnv1a64().to_text()]), s)
        })
    },
};

/// The key of a work: its composer, and its catalogue number or its title.
pub static WORK_KEY: Def3<Text, Text, Text, Text> = Def3 {
    name: "library.work_key",
    kind: Kind::Helper,
    body: |composer, catalogue, title| {
        concat(vec![
            KEY_PART.call(composer),
            txt("/"),
            KEY_PART.call(if_(catalogue.is_empty(), title, catalogue)),
        ])
    },
};

/// The key of the work a track is of, or none: a work needs a title and a
/// composer.
pub static WORK_ID: Def3<Text, Text, Text, Option<Text>> = Def3 {
    name: "library.work_id",
    kind: Kind::Helper,
    body: |artist, catalogue, work_title| {
        if_(
            work_title.is_empty().or(artist.is_empty()),
            none(),
            some(WORK_KEY.call(artist, catalogue, work_title)),
        )
    },
};

/// The key of a movement: its work, and where it sits in it.
pub static MOVEMENT_KEY: Def2<Text, Int, Text> = Def2 {
    name: "library.movement_key",
    kind: Kind::Helper,
    body: |work_id, no| concat(vec![work_id, txt("#"), no.to_text()]),
};

/// The key of a recording: what it is of, and by whom.
pub static RECORDING_KEY: Def2<Text, Text, Text> = Def2 {
    name: "library.recording_key",
    kind: Kind::Helper,
    body: |of, who| concat(vec![of, txt("@"), KEY_PART.call(who)]),
};

/// The key of the recording a track is part of; where there is no work,
/// the release and the title make a pop track its own performance.
pub static RECORDING_ID: Def5<Option<Text>, Text, Text, Text, Text, Text> = Def5 {
    name: "library.recording_id",
    kind: Kind::Helper,
    body: |work_id, album, title, artist, performer| {
        RECORDING_KEY.call(
            work_id.unwrap_or(concat(vec![KEY_PART.call(album), txt("/"), KEY_PART.call(title)])),
            if_(performer.is_empty(), artist, performer),
        )
    },
};

/// Who the one credit a track carries names: the performer, the artist of
/// a track of no work, and nobody for a classical track naming no performer.
pub static CREDITED_AS: Def3<Option<Text>, Text, Text, Text> = Def3 {
    name: "library.credited_as",
    kind: Kind::Helper,
    body: |work_id, artist, performer| if_(performer.is_empty(), if_(work_id.is_some(), txt(""), artist), performer),
};

/// The key of the movement a track is, or none when it is of no work.
pub static MOVEMENT_ID: Def3<Option<Text>, Int, Int, Option<Text>> = Def3 {
    name: "library.movement_id",
    kind: Kind::Helper,
    body: |work_id, movement_no, track| work_id.map(|w| MOVEMENT_KEY.call(w, if_(movement_no.eq(0), track, movement_no))),
};

// * Reading

/// A media row as a list renders it.
pub static LIBRARY_ENTRY: Def2<Media, Option<Int>, LibraryEntry> = Def2 {
    name: "library.library_entry",
    kind: Kind::Helper,
    body: |media, playlist_pos| {
        let m = media.f();
        LibraryEntryEx {
            added_ms: m.added_ms,
            creator: m.creator,
            duration_ms: m.duration_ms,
            file: m.file,
            id: m.id,
            kind: m.kind,
            playlist_pos,
            pos: m.pos,
            title: m.title,
            user_id: m.user_id,
        }
        .into()
    },
};

/// The credits worth drawing: the real ones (`pos` from 1) where there are
/// any, which displace the lumped string a track carried at `pos` 0.
pub static CREDITED: Def1<Vec<Credit>, Vec<Credit>> = Def1 {
    name: "library.credited",
    kind: Kind::Helper,
    body: |credits| credits.filter(|c| c.f().pos.gt(0).or(credits.filter(|d| d.f().pos.gt(0)).is_empty())),
};

/// Names, as one line: "Hermann Scherchen, London Symphony Orchestra".
pub static JOINED: Def1<Vec<Text>, Text> = Def1 {
    name: "library.joined",
    kind: Kind::Helper,
    body: |names| {
        concat_of(&names.map(|n| concat(vec![txt(", "), n])))
            .split_once(", ")
            .map_or("", |p| p.f().1)
    },
};

/// Who played one recording: its credits in billing order, the composer
/// left out.
pub static PERFORMERS: Def2<Vec<Credit>, Text, Text> = Def2 {
    name: "library.performers",
    kind: Kind::Helper,
    body: |credits, recording_id| {
        let mine = credits.filter(|c| c.f().recording_id.eq(&recording_id).and(c.f().role.ne("composer")));
        JOINED.call(CREDITED.call(mine).sort(|c| (c.f().pos, c.f().person_name)).map(|c| c.f().person_name))
    },
};

/// A work, with how much of it the library holds.
pub static WORK_SUMMARY: Def3<Work, Int, Int, WorkSummary> = Def3 {
    name: "library.work_summary",
    kind: Kind::Helper,
    body: |work, recordings, tracks| {
        let w = work.f();
        WorkSummaryEx {
            art: w.art,
            catalogue: w.catalogue,
            composer: w.composer,
            form: w.form,
            id: w.id,
            period: w.period,
            recordings,
            title: w.title,
            tracks,
        }
        .into()
    },
};

/// The sum of some counts.
pub static TOTAL: Def1<Vec<Int>, Int> = Def1 {
    name: "library.total",
    kind: Kind::Helper,
    body: |counts| counts.sum(),
};

/// How many songs are movements of a work.
pub static WORK_TRACKS: Def1<Text, Int> = Def1 {
    name: "library.work_tracks",
    kind: Kind::Helper,
    body: |work_id| {
        let db = db();
        TOTAL.call(
            db.movement
                .filter(|m| m.f().work_id.eq(&work_id))
                .map(|m| db.song.filter(|s| s.f().movement_id.eq(some(m.f().id))).len()),
        )
    },
};

/// Works, each with how many recordings of it there are and how many
/// tracks are movements of it.
pub static SUMMARIES: Def1<Vec<Work>, Vec<WorkSummary>> = Def1 {
    name: "library.summaries",
    kind: Kind::Helper,
    body: |works| {
        let db = db();
        works.map(|w| {
            WORK_SUMMARY.call(
                &w,
                db.recording.filter(|r| r.f().work_id.eq(some(w.f().id))).len(),
                WORK_TRACKS.call(w.f().id),
            )
        })
    },
};

/// The library's role, or a refusal: what every library mutation runs first.
pub static IS_LIBRARY: Def0<()> = Def0 {
    name: "library.is_library",
    kind: Kind::Helper,
    body: || if_(!Ctx.has_role("library"), refuse_("only the library writes the library"), unit()),
};

/// A playlist's entry for a media row, if it is on it.
pub static PLAYLIST_POS: Def2<Id<Playlist>, Id<Media>, Option<Int>> = Def2 {
    name: "library.playlist_pos",
    kind: Kind::Helper,
    body: |playlist_id, media_id| db().playlist_item.get((playlist_id, media_id)).map(|i| i.f().pos),
};

/// The movement a song is, if it is one.
pub static MOVEMENT_OF: Def1<Song, Option<Movement>> = Def1 {
    name: "library.movement_of",
    kind: Kind::Helper,
    body: |song| song.f().movement_id.map_or(none(), |m| db().movement.get(m)),
};

/// Whether a text holds the needle, both folded by the pinned `lower`.
pub static HAS: Def2<Text, Text, bool> = Def2 {
    name: "library.has",
    kind: Kind::Helper,
    body: |text, needle| needle.is_empty().or(text.lower().split_once(needle.lower()).is_some()),
};

// * The mutations

/// `if exists { update } else { insert }` of a row by its key.
fn put<R: ark::author::Row>(t: Table<R>, exists: Ex<bool>, key: impl Key + Clone, row: &Ex<R>) -> Ex<()> {
    if_(exists, t.update(key, |_| row.clone()).s(), t.insert(row).s())
}

/// Put a song in the library, and everything it belongs to as rows. An
/// upsert by `file`: a file the library has is the song it is, said again,
/// so the media row keeps its id, place, when and by whom it was added;
/// the same file with nothing new writes no row. What the song was part of
/// and no longer is — a recording, a movement, a work — goes when nothing
/// else is part of it.
pub static ADD_SONG: Def1<AddSong, ()> = Def1 {
    name: "library.add_song",
    kind: Kind::Mutator,
    body: |input| {
        let db = db();
        let i = input.f();
        block(|b| {
            b.s(IS_LIBRARY.call());
            let was = b.let_(db.media.filter(|m| m.f().file.eq(&i.file)).first().filter(|m| !m.f().file.is_empty()));
            let id = b.let_(was.map_or(Ctx.new_id::<Media>(), |m| m.f().id));
            let newest = b.let_(db.media.sort(|m| desc(m.f().pos)).first());
            let media = b.let_(MediaEx {
                id: id.clone(),
                kind: was.map_or("song", |m| m.f().kind),
                title: i.title.clone(),
                creator: i.artist.clone(),
                duration_ms: i.duration_ms.clone(),
                file: i.file.clone(),
                pos: was.map_or(newest.map_or(0, |m| m.f().pos) + 1, |m| m.f().pos),
                added_ms: was.map_or(Ctx.now(), |m| m.f().added_ms),
                user_id: was.map_or(Ctx.user(), |m| m.f().user_id),
            });
            b.s(put(db.media, db.media.get(&id).is_some(), &id, &media));
            b.s(if_(
                !i.album.is_empty(),
                block(|b| {
                    let album = b.let_(db.album.get(&i.album));
                    let fresh = album.map_or(true, |a| (!i.album_art.is_empty()).and(a.f().art.ne(&i.album_art)));
                    let row = |b: &mut Block| {
                        b.let_(AlbumEx {
                            name: i.album.clone(),
                            label: album.map_or("", |a| a.f().label),
                            released: album.map_or(0, |a| a.f().released),
                            art: i.album_art.clone(),
                            added_ms: Ctx.now(),
                            user_id: Ctx.user(),
                        })
                    };
                    if_(
                        fresh,
                        block(|b| {
                            let row = row(b);
                            put(db.album, album.is_some(), &i.album, &row)
                        }),
                        unit(),
                    )
                }),
                unit(),
            ));
            b.s(if_(
                !i.artist.is_empty(),
                block(|b| {
                    let person = b.let_(db.person.get(&i.artist));
                    let fresh = person.map_or(true, |p| (!i.artist_art.is_empty()).and(p.f().art.ne(&i.artist_art)));
                    if_(
                        fresh,
                        block(|b| {
                            let row = b.let_(PersonEx {
                                name: i.artist.clone(),
                                sort_name: person.map_or("", |p| p.f().sort_name),
                                born: person.map_or(0, |p| p.f().born),
                                died: person.map_or(0, |p| p.f().died),
                                art: i.artist_art.clone(),
                                added_ms: person.map_or(Ctx.now(), |p| p.f().added_ms),
                                user_id: person.map_or(Ctx.user(), |p| p.f().user_id),
                            });
                            put(db.person, person.is_some(), &i.artist, &row)
                        }),
                        unit(),
                    )
                }),
                unit(),
            ));
            let work_named = b.let_(WORK_TITLE.call(&i.work_title, &i.album, &i.catalogue, &i.part));
            let work_of = b.let_(WORK_ID.call(&i.artist, &i.catalogue, &work_named));
            b.s(if_(
                work_of.is_some(),
                block(|b| {
                    let key = b.let_(WORK_KEY.call(&i.artist, &i.catalogue, &work_named));
                    let no = b.let_(if_(i.movement_no.eq(0), i.track.clone(), i.movement_no.clone()));
                    let movement_ref = b.let_(MOVEMENT_KEY.call(&key, &no));
                    let work = b.let_(db.work.get(&key));
                    let row = b.let_(WorkEx {
                        id: key.clone(),
                        composer: i.artist.clone(),
                        title: work_named.clone(),
                        catalogue: i.catalogue.clone(),
                        opus: work.map_or("", |w| w.f().opus),
                        key_sig: work.map_or("", |w| w.f().key_sig),
                        form: work.map_or("", |w| w.f().form),
                        period: work.map_or("", |w| w.f().period),
                        composed: work.map_or(0, |w| w.f().composed),
                        art: work.map_or("", |w| w.f().art),
                        added_ms: work.map_or(Ctx.now(), |w| w.f().added_ms),
                        user_id: work.map_or(Ctx.user(), |w| w.f().user_id),
                    });
                    b.s(put(db.work, work.is_some(), &key, &row));
                    let movement = b.let_(db.movement.get(&movement_ref));
                    let row = b.let_(MovementEx {
                        id: movement_ref.clone(),
                        work_id: key.clone(),
                        no: no.clone(),
                        title: i.title.clone(),
                        part: i.part.clone(),
                        added_ms: movement.map_or(Ctx.now(), |m| m.f().added_ms),
                        user_id: movement.map_or(Ctx.user(), |m| m.f().user_id),
                    });
                    put(db.movement, movement.is_some(), &movement_ref, &row)
                }),
                unit(),
            ));
            let recording = b.let_(RECORDING_ID.call(&work_of, &i.album, &i.title, &i.artist, &i.performer));
            let credited = b.let_(CREDITED_AS.call(&work_of, &i.artist, &i.performer));
            b.s(if_(
                db.recording.get(&recording).is_none(),
                db.recording
                    .insert(RecordingEx {
                        id: recording.clone(),
                        work_id: work_of.clone(),
                        recorded: int(0),
                        venue: txt(""),
                        label: txt(""),
                        licence: txt(""),
                        art: txt(""),
                        added_ms: Ctx.now(),
                        user_id: Ctx.user(),
                    })
                    .s(),
                unit(),
            ));
            b.s(if_(
                !credited.is_empty(),
                block(|b| {
                    b.s(if_(
                        db.person.get(&credited).is_none(),
                        db.person
                            .insert(PersonEx {
                                name: credited.clone(),
                                sort_name: txt(""),
                                born: int(0),
                                died: int(0),
                                art: txt(""),
                                added_ms: Ctx.now(),
                                user_id: Ctx.user(),
                            })
                            .s(),
                        unit(),
                    ));
                    let role = b.let_(if_(work_of.is_some(), txt("performer"), txt("artist")));
                    if_(
                        db.credit.get((recording.clone(), credited.clone(), role.clone())).is_none(),
                        db.credit
                            .insert(CreditEx {
                                recording_id: recording.clone(),
                                person_name: credited.clone(),
                                role: role.clone(),
                                instrument: txt(""),
                                pos: int(0),
                                added_ms: Ctx.now(),
                                user_id: Ctx.user(),
                            })
                            .s(),
                        unit(),
                    )
                }),
                unit(),
            ));
            let movement = b.let_(MOVEMENT_ID.call(&work_of, &i.movement_no, &i.track));
            let song = b.let_(db.song.get(&id));
            let row = b.let_(SongEx {
                media_id: id.clone(),
                album_name: if_(i.album.is_empty(), none(), some(&i.album)),
                disc: if_(i.disc.gt(0), i.disc.min(99), int(1)),
                track: i.track.clamp(0, 999),
                recording_id: recording.clone(),
                movement_id: movement.clone(),
                bpm: if_(i.bpm.ge(20).and(i.bpm.le(300)), i.bpm.clone(), int(0)),
            });
            b.s(put(db.song, song.is_some(), &id, &row));
            let left = b.let_(song.map(|s| s.f().recording_id).filter(|r| r.ne(&recording)));
            b.s(if_(
                left.is_some().and(db.song.filter(|s| s.f().recording_id.eq(left.unwrap())).is_empty()),
                block(|b| {
                    b.s(db
                        .credit
                        .filter(|c| c.f().recording_id.eq(left.unwrap()))
                        .map(|c| db.credit.delete((c.f().recording_id, c.f().person_name, c.f().role))));
                    db.recording.delete(left.unwrap()).s()
                }),
                unit(),
            ));
            let moved = b.let_(song.map_or(none(), |s| s.f().movement_id).filter(|m| some(m).ne(&movement)));
            if_(
                moved.is_some().and(db.song.filter(|s| s.f().movement_id.eq(&moved)).is_empty()),
                block(|b| {
                    let of = b.let_(db.movement.get(moved.unwrap()).map_or("", |m| m.f().work_id));
                    b.s(db.movement.delete(moved.unwrap()));
                    let unused = db
                        .movement
                        .filter(|m| m.f().work_id.eq(&of))
                        .is_empty()
                        .and(db.recording.filter(|r| r.f().work_id.eq(some(&of))).is_empty());
                    if_(unused, db.work.delete(&of).s(), unit())
                }),
                unit(),
            )
        })
    },
};

/// `if x is empty { kept } else { x }`, a fill-if-given field.
fn given(x: &Ex<Text>, kept: Ex<Text>) -> Ex<Text> {
    if_(x.is_empty(), kept, x.clone())
}

/// Say more about a work. A work exists because a song named it, so an
/// unknown one is refused; every field is fill-if-given.
pub static DESCRIBE_WORK: Def1<DescribeWork, ()> = Def1 {
    name: "library.describe_work",
    kind: Kind::Mutator,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            b.s(IS_LIBRARY.call());
            b.s(if_(
                db.work.get(&i.id).is_none(),
                refuse_(concat(vec![
                    txt("no work "),
                    i.id.clone(),
                    txt("; a work exists because a song named it"),
                ])),
                unit(),
            ));
            db.work
                .update(&i.id, |w| {
                    let w = w.f();
                    WorkEx {
                        opus: given(&i.opus, w.opus.clone()),
                        key_sig: given(&i.key_sig, w.key_sig.clone()),
                        form: given(&i.form, w.form.clone()),
                        period: given(&i.period, w.period.clone()),
                        composed: if_(i.composed.eq(0), w.composed.clone(), i.composed.clone()),
                        art: given(&i.art, w.art.clone()),
                        added_ms: Ctx.now(),
                        ..w
                    }
                    .into()
                })
                .s()
        })
    },
};

/// Say more about a performance; refuses an unknown one.
pub static DESCRIBE_RECORDING: Def1<DescribeRecording, ()> = Def1 {
    name: "library.describe_recording",
    kind: Kind::Mutator,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            b.s(IS_LIBRARY.call());
            b.s(if_(
                db.recording.get(&i.id).is_none(),
                refuse_(concat(vec![
                    txt("no recording "),
                    i.id.clone(),
                    txt("; one exists because a song is part of it"),
                ])),
                unit(),
            ));
            db.recording
                .update(&i.id, |r| {
                    let r = r.f();
                    RecordingEx {
                        recorded: if_(i.recorded.eq(0), r.recorded.clone(), i.recorded.clone()),
                        venue: given(&i.venue, r.venue.clone()),
                        label: given(&i.label, r.label.clone()),
                        licence: given(&i.licence, r.licence.clone()),
                        art: given(&i.art, r.art.clone()),
                        added_ms: Ctx.now(),
                        ..r
                    }
                    .into()
                })
                .s()
        })
    },
};

/// Say more about somebody, making the row when it is not there.
pub static DESCRIBE_PERSON: Def1<DescribePerson, ()> = Def1 {
    name: "library.describe_person",
    kind: Kind::Mutator,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            b.s(IS_LIBRARY.call());
            let person = b.let_(db.person.get(&i.name));
            let row = b.let_(PersonEx {
                name: i.name.clone(),
                sort_name: given(&i.sort_name, person.map_or("", |p| p.f().sort_name)),
                born: if_(i.born.eq(0), person.map_or(0, |p| p.f().born), i.born.clone()),
                died: if_(i.died.eq(0), person.map_or(0, |p| p.f().died), i.died.clone()),
                art: given(&i.art, person.map_or("", |p| p.f().art)),
                added_ms: person.map_or(Ctx.now(), |p| p.f().added_ms),
                user_id: person.map_or(Ctx.user(), |p| p.f().user_id),
            });
            put(db.person, person.is_some(), &i.name, &row)
        })
    },
};

/// Credit somebody on a recording, properly: keyed by the three, so a
/// person may hold two roles and a second entry is a correction. A real
/// credit is at least 1; 0 is the lumped fallback's.
pub static CREDIT_RECORDING: Def1<CreditRecording, ()> = Def1 {
    name: "library.credit_recording",
    kind: Kind::Mutator,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            b.s(IS_LIBRARY.call());
            b.s(if_(
                db.recording.get(&i.recording_id).is_none(),
                refuse_(concat(vec![txt("no recording "), i.recording_id.clone(), txt(" to credit anybody on")])),
                unit(),
            ));
            b.s(if_(
                db.person.get(&i.person_name).is_none(),
                db.person
                    .insert(PersonEx {
                        name: i.person_name.clone(),
                        sort_name: txt(""),
                        born: int(0),
                        died: int(0),
                        art: txt(""),
                        added_ms: Ctx.now(),
                        user_id: Ctx.user(),
                    })
                    .s(),
                unit(),
            ));
            let role = b.let_(given(&i.role, txt("artist")));
            let key = || (i.recording_id.clone(), i.person_name.clone(), role.clone());
            let credit = b.let_(db.credit.get(key()));
            let row = b.let_(CreditEx {
                recording_id: i.recording_id.clone(),
                person_name: i.person_name.clone(),
                role: role.clone(),
                instrument: given(&i.instrument, credit.map_or("", |c| c.f().instrument)),
                pos: if_(i.pos.eq(0), credit.map_or(0, |c| c.f().pos).max(1), i.pos.max(1)),
                added_ms: Ctx.now(),
                user_id: Ctx.user(),
            });
            put(db.credit, credit.is_some(), key(), &row)
        })
    },
};

/// Take something out of the library, and off every playlist holding it.
pub static REMOVE_MEDIA: Def1<RemoveMedia, ()> = Def1 {
    name: "library.remove_media",
    kind: Kind::Mutator,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            b.s(IS_LIBRARY.call());
            b.s(db
                .playlist_item
                .filter(|p| p.f().media_id.eq(&i.id))
                .map(|p| db.playlist_item.delete((p.f().playlist_id, p.f().media_id))));
            b.s(db.song.delete(&i.id));
            db.media.delete(&i.id).s()
        })
    },
};

// * The queries

/// A media row as a list renders it, on the playlist `on`.
fn entry(m: &Ex<Media>, on: &Ex<Id<Playlist>>) -> Ex<LibraryEntry> {
    LIBRARY_ENTRY.call(m, PLAYLIST_POS.call(on, m.f().id))
}

/// The whole library in the order things were added, each with where it
/// sits on one playlist.
pub static LIBRARY_LIST: Def1<Library, Vec<LibraryEntry>> = Def1 {
    name: "library.library",
    kind: Kind::Query,
    body: |input| db().media.sort(|m| m.f().pos).map(|m| entry(&m, &input.f().playlist_id)),
};

/// The library's rows whose title or creator holds the needle, case folded,
/// in library order.
pub static SEARCH: Def1<Search, Vec<LibraryEntry>> = Def1 {
    name: "library.search",
    kind: Kind::Query,
    body: |input| {
        let i = input.f();
        db().media
            .filter(|m| HAS.call(m.f().title, &i.needle).or(HAS.call(m.f().creator, &i.needle)))
            .sort(|m| m.f().pos)
            .map(|m| entry(&m, &i.playlist_id))
    },
};

/// Every album some song is on, by name, with whoever made its first track
/// (in the song table's order) and how many tracks it has.
pub static ALBUMS: Def0<Vec<AlbumsEntry>> = Def0 {
    name: "library.albums",
    kind: Kind::Query,
    body: || {
        let db = db();
        db.album
            .map(|a| {
                let creators = db
                    .song
                    .filter(|s| s.f().album_name.eq(some(a.f().name)))
                    .map(|s| db.media.get(s.f().media_id).map_or("", |m| m.f().creator));
                tup((a, creators))
            })
            .filter(|g| g.f().1.len().gt(0))
            .map(|g| {
                let (a, cs) = g.f();
                AlbumsEntryEx {
                    art: a.f().art,
                    creator: cs.first().unwrap_or(""),
                    name: a.f().name,
                    tracks: cs.len(),
                }
                .into()
            })
    },
};

/// Everyone who made something in the library, by name, with the picture
/// `person` has for the name.
pub static ARTISTS: Def0<Vec<ArtistsEntry>> = Def0 {
    name: "library.artists",
    kind: Kind::Query,
    body: || {
        let db = db();
        db.media
            .all()
            .group(
                |m| m.f().creator,
                |c, ms| {
                    ArtistsEntryEx {
                        art: db.person.get(&c).map_or("", |p| p.f().art),
                        name: c,
                        tracks: ms.len(),
                    }
                    .into()
                },
            )
            .map(|g| g.f().1)
    },
};

/// Every song, with the columns only a song has.
pub static TRACK_DETAILS: Def0<Vec<TrackDetailsEntry>> = Def0 {
    name: "library.track_details",
    kind: Kind::Query,
    body: || {
        let db = db();
        db.song.map(|s| {
            block(|b| {
                let movement = b.let_(MOVEMENT_OF.call(&s));
                let work = b.let_(movement.map_or(none(), |m| db.work.get(m.f().work_id)));
                let recording = b.let_(db.recording.get(s.f().recording_id));
                let mine = db.credit.filter(|c| c.f().recording_id.eq(s.f().recording_id));
                TrackDetailsEntryEx {
                    album: s.f().album_name.unwrap_or(""),
                    bpm: s.f().bpm,
                    catalogue: work.map_or("", |w| w.f().catalogue),
                    licence: recording.map_or("", |r| r.f().licence),
                    media_id: s.f().media_id,
                    part: movement.map_or("", |m| m.f().part),
                    performer: PERFORMERS.call(mine, s.f().recording_id),
                    track: s.f().track,
                }
                .into()
            })
        })
    },
};

/// A song's media row, there.
fn media_of(s: &Ex<Song>) -> Ex<Media> {
    db().media.get(s.f().media_id).unwrap()
}

/// One album's tracks in the order the work goes: part, then track (an
/// untagged one, 0, last in its part), then title, then the key.
pub static ALBUM: Def1<AlbumInput, Vec<LibraryEntry>> = Def1 {
    name: "library.album",
    kind: Kind::Query,
    body: |input| {
        let (db, i) = (db(), input.f());
        db.song
            .filter(|s| s.f().album_name.eq(some(&i.name)).and(db.media.get(s.f().media_id).is_some()))
            .sort(|s| {
                (
                    MOVEMENT_OF.call(&s).map_or("", |m| m.f().part),
                    s.f().track.eq(0),
                    s.f().track,
                    db.media.get(s.f().media_id).map_or("", |m| m.f().title),
                )
            })
            .map(|s| LIBRARY_ENTRY.call(media_of(&s), PLAYLIST_POS.call(&i.playlist_id, s.f().media_id)))
    },
};

/// One artist's tracks, in library order.
pub static ARTIST: Def1<Artist, Vec<LibraryEntry>> = Def1 {
    name: "library.artist",
    kind: Kind::Query,
    body: |input| {
        let i = input.f();
        db().media
            .filter(|m| m.f().creator.eq(&i.name))
            .sort(|m| m.f().pos)
            .map(|m| entry(&m, &i.playlist_id))
    },
};

/// Everyone some work here is by, with how many tracks are movements of
/// their works and how many works.
pub static COMPOSERS: Def0<Vec<ComposersEntry>> = Def0 {
    name: "library.composers",
    kind: Kind::Query,
    body: || {
        let db = db();
        db.person
            .map(|p| {
                tup((
                    p.clone(),
                    db.work.filter(|w| w.f().composer.eq(p.f().name)).map(|w| WORK_TRACKS.call(w.f().id)),
                ))
            })
            .filter(|g| g.f().1.len().gt(0))
            .map(|g| {
                let (p, ts) = g.f();
                let p = p.f();
                ComposersEntryEx {
                    art: p.art,
                    born: p.born,
                    died: p.died,
                    name: p.name.clone(),
                    sort_name: if_(p.sort_name.is_empty(), p.name, p.sort_name),
                    tracks: TOTAL.call(&ts),
                    works: ts.len(),
                }
                .into()
            })
    },
};

/// One composer's works, by catalogue and then title.
pub static WORKS: Def1<Works, Vec<WorkSummary>> = Def1 {
    name: "library.works",
    kind: Kind::Query,
    body: |input| {
        let mine = db().work.filter(|w| w.f().composer.eq(input.f().composer));
        SUMMARIES.call(mine.sort(|w| (w.f().catalogue, w.f().title)))
    },
};

/// One work by its key: none or one.
pub static WORK: Def1<WorkInput, Vec<WorkSummary>> = Def1 {
    name: "library.work",
    kind: Kind::Query,
    body: |input| SUMMARIES.call(db().work.filter(|w| w.f().id.eq(input.f().id))),
};

/// Every performance of one work: the most complete first, then the
/// oldest, then the key.
pub static RECORDINGS: Def1<Recordings, Vec<RecordingsEntry>> = Def1 {
    name: "library.recordings",
    kind: Kind::Query,
    body: |input| {
        let db = db();
        db.recording
            .filter(|r| r.f().work_id.eq(some(input.f().work_id)))
            .map(|r| tup((r.clone(), db.song.filter(|s| s.f().recording_id.eq(r.f().id)).len())))
            .sort(|g| (-g.f().1, g.f().0.f().recorded))
            .map(|g| {
                let (r, n) = g.f();
                let r = r.f();
                RecordingsEntryEx {
                    art: r.art,
                    id: r.id.clone(),
                    label: r.label,
                    licence: r.licence,
                    performers: PERFORMERS.call(db.credit.filter(|c| c.f().recording_id.eq(&r.id)), &r.id),
                    recorded: r.recorded,
                    tracks: n,
                }
                .into()
            })
    },
};

/// Who is on one recording, the roles kept, in billing order: the real
/// credits where there are any.
pub static CREDITS: Def1<Credits, Vec<CreditsEntry>> = Def1 {
    name: "library.credits",
    kind: Kind::Query,
    body: |input| {
        block(|b| {
            let all = b.let_(db().credit.filter(|c| c.f().recording_id.eq(input.f().recording_id)));
            CREDITED.call(all).sort(|c| (c.f().pos, c.f().person_name)).map(|c| {
                let c = c.f();
                CreditsEntryEx {
                    instrument: c.instrument,
                    name: c.person_name,
                    pos: c.pos,
                    role: c.role,
                }
                .into()
            })
        })
    },
};

/// One performance's tracks in the order the work goes: by movement number,
/// a track of no movement after the rest.
pub static RECORDING: Def1<RecordingInput, Vec<LibraryEntry>> = Def1 {
    name: "library.recording",
    kind: Kind::Query,
    body: |input| {
        let (db, i) = (db(), input.f());
        db.song
            .filter(|s| s.f().recording_id.eq(&i.id).and(db.media.get(s.f().media_id).is_some()))
            .sort(|s| {
                (
                    s.f().movement_id.is_none(),
                    MOVEMENT_OF.call(&s).map_or(0, |m| m.f().no),
                    db.media.get(s.f().media_id).map_or("", |m| m.f().title),
                )
            })
            .map(|s| LIBRARY_ENTRY.call(media_of(&s), PLAYLIST_POS.call(&i.playlist_id, s.f().media_id)))
    },
};

// * Reading

// * The mutations

// * The queries
