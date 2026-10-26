//! How far each person has listened to each episode: a [`Progress`] row
//! per person per episode, in the log, so it follows them to every device
//! and every member sees how far the others have got.
//!
//! The device making the sound says where it is now and then while an
//! episode plays, and when it pauses (`harken-player`'s reconciler,
//! [`report_progress`]); a hand says an episode is played or not
//! ([`mark_played`]). A song has none: it is not a thing one is part way
//! through.

use ark::prelude::*;

use crate::browse::Plan;
use crate::schema::*;
use crate::types::*;

/// How near its end an episode counts as played: the credits, the
/// announcements after the last word.
pub const FINISHED_WITHIN_MS: i64 = 30_000;

/// Whether `position_ms` of `duration_ms` is the end of it.
pub fn is_finished(position_ms: i64, duration_ms: i64) -> bool {
    duration_ms > 0 && duration_ms - position_ms <= FINISHED_WITHIN_MS
}

/// What to call the caller: what they said, else the name their sign-in
/// carries, else what their row was last called.
fn name_of(ctx: &Ctx, db: &Db<Harken>, said: String, was: Option<&Progress>) -> String {
    if !said.is_empty() {
        return said;
    }
    match db.sign_in.get(ctx.session.clone()).map(|s| s.name).filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => was.map(|p| p.name.clone()).unwrap_or_default(),
    }
}

fn signed_in(ctx: &Ctx) -> Result<String, Refusal> {
    match ctx.user() {
        "" => Err(Refusal::new("sign in to keep your place")),
        u => Ok(u.to_string()),
    }
}

/// Where the caller has got to in an episode. Of a song, or of something
/// the library has not got, it writes nothing; said again where it was, it
/// writes nothing.
#[mutator("library.report_progress")]
pub fn report_progress(ctx: &Ctx, db: &mut Db<Harken>, input: ReportProgress) -> Result<(), Refusal> {
    let user = signed_in(ctx)?;
    if !db.episode.has(input.media_id) {
        return Ok(());
    }
    let duration_ms = match input.duration_ms {
        0 => db.media.get(input.media_id).map_or(0, |m| m.duration_ms),
        d => d,
    };
    let was = db.progress.get((user.clone(), input.media_id));
    if was
        .as_ref()
        .is_some_and(|p| p.position_ms == input.position_ms && p.duration_ms == duration_ms)
    {
        return Ok(());
    }
    let name = name_of(ctx, db, input.name, was.as_ref());
    db.progress.put(Progress {
        user_id: user,
        media_id: input.media_id,
        position_ms: input.position_ms,
        duration_ms,
        finished: is_finished(input.position_ms, duration_ms),
        name,
        updated_ms: ctx.now,
    })
}

/// An episode said to be played — at its end — or not played: from its
/// start, as if never begun.
#[mutator("library.mark_played")]
pub fn mark_played(ctx: &Ctx, db: &mut Db<Harken>, input: MarkPlayed) -> Result<(), Refusal> {
    let user = signed_in(ctx)?;
    if !db.episode.has(input.media_id) {
        return Err(Refusal::new("only an episode is played or not"));
    }
    if !input.finished {
        return db.progress.delete((user, input.media_id));
    }
    let was = db.progress.get((user.clone(), input.media_id));
    let duration_ms = db.media.get(input.media_id).map_or(0, |m| m.duration_ms);
    let name = name_of(ctx, db, input.name, was.as_ref());
    db.progress.put(Progress {
        user_id: user,
        media_id: input.media_id,
        position_ms: duration_ms,
        duration_ms,
        finished: true,
        name,
        updated_ms: ctx.now,
    })
}

fn entry(p: &Progress) -> ProgressEntry {
    ProgressEntry {
        duration_ms: p.duration_ms,
        finished: p.finished,
        media_id: p.media_id,
        name: p.name.clone(),
        position_ms: p.position_ms,
        updated_ms: p.updated_ms,
        user_id: p.user_id.clone(),
    }
}

/// The caller's own place in every episode they have begun, the newest
/// first.
#[view("library.my_progress")]
pub fn my_progress(ctx: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<ProgressEntry>, Refusal> {
    let user = ctx.user().to_string();
    Ok(scan::<Harken, Progress>()
        .filter(move |p| p.user_id == user)
        .sort(|p| (Desc(p.updated_ms), p.media_id))
        .map(entry))
}

/// How far everybody has listened to one show's episodes, the newest
/// listening first: what its page says beside each episode.
#[view("library.show_progress")]
pub fn show_progress(_: &Ctx, _: &Db<Harken>, input: ShowInput) -> Result<Plan<ProgressEntry>, Refusal> {
    let id = input.id;
    Ok(scan::<Harken, Progress>()
        .filter_with(move |db, p| db.episode.get(p.media_id).is_some_and(|e| e.show_id == id))
        .sort(|p| (Desc(p.updated_ms), p.user_id.clone(), p.media_id))
        .map(entry))
}
