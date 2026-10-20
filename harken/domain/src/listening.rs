//! One account, one thing playing, however many devices are watching it —
//! as live state over volatile tables (§1.9) — and the plays worth keeping,
//! as the log (`harken/domain/src/listening.rs`, transition for transition).
//!
//! A person with a phone, a laptop and a browser tab has **one** listening
//! session. Any of them can change it; exactly one of them is making the
//! sound. **The state is the instruction**: nothing routes a command to the
//! device making the sound; the output reconciles its player against the
//! session and reports what happens to it (`started`, `advance`,
//! `stalled`, `release`, `report_position`) and nothing else.
//!
//! **None of the session is in the log.** It is three volatile tables
//! (`crate::schema`): `listening`, one row per account, the session (no
//! row is `Idle`); `listening_device`, one row per device standing in it,
//! with its volume and its group — both ordered; and `listening_scrub`,
//! where each device drags its scrubber — ephemeral, written only by its
//! own device and read by nothing ordered. A transition is a live
//! mutator over them keyed by the account (`ctx.user`, v4's
//! `scope(|ctx| ctx.user)`); v4's guards are its first refusals, in their
//! order. The calling device is `ctx.session` — a device is a session
//! until the context carries one (`Ctx(device)` is not in v5 yet).
//! `on_join` and `on_leave` are entries the authority authors as devices
//! come and go, the device's record their input; `on_restore` is the one
//! it authors after a restart.
//!
//! The four rules of v4's desk are properties of the transitions: one
//! output (`Active.output` is one value); the output survives its socket
//! (`on_leave` keeps it `Away`, paused; `on_join` gives it back, paused); a
//! command is a state change the output obeys (`pause`, `seek`,
//! `transfer` move nothing but the session); a press that means a sound
//! takes an output that cannot answer, and a device arriving takes
//! nothing. Volume is per device and so is a group: a member follows a
//! leader of its family, Sonos's coordinator model.
//!
//! [`read`] is the state as plain Rust, for a player; [`url`] joins a
//! track's file to a server.

use std::collections::BTreeMap;

use ark::author::*;

use crate::schema::*;

// * The state

/// A device standing in the session: what it said it is when it came, what
/// it can do, and where its volume and its group are.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Present {
    pub can_play: bool,
    /// Whether anybody can move its volume: what it said of itself
    /// (`equip`). Nothing has a volume until it says so.
    pub can_volume: bool,
    /// Which speakers it can be joined with; none for anything that is not
    /// a speaker that groups.
    pub family: Option<Text>,
    /// The leader whose playback it follows, while it is joined to one.
    pub follows: Option<Text>,
    pub id: Text,
    pub kind: Text,
    pub muted: bool,
    pub name: Text,
    pub user: Text,
    /// 0 to 100.
    pub volume: Int,
}

/// The level a device that has said nothing of its volume is drawn at.
pub const UNSAID_VOLUME: i64 = 100;

/// Something queued, or nothing.
#[derive(Sum, Node, Clone, Debug, PartialEq, Eq)]
pub enum Session {
    Active(Active),
    Idle,
}

/// A queue, where in it, whether it is playing, and on which device.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Active {
    /// The track playing: always inside the queue.
    pub at: Int,
    pub clock: Clock,
    /// Where the queue was taken from, as the client that pressed it names
    /// it (`album/Water Music`); empty for nowhere in particular.
    pub from: Text,
    pub output: Output,
    /// Never empty: every transition that could empty it goes `Idle`.
    pub queue: Vec<Track>,
}

/// How far into the track: paused at a point, or playing from a point
/// since a moment of the server's clock.
#[derive(Sum, Node, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    Paused { at_ms: Int },
    Playing { at_ms: Int, since: Int },
}

/// Which device the sound belongs to: one that has gone and keeps it (its
/// name kept beside its id, for the picker), one that is here (and, until
/// it says it has started, `moving`), or none.
#[derive(Sum, Node, Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Away { id: Text, name: Text },
    Device { id: Text, moving: bool },
    Nowhere,
}

/// A track, carried rather than looked up: everything needed to play it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub album: Text,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub title: Text,
}

/// A device as it introduces itself: what `on_join` and `on_leave` are
/// told.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub can_play: bool,
    pub id: Text,
    pub kind: Text,
    pub name: Text,
    pub user: Text,
}

// * The inputs

/// `NonEmpty<Track>`.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
#[ark(non_empty)]
pub struct Queue(pub Vec<Track>);

/// An `Int` from 0.
#[derive(Input, Node, Clone, Copy, Debug, PartialEq, Eq)]
#[ark(min = 0)]
pub struct Ms(pub Int);

/// A speaker family's name: at least one character.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
#[ark(len = "1..")]
pub struct Family(pub Text);

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Start {
    #[ark(min = 0)]
    pub at: Int,
    pub from: Text,
    #[ark(min = 0)]
    pub position: Int,
    pub queue: Queue,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Seek {
    #[ark(min = 0)]
    pub ms: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    #[ark(range = "-1..=1", why = "a skip is one track either way", ne = 0, why = "a skip is one track either way")]
    pub by: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Jump {
    #[ark(min = 0)]
    pub index: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub to: Text,
}

/// What the output says of where it is: `started` and `stalled`.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct At {
    #[ark(min = 0)]
    pub at_ms: Int,
}

/// Where the output, playing, says it is, against the clock that started
/// playing at `since`.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Report {
    #[ark(min = 0)]
    pub at_ms: Int,
    pub since: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Advance {
    pub from_track: Int,
}

/// `enqueue`: tracks put after the one playing, or at the end.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Enqueue {
    pub next: bool,
    pub tracks: Queue,
}

/// `remove`: a track of the queue, by where it is in it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Remove {
    #[ark(min = 0)]
    pub index: Int,
}

/// `move`: a track of the queue from one place in it to another.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Move {
    #[ark(min = 0)]
    pub from: Int,
    #[ark(min = 0)]
    pub to: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Scrub {
    pub at_ms: Option<Ms>,
}

/// What a device says of itself once it stands in the session.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Equip {
    pub can_volume: bool,
    pub family: Option<Family>,
    pub muted: bool,
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub volume: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct SetVolume {
    pub device: Text,
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub level: Int,
}

/// A device's level moved by `delta`, clamped: what a volume button sends.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct NudgeVolume {
    #[ark(range = "-100..=100", why = "between -100 and 100", ne = 0, why = "a nudge moves the volume")]
    pub delta: Int,
    pub device: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct SetMute {
    pub device: Text,
    pub muted: bool,
}

/// The device's own word on its volume.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct ReportVolume {
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub level: Int,
    pub muted: bool,
}

/// `join` and `unjoin`: which speaker.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub device: Text,
}

