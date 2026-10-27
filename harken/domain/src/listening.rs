//! The listening session: one account, one thing playing, however many
//! devices are watching it — live state over volatile tables — and the
//! plays worth keeping, in the log.
//!
//! A person with a phone, a laptop and a browser tab has one listening
//! session. Any of them can change it; exactly one of them is making the
//! sound. The state is the instruction: nothing routes a command to the
//! device making the sound; the output reconciles its player against the
//! session and reports what happens to it (`started`, `advance`, `stalled`,
//! `release`, `report_position`) and nothing else.
//!
//! None of the session is in the log. A transition is a live mutator keyed
//! by the account (`ctx.user()`); its first refusals are its guards, in
//! their order. The calling device is `ctx.device()`. The four rules: one
//! output (`Active.output` is one value); the output survives its socket
//! (`on_leave` keeps it `Away`, paused; `on_join` gives it back, paused); a
//! command is a state change the output obeys (`pause`, `seek`, `transfer`
//! move nothing but the session); a press that means a sound takes an
//! output that cannot answer, and a device arriving takes nothing.

use ark::prelude::*;

use crate::groups::led_by;
use crate::schema::*;
use crate::types::*;

// * The clock

/// How far into the track a clock is at `now`.
pub fn position(clock: &Clock, now: i64) -> i64 {
    match *clock {
        Clock::Paused { at_ms } => at_ms,
        Clock::Playing { at_ms, since } => (at_ms + (now - since)).max(at_ms),
    }
}

/// Where the session stands at `now`: its clock's position — except while
/// the sound is on its way to a device (`moving`), when it stands where it
/// was handed over, since nothing is making a sound in between.
pub fn standing(clock: &Clock, output: &Output, now: i64) -> i64 {
    match (output, clock) {
        (Output::Device { moving: true, .. }, Clock::Paused { at_ms } | Clock::Playing { at_ms, .. }) => *at_ms,
        _ => position(clock, now),
    }
}

/// The session's clock paused where the session stands at `now`.
pub fn halted(clock: &Clock, output: &Output, now: i64) -> Clock {
    Clock::Paused {
        at_ms: standing(clock, output, now),
    }
}

/// The clock at `ms`, playing on from there if it was playing.
pub fn clock_at(clock: &Clock, ms: i64, now: i64) -> Clock {
    match clock {
        Clock::Paused { .. } => Clock::Paused { at_ms: ms },
        Clock::Playing { .. } => Clock::Playing { at_ms: ms, since: now },
    }
}

// * The session

/// Whether the output is a device that is here.
pub fn is_here(output: &Output) -> bool {
    matches!(output, Output::Device { .. })
}

/// Whether something is queued.
pub fn is_active(session: &Session) -> bool {
    matches!(session, Session::Active { .. })
}

/// Whether the session has an output that is here to answer.
pub fn reachable(session: &Session) -> bool {
    matches!(session, Session::Active { active } if is_here(&active.output))
}

/// Whether something is making a sound: a clock running on an output that
/// is here.
pub fn sounding(session: &Session) -> bool {
    matches!(session, Session::Active { active } if is_here(&active.output) && matches!(active.clock, Clock::Playing { .. }))
}

/// The account's session: its row's, or `Idle`.
pub fn session_of(db: &Db<Harken, Volatile>, user: &str) -> Session {
    db.listening.get(user).map_or(Session::Idle, |r| r.session)
}

/// The account's session, written.
pub fn put_session(db: &mut Db<Harken, Volatile>, user: &str, session: Session) -> Result<(), Refusal> {
    db.listening.put(Listening { user: user.into(), session })
}

/// Whether the device is here and can make a sound.
pub fn can_play(db: &Db<Harken, Volatile>, user: &str, device: &str) -> bool {
    db.listening_device
        .get((user.to_string(), device.to_string()))
        .is_some_and(|d| d.present.can_play)
}

