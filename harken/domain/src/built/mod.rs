//! harken's domain built with the Rust builders (§1.15): every function of
//! `crate::module::domain`, one for one, held to its hash.

pub mod library;
pub mod listening;
pub mod playlists;

use ark::build::{assemble, Domain, Fun};

use crate::schema::Harken;
use library::*;
use listening::*;
use playlists::*;

/// harken's module, every function checked and bound by its name.
pub fn domain() -> Domain {
    let fns: [&'static dyn Fun; 66] = [
        &ADD_SONG,
        &DESCRIBE_WORK,
        &DESCRIBE_RECORDING,
        &DESCRIBE_PERSON,
        &CREDIT_RECORDING,
        &REMOVE_MEDIA,
        &LIBRARY,
        &SEARCH,
        &ALBUMS,
        &ARTISTS,
        &TRACK_DETAILS,
        &ALBUM,
        &ARTIST,
        &COMPOSERS,
        &WORKS,
        &WORK,
        &RECORDINGS,
        &CREDITS,
        &RECORDING,
        &WORK_KEY,
        &MOVEMENT_KEY,
        &RECORDING_KEY,
        &KEY_PART,
        &PLAYLIST_NAME,
        &CREATE_PLAYLIST,
        &ADD_TO_PLAYLIST,
        &ADD_ALL_TO_PLAYLIST,
        &REMOVE_FROM_PLAYLIST,
        &PLAYLISTS,
        &PLAYLISTS_OF,
        &PLAYLIST,
        &START,
        &PLAY,
        &PAUSE,
        &SEEK,
        &SKIP,
        &JUMP,
        &TRANSFER,
        &STOP,
        &ENQUEUE,
        &REMOVE,
        &MOVE_TRACK,
        &CLEAR_UPCOMING,
        &STARTED,
        &ADVANCE,
        &STALLED,
        &REPORT_POSITION,
        &RELEASE,
        &EQUIP,
        &SET_VOLUME,
        &NUDGE_VOLUME,
        &SET_MUTE,
        &REPORT_VOLUME,
        &JOIN,
        &UNJOIN,
        &REPORT_GROUP,
        &SET_GROUP_VOLUME,
        &NUDGE_GROUP_VOLUME,
        &SET_GROUP_MUTE,
        &SCRUB,
        &ON_JOIN,
        &ON_LEAVE,
        &ON_RESTORE,
        &RECORD_PLAY,
        &LAST_PLAYED,
        &RECENTLY_PLAYED,
    ];
    assemble(Harken::schema(), &fns).unwrap_or_else(|r| panic!("harken's module: {r}"))
}