/// The leader's word on who follows it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct ReportGroup {
    pub members: Vec<Text>,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct GroupVolume {
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub level: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct GroupNudge {
    #[ark(range = "-100..=100", why = "between -100 and 100", ne = 0, why = "a nudge moves the volume")]
    pub delta: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct GroupMute {
    pub muted: bool,
}

// * The helpers

/// The tables, as a body reads them.
fn db() -> Harken {
    Harken::db()
}

// * The helpers

/// How far into the track a clock is at `now`.
pub static POSITION: Def2<Clock, Int, Int> = Def2 {
    name: "listening.position",
    kind: Kind::Helper,
    body: |clock, now| ClockEx::match_(&clock, |at_ms| at_ms, |at_ms, since| (&at_ms + (&now - since)).max(&at_ms)),
};

/// Where the session stands at `now`: its clock's position — except while
/// the sound is on its way to a device (`moving`), when it stands where it
/// was handed over, since nothing is making a sound in between.
pub static STANDING: Def3<Clock, Output, Int, Int> = Def3 {
    name: "listening.standing",
    kind: Kind::Helper,
    body: |clock, output, now| {
        block(|b| {
            let moving = b.let_(OutputEx::match_(&output, |_, _| bool(false), |_, moving| moving, || bool(false)));
            let anchor = b.let_(ClockEx::match_(&clock, |at_ms| at_ms, |at_ms, _| at_ms));
            if_(moving, anchor, POSITION.call(&clock, &now))
        })
    },
};

/// The clock paused where it is at `now`.
pub static PAUSED_AT: Def2<Clock, Int, Clock> = Def2 {
    name: "listening.paused_at",
    kind: Kind::Helper,
    body: |clock, now| ClockEx::paused(POSITION.call(clock, now)),
};

/// The session's clock paused where the session stands at `now`.
pub static HALTED: Def3<Clock, Output, Int, Clock> = Def3 {
    name: "listening.halted",
    kind: Kind::Helper,
    body: |clock, output, now| ClockEx::paused(STANDING.call(clock, output, now)),
};

/// The clock at `ms`, playing on from there if it was playing.
pub static CLOCK_AT: Def3<Clock, Int, Int, Clock> = Def3 {
    name: "listening.clock_at",
    kind: Kind::Helper,
    body: |clock, ms, now| ClockEx::match_(&clock, |_| ClockEx::paused(&ms), |_, _| ClockEx::playing(&ms, &now)),
};

/// Whether the output is a device that is here.
pub static IS_HERE: Def1<Output, bool> = Def1 {
    name: "listening.is_here",
    kind: Kind::Helper,
    body: |output| OutputEx::match_(&output, |_, _| bool(false), |_, _| bool(true), || bool(false)),
};

/// Whether something is queued.
pub static IS_ACTIVE: Def1<Session, bool> = Def1 {
    name: "listening.is_active",
    kind: Kind::Helper,
    body: |session| SessionEx::match_(&session, |_| bool(true), || bool(false)),
};

/// Whether the session has an output that is here to answer.
pub static REACHABLE: Def1<Session, bool> = Def1 {
    name: "listening.reachable",
    kind: Kind::Helper,
    body: |session| SessionEx::match_(&session, |a| IS_HERE.call(a.f().output), || bool(false)),
};

/// Whether something is making a sound: a clock running on an output that
/// is here.
pub static SOUNDING: Def1<Session, bool> = Def1 {
    name: "listening.sounding",
    kind: Kind::Helper,
    body: |session| {
        SessionEx::match_(
            &session,
            |a| {
                IS_HERE
                    .call(a.f().output)
                    .and(ClockEx::match_(&a.f().clock, |_| bool(false), |_, _| bool(true)))
            },
            || bool(false),
        )
    },
};

/// The account's session: its row's, or `Idle`.
pub static SESSION_OF: Def1<Text, Session> = Def1 {
    name: "listening.session_of",
    kind: Kind::Helper,
    body: |user| db().listening.get(user).map_or(SessionEx::idle(), |r| r.f().session),
};

/// The account's session, written.
pub static PUT_SESSION: Def2<Text, Session, ()> = Def2 {
    name: "listening.put_session",
    kind: Kind::Helper,
    body: |user, session| {
        let db = db();
        if_(
            db.listening.get(&user).is_some(),
            db.listening
                .update(&user, |r| {
                    ListeningEx {
                        session: session.clone(),
                        ..r.f()
                    }
                    .into()
                })
                .s(),
            db.listening.insert(ListeningEx { user, session }).s(),
        )
    },
};

/// The devices standing in the account's session, in id order.
pub static DEVICES_OF: Def1<Text, Vec<ListeningDevice>> = Def1 {
    name: "listening.devices_of",
    kind: Kind::Helper,
    body: |user| db().listening_device.filter(|r| r.f().user.eq(&user)),
};

/// A device standing in the session; refused when it is not (every caller
/// has held it there first).
pub static PRESENT_OF: Def2<Text, Text, Present> = Def2 {
    name: "listening.present_of",
    kind: Kind::Helper,
    body: |user, id| db().listening_device.get((user, id)).unwrap().f().present,
};

/// A device's entry, written.
pub static PUT_PRESENT: Def3<Text, Text, Present, ()> = Def3 {
    name: "listening.put_present",
    kind: Kind::Helper,
    body: |user, id, present| {
        db().listening_device
            .update((user, id), |r| ListeningDeviceEx { present, ..r.f() }.into())
            .s()
    },
};

/// Whether the device is here and can make a sound.
pub static CAN_PLAY: Def2<Text, Text, bool> = Def2 {
    name: "listening.can_play",
    kind: Kind::Helper,
    body: |user, id| db().listening_device.get((user, id)).map_or(false, |d| d.f().present.f().can_play),
};

/// Whether the device is the session's output and here.
pub static IS_OUTPUT: Def2<Session, Text, bool> = Def2 {
    name: "listening.is_output",
    kind: Kind::Helper,
    body: |session, id| {
        SessionEx::match_(
            &session,
            |a| OutputEx::match_(&a.f().output, |_, _| bool(false), |d, _| d.eq(&id), || bool(false)),
            || bool(false),
        )
    },
};

/// The sound on this device, which has it.
pub static HERE: Def1<Text, Output> = Def1 {
    name: "listening.here",
    kind: Kind::Helper,
    body: |id| OutputEx::device(id, false),
};

/// The output a press that means a sound leaves: the one there is, where it
/// can answer, or the caller.
pub static PRESSED: Def2<Output, Text, Output> = Def2 {
    name: "listening.pressed",
    kind: Kind::Helper,
    body: |output, caller| if_(IS_HERE.call(&output), output, HERE.call(caller)),
};

/// The output once `id` has come back: given back to it, if it was the one
/// that went.
pub static RETURNED: Def2<Output, Text, Output> = Def2 {
    name: "listening.returned",
    kind: Kind::Helper,
    body: |output, id| {
        OutputEx::match_(
            &output,
            |a, _| if_(a.eq(&id), HERE.call(&a), output.clone()),
            |_, _| output.clone(),
            || output.clone(),
        )
    },
};

/// How long the track at `at` is; 0 for no track there.
pub static DURATION_AT: Def2<Vec<Track>, Int, Int> = Def2 {
    name: "listening.duration_at",
    kind: Kind::Helper,
    body: |queue, at| {
        if_(
            at.ge(0).and(at.lt(queue.len())),
            queue.slice(&at, &at + 1).first().map_or(0, |t| t.f().duration_ms),
            int(0),
        )
    },
};

/// A session at a new place in its queue, playing from the start of it, on
/// whichever device a press leaves the sound with.
pub static MOVED_TO: Def4<Active, Int, Text, Int, Session> = Def4 {
    name: "listening.moved_to",
    kind: Kind::Helper,
    body: |a, at, caller, now| {
        SessionEx::active(ActiveEx {
            at,
            clock: ClockEx::playing(0, now),
            output: PRESSED.call(a.f().output, caller),
            ..a.f()
        })
    },
};

/// Where the track at `i` goes when the one at `from` moves to `to`.
pub static MOVED_PLACE: Def3<Int, Int, Int, Int> = Def3 {
    name: "listening.moved_place",
    kind: Kind::Helper,
    // ArkLang: the three cases read plainer than nested `if_`s.
    body: |i, from, to| {
        text(
            "(if (== i from) to
               (if (and (and (< from to) (> i from)) (<= i to)) (- i 1)
                 (if (and (and (< to from) (>= i to)) (< i from)) (+ i 1) i)))",
            &[("i", &i), ("from", &from), ("to", &to)],
        )
    },
};

/// Whether `index` is a place in the session's queue.
pub static IN_QUEUE: Def2<Session, Int, bool> = Def2 {
    name: "listening.in_queue",
    kind: Kind::Helper,
    body: |session, index| SessionEx::match_(&session, |a| index.lt(a.f().queue.len()), || bool(false)),
};

/// A device as `on_join` first writes it: what it declared, and nothing it
/// has not said.
pub static ARRIVED: Def1<Device, Present> = Def1 {
    name: "listening.arrived",
    kind: Kind::Helper,
    body: |device| {
        let d = device.f();
        PresentEx {
            can_play: d.can_play,
            can_volume: bool(false),
            family: none(),
            follows: none(),
            id: d.id,
            kind: d.kind,
            muted: bool(false),
            name: d.name,
            user: d.user,
            volume: int(100),
        }
        .into()
    },
};

/// The output, where it is a device that is here: the one a group is led by.
pub static OUTPUT_ID: Def1<Session, Option<Text>> = Def1 {
    name: "listening.output_id",
    kind: Kind::Helper,
    body: |session| SessionEx::match_(&session, |a| OutputEx::match_(&a.f().output, |_, _| none(), |id, _| some(id), none), none),
};

/// Whether the device can have its volume moved: here, and said it has one.
pub static HAS_VOLUME: Def2<Text, Text, bool> = Def2 {
    name: "listening.has_volume",
    kind: Kind::Helper,
    body: |user, id| db().listening_device.get((user, id)).map_or(false, |d| d.f().present.f().can_volume),
};

/// Whether `d` is in the group `leader` leads: the leader, or following it.
pub static IN_GROUP: Def2<Present, Text, bool> = Def2 {
    name: "listening.in_group",
    kind: Kind::Helper,
    body: |d, leader| d.f().id.eq(&leader).or(d.f().follows.eq(some(leader))),
};

/// Whether `id` can be joined to the group `leader` leads now: a speaker
/// that is here, can play, of the leader's family, and not the leader.
pub static JOINABLE: Def3<Text, Text, Text, bool> = Def3 {
    name: "listening.joinable",
    kind: Kind::Helper,
    body: |user, leader, id| {
        let db = db();
        block(|b| {
            let family = b.let_(db.listening_device.get((user.clone(), leader.clone())).map_or(none(), |l| {
                let p = l.f().present.f();
                if_(p.follows.is_none(), p.family, none())
            }));
            db.listening_device.get((user, id)).map_or(false, |d| {
                let p = d.f().present.f();
                p.id.ne(&leader).and(p.can_play).and(p.family.is_some()).and(p.family.eq(&family))
            })
        })
    },
};

/// The group `leader` leads, those of it that have a volume.
pub static VOICED: Def2<Text, Text, Vec<ListeningDevice>> = Def2 {
    name: "listening.voiced",
    kind: Kind::Helper,
    body: |user, leader| {
        DEVICES_OF
            .call(user)
            .filter(|r| IN_GROUP.call(r.f().present, &leader).and(r.f().present.f().can_volume))
    },
};

pub static GROUP_TALLY: Def2<Text, Text, Tally> = Def2 {
    name: "listening.group_tally",
    kind: Kind::Helper,
    body: |user, leader| {
        block(|b| {
            let members = b.let_(VOICED.call(user, leader));
            TallyEx {
                n: members.len(),
                sum: members.map(|r| r.f().present.f().volume).sum(),
            }
            .into()
        })
    },
};

/// The group's volume: the mean of its members', rounded down; 0 for a
/// group with no volume at all.
pub static GROUP_LEVEL: Def2<Text, Text, Int> = Def2 {
    name: "listening.group_level",
    kind: Kind::Helper,
    body: |user, leader| {
        block(|b| {
            let t = b.let_(GROUP_TALLY.call(user, leader));
            t.f().sum / t.f().n.max(1)
        })
    },
};

/// One member's level once the group's moves from `from` to `to`: in
/// proportion, rounded to the nearest, clamped — and from a group at 0,
/// `to` itself.
pub static SCALED: Def3<Int, Int, Int, Int> = Def3 {
    name: "listening.scaled",
    kind: Kind::Helper,
    // ArkLang: the arithmetic in one form.
    body: |volume, from, to| {
        text(
            "(if (== from 0) to (clamp (/ (+ (* (* volume to) 2) from) (* (max from 1) 2)) 0 100))",
            &[("volume", &volume), ("from", &from), ("to", &to)],
        )
    },
};

/// The group `leader` leads at the level `to`: every member with a volume
/// moved in proportion and unmuted.
pub static GROUP_AT: Def3<Text, Text, Int, ()> = Def3 {
    name: "listening.group_at",
    kind: Kind::Helper,
    body: |user, leader, to| {
        block(|b| {
            let from = b.let_(GROUP_LEVEL.call(&user, &leader));
            VOICED
                .call(&user, &leader)
                .map(|r| {
                    let p = r.f().present;
                    let present = PresentEx {
                        volume: SCALED.call(p.f().volume, &from, &to),
                        muted: bool(false),
                        ..p.f()
                    };
                    PUT_PRESENT.call(&user, r.f().device, present)
                })
                .s()
        })
    },
};

/// The group `leader` leads, every member with a volume muted or not.
pub static GROUP_MUTED: Def3<Text, Text, bool, ()> = Def3 {
    name: "listening.group_muted",
    kind: Kind::Helper,
    body: |user, leader, muted| {
        VOICED
            .call(&user, leader)
            .map(|r| PUT_PRESENT.call(&user, r.f().device, PresentEx { muted, ..r.f().present.f() }))
            .s()
    },
};

/// `id` follows nobody, and nobody follows it.
pub static DISSOLVED: Def2<Text, Text, ()> = Def2 {
    name: "listening.dissolved",
    kind: Kind::Helper,
    body: |user, id| {
        DEVICES_OF
            .call(&user)
            .filter(|r| r.f().present.f().id.eq(&id).or(r.f().present.f().follows.eq(some(&id))))
            .map(|r| {
                PUT_PRESENT.call(
                    &user,
                    r.f().device,
                    PresentEx {
                        follows: none(),
                        ..r.f().present.f()
                    },
                )
            })
            .s()
    },
};

/// `id` follows `leader`; whoever followed `id` follows nobody.
pub static FOLLOWED: Def3<Text, Text, Text, ()> = Def3 {
    name: "listening.followed",
    kind: Kind::Helper,
    body: |user, id, leader| {
        DEVICES_OF
            .call(&user)
            .map(|r| {
                let p = r.f().present.f();
                if_(
                    p.id.eq(&id),
                    PUT_PRESENT.call(
                        &user,
                        r.f().device,
                        PresentEx {
                            follows: some(&leader),
                            ..p.clone()
                        },
                    ),
                    if_(
                        p.follows.eq(some(&id)),
                        PUT_PRESENT.call(
                            &user,
                            r.f().device,
                            PresentEx {
                                follows: none(),
                                ..p.clone()
                            },
                        ),
                        unit(),
                    ),
                )
            })
            .s()
    },
};

/// The sound handed to `to`: if it was following a leader, it leads that
/// group now, as Sonos makes a member the coordinator when it is picked.
pub static LED_BY: Def2<Text, Text, ()> = Def2 {
    name: "listening.led_by",
    kind: Kind::Helper,
    body: |user, to| {
        let db = db();
        block(|b| {
            let old = b.let_(
                db.listening_device
                    .get((user.clone(), to.clone()))
                    .map_or(none(), |t| t.f().present.f().follows),
            );
            DEVICES_OF
                .call(&user)
                .map(|r| {
                    let p = r.f().present.f();
                    if_(
                        p.id.eq(&to),
                        PUT_PRESENT.call(
                            &user,
                            r.f().device,
                            PresentEx {
                                follows: none(),
                                ..p.clone()
                            },
                        ),
                        if_(
                            old.is_some().and(some(&p.id).eq(&old).or(p.follows.eq(&old))),
                            PUT_PRESENT.call(
                                &user,
                                r.f().device,
                                PresentEx {
                                    follows: some(&to),
                                    ..p.clone()
                                },
                            ),
                            unit(),
                        ),
                    )
                })
                .s()
        })
    },
};

/// The leader `caller`'s word on who follows it: each member that is here
/// and of its family follows it; whoever followed it and is not named, or
/// followed a device that is a member now, follows nobody.
pub static REGROUPED: Def3<Text, Text, Vec<Text>, ()> = Def3 {
    name: "listening.regrouped",
    kind: Kind::Helper,
    body: |user, caller, members| {
        let db = db();
        block(|b| {
            let family = b.let_(
                db.listening_device
                    .get((user.clone(), caller.clone()))
                    .map_or(none(), |c| c.f().present.f().family),
            );
            DEVICES_OF
                .call(&user)
                .map(|r| {
                    block(|b| {
                        let d = b.let_(r.f().present);
                        let p = d.f();
                        let named = b.let_(
                            members
                                .contains(&p.id)
                                .and(p.id.ne(&caller))
                                .and(p.can_play.clone())
                                .and(family.is_some())
                                .and(p.family.eq(&family)),
                        );
                        let behind = b.let_(p.follows.map_or(false, |l| members.contains(l)));
                        let follows = b.let_(if_(
                            p.id.eq(&caller),
                            none(),
                            if_(
                                named,
                                some(&caller),
                                if_(p.follows.eq(some(&caller)).or(behind), none(), p.follows.clone()),
                            ),
                        ));
                        PUT_PRESENT.call(&user, r.f().device, PresentEx { follows, ..d.f() })
                    })
                })
                .s()
        })
    },
};

/// Whether the output is a device that is here and something in its group
/// has a volume: what every group transition is guarded by.
pub static GROUP_HAS_VOLUME: Def1<Text, bool> = Def1 {
    name: "listening.group_has_volume",
    kind: Kind::Helper,
    body: |user| {
        OUTPUT_ID
            .call(SESSION_OF.call(&user))
            .map_or(false, |leader| GROUP_TALLY.call(&user, leader).f().n.gt(0))
    },
};

/// The output's id: the leader the group transitions move.
pub static LEADER: Def1<Text, Text> = Def1 {
    name: "listening.leader",
    kind: Kind::Helper,
    body: |user| OUTPUT_ID.call(SESSION_OF.call(user)).unwrap(),
};

/// The session with its active arm moved by `f`'s result, `Idle` kept.
pub static HALT: Def2<Session, Int, Session> = Def2 {
    name: "listening.halt",
    kind: Kind::Helper,
    body: |session, now| {
        SessionEx::match_(
            &session,
            |a| {
                SessionEx::active(ActiveEx {
                    clock: HALTED.call(a.f().clock, a.f().output, &now),
                    ..a.f()
                })
            },
            SessionEx::idle,
        )
    },
};

// * The transitions

/// `if c { refuse why }`, a guard's statement.
fn unless(b: &mut Block, c: Ex<bool>, why: &str) {
    b.s(if_(!c, refuse_(why), unit()))
}

/// The caller's session `s`, each arm of it: what most transitions put.
fn per<R>(s: &Ex<Session>, active: impl FnOnce(Ex<Active>) -> Ex<R>, idle: impl FnOnce() -> Ex<R>) -> Ex<R> {
    SessionEx::match_(s, active, idle)
}

/// A track pressed: this queue, from here, at this point, playing. While
/// something is sounding the press goes to the device making it; otherwise
/// the caller takes the sound if it can play.
pub static START: Def1<Start, ()> = Def1 {
    name: "listening.start",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, i.at.lt(i.queue.len()), "the track is not in the queue");
            let can = || CAN_PLAY.call(Ctx.user(), Ctx.session());
            unless(b, SOUNDING.call(&s).or(REACHABLE.call(&s)).or(can()), "nothing here can play it");
            let keep = b.let_(SOUNDING.call(&s).or(!can()));
            let output = b.let_(per(
                &s,
                |a| if_(&keep, a.f().output, HERE.call(Ctx.session())),
                || HERE.call(Ctx.session()),
            ));
            let active = ActiveEx {
                at: i.at.clone(),
                clock: ClockEx::playing(&i.position, Ctx.now()),
                from: i.from.clone(),
                output,
                queue: i.queue.clone(),
            };
            PUT_SESSION.call(Ctx.user(), SessionEx::active(active))
        })
    },
};

