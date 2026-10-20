//! The module: every function, bound by its path (`library.add_song`),
//! over the schema; their callees come with them.

use ark::author::Domain;

use crate::library::*;
use crate::listening::*;
use crate::playlists::*;
use crate::schema::Harken;

/// harken's module, every function checked.
pub fn domain() -> Domain {
    ark::module! {
        schema: Harken,
        functions: [
            add_song, describe_work, describe_recording, describe_person, credit_recording, remove_media,
            library, search, albums, artists, track_details, album, artist, composers, works, work, recordings,
            credits, recording, work_key, movement_key, recording_key, key_part, playlist_name,
            create_playlist, add_to_playlist, add_all_to_playlist, remove_from_playlist, playlists, playlists_of,
            playlist,
            start, play, pause, seek, skip, jump, transfer, stop, enqueue, remove, move_track, clear_upcoming,
            started, advance, stalled, report_position, release, equip, set_volume, nudge_volume, set_mute,
            report_volume, join, unjoin, report_group, set_group_volume, nudge_group_volume, set_group_mute,
            scrub, on_join, on_leave, on_restore, record_play, last_played, recently_played,
        ],
    }
    .unwrap_or_else(|r| panic!("harken's module: {r}"))
}
