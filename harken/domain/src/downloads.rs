//! The files the library has a row for and the server has not got yet.
//! What the server downloads — an episode of a followed feed, a recording
//! of the catalogue — is in the library before its file is: its row is
//! authored first, then [`queue_download`] says the file is still to come,
//! [`download_progress`] how it is going, and [`downloaded`] that it is
//! here. A media row with no [`Download`] is on the server.
//!
//! A file queued is a job too ([`Fetch`], `ark::effect`): the server's
//! handler claims it, downloads it, says how it goes now and then rather
//! than byte by byte, so a file is a handful of entries, and finishes it.
//! An episode here is heard the same way ([`Hearing`], [`hear`]).

use ark::prelude::*;

use crate::browse::Plan;
use crate::library::is_library;
use crate::schema::*;
use crate::types::*;

/// What a download can be, while it is one.
pub const STATES: &[&str] = &["queued", "downloading", "failed"];

/// The media row whose file is `file`.
fn media_of(db: &Db<Harken>, file: &str) -> Result<Media, String> {
    db.media
        .by(Media::FILE)
        .from(&file.to_string())
        .first()
        .filter(|m| m.file == file)
        .ok_or_else(|| format!("no media has the file {file}"))
}

/// A job pended now, unless it is due already or claimed: what it is
/// about asked for again is the one job, and one due later is due now.
pub(crate) fn pend<R: ark::effect::Effect>(ctx: &Ctx, db: &mut Db<Harken>, row: impl FnOnce(Job<R::Outcome>) -> R) -> Result<bool, String> {
    let r = row(Job::Pending { due_ms: ctx.now });
    if let Some(was) = db.tab::<R>().at(&r.key()) {
        if matches!(was.job(), Job::Pending { due_ms } if *due_ms <= ctx.now) || matches!(was.job(), Job::Claimed { .. }) {
            return Ok(false);
        }
    }
    db.tab::<R>().put(r).map(|_| true)
}

/// A file still to come: queued, from `source`, and its download a job
/// ([`Fetch`]). Said again of a file queued or downloading already, it
/// writes nothing; of one that failed, it is queued again.
#[mutator("library.queue_download")]
pub fn queue_download(ctx: &Ctx, db: &mut Db<Harken>, input: QueueDownload) -> Result<(), String> {
    is_library(ctx)?;
    let media = media_of(db, &input.file)?;
    if !pend::<Fetch>(ctx, db, |state| Fetch { media_id: media.id, state })? {
        return Ok(());
    }
    let was = db.download.get(media.id);
    db.download.put(Download {
        media_id: media.id,
        source: input.source,
        state: "queued".into(),
        bytes: 0,
        total: 0,
        error: String::new(),
        asked_ms: was.map_or(ctx.now, |d| d.asked_ms),
    })
}

/// How a file still to come is going. One not queued is refused: it is
/// here, or was never asked for.
#[mutator("library.download_progress")]
pub fn download_progress(ctx: &Ctx, db: &mut Db<Harken>, input: DownloadProgress) -> Result<(), String> {
    is_library(ctx)?;
    let media = media_of(db, &input.file)?;
    let Some(was) = db.download.get(media.id) else {
        return Err(format!("{} is not being downloaded", input.file));
    };
    db.download.put(Download {
        state: input.state,
        bytes: input.bytes.max(0),
        total: input.total.max(0),
        error: input.error,
        ..was
    })
}

/// A file downloaded: on the server now. Of one that was never still to
/// come, it writes nothing.
#[mutator("library.downloaded")]
pub fn downloaded(ctx: &Ctx, db: &mut Db<Harken>, input: Downloaded) -> Result<(), String> {
    is_library(ctx)?;
    let media = media_of(db, &input.file)?;
    db.download.delete(media.id)
}

/// An episode's file heard ([`Hearing`]), unless it is being heard.
#[mutator("hearing.hear")]
pub fn hear(ctx: &Ctx, db: &mut Db<Harken>, input: Hear) -> Result<(), String> {
    is_library(ctx)?;
    let media = media_of(db, &input.file)?;
    pend::<Hearing>(ctx, db, |state| Hearing { media_id: media.id, state }).map(|_| ())
}

/// Every file still to come, with its title, by when it was asked for.
#[view("library.downloads")]
pub fn downloads(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<DownloadsEntry>, String> {
    Ok(scan::<Harken, Download>()
        .sort(|d| (d.asked_ms, d.media_id))
        .map_with(|db, d| DownloadsEntry {
            bytes: d.bytes,
            error: d.error.clone(),
            media_id: d.media_id,
            state: d.state.clone(),
            title: db.media.get(d.media_id).map(|m| m.title).unwrap_or_default(),
            total: d.total,
        }))
}

/// Whether a state is a download's. A `&String`, as a refinement's check
/// is handed the field.
#[allow(clippy::ptr_arg)]
pub fn is_state(state: &String) -> bool {
    STATES.contains(&state.as_str())
}