/// Resume, on the output where it can answer, else on the caller.
pub static PLAY: Def0<()> = Def0 {
    name: "listening.play",
    kind: Kind::Live,
    body: || {
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            unless(
                b,
                REACHABLE.call(&s).or(CAN_PLAY.call(Ctx.user(), Ctx.session())),
                "nothing here can play it",
            );
            let session = b.let_(per(
                &s,
                |a| {
                    let x = a.f();
                    SessionEx::active(ActiveEx {
                        clock: ClockEx::playing(STANDING.call(&x.clock, &x.output, Ctx.now()), Ctx.now()),
                        output: PRESSED.call(&x.output, Ctx.session()),
                        ..a.f()
                    })
                },
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// Pause, wherever the sound is. Nobody takes anything.
pub static PAUSE: Def0<()> = Def0 {
    name: "listening.pause",
    kind: Kind::Live,
    body: || {
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            PUT_SESSION.call(Ctx.user(), HALT.call(&s, Ctx.now()))
        })
    },
};

/// Somewhere else in the track, playing on if it was.
pub static SEEK: Def1<Seek, ()> = Def1 {
    name: "listening.seek",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            let session = b.let_(per(
                &s,
                |a| {
                    SessionEx::active(ActiveEx {
                        clock: CLOCK_AT.call(a.f().clock, input.f().ms, Ctx.now()),
                        ..a.f()
                    })
                },
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// The next track or the one before, from its start; before the first is
/// the first again. A press that means a sound.
pub static SKIP: Def1<Skip, ()> = Def1 {
    name: "listening.skip",
    kind: Kind::Live,
    body: |input| {
        let by = input.f().by;
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            let after = b.let_(per(&s, |a| by.lt(0).or((a.f().at + 1).lt(a.f().queue.len())), || bool(false)));
            unless(b, after, "nothing comes after this");
            unless(
                b,
                REACHABLE.call(&s).or(CAN_PLAY.call(Ctx.user(), Ctx.session())),
                "nothing here can play it",
            );
            let session = b.let_(per(
                &s,
                |a| MOVED_TO.call(&a, (a.f().at + &by).max(0), Ctx.session(), Ctx.now()),
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// A track of the queue, from its start. A press that means a sound.
pub static JUMP: Def1<Jump, ()> = Def1 {
    name: "listening.jump",
    kind: Kind::Live,
    body: |input| {
        let index = input.f().index;
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            unless(b, IN_QUEUE.call(&s, &index), "the track is not in the queue");
            unless(
                b,
                REACHABLE.call(&s).or(CAN_PLAY.call(Ctx.user(), Ctx.session())),
                "nothing here can play it",
            );
            let session = b.let_(per(&s, |a| MOVED_TO.call(a, &index, Ctx.session(), Ctx.now()), SessionEx::idle));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// Move the sound to a device that is here and can play: `moving` until it
/// says it has started, the clock standing where it stopped meanwhile.
/// Handing it to the device that already has it, settled, changes nothing.
pub static TRANSFER: Def1<Transfer, ()> = Def1 {
    name: "listening.transfer",
    kind: Kind::Live,
    body: |input| {
        let to = input.f().to;
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is playing to move");
            unless(b, CAN_PLAY.call(Ctx.user(), &to), "that device is not here, or cannot play");
            b.s(LED_BY.call(Ctx.user(), &to));
            let session = b.let_(per(
                &s,
                |a| {
                    block(|b| {
                        let same = b.let_(a.f().output.eq(HERE.call(&to)));
                        let x = a.f();
                        let moved = CLOCK_AT.call(&x.clock, STANDING.call(&x.clock, &x.output, Ctx.now()), Ctx.now());
                        SessionEx::active(ActiveEx {
                            clock: if_(&same, x.clock.clone(), moved),
                            output: if_(&same, x.output.clone(), OutputEx::device(&to, true)),
                            ..a.f()
                        })
                    })
                },
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// Nothing queued, nothing playing, nowhere.
pub static STOP: Def0<()> = Def0 {
    name: "listening.stop",
    kind: Kind::Live,
    body: || PUT_SESSION.call(Ctx.user(), SessionEx::idle()),
};

/// Tracks put after the one playing ("Play next") or at the end ("Add to
/// queue"); into nothing queued it is `start` of them from the first.
pub static ENQUEUE: Def1<Enqueue, ()> = Def1 {
    name: "listening.enqueue",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(
                b,
                IS_ACTIVE.call(&s).or(CAN_PLAY.call(Ctx.user(), Ctx.session())),
                "nothing here can play it",
            );
            let session = b.let_(per(
                &s,
                |a| {
                    block(|b| {
                        let q = a.f().queue;
                        let n = b.let_(q.len());
                        let cut = b.let_(if_(&i.next, a.f().at + 1, n.clone()).min(&n));
                        let queue = q.slice(0, &cut).concat(&i.tracks).concat(q.slice(&cut, &n));
                        SessionEx::active(ActiveEx { queue, ..a.f() })
                    })
                },
                || {
                    SessionEx::active(ActiveEx {
                        at: int(0),
                        clock: ClockEx::playing(0, Ctx.now()),
                        from: txt(""),
                        output: HERE.call(Ctx.session()),
                        queue: i.tracks.clone(),
                    })
                },
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// A track out of the queue. The one playing taken out is the next one
/// playing, from its start — or with nothing after it, the one before,
/// paused at its start; the last track of all taken out is nothing queued.
pub static REMOVE: Def1<Remove, ()> = Def1 {
    name: "listening.remove",
    kind: Kind::Live,
    body: |input| {
        let index = input.f().index;
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            unless(b, IN_QUEUE.call(&s, &index), "the track is not in the queue");
            let session = b.let_(per(
                &s,
                |a| {
                    block(|b| {
                        let x = a.f();
                        let n = b.let_(x.queue.len());
                        let current = b.let_(index.eq(&x.at));
                        let last = b.let_((&x.at + 1).ge(&n));
                        let playing = b.let_(ClockEx::match_(&x.clock, |_| bool(false), |_, _| bool(true)));
                        let start = b.let_(if_(playing.and(!&last), ClockEx::playing(0, Ctx.now()), ClockEx::paused(0)));
                        let at = if_(index.lt(&x.at).or(current.and(&last)), &x.at - 1, x.at.clone());
                        let kept = SessionEx::active(ActiveEx {
                            at,
                            clock: if_(&current, start, x.clock.clone()),
                            queue: x.queue.remove_at(&index),
                            ..a.f()
                        });
                        if_(n.le(1), SessionEx::idle(), kept)
                    })
                },
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// A track of the queue somewhere else in it; the one playing plays on.
pub static MOVE_TRACK: Def1<Move, ()> = Def1 {
    name: "listening.move",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            unless(
                b,
                IN_QUEUE.call(&s, &i.from).and(IN_QUEUE.call(&s, &i.to)),
                "the track is not in the queue",
            );
            let session = b.let_(per(
                &s,
                |a| {
                    SessionEx::active(ActiveEx {
                        at: MOVED_PLACE.call(a.f().at, &i.from, &i.to),
                        queue: a.f().queue.move_at(&i.from, &i.to),
                        ..a.f()
                    })
                },
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// Everything after the track playing, gone; it plays on.
pub static CLEAR_UPCOMING: Def0<()> = Def0 {
    name: "listening.clear_upcoming",
    kind: Kind::Live,
    body: || {
        block(|b| {
            let s = b.let_(SESSION_OF.call(Ctx.user()));
            unless(b, IS_ACTIVE.call(&s), "nothing is queued");
            let session = b.let_(per(
                &s,
                |a| {
                    SessionEx::active(ActiveEx {
                        queue: a.f().queue.slice(0, (a.f().at + 1).min(a.f().queue.len())),
                        ..a.f()
                    })
                },
                SessionEx::idle,
            ));
            PUT_SESSION.call(Ctx.user(), session)
        })
    },
};

/// The session, refused unless the caller is its output, then `f` of it.
fn output_put(b: &mut Block, f: impl FnOnce(&Ex<Session>, Ex<Active>) -> Ex<Session>) -> Ex<()> {
    let s = b.let_(SESSION_OF.call(Ctx.user()));
    unless(b, IS_OUTPUT.call(&s, Ctx.session()), "not the output");
    let session = b.let_(per(&s, |a| f(&s, a), SessionEx::idle));
    PUT_SESSION.call(Ctx.user(), session)
}

/// The output has the hand-off, at this point in the track.
pub static STARTED: Def1<At, ()> = Def1 {
    name: "listening.started",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            output_put(b, |_, a| {
                SessionEx::active(ActiveEx {
                    clock: CLOCK_AT.call(a.f().clock, input.f().at_ms, Ctx.now()),
                    output: HERE.call(Ctx.session()),
                    ..a.f()
                })
            })
        })
    },
};

/// The output's track ended: the next one, or paused at the start of the
/// last when there is none. About a track no longer playing, nothing.
pub static ADVANCE: Def1<Advance, ()> = Def1 {
    name: "listening.advance",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            output_put(b, |s, a| {
                let x = a.f();
                if_(
                    x.at.ne(input.f().from_track),
                    s.clone(),
                    if_(
                        (&x.at + 1).lt(x.queue.len()),
                        SessionEx::active(ActiveEx {
                            at: &x.at + 1,
                            clock: ClockEx::playing(0, Ctx.now()),
                            ..a.f()
                        }),
                        SessionEx::active(ActiveEx {
                            clock: ClockEx::paused(0),
                            ..a.f()
                        }),
                    ),
                )
            })
        })
    },
};

/// The output stopped making a sound by itself, here.
pub static STALLED: Def1<At, ()> = Def1 {
    name: "listening.stalled",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            output_put(b, |_, a| {
                SessionEx::active(ActiveEx {
                    clock: ClockEx::paused(input.f().at_ms),
                    ..a.f()
                })
            })
        })
    },
};

/// The output, playing, says where it is; the clock is re-anchored there.
/// About a clock that has moved since, or a hand-off in flight, nothing.
pub static REPORT_POSITION: Def1<Report, ()> = Def1 {
    name: "listening.report_position",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            output_put(b, |s, a| {
                block(|b| {
                    let current = b.let_(ClockEx::match_(&a.f().clock, |_| bool(false), |_, since| since.eq(&i.since)));
                    let settled = b.let_(OutputEx::match_(&a.f().output, |_, _| bool(false), |_, moving| !moving, || bool(false)));
                    if_(
                        current.and(settled),
                        SessionEx::active(ActiveEx {
                            clock: ClockEx::playing(&i.at_ms, Ctx.now()),
                            ..a.f()
                        }),
                        s.clone(),
                    )
                })
            })
        })
    },
};

/// The output lets the sound go: the queue stays, paused, and nothing is
/// the output.
pub static RELEASE: Def0<()> = Def0 {
    name: "listening.release",
    kind: Kind::Live,
    body: || {
        block(|b| {
            output_put(b, |_, a| {
                SessionEx::active(ActiveEx {
                    clock: HALTED.call(a.f().clock, a.f().output, Ctx.now()),
                    output: OutputEx::nowhere(),
                    ..a.f()
                })
            })
        })
    },
};

/// What a device says of itself once it stands here. A family that moves
/// takes it out of any group it was in.
pub static EQUIP: Def1<Equip, ()> = Def1 {
    name: "listening.equip",
    kind: Kind::Live,
    body: |input| {
        let (db, i) = (db(), input.f());
        block(|b| {
            b.s(if_(
                db.listening_device.get((Ctx.user(), Ctx.session())).is_none(),
                refuse_("not here"),
                unit(),
            ));
            b.s(if_(
                PRESENT_OF.call(Ctx.user(), Ctx.session()).f().family.ne(&i.family),
                DISSOLVED.call(Ctx.user(), Ctx.session()),
                unit(),
            ));
            let me = b.let_(PRESENT_OF.call(Ctx.user(), Ctx.session()));
            let present = PresentEx {
                can_volume: i.can_volume.clone(),
                family: i.family.clone(),
                muted: i.muted.clone(),
                volume: i.volume.clone(),
                ..me.f()
            };
            PUT_PRESENT.call(Ctx.user(), Ctx.session(), present)
        })
    },
};

/// The device `device`'s entry, refused unless it has a volume, through `f`.
fn volume_put(b: &mut Block, device: &Ex<Text>, why: &str, f: impl FnOnce(PresentEx) -> PresentEx) -> Ex<()> {
    unless(b, HAS_VOLUME.call(Ctx.user(), device), why);
    let d = b.let_(PRESENT_OF.call(Ctx.user(), device));
    PUT_PRESENT.call(Ctx.user(), device, f(d.f()))
}

const NO_VOLUME: &str = "that device is not here, or has no volume";

/// A device's volume, from anywhere. Moving the volume unmutes.
pub static SET_VOLUME: Def1<SetVolume, ()> = Def1 {
    name: "listening.set_volume",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            volume_put(b, &i.device, NO_VOLUME, |d| PresentEx {
                volume: i.level.clone(),
                muted: bool(false),
                ..d
            })
        })
    },
};

/// …moved by so much, clamped: what a volume button sends.
pub static NUDGE_VOLUME: Def1<NudgeVolume, ()> = Def1 {
    name: "listening.nudge_volume",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            volume_put(b, &i.device, NO_VOLUME, |d| PresentEx {
                volume: (&d.volume + &i.delta).clamp(0, 100),
                muted: bool(false),
                ..d
            })
        })
    },
};

pub static SET_MUTE: Def1<SetMute, ()> = Def1 {
    name: "listening.set_mute",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| volume_put(b, &i.device, NO_VOLUME, |d| PresentEx { muted: i.muted.clone(), ..d }))
    },
};

/// The device's own word on its volume: its own entry only.
pub static REPORT_VOLUME: Def1<ReportVolume, ()> = Def1 {
    name: "listening.report_volume",
    kind: Kind::Live,
    body: |input| {
        let i = input.f();
        block(|b| {
            volume_put(b, &Ctx.session(), "not here, or no volume", |d| PresentEx {
                volume: i.level.clone(),
                muted: i.muted.clone(),
                ..d
            })
        })
    },
};

/// A speaker joins the output's group; whatever followed it follows nobody.
pub static JOIN: Def1<Member, ()> = Def1 {
    name: "listening.join",
    kind: Kind::Live,
    body: |input| {
        let device = input.f().device;
        block(|b| {
            let leader = b.let_(OUTPUT_ID.call(SESSION_OF.call(Ctx.user())));
            b.s(if_(leader.is_none(), refuse_("nothing is playing to join"), unit()));
            unless(
                b,
                leader.map_or(false, |l| JOINABLE.call(Ctx.user(), l, &device)),
                "that device cannot join this one",
            );
            FOLLOWED.call(Ctx.user(), &device, leader.unwrap())
        })
    },
};

/// …and leaves whatever group it is in.
pub static UNJOIN: Def1<Member, ()> = Def1 {
    name: "listening.unjoin",
    kind: Kind::Live,
    body: |input| {
        let device = input.f().device;
        block(|b| {
            let grouped = db()
                .listening_device
                .get((Ctx.user(), device.clone()))
                .map_or(false, |d| d.f().present.f().follows.is_some());
            unless(b, grouped, "that device is not in a group");
            let d = b.let_(PRESENT_OF.call(Ctx.user(), &device));
            PUT_PRESENT.call(Ctx.user(), &device, PresentEx { follows: none(), ..d.f() })
        })
    },
};

/// A leader's word on who follows it, changed outside harken.
pub static REPORT_GROUP: Def1<ReportGroup, ()> = Def1 {
    name: "listening.report_group",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            let groups = db()
                .listening_device
                .get((Ctx.user(), Ctx.session()))
                .map_or(false, |d| d.f().present.f().family.is_some());
            unless(b, groups, "not here, or not a speaker that groups");
            REGROUPED.call(Ctx.user(), Ctx.session(), input.f().members)
        })
    },
};

const NO_GROUP_VOLUME: &str = "nothing playing has a volume";

/// The output's group at a level, every member in proportion, unmuted.
pub static SET_GROUP_VOLUME: Def1<GroupVolume, ()> = Def1 {
    name: "listening.set_group_volume",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            unless(b, GROUP_HAS_VOLUME.call(Ctx.user()), NO_GROUP_VOLUME);
            GROUP_AT.call(Ctx.user(), LEADER.call(Ctx.user()), input.f().level)
        })
    },
};