/// Whether the device is the session's output and here.
pub fn is_output(session: &Session, device: &str) -> bool {
    matches!(session, Session::Active { active } if matches!(&active.output, Output::Device { id, .. } if id == device))
}

/// The sound on this device, which has it.
pub fn here(device: &str) -> Output {
    Output::Device {
        id: device.into(),
        moving: false,
    }
}

/// The output a press that means a sound leaves: the one there is, where it
/// can answer, or the caller.
pub fn pressed(output: &Output, caller: &str) -> Output {
    if is_here(output) {
        output.clone()
    } else {
        here(caller)
    }
}

/// The output once `device` has come back: given back to it, if it was the
/// one that went.
pub fn returned(output: &Output, device: &str) -> Output {
    match output {
        Output::Away { id, .. } if id == device => here(id),
        _ => output.clone(),
    }
}

/// How long the track at `at` is; 0 for no track there.
pub fn duration_at(queue: &[Track], at: i64) -> i64 {
    usize::try_from(at).ok().and_then(|i| queue.get(i)).map_or(0, |t| t.duration_ms)
}

/// A session at a new place in its queue, playing from the start of it, on
/// whichever device a press leaves the sound with.
pub fn moved_to(active: &Active, at: i64, caller: &str, now: i64) -> Session {
    Session::Active {
        active: Active {
            at,
            clock: Clock::Playing { at_ms: 0, since: now },
            from: active.from.clone(),
            output: pressed(&active.output, caller),
            queue: active.queue.clone(),
        },
    }
}

/// Where the track at `at` goes when the one at `from` moves to `to`.
pub fn moved_place(at: i64, from: i64, to: i64) -> i64 {
    if at == from {
        to
    } else if from < to && at > from && at <= to {
        at - 1
    } else if to < from && at >= to && at < from {
        at + 1
    } else {
        at
    }
}

/// Whether `index` is a place in the session's queue.
pub fn in_queue(session: &Session, index: i64) -> bool {
    matches!(session, Session::Active { active } if index < active.queue.len() as i64)
}

/// The output, where it is a device that is here: the one a group is led
/// by.
pub fn output_id(session: &Session) -> Option<String> {
    match session {
        Session::Active { active } => match &active.output {
            Output::Device { id, .. } => Some(id.clone()),
            _ => None,
        },
        Session::Idle => None,
    }
}

/// The session paused where it stands at `now`, `Idle` kept.
pub fn halt(session: &Session, now: i64) -> Session {
    match session {
        Session::Active { active } => Session::Active {
            active: Active {
                clock: halted(&active.clock, &active.output, now),
                ..active.clone()
            },
        },
        Session::Idle => Session::Idle,
    }
}

/// The session with its active state as `f` makes it; `Idle` kept.
pub fn with_active(session: &Session, f: impl FnOnce(&Active) -> Active) -> Session {
    match session {
        Session::Active { active } => Session::Active { active: f(active) },
        Session::Idle => Session::Idle,
    }
}

fn refuse(why: &str) -> Result<(), Refusal> {
    Err(Refusal::new(why))
}

/// The guard most transitions start with.
pub fn queued(session: &Session) -> Result<(), Refusal> {
    if !is_active(session) {
        return refuse("nothing is queued");
    }
    Ok(())
}

/// The guard of a press that means a sound.
fn playable(db: &Db<Harken, Volatile>, ctx: &Ctx, session: &Session) -> Result<(), Refusal> {
    if !(reachable(session) || can_play(db, ctx.user(), ctx.device())) {
        return refuse("nothing here can play it");
    }
    Ok(())
}

// * The presses

