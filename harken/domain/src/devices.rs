//! The devices standing in a session: what each said it is, its volume,
//! and where its scrubber is. Volume is per device; a device has none until
//! it says it has one (`equip`).

use ark::prelude::*;

use crate::groups::dissolved;
use crate::listening::*;
use crate::schema::*;
use crate::types::*;

fn refuse(why: &str) -> Result<(), Refusal> {
    Err(Refusal::new(why))
}

fn key(user: &str, device: &str) -> (String, String) {
    (user.to_string(), device.to_string())
}

/// The devices standing in the account's session, in id order.
pub fn devices_of(db: &Db<Harken>, user: &str) -> Vec<ListeningDevice> {
    db.listening_device.by(ListeningDevice::KEY).from(user).all()
}

/// A device standing in the session; refused when it is not (every caller
/// has held it there first).
pub fn present_of(db: &Db<Harken>, user: &str, device: &str) -> Result<Present, Refusal> {
    db.listening_device
        .get(key(user, device))
        .map(|d| d.present)
        .ok_or_else(|| Refusal::new("unwrapped none"))
}

/// A device's entry, written.
pub fn put_present(db: &mut Db<Harken>, user: &str, device: &str, present: Present) -> Result<(), Refusal> {
    db.listening_device.update(ListeningDevice {
        user: user.into(),
        device: device.into(),
        present,
    })
}

/// A device as `on_join` first writes it: what it declared, and nothing it
/// has not said.
pub fn arrived(device: &Device) -> Present {
    Present {
        can_play: device.can_play,
        can_volume: false,
        family: None,
        follows: None,
        id: device.id.clone(),
        kind: device.kind.clone(),
        muted: false,
        name: device.name.clone(),
        user: device.user.clone(),
        volume: 100,
    }
}

/// Whether the device can have its volume moved: here, and said it has one.
pub fn has_volume(db: &Db<Harken>, user: &str, device: &str) -> bool {
    db.listening_device.get(key(user, device)).is_some_and(|d| d.present.can_volume)
}

/// What a device says of itself once it stands here. A family that moves
/// takes it out of any group it was in.
#[live("listening.equip")]
pub fn equip(ctx: &Ctx, db: &mut Db<Harken>, input: Equip) -> Result<(), Refusal> {
    let (user, me) = (ctx.user(), ctx.device());
    if !db.listening_device.has(key(user, me)) {
        return refuse("not here");
    }
    if present_of(db, user, me)?.family != input.family {
        dissolved(db, user, me)?;
    }
    let present = present_of(db, user, me)?;
    put_present(
        db,
        user,
        me,
        Present {
            can_volume: input.can_volume,
            family: input.family,
            muted: input.muted,
            volume: input.volume,
            ..present
        },
    )
}

/// The guard of a volume moved from anywhere.
fn voiced_device(db: &Db<Harken>, user: &str, device: &str) -> Result<Present, Refusal> {
    if !has_volume(db, user, device) {
        return Err(Refusal::new("that device is not here, or has no volume"));
    }
    present_of(db, user, device)
}

/// A device's volume, from anywhere. Moving the volume unmutes.
#[live("listening.set_volume")]
pub fn set_volume(ctx: &Ctx, db: &mut Db<Harken>, input: SetVolume) -> Result<(), Refusal> {
    let present = voiced_device(db, ctx.user(), &input.device)?;
    let p = Present {
        muted: false,
        volume: input.level,
        ..present
    };
    put_present(db, ctx.user(), &input.device, p)
}

/// …moved by so much, clamped: what a volume button sends.
#[live("listening.nudge_volume")]
pub fn nudge_volume(ctx: &Ctx, db: &mut Db<Harken>, input: NudgeVolume) -> Result<(), Refusal> {
    let present = voiced_device(db, ctx.user(), &input.device)?;
    let p = Present {
        muted: false,
        volume: (present.volume + input.delta).clamp(0, 100),
        ..present
    };
    put_present(db, ctx.user(), &input.device, p)
}