pub static NUDGE_GROUP_VOLUME: Def1<GroupNudge, ()> = Def1 {
    name: "listening.nudge_group_volume",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            unless(b, GROUP_HAS_VOLUME.call(Ctx.user()), NO_GROUP_VOLUME);
            let l = b.let_(LEADER.call(Ctx.user()));
            GROUP_AT.call(Ctx.user(), &l, (GROUP_LEVEL.call(Ctx.user(), &l) + input.f().delta).clamp(0, 100))
        })
    },
};

pub static SET_GROUP_MUTE: Def1<GroupMute, ()> = Def1 {
    name: "listening.set_group_mute",
    kind: Kind::Live,
    body: |input| {
        block(|b| {
            unless(b, GROUP_HAS_VOLUME.call(Ctx.user()), NO_GROUP_VOLUME);
            GROUP_MUTED.call(Ctx.user(), LEADER.call(Ctx.user()), input.f().muted)
        })
    },
};

/// Ephemeral: where this device's scrubber is being dragged to, or nowhere
/// once it is let go. It reads nothing ordered and writes its own row.
pub static SCRUB: Def1<Scrub, ()> = Def1 {
    name: "listening.scrub",
    kind: Kind::Live,
    body: |input| {
        let db = db();
        block(|b| {
            b.s(db.listening_scrub.delete((Ctx.user(), Ctx.session())));
            input.f().at_ms.map_or(unit(), |at| {
                db.listening_scrub.insert(ListeningScrubEx {
                    user: Ctx.user(),
                    device: Ctx.session(),
                    at_ms: at,
                })
            })
        })
    },
};