/// A track pressed: this queue, from here, at this point, playing. It goes to
/// the device the session has chosen while that device is here, playing or
/// paused — a device somebody picked stays picked; with none, or the one
/// there was gone, the caller takes the sound if it can play.
#[live("listening.start")]
pub fn start(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Start) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    if input.at >= input.queue.len() as i64 {
        return refuse("the track is not in the queue");
    }
    let can = can_play(db, ctx.user(), ctx.device());
    if !(sounding(&session) || reachable(&session) || can) {
        return refuse("nothing here can play it");
    }
    let stays = reachable(&session) || !can;
    let output = match &session {
        Session::Active { active } if stays => active.output.clone(),
        _ => here(ctx.device()),
    };
    let active = Active {
        at: input.at,
        clock: Clock::Playing {
            at_ms: input.position,
            since: ctx.now,
        },
        from: input.from,
        output,
        queue: input.queue,
    };
    put_session(db, ctx.user(), Session::Active { active })
}

/// Resume, on the output where it can answer, else on the caller.
#[live("listening.play")]
pub fn play(ctx: &Ctx, db: &mut Db<Harken, Volatile>, _: ()) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    playable(db, ctx, &session)?;
    let next = with_active(&session, |a| Active {
        clock: Clock::Playing {
            at_ms: standing(&a.clock, &a.output, ctx.now),
            since: ctx.now,
        },
        output: pressed(&a.output, ctx.device()),
        ..a.clone()
    });
    put_session(db, ctx.user(), next)
}

/// Pause, wherever the sound is. Nobody takes anything.
#[live("listening.pause")]
pub fn pause(ctx: &Ctx, db: &mut Db<Harken, Volatile>, _: ()) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    put_session(db, ctx.user(), halt(&session, ctx.now))
}

/// Somewhere else in the track, playing on if it was.
#[live("listening.seek")]
pub fn seek(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Seek) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    let next = with_active(&session, |a| Active {
        clock: clock_at(&a.clock, input.ms, ctx.now),
        ..a.clone()
    });
    put_session(db, ctx.user(), next)
}

/// The next track or the one before, from its start; before the first is
/// the first again. A press that means a sound.
#[live("listening.skip")]
pub fn skip(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Skip) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    let onward = matches!(&session, Session::Active { active } if input.by < 0 || active.at + 1 < active.queue.len() as i64);
    if !onward {
        return refuse("nothing comes after this");
    }
    playable(db, ctx, &session)?;
    let next = match &session {
        Session::Active { active } => moved_to(active, (active.at + input.by).max(0), ctx.device(), ctx.now),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user(), next)
}

/// A track of the queue, from its start. A press that means a sound.
#[live("listening.jump")]
pub fn jump(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Jump) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    queued(&session)?;
    if !in_queue(&session, input.index) {
        return refuse("the track is not in the queue");
    }
    playable(db, ctx, &session)?;
    let next = match &session {
        Session::Active { active } => moved_to(active, input.index, ctx.device(), ctx.now),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user(), next)
}

/// Move the sound to a device that is here and can play: `moving` until it
/// says it has started, the clock standing where it stopped meanwhile.
/// Handing it to the device that already has it, settled, changes nothing.
#[live("listening.transfer")]
pub fn transfer(ctx: &Ctx, db: &mut Db<Harken, Volatile>, input: Transfer) -> Result<(), Refusal> {
    let session = session_of(db, ctx.user());
    if !is_active(&session) {
        return refuse("nothing is playing to move");
    }
    if !can_play(db, ctx.user(), &input.to) {
        return refuse("that device is not here, or cannot play");
    }
    led_by(db, ctx.user(), &input.to)?;
    let next = with_active(&session, |a| {
        let settled = a.output == here(&input.to);
        Active {
            clock: if settled {
                a.clock
            } else {
                clock_at(&a.clock, standing(&a.clock, &a.output, ctx.now), ctx.now)
            },
            output: if settled {
                a.output.clone()
            } else {
                Output::Device {
                    id: input.to.clone(),
                    moving: true,
                }
            },
            ..a.clone()
        }
    });
    put_session(db, ctx.user(), next)
}

/// Nothing queued, nothing playing, nowhere.
#[live("listening.stop")]
pub fn stop(ctx: &Ctx, db: &mut Db<Harken, Volatile>, _: ()) -> Result<(), Refusal> {
    put_session(db, ctx.user(), Session::Idle)
}
