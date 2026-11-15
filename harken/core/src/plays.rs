//! The plays worth keeping: in the log, one row per play, so every peer has
//! its listening history.

use ark::prelude::*;

use crate::browse::Plan;
use crate::keys::text_of_id;
use crate::schema::*;
use crate::types::*;

/// A track was played: authored by the device making the sound once it has
/// played long enough to count, dated by its own clock.
#[mutator("listening.record_play")]
pub fn record_play(ctx: &Ctx, db: &mut Db<Harken>, input: RecordPlay) -> Result<(), Refusal> {
    db.play.insert(Play {
        id: ctx.new_id(),
        user_id: ctx.user().into(),
        track: text_of_id(&input.track.0),
        title: input.title,
        creator: input.creator,
        album: input.album,
        duration_ms: input.duration_ms,
        file: input.file,
        played_ms: ctx.now,
        from: input.from,
    })
}

/// The caller's plays, newest first, the last `n` of them.
fn newest(ctx: &Ctx, n: usize) -> Plan<Play> {
    let user = ctx.user().to_string();
    scan::<Harken, Play>()
        .filter(move |p| p.user_id == user)
        .sort(|p| (Desc(p.played_ms), Desc(p.id)))
        .take(n)
}

/// The caller's newest play.
#[view("listening.last_played")]
pub fn last_played(ctx: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<Play>, Refusal> {
    Ok(newest(ctx, 1))
}

/// The caller's plays, newest first, the last fifty of them.
#[view("listening.recently_played")]
pub fn recently_played(ctx: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<Play>, Refusal> {
    Ok(newest(ctx, 50))
}
