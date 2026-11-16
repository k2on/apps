//! The queue, edited from any device while it plays.

use ark::prelude::*;

use crate::listening::*;
use crate::schema::*;
use crate::types::*;

fn refuse(why: &str) -> Result<(), Refusal> {
    Err(Refusal::new(why))
}

/// Tracks put after the one playing ("Play next") or at the end ("Add to
/// queue"); into nothing queued it is `start` of them from the first.
#[live("listening.enqueue")]
pub fn enqueue(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Enqueue) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    if !(is_active(&session) || can_play(db, ctx.user(), ctx.device())) {
        return refuse("nothing here can play it");
    }
    let next = match &session {
        Session::Active { active } => {
            let end = active.queue.len();
            let at = if input.next { active.at as usize + 1 } else { end }.min(end);
            let queue = [&active.queue[..at], &input.tracks[..], &active.queue[at..end]].concat();
            Session::Active {
                active: Active { queue, ..active.clone() },
            }
        }
        Session::Idle => Session::Active {
            active: Active {
                at: 0,
                clock: Clock::Playing { at_ms: 0, since: ctx.now },
                from: String::new(),
                output: here(ctx.device()),
                queue: input.tracks,
            },
        },
    };
    put_session(db, ctx.user(), next)
}

/// A track out of the queue. The one playing taken out is the next one
/// playing, from its start — or with nothing after it, the one before,
/// paused at its start; the last track of all taken out is nothing queued.
#[live("listening.remove")]
pub fn remove(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Remove) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    if !in_queue(&session, input.index) {
        return refuse("the track is not in the queue");
    }
    let next = match &session {
        Session::Active { active } => {
            let end = active.queue.len() as i64;
            let playing_one = input.index == active.at;
            let last = active.at + 1 >= end;
            let playing = matches!(active.clock, Clock::Playing { .. });
            let clock = if playing && !last {
                Clock::Playing { at_ms: 0, since: ctx.now }
            } else {
                Clock::Paused { at_ms: 0 }
            };
            if end <= 1 {
                Session::Idle
            } else {
                let mut queue = active.queue.clone();
                queue.remove(input.index as usize);
                Session::Active {
                    active: Active {
                        at: if input.index < active.at || (playing_one && last) {
                            active.at - 1
                        } else {
                            active.at
                        },
                        clock: if playing_one { clock } else { active.clock },
                        queue,
                        ..active.clone()
                    },
                }
            }
        }
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user(), next)
}

/// A track of the queue somewhere else in it; the one playing plays on.
#[live("listening.move")]
pub fn move_track(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Move) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    if !(in_queue(&session, input.from) && in_queue(&session, input.to)) {
        return refuse("the track is not in the queue");
    }
    let next = with_active(&session, |a| {
        let mut queue = a.queue.clone();
        let t = queue.remove(input.from as usize);
        queue.insert(input.to as usize, t);
        Active {
            at: moved_place(a.at, input.from, input.to),
            queue,
            ..a.clone()
        }
    });
    put_session(db, ctx.user(), next)
}

/// Everything after the track playing, gone; it plays on.
#[live("listening.clear_upcoming")]
pub fn clear_upcoming(ctx: &Ctx, db: &mut Db<Harken, Volatile>, _: ()) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    let next = with_active(&session, |a| Active {
        queue: a.queue[..(a.at as usize + 1).min(a.queue.len())].to_vec(),
        ..a.clone()
    });
    put_session(db, ctx.user(), next)
}

// * What the output says

/// The guard of what the output says.
fn the_output(ctx: &Ctx, session: &Session) -> Result<(), Refusal> {
    if !is_output(session, ctx.device()) {
        return refuse("not the output");
    }
    Ok(())
}

/// The output has the hand-off, at this point in the track.
#[live("listening.started")]
pub fn started(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: At) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    the_output(ctx, &session)?;
    let next = with_active(&session, |a| Active {
        clock: clock_at(&a.clock, input.at_ms, ctx.now),
        output: here(ctx.device()),
        ..a.clone()
    });
    put_session(db, ctx.user(), next)
}

/// The output's track ended: the next one, or paused at the start of the
/// last when there is none. About a track no longer playing, nothing.
#[live("listening.advance")]
pub fn advance(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Advance) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    the_output(ctx, &session)?;
    let next = match &session {
        Session::Active { active } if active.at != input.from_track => session.clone(),
        Session::Active { active } if active.at + 1 < active.queue.len() as i64 => Session::Active {
            active: Active {
                at: active.at + 1,
                clock: Clock::Playing { at_ms: 0, since: ctx.now },
                ..active.clone()
            },
        },
        Session::Active { active } => Session::Active {
            active: Active {
                clock: Clock::Paused { at_ms: 0 },
                ..active.clone()
            },
        },
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user(), next)
}

/// The output stopped making a sound by itself, here.
#[live("listening.stalled")]
pub fn stalled(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: At) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    the_output(ctx, &session)?;
    let next = with_active(&session, |a| Active {
        clock: Clock::Paused { at_ms: input.at_ms },
        ..a.clone()
    });
    put_session(db, ctx.user(), next)
}

/// The output, playing, says where it is; the clock is re-anchored there.
/// About a clock that has moved since, or a hand-off in flight, nothing.
#[live("listening.report_position")]
pub fn report_position(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Report) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    the_output(ctx, &session)?;
    let next = match &session {
        Session::Active { active } => {
            let same_clock = matches!(active.clock, Clock::Playing { since, .. } if since == input.since);
            let settled = matches!(active.output, Output::Device { moving: false, .. });
            if same_clock && settled {
                Session::Active {
                    active: Active {
                        clock: Clock::Playing {
                            at_ms: input.at_ms,
                            since: ctx.now,
                        },
                        ..active.clone()
                    },
                }
            } else {
                session.clone()
            }
        }
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user(), next)
}

/// The output lets the sound go: the queue stays, paused, and nothing is
/// the output.
#[live("listening.release")]
pub fn release(ctx: &Ctx, db: &mut Db<Harken, Volatile>, _: ()) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    the_output(ctx, &session)?;
    let next = with_active(&session, |a| Active {
        clock: halted(&a.clock, &a.output, ctx.now),
        output: Output::Nowhere,
        ..a.clone()
    });
    put_session(db, ctx.user(), next)
}
