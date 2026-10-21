//! Groups of speakers: a member follows a leader of its family, Sonos's
//! coordinator model. The group a session plays on is the one its output
//! leads.

use ark::prelude::*;

use crate::devices::{devices_of, present_of, put_present};
use crate::listening::*;
use crate::schema::*;
use crate::types::*;

fn refuse(why: &str) -> Result<(), Refusal> {
    Err(Refusal::new(why))
}

/// Whether `present` is in the group `leader` leads: the leader, or
/// following it.
pub fn in_group(present: &Present, leader: &str) -> bool {
    present.id == leader || present.follows.as_deref() == Some(leader)
}

fn entry(db: &Db<Harken>, user: &str, device: &str) -> Option<ListeningDevice> {
    db.listening_device.get((user.to_string(), device.to_string()))
}

/// Whether `id` can be joined to the group `leader` leads now: a speaker
/// that is here, can play, of the leader's family, and not the leader.
pub fn joinable(db: &Db<Harken>, user: &str, leader: &str, id: &str) -> bool {
    let family = entry(db, user, leader).and_then(|e| if e.present.follows.is_none() { e.present.family } else { None });
    entry(db, user, id).is_some_and(|e| {
        let p = &e.present;
        p.id != leader && p.can_play && p.family.is_some() && p.family == family
    })
}

/// The group `leader` leads, those of it that have a volume.
pub fn voiced(db: &Db<Harken>, user: &str, leader: &str) -> Vec<ListeningDevice> {
    devices_of(db, user)
        .into_iter()
        .filter(|e| in_group(&e.present, leader) && e.present.can_volume)
        .collect()
}

pub fn group_tally(db: &Db<Harken>, user: &str, leader: &str) -> Tally {
    let voiced = voiced(db, user, leader);
    Tally {
        n: voiced.len() as i64,
        sum: voiced.iter().map(|e| e.present.volume).sum(),
    }
}

/// The group's volume: the mean of its members', rounded down; 0 for a
/// group with no volume at all.
pub fn group_level(db: &Db<Harken>, user: &str, leader: &str) -> i64 {
    let tally = group_tally(db, user, leader);
    tally.sum / tally.n.max(1)
}

/// One member's level once the group's moves from `from` to `to`: in
/// proportion, rounded to the nearest, clamped — and from a group at 0,
/// `to` itself.
pub fn scaled(level: i64, from: i64, to: i64) -> i64 {
    if from == 0 {
        to
    } else {
        ((level * to * 2 + from) / (from.max(1) * 2)).clamp(0, 100)
    }
}

/// Every device of the account's for which `f` says a new entry, written.
fn each(db: &mut Db<Harken>, user: &str, devices: Vec<ListeningDevice>, f: impl Fn(&Present) -> Option<Present>) -> Result<(), Refusal> {
    for e in devices {
        if let Some(p) = f(&e.present) {
            put_present(db, user, &e.device, p)?;
        }
    }
    Ok(())
}

/// The group `leader` leads at the level `to`: every member with a volume
/// moved in proportion and unmuted.
pub fn group_at(db: &mut Db<Harken>, user: &str, leader: &str, to: i64) -> Result<(), Refusal> {
    let from = group_level(db, user, leader);
    let members = voiced(db, user, leader);
    each(db, user, members, |p| {
        Some(Present {
            muted: false,
            volume: scaled(p.volume, from, to),
            ..p.clone()
        })
    })
}

/// The group `leader` leads, every member with a volume muted or not.
pub fn group_muted(db: &mut Db<Harken>, user: &str, leader: &str, muted: bool) -> Result<(), Refusal> {
    let members = voiced(db, user, leader);
    each(db, user, members, |p| Some(Present { muted, ..p.clone() }))
}

/// `id` follows nobody, and nobody follows it.
pub fn dissolved(db: &mut Db<Harken>, user: &str, id: &str) -> Result<(), Refusal> {
    let members = devices_of(db, user);
    each(db, user, members, |p| {
        (p.id == id || p.follows.as_deref() == Some(id)).then(|| Present { follows: None, ..p.clone() })
    })
}

/// `id` follows `leader`; whoever followed `id` follows nobody.
pub fn followed(db: &mut Db<Harken>, user: &str, id: &str, leader: &str) -> Result<(), Refusal> {
    let all = devices_of(db, user);
    each(db, user, all, |p| {
        if p.id == id {
            Some(Present {
                follows: Some(leader.into()),
                ..p.clone()
            })
        } else if p.follows.as_deref() == Some(id) {
            Some(Present { follows: None, ..p.clone() })
        } else {
            None
        }
    })
}