/// A device arriving takes nothing; one the sound stayed with gets it back,
/// still paused.
pub static ON_JOIN: Def1<Device, ()> = Def1 {
    name: "listening.on_join",
    kind: Kind::Live,
    body: |device| {
        let db = db();
        block(|b| {
            let user = b.let_(device.f().user);
            let id = b.let_(device.f().id);
            b.s(db.listening_device.delete((user.clone(), id.clone())));
            b.s(db.listening_device.insert(ListeningDeviceEx {
                user: user.clone(),
                device: id.clone(),
                present: ARRIVED.call(&device),
            }));
            let session = b.let_(per(
                &SESSION_OF.call(&user),
                |a| {
                    SessionEx::active(ActiveEx {
                        output: RETURNED.call(a.f().output, &id),
                        ..a.f()
                    })
                },
                SessionEx::idle,
            ));
            if_(db.listening.get(&user).is_some(), PUT_SESSION.call(&user, session), unit())
        })
    },
};

/// A device leaving is gone, and so is its group; the output leaving keeps
/// the sound, away, the clock stopped.
pub static ON_LEAVE: Def1<Device, ()> = Def1 {
    name: "listening.on_leave",
    kind: Kind::Live,
    body: |device| {
        let db = db();
        block(|b| {
            let user = b.let_(device.f().user);
            let id = b.let_(device.f().id);
            b.s(DISSOLVED.call(&user, &id));
            b.s(db.listening_device.delete((user.clone(), id.clone())));
            b.s(db.listening_scrub.delete((user.clone(), id.clone())));
            let s = b.let_(SESSION_OF.call(&user));
            let session = b.let_(per(
                &s,
                |a| {
                    let away = SessionEx::active(ActiveEx {
                        clock: HALTED.call(a.f().clock, a.f().output, Ctx.now()),
                        output: OutputEx::away(&id, device.f().name),
                        ..a.f()
                    });
                    if_(IS_OUTPUT.call(&s, &id), away, s.clone())
                },
                SessionEx::idle,
            ));
            if_(db.listening.get(&user).is_some(), PUT_SESSION.call(&user, session), unit())
        })
    },
};