#[live("listening.set_mute")]
pub fn set_mute(ctx: &Ctx, db: &mut Db<Harken>, input: SetMute) -> Result<(), Refusal> {
    let present = voiced_device(db, ctx.user(), &input.device)?;
    let p = Present {
        muted: input.muted,
        ..present
    };
    put_present(db, ctx.user(), &input.device, p)
}

/// The device's own word on its volume: its own entry only.
#[live("listening.report_volume")]
pub fn report_volume(ctx: &Ctx, db: &mut Db<Harken>, input: ReportVolume) -> Result<(), Refusal> {
    if !has_volume(db, ctx.user(), ctx.device()) {
        return refuse("not here, or no volume");
    }
    let present = present_of(db, ctx.user(), ctx.device())?;
    let p = Present {
        muted: input.muted,
        volume: input.level,
        ..present
    };
    put_present(db, ctx.user(), ctx.device(), p)
}

/// Ephemeral: where this device's scrubber is being dragged to, or nowhere
/// once it is let go. It reads nothing ordered and writes its own row.
#[live("listening.scrub")]
pub fn scrub(ctx: &Ctx, db: &mut Db<Harken>, input: Scrub) -> Result<(), Refusal> {
    db.listening_scrub.delete(key(ctx.user(), ctx.device()))?;
    if let Some(at_ms) = input.at_ms {
        db.listening_scrub.insert(ListeningScrub {
            user: ctx.user().into(),
            device: ctx.device().into(),
            at_ms,
        })?;
    }
    Ok(())
}

// * Coming and going

/// A device arriving takes nothing; one the sound stayed with gets it back,
/// still paused.
#[live("listening.on_join")]
pub fn on_join(_: &Ctx, db: &mut Db<Harken>, device: Device) -> Result<(), Refusal> {
    let (user, id) = (device.user.clone(), device.id.clone());
    db.listening_device.delete(key(&user, &id))?;
    db.listening_device.insert(ListeningDevice {
        user: user.clone(),
        device: id.clone(),
        present: arrived(&device),
    })?;
    let session = with_active(&session_of(db, &user), |a| Active {
        output: returned(&a.output, &id),
        ..a.clone()
    });
    if db.listening.has(&user) {
        put_session(db, &user, session)?;
    }
    Ok(())
}

/// A device leaving is gone, and so is its group; the output leaving keeps
/// the sound, away, the clock stopped.
#[live("listening.on_leave")]
pub fn on_leave(ctx: &Ctx, db: &mut Db<Harken>, device: Device) -> Result<(), Refusal> {
    let (user, id) = (device.user.clone(), device.id.clone());
    dissolved(db, &user, &id)?;
    db.listening_device.delete(key(&user, &id))?;
    db.listening_scrub.delete(key(&user, &id))?;
    let session = session_of(db, &user);
    let next = match &session {
        Session::Active { active } if is_output(&session, &id) => Session::Active {
            active: Active {
                clock: halted(&active.clock, &active.output, ctx.now),
                output: Output::Away {
                    id: id.clone(),
                    name: device.name.clone(),
                },
                ..active.clone()
            },
        },
        _ => session.clone(),
    };
    if db.listening.has(&user) {
        put_session(db, &user, next)?;
    }
    Ok(())
}

/// A restart: every queue as it was, paused where it had got to (never past
/// the end of the track), with no output and nobody here. Authored over the
/// ordered tables a restart kept; the scrubbers were never kept.
#[live("listening.on_restore")]
pub fn on_restore(ctx: &Ctx, db: &mut Db<Harken>, _: ()) -> Result<(), Refusal> {
    for d in db.listening_device.scan() {
        db.listening_device.remove(&d)?;
    }
    for row in db.listening.scan() {
        let session = with_active(&row.session, |a| Active {
            clock: Clock::Paused {
                at_ms: standing(&a.clock, &a.output, ctx.now).min(duration_at(&a.queue, a.at)),
            },
            output: Output::Nowhere,
            ..a.clone()
        });
        put_session(db, &row.user, session)?;
    }
    Ok(())
}
