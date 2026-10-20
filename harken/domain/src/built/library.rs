//! The library router, built (`crate::library`'s functions, one for one).

use ark::author::{Id, Int, Text};
use ark::build::*;

use crate::library::*;
use crate::schema::*;

fn db() -> HarkenDb {
    Harken::db()
}

// * The keys

pub static WORK_TITLE: Def4<Text, Text, Text, Text, Text> = Def4 {
    name: "library.work_title",
    kind: Kind::Helper,
    body: |work_title, album, catalogue, part| if_(work_title.is_empty().and((!catalogue.is_empty()).or(!part.is_empty())), album, work_title),
};

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

pub static MOVEMENT_KEY: Def2<Text, Int, Text> = Def2 {
    name: "library.movement_key",
    kind: Kind::Helper,
    body: |work_id, no| concat(vec![work_id, txt("#"), no.to_text()]),
};

pub static RECORDING_KEY: Def2<Text, Text, Text> = Def2 {
    name: "library.recording_key",
    kind: Kind::Helper,
    body: |of, who| concat(vec![of, txt("@"), KEY_PART.call(who)]),
};

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

pub static CREDITED_AS: Def3<Option<Text>, Text, Text, Text> = Def3 {
    name: "library.credited_as",
    kind: Kind::Helper,
    body: |work_id, artist, performer| if_(performer.is_empty(), if_(work_id.is_some(), txt(""), artist), performer),
};

pub static MOVEMENT_ID: Def3<Option<Text>, Int, Int, Option<Text>> = Def3 {
    name: "library.movement_id",
    kind: Kind::Helper,
    body: |work_id, movement_no, track| work_id.map(|w| MOVEMENT_KEY.call(w, if_(movement_no.eq(0), track, movement_no))),
};

// * Reading

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

pub static CREDITED: Def1<Vec<Credit>, Vec<Credit>> = Def1 {
    name: "library.credited",
    kind: Kind::Helper,
    body: |credits| credits.filter(|c| c.f().pos.gt(0).or(credits.filter(|d| d.f().pos.gt(0)).is_empty())),
};

pub static JOINED: Def1<Vec<Text>, Text> = Def1 {
    name: "library.joined",
    kind: Kind::Helper,
    body: |names| {
        concat_of(&names.map(|n| concat(vec![txt(", "), n])))
            .split_once(", ")
            .map_or("", |p| p.f().1)
    },
};

pub static PERFORMERS: Def2<Vec<Credit>, Text, Text> = Def2 {
    name: "library.performers",
    kind: Kind::Helper,
    body: |credits, recording_id| {
        let mine = credits.filter(|c| c.f().recording_id.eq(&recording_id).and(c.f().role.ne("composer")));
        JOINED.call(CREDITED.call(mine).sort(|c| (c.f().pos, c.f().person_name)).map(|c| c.f().person_name))
    },
};

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

pub static TOTAL: Def1<Vec<Int>, Int> = Def1 {
    name: "library.total",
    kind: Kind::Helper,
    body: |counts| counts.sum(),
};

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

pub static IS_LIBRARY: Def0<()> = Def0 {
    name: "library.is_library",
    kind: Kind::Helper,
    body: || if_(!Ctx.has_role("library"), refuse_("only the library writes the library"), unit()),
};

pub static PLAYLIST_POS: Def2<Id<Playlist>, Id<Media>, Option<Int>> = Def2 {
    name: "library.playlist_pos",
    kind: Kind::Helper,
    body: |playlist_id, media_id| db().playlist_item.get((playlist_id, media_id)).map(|i| i.f().pos),
};

pub static MOVEMENT_OF: Def1<Song, Option<Movement>> = Def1 {
    name: "library.movement_of",
    kind: Kind::Helper,
    body: |song| song.f().movement_id.map_or(none(), |m| db().movement.get(m)),
};

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

pub static LIBRARY: Def1<Library, Vec<LibraryEntry>> = Def1 {
    name: "library.library",
    kind: Kind::Query,
    body: |input| db().media.sort(|m| m.f().pos).map(|m| entry(&m, &input.f().playlist_id)),
};

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

pub static WORKS: Def1<Works, Vec<WorkSummary>> = Def1 {
    name: "library.works",
    kind: Kind::Query,
    body: |input| {
        let mine = db().work.filter(|w| w.f().composer.eq(input.f().composer));
        SUMMARIES.call(mine.sort(|w| (w.f().catalogue, w.f().title)))
    },
};

pub static WORK: Def1<WorkInput, Vec<WorkSummary>> = Def1 {
    name: "library.work",
    kind: Kind::Query,
    body: |input| SUMMARIES.call(db().work.filter(|w| w.f().id.eq(input.f().id))),
};

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