/// A restart: every queue as it was, paused where it had got to (never
/// past the end of the track), with no output and nobody here. Authored
/// over the ordered tables a restart kept; the scrubbers were never kept.
pub static ON_RESTORE: Def0<()> = Def0 {
    name: "listening.on_restore",
    kind: Kind::Live,
    body: || {
        let db = db();
        block(|b| {
            b.s(db.listening_device.map(|d| db.listening_device.delete((d.f().user, d.f().device))));
            db.listening
                .map(|r| {
                    block(|b| {
                        let session = b.let_(per(
                            &r.f().session,
                            |a| {
                                let x = a.f();
                                let at = STANDING.call(&x.clock, &x.output, Ctx.now()).min(DURATION_AT.call(&x.queue, &x.at));
                                SessionEx::active(ActiveEx {
                                    clock: ClockEx::paused(at),
                                    output: OutputEx::nowhere(),
                                    ..a.f()
                                })
                            },
                            SessionEx::idle,
                        ));
                        PUT_SESSION.call(r.f().user, session)
                    })
                })
                .s()
        })
    },
};

// * The plays

/// A track was played: authored by the device making the sound once it has
/// played long enough to count, dated by its own clock.
pub static RECORD_PLAY: Def1<RecordPlay, ()> = Def1 {
    name: "listening.record_play",
    kind: Kind::Mutator,
    body: |input| {
        let i = input.f();
        db().play
            .insert(PlayEx {
                id: Ctx.new_id::<Play>(),
                user_id: Ctx.user(),
                track: i.track.text_of_id(),
                title: i.title,
                creator: i.creator,
                album: i.album,
                duration_ms: i.duration_ms,
                file: i.file,
                played_ms: Ctx.now(),
                from: i.from,
            })
            .s()
    },
};