/// The sound handed to `to`: if it was following a leader, it leads that
/// group now, as Sonos makes a member the coordinator when it is picked.
pub fn led_by(db: &mut Db<Harken>, user: &str, to: &str) -> Result<(), Refusal> {
    let was = entry(db, user, to).and_then(|e| e.present.follows);
    let all = devices_of(db, user);
    each(db, user, all, |p| {
        if p.id == to {
            Some(Present { follows: None, ..p.clone() })
        } else if was.is_some() && (Some(&p.id) == was.as_ref() || p.follows == was) {
            Some(Present {
                follows: Some(to.into()),
                ..p.clone()
            })
        } else {
            None
        }
    })
}

/// The leader `caller`'s word on who follows it: each member that is here
/// and of its family follows it; whoever followed it and is not named, or
/// followed a device that is a member now, follows nobody.
pub fn regrouped(db: &mut Db<Harken>, user: &str, caller: &str, members: &[String]) -> Result<(), Refusal> {
    let family = entry(db, user, caller).and_then(|e| e.present.family);
    let all = devices_of(db, user);
    each(db, user, all, |p| {
        let joins = members.contains(&p.id) && p.id != caller && p.can_play && family.is_some() && p.family == family;
        let led_by_a_member = p.follows.as_ref().is_some_and(|id| members.contains(id));
        let follows = if p.id == caller {
            None
        } else if joins {
            Some(caller.to_string())
        } else if p.follows.as_deref() == Some(caller) || led_by_a_member {
            None
        } else {
            p.follows.clone()
        };
        Some(Present { follows, ..p.clone() })
    })
}

/// Whether the output is a device that is here and something in its group
/// has a volume: what every group transition is guarded by.
pub fn group_has_volume(db: &Db<Harken>, user: &str) -> bool {
    output_id(&session_of(db, user)).is_some_and(|leader| group_tally(db, user, &leader).n > 0)
}

/// The output's id: the leader the group transitions move.
pub fn leader(db: &Db<Harken>, user: &str) -> Result<String, Refusal> {
    output_id(&session_of(db, user)).ok_or_else(|| Refusal::new("unwrapped none"))
}

/// A speaker joins the output's group; whatever followed it follows nobody.
#[live("listening.join")]
pub fn join(ctx: &Ctx, db: &mut Db<Harken>, input: Member) -> Result<(), Refusal> {
    let Some(leader) = output_id(&session_of(db, ctx.user())) else {
        return refuse("nothing is playing to join");
    };
    if !joinable(db, ctx.user(), &leader, &input.device) {
        return refuse("that device cannot join this one");
    }
    followed(db, ctx.user(), &input.device, &leader)
}

/// …and leaves whatever group it is in.
#[live("listening.unjoin")]
pub fn unjoin(ctx: &Ctx, db: &mut Db<Harken>, input: Member) -> Result<(), Refusal> {
    if !entry(db, ctx.user(), &input.device).is_some_and(|e| e.present.follows.is_some()) {
        return refuse("that device is not in a group");
    }
    let present = present_of(db, ctx.user(), &input.device)?;
    put_present(db, ctx.user(), &input.device, Present { follows: None, ..present })
}

/// A leader's word on who follows it, changed outside harken.
#[live("listening.report_group")]
pub fn report_group(ctx: &Ctx, db: &mut Db<Harken>, input: ReportGroup) -> Result<(), Refusal> {
    if !entry(db, ctx.user(), ctx.device()).is_some_and(|e| e.present.family.is_some()) {
        return refuse("not here, or not a speaker that groups");
    }
    regrouped(db, ctx.user(), ctx.device(), &input.members)
}

/// The guard of every group transition.
fn grouped(ctx: &Ctx, db: &Db<Harken>) -> Result<(), Refusal> {
    if !group_has_volume(db, ctx.user()) {
        return refuse("nothing playing has a volume");
    }
    Ok(())
}

/// The output's group at a level, every member in proportion, unmuted.
#[live("listening.set_group_volume")]
pub fn set_group_volume(ctx: &Ctx, db: &mut Db<Harken>, input: GroupVolume) -> Result<(), Refusal> {
    grouped(ctx, db)?;
    let leader = leader(db, ctx.user())?;
    group_at(db, ctx.user(), &leader, input.level)
}

#[live("listening.nudge_group_volume")]
pub fn nudge_group_volume(ctx: &Ctx, db: &mut Db<Harken>, input: GroupNudge) -> Result<(), Refusal> {
    grouped(ctx, db)?;
    let leader = leader(db, ctx.user())?;
    let to = (group_level(db, ctx.user(), &leader) + input.delta).clamp(0, 100);
    group_at(db, ctx.user(), &leader, to)
}

#[live("listening.set_group_mute")]
pub fn set_group_mute(ctx: &Ctx, db: &mut Db<Harken>, input: GroupMute) -> Result<(), Refusal> {
    grouped(ctx, db)?;
    let leader = leader(db, ctx.user())?;
    group_muted(db, ctx.user(), &leader, input.muted)
}