/// The caller's newest play.
pub static LAST_PLAYED: Def0<Vec<Play>> = Def0 {
    name: "listening.last_played",
    kind: Kind::Query,
    body: || {
        db().play
            .filter(|p| p.f().user_id.eq(Ctx.user()))
            .sort(|p| (desc(p.f().played_ms), desc(p.f().id)))
            .take(1)
    },
};

/// The caller's plays, newest first, the last [`RECENT`] of them.
pub static RECENTLY_PLAYED: Def0<Vec<Play>> = Def0 {
    name: "listening.recently_played",
    kind: Kind::Query,
    body: || {
        db().play
            .filter(|p| p.f().user_id.eq(Ctx.user()))
            .sort(|p| (desc(p.f().played_ms), desc(p.f().id)))
            .take(50)
    },
};

/// How many of a group have a volume, and their sum.
#[derive(Input, Node, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub n: Int,
    pub sum: Int,
}

// * The transitions

// * The plays

/// What `record_play` takes: the track as the session carried it, and
/// where it was played from.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct RecordPlay {
    pub track: Id<Media>,
    pub title: Text,
    pub creator: Text,
    pub album: Text,
    #[ark(min = 0)]
    pub duration_ms: Int,
    pub file: Text,
    pub from: Text,
}

/// How many plays `recently_played` lists.
pub const RECENT: i64 = 50;

// * Reading it

/// The state as plain Rust: what a player, a speaker or a screen is driven
/// from — the rows of the three tables, decoded.
pub mod read {
    use super::*;
    use ark::store::{Read, Scan};

    /// How far the output's player may be from where the session counts it
    /// before one is put right (`report_position`): 600 ms, the grain of
    /// the coarsest output (a speaker read through Home Assistant).
    pub const SYNC_MS: i64 = 600;

    /// What sort of thing a device is, for the glyph a picker draws.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Kind {
        #[default]
        Computer,
        Phone,
        Speaker,
    }

    impl Kind {
        pub fn name(self) -> &'static str {
            match self {
                Kind::Computer => "computer",
                Kind::Phone => "phone",
                Kind::Speaker => "speaker",
            }
        }

        /// A kind this build has never heard of reads as a computer.
        pub fn parse(s: &str) -> Kind {
            match s {
                "phone" => Kind::Phone,
                "speaker" => Kind::Speaker,
                _ => Kind::Computer,
            }
        }
    }

    impl Clock {
        /// How far in at `now`, the server's clock.
        pub fn position(&self, now: i64) -> i64 {
            match *self {
                Clock::Paused { at_ms } => at_ms,
                Clock::Playing { at_ms, since } => at_ms + (now - since).max(0),
            }
        }

        pub fn playing(&self) -> bool {
            matches!(self, Clock::Playing { .. })
        }
    }

    impl Output {
        /// The device the sound belongs to, here or not.
        pub fn id(&self) -> Option<&str> {
            match self {
                Output::Device { id, .. } | Output::Away { id, .. } => Some(id),
                Output::Nowhere => None,
            }
        }
    }

    impl Active {
        /// The track playing.
        pub fn now(&self) -> &Track {
            &self.queue[(self.at.max(0) as usize).min(self.queue.len() - 1)]
        }

        /// Where the session stands at `now`: the domain's `standing`.
        pub fn position(&self, now: i64) -> i64 {
            match (&self.output, self.clock) {
                (Output::Device { moving: true, .. }, Clock::Playing { at_ms, .. } | Clock::Paused { at_ms }) => at_ms,
                (_, clock) => clock.position(now),
            }
        }
    }

    /// One account's state.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct Listening {
        pub devices: Vec<Present>,
        /// Where each device is dragging its scrubber, by device id.
        pub scrub: BTreeMap<String, i64>,
        pub session: Option<Active>,
    }

    impl Listening {
        /// `user`'s state as a store holds it.
        pub fn of(store: &dyn Read, user: &str) -> Listening {
            let rows = |t: &str| store.scan(t, &Scan::all()).into_iter();
            let session = rows("listening")
                .map(|r| crate::schema::Listening::from_bytes(&r).expect("a listening row"))
                .find(|r| r.user == user)
                .and_then(|r| match r.session {
                    Session::Active(a) => Some(a),
                    Session::Idle => None,
                });
            let devices = rows("listening_device")
                .map(|r| ListeningDevice::from_bytes(&r).expect("a device row"))
                .filter(|r| r.user == user)
                .map(|r| r.present)
                .collect();
            let scrub = rows("listening_scrub")
                .map(|r| ListeningScrub::from_bytes(&r).expect("a scrub row"))
                .filter(|r| r.user == user)
                .map(|r| (r.device, r.at_ms))
                .collect();
            Listening { devices, scrub, session }
        }

        pub fn device(&self, id: &str) -> Option<&Present> {
            self.devices.iter().find(|d| d.id == id)
        }

        /// The output, if anything is queued.
        pub fn output(&self) -> Option<&Output> {
            self.session.as_ref().map(|a| &a.output)
        }

        /// Whether `id` is the output and here.
        pub fn outputs(&self, id: &str) -> bool {
            matches!(self.output(), Some(Output::Device { id: o, .. }) if o == id)
        }

        /// Whether a hand-off to `id` is in flight.
        pub fn moving_to(&self, id: &str) -> bool {
            matches!(self.output(), Some(Output::Device { id: o, moving: true }) if o == id)
        }

        /// Whether something is making a sound.
        pub fn sounding(&self) -> bool {
            matches!(&self.session, Some(a) if a.clock.playing() && matches!(a.output, Output::Device { .. }))
        }

        /// The output, where it is a device that is here.
        pub fn leader(&self) -> Option<&str> {
            match self.output()? {
                Output::Device { id, .. } => Some(id),
                _ => None,
            }
        }

        /// Who follows `leader`, in id order.
        pub fn followers(&self, leader: &str) -> Vec<&Present> {
            let mut out: Vec<&Present> = self.devices.iter().filter(|d| d.follows.as_deref() == Some(leader)).collect();
            out.sort_by(|a, b| a.id.cmp(&b.id));
            out
        }

        /// The group `leader` leads: it, then whoever follows it.
        pub fn group(&self, leader: &str) -> Vec<&Present> {
            self.device(leader).into_iter().chain(self.followers(leader)).collect()
        }

        /// Whether the output is a group.
        pub fn grouped(&self) -> bool {
            self.leader().is_some_and(|l| !self.followers(l).is_empty())
        }

        /// The group's level as the domain reads it; `None` when nothing in
        /// it has a volume.
        pub fn group_volume(&self, leader: &str) -> Option<i64> {
            let levels: Vec<i64> = self.group(leader).iter().filter(|d| d.can_volume).map(|d| d.volume).collect();
            (!levels.is_empty()).then(|| levels.iter().sum::<i64>() / levels.len() as i64)
        }

        /// Whether every member of `leader`'s group with a volume is muted.
        pub fn group_muted(&self, leader: &str) -> bool {
            let members: Vec<&Present> = self.group(leader).into_iter().filter(|d| d.can_volume).collect();
            !members.is_empty() && members.iter().all(|d| d.muted)
        }

        /// Whether `id` can be joined to the output's group now.
        pub fn joinable(&self, id: &str) -> bool {
            let Some(leader) = self.leader().and_then(|l| self.device(l)) else {
                return false;
            };
            let Some(d) = self.device(id) else { return false };
            leader.follows.is_none() && d.id != leader.id && d.can_play && d.family.is_some() && d.family == leader.family
        }
    }

    /// How long a track has to have played before it counts as a play:
    /// thirty seconds or half its length, whichever is first.
    pub fn counts_after(duration_ms: i64) -> i64 {
        if duration_ms <= 0 {
            return 30_000;
        }
        (duration_ms / 2).min(30_000)
    }
}

/// Where a track's file is, from where a server is. An absolute URL is
/// already an answer.
pub fn url(base: &str, file: &str) -> String {
    if file.is_empty() || file.starts_with("http://") || file.starts_with("https://") {
        return file.to_string();
    }
    let mut out = format!("{}/media", base.trim_end_matches('/'));
    for part in file.split('/') {
        out.push('/');
        for b in part.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path becomes a URL against whichever server is asking, and a URL
    /// is already an answer. Falsified by dropping the percent-encoding.
    #[test]
    fn a_file_is_joined_to_the_server_that_is_asking() {
        assert_eq!(
            url("http://10.0.0.2:8787", "music/Bach/air.flac"),
            "http://10.0.0.2:8787/media/music/Bach/air.flac"
        );
        assert_eq!(
            url("https://harken.example.com/", "music/a.mp3"),
            "https://harken.example.com/media/music/a.mp3"
        );
        assert_eq!(
            url("http://h", "music/Boléro & co/no #1.mp3"),
            "http://h/media/music/Bol%C3%A9ro%20%26%20co/no%20%231.mp3"
        );
        assert_eq!(
            url("http://h", "https://upload.wikimedia.org/x.mp3"),
            "https://upload.wikimedia.org/x.mp3"
        );
        assert_eq!(url("http://h", ""), "");
    }

    /// Thirty seconds or half the track, whichever is first. Falsified by
    /// dropping the halving.
    #[test]
    fn a_play_counts_after_thirty_seconds_or_half_the_track() {
        assert_eq!(read::counts_after(300_000), 30_000);
        assert_eq!(read::counts_after(40_000), 20_000);
        assert_eq!(read::counts_after(60_000), 30_000);
        assert_eq!(read::counts_after(0), 30_000);
    }
}
