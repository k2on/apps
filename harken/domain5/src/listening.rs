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

use ark5::author::{self as ark, Id, Input, Int, Node, Sum, Text};

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
    #[ark(min = -1, max = 1, ne = 0, why = "a skip is one track either way")]
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
    #[ark(min = 0, max = 100)]
    pub volume: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct SetVolume {
    pub device: Text,
    #[ark(min = 0, max = 100)]
    pub level: Int,
}

/// A device's level moved by `delta`, clamped: what a volume button sends.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct NudgeVolume {
    #[ark(min = -100, max = 100, ne = 0, why = "a nudge moves the volume")]
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
    #[ark(min = 0, max = 100)]
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
    #[ark(min = 0, max = 100)]
    pub level: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct GroupNudge {
    #[ark(min = -100, max = 100, ne = 0, why = "a nudge moves the volume")]
    pub delta: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct GroupMute {
    pub muted: bool,
}

// * The helpers

/// How far into the track a clock is at `now`.
#[ark::helper("listening.position")]
pub fn position(clock: Clock, now: Int) -> Int {
    match clock {
        Clock::Paused { at_ms } => at_ms,
        Clock::Playing { at_ms, since } => (at_ms + (now - since)).max(at_ms),
    }
}

/// Where the session stands at `now`: its clock's position — except while
/// the sound is on its way to a device (`moving`), when it stands where it
/// was handed over, since nothing is making a sound in between.
#[ark::helper("listening.standing")]
pub fn standing(clock: Clock, output: Output, now: Int) -> Int {
    let moving = match output {
        Output::Device { moving, .. } => moving,
        _ => false,
    };
    let anchor = match clock {
        Clock::Paused { at_ms } => at_ms,
        Clock::Playing { at_ms, .. } => at_ms,
    };
    if moving {
        anchor
    } else {
        position(clock, now)
    }
}

/// The clock paused where it is at `now`.
#[ark::helper("listening.paused_at")]
pub fn paused_at(clock: Clock, now: Int) -> Clock {
    Clock::Paused { at_ms: position(clock, now) }
}

/// The session's clock paused where the session stands at `now`.
#[ark::helper("listening.halted")]
pub fn halted(clock: Clock, output: Output, now: Int) -> Clock {
    Clock::Paused { at_ms: standing(clock, output, now) }
}

/// The clock at `ms`, playing on from there if it was playing.
#[ark::helper("listening.clock_at")]
pub fn clock_at(clock: Clock, ms: Int, now: Int) -> Clock {
    match clock {
        Clock::Paused { .. } => Clock::Paused { at_ms: ms },
        Clock::Playing { .. } => Clock::Playing { at_ms: ms, since: now },
    }
}

/// Whether the output is a device that is here.
#[ark::helper("listening.is_here")]
pub fn is_here(output: Output) -> bool {
    match output {
        Output::Device { .. } => true,
        _ => false,
    }
}

/// Whether something is queued.
#[ark::helper("listening.is_active")]
pub fn is_active(session: Session) -> bool {
    match session {
        Session::Active(_) => true,
        Session::Idle => false,
    }
}

/// Whether the session has an output that is here to answer.
#[ark::helper("listening.reachable")]
pub fn reachable(session: Session) -> bool {
    match session {
        Session::Active(a) => is_here(a.output),
        Session::Idle => false,
    }
}

/// Whether something is making a sound: a clock running on an output that
/// is here.
#[ark::helper("listening.sounding")]
pub fn sounding(session: Session) -> bool {
    match session {
        Session::Active(a) => {
            is_here(a.output)
                && match a.clock {
                    Clock::Playing { .. } => true,
                    Clock::Paused { .. } => false,
                }
        }
        Session::Idle => false,
    }
}

/// The account's session: its row's, or `Idle`.
#[ark::helper("listening.session_of")]
pub fn session_of(db: Harken, user: Text) -> Session {
    match db.listening.get(user) {
        Some(r) => r.session,
        None => Session::Idle,
    }
}

/// The account's session, written.
#[ark::helper("listening.put_session")]
pub fn put_session(db: Harken, user: Text, session: Session) {
    if db.listening.get(user).is_some() {
        db.listening.update(user, |r| Listening { session, ..r });
    } else {
        db.listening.insert(Listening { user, session });
    }
}

/// The devices standing in the account's session, in id order.
#[ark::helper("listening.devices_of")]
pub fn devices_of(db: Harken, user: Text) -> Vec<ListeningDevice> {
    db.listening_device.filter(|r| r.user == user)
}

/// A device standing in the session; refused when it is not (every caller
/// has held it there first).
#[ark::helper("listening.present_of")]
pub fn present_of(db: Harken, user: Text, id: Text) -> Present {
    db.listening_device.get((user, id)).unwrap().present
}

/// A device's entry, written.
#[ark::helper("listening.put_present")]
pub fn put_present(db: Harken, user: Text, id: Text, present: Present) {
    db.listening_device.update((user, id), |r| ListeningDevice { present, ..r });
}

/// Whether the device is here and can make a sound.
#[ark::helper("listening.can_play")]
pub fn can_play(db: Harken, user: Text, id: Text) -> bool {
    db.listening_device.get((user, id)).map_or(false, |d| d.present.can_play)
}

/// Whether the device is the session's output and here.
#[ark::helper("listening.is_output")]
pub fn is_output(session: Session, id: Text) -> bool {
    match session {
        Session::Active(a) => match a.output {
            Output::Device { id: d, .. } => d == id,
            _ => false,
        },
        Session::Idle => false,
    }
}

/// The sound on this device, which has it.
#[ark::helper("listening.here")]
pub fn here(id: Text) -> Output {
    Output::Device { id, moving: false }
}

/// The output a press that means a sound leaves: the one there is, where it
/// can answer, or the caller.
#[ark::helper("listening.pressed")]
pub fn pressed(output: Output, caller: Text) -> Output {
    if is_here(output) {
        output
    } else {
        here(caller)
    }
}

/// The output once `id` has come back: given back to it, if it was the one
/// that went.
#[ark::helper("listening.returned")]
pub fn returned(output: Output, id: Text) -> Output {
    match output {
        Output::Away { id: a, .. } => {
            if a == id {
                here(a)
            } else {
                output
            }
        }
        _ => output,
    }
}

/// How long the track at `at` is; 0 for no track there.
#[ark::helper("listening.duration_at")]
pub fn duration_at(queue: Vec<Track>, at: Int) -> Int {
    if at >= 0 && at < queue.len() {
        queue.slice(at, at + 1).first().map_or(0, |t| t.duration_ms)
    } else {
        0
    }
}

/// A session at a new place in its queue, playing from the start of it, on
/// whichever device a press leaves the sound with.
#[ark::helper("listening.moved_to")]
pub fn moved_to(a: Active, at: Int, caller: Text, now: Int) -> Session {
    Session::Active(Active { at, clock: Clock::Playing { at_ms: 0, since: now }, output: pressed(a.output, caller), ..a })
}

/// Where the track at `i` goes when the one at `from` moves to `to`.
#[ark::helper("listening.moved_place")]
pub fn moved_place(i: Int, from: Int, to: Int) -> Int {
    if i == from {
        to
    } else if from < to && i > from && i <= to {
        i - 1
    } else if to < from && i >= to && i < from {
        i + 1
    } else {
        i
    }
}

/// Whether `index` is a place in the session's queue.
#[ark::helper("listening.in_queue")]
pub fn in_queue(session: Session, index: Int) -> bool {
    match session {
        Session::Active(a) => index < a.queue.len(),
        Session::Idle => false,
    }
}

/// A device as `on_join` first writes it: what it declared, and nothing it
/// has not said.
#[ark::helper("listening.arrived")]
pub fn arrived(device: Device) -> Present {
    Present {
        can_play: device.can_play,
        can_volume: false,
        family: None,
        follows: None,
        id: device.id,
        kind: device.kind,
        muted: false,
        name: device.name,
        user: device.user,
        volume: 100,
    }
}

/// The output, where it is a device that is here: the one a group is led by.
#[ark::helper("listening.output_id")]
pub fn output_id(session: Session) -> Option<Text> {
    match session {
        Session::Active(a) => match a.output {
            Output::Device { id, .. } => Some(id),
            _ => None,
        },
        Session::Idle => None,
    }
}

/// Whether the device can have its volume moved: here, and said it has one.
#[ark::helper("listening.has_volume")]
pub fn has_volume(db: Harken, user: Text, id: Text) -> bool {
    db.listening_device.get((user, id)).map_or(false, |d| d.present.can_volume)
}

/// Whether `d` is in the group `leader` leads: the leader, or following it.
#[ark::helper("listening.in_group")]
pub fn in_group(d: Present, leader: Text) -> bool {
    d.id == leader || d.follows == Some(leader)
}

/// Whether `id` can be joined to the group `leader` leads now: a speaker
/// that is here, can play, of the leader's family, and not the leader.
#[ark::helper("listening.joinable")]
pub fn joinable(db: Harken, user: Text, leader: Text, id: Text) -> bool {
    let family = db.listening_device.get((user, leader)).map_or(None, |l| {
        if l.present.follows.is_none() {
            l.present.family
        } else {
            None
        }
    });
    db.listening_device.get((user, id)).map_or(false, |d| {
        d.present.id != leader && d.present.can_play && d.present.family.is_some() && d.present.family == family
    })
}

/// How many of a group have a volume, and their sum.
#[derive(Input, Node, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub n: Int,
    pub sum: Int,
}

/// The group `leader` leads, those of it that have a volume.
#[ark::helper("listening.voiced")]
pub fn voiced(db: Harken, user: Text, leader: Text) -> Vec<ListeningDevice> {
    devices_of(db, user).filter(|r| in_group(r.present, leader) && r.present.can_volume)
}

#[ark::helper("listening.group_tally")]
pub fn group_tally(db: Harken, user: Text, leader: Text) -> Tally {
    let members = voiced(db, user, leader);
    Tally { n: members.len(), sum: members.map(|r| r.present.volume).sum() }
}

/// The group's volume: the mean of its members', rounded down; 0 for a
/// group with no volume at all.
#[ark::helper("listening.group_level")]
pub fn group_level(db: Harken, user: Text, leader: Text) -> Int {
    let t = group_tally(db, user, leader);
    t.sum / t.n.max(1)
}

/// One member's level once the group's moves from `from` to `to`: in
/// proportion, rounded to the nearest, clamped — and from a group at 0,
/// `to` itself.
#[ark::helper("listening.scaled")]
pub fn scaled(volume: Int, from: Int, to: Int) -> Int {
    if from == 0 {
        to
    } else {
        ((volume * to * 2 + from) / (from.max(1) * 2)).clamp(0, 100)
    }
}

/// The group `leader` leads at the level `to`: every member with a volume
/// moved in proportion and unmuted.
#[ark::helper("listening.group_at")]
pub fn group_at(db: Harken, user: Text, leader: Text, to: Int) {
    let from = group_level(db, user, leader);
    voiced(db, user, leader).map(|r| {
        put_present(db, user, r.device, Present { volume: scaled(r.present.volume, from, to), muted: false, ..r.present })
    });
}

/// The group `leader` leads, every member with a volume muted or not.
#[ark::helper("listening.group_muted")]
pub fn group_muted(db: Harken, user: Text, leader: Text, muted: bool) {
    voiced(db, user, leader).map(|r| put_present(db, user, r.device, Present { muted, ..r.present }));
}

/// `id` follows nobody, and nobody follows it.
#[ark::helper("listening.dissolved")]
pub fn dissolved(db: Harken, user: Text, id: Text) {
    devices_of(db, user)
        .filter(|r| r.present.id == id || r.present.follows == Some(id))
        .map(|r| put_present(db, user, r.device, Present { follows: None, ..r.present }));
}

/// `id` follows `leader`; whoever followed `id` follows nobody.
#[ark::helper("listening.followed")]
pub fn followed(db: Harken, user: Text, id: Text, leader: Text) {
    devices_of(db, user).map(|r| {
        if r.present.id == id {
            put_present(db, user, r.device, Present { follows: Some(leader), ..r.present })
        } else if r.present.follows == Some(id) {
            put_present(db, user, r.device, Present { follows: None, ..r.present })
        }
    });
}

/// The sound handed to `to`: if it was following a leader, it leads that
/// group now, as Sonos makes a member the coordinator when it is picked.
#[ark::helper("listening.led_by")]
pub fn led_by(db: Harken, user: Text, to: Text) {
    let old = db.listening_device.get((user, to)).map_or(None, |t| t.present.follows);
    devices_of(db, user).map(|r| {
        if r.present.id == to {
            put_present(db, user, r.device, Present { follows: None, ..r.present })
        } else if old.is_some() && (Some(r.present.id) == old || r.present.follows == old) {
            put_present(db, user, r.device, Present { follows: Some(to), ..r.present })
        }
    });
}

/// The leader `caller`'s word on who follows it: each member that is here
/// and of its family follows it; whoever followed it and is not named, or
/// followed a device that is a member now, follows nobody.
#[ark::helper("listening.regrouped")]
pub fn regrouped(db: Harken, user: Text, caller: Text, members: Vec<Text>) {
    let family = db.listening_device.get((user, caller)).map_or(None, |c| c.present.family);
    devices_of(db, user).map(|r| {
        let d = r.present;
        let named = members.contains(d.id) && d.id != caller && d.can_play && family.is_some() && d.family == family;
        let behind = d.follows.map_or(false, |l| members.contains(l));
        let follows = if d.id == caller {
            None
        } else if named {
            Some(caller)
        } else if d.follows == Some(caller) || behind {
            None
        } else {
            d.follows
        };
        put_present(db, user, r.device, Present { follows, ..d })
    });
}

/// Whether the output is a device that is here and something in its group
/// has a volume: what every group transition is guarded by.
#[ark::helper("listening.group_has_volume")]
pub fn group_has_volume(db: Harken, user: Text) -> bool {
    output_id(session_of(db, user)).map_or(false, |leader| group_tally(db, user, leader).n > 0)
}

/// The output's id: the leader the group transitions move.
#[ark::helper("listening.leader")]
pub fn leader(db: Harken, user: Text) -> Text {
    output_id(session_of(db, user)).unwrap()
}

/// The session with its active arm moved by `f`'s result, `Idle` kept.
#[ark::helper("listening.halt")]
pub fn halt(session: Session, now: Int) -> Session {
    match session {
        Session::Active(a) => Session::Active(Active { clock: halted(a.clock, a.output, now), ..a }),
        Session::Idle => Session::Idle,
    }
}

// * The transitions

/// A track pressed: this queue, from here, at this point, playing. While
/// something is sounding the press goes to the device making it; otherwise
/// the caller takes the sound if it can play.
#[ark::live("listening.start")]
pub fn start(ctx: Ctx, db: Harken, input: Start) {
    let s = session_of(db, ctx.user);
    if !(input.at < input.queue.len()) {
        refuse!("the track is not in the queue");
    }
    if !(sounding(s) || reachable(s) || can_play(db, ctx.user, ctx.session)) {
        refuse!("nothing here can play it");
    }
    let keep = sounding(s) || !can_play(db, ctx.user, ctx.session);
    let output = match s {
        Session::Active(a) => {
            if keep {
                a.output
            } else {
                here(ctx.session)
            }
        }
        Session::Idle => here(ctx.session),
    };
    put_session(
        db,
        ctx.user,
        Session::Active(Active {
            at: input.at,
            clock: Clock::Playing { at_ms: input.position, since: ctx.now },
            from: input.from,
            output,
            queue: input.queue,
        }),
    );
}

/// Resume, on the output where it can answer, else on the caller.
#[ark::live("listening.play")]
pub fn play(ctx: Ctx, db: Harken) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    if !(reachable(s) || can_play(db, ctx.user, ctx.session)) {
        refuse!("nothing here can play it");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active {
            clock: Clock::Playing { at_ms: standing(a.clock, a.output, ctx.now), since: ctx.now },
            output: pressed(a.output, ctx.session),
            ..a
        }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// Pause, wherever the sound is. Nobody takes anything.
#[ark::live("listening.pause")]
pub fn pause(ctx: Ctx, db: Harken) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    put_session(db, ctx.user, halt(s, ctx.now));
}

/// Somewhere else in the track, playing on if it was.
#[ark::live("listening.seek")]
pub fn seek(ctx: Ctx, db: Harken, input: Seek) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active { clock: clock_at(a.clock, input.ms, ctx.now), ..a }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// The next track or the one before, from its start; before the first is
/// the first again. A press that means a sound.
#[ark::live("listening.skip")]
pub fn skip(ctx: Ctx, db: Harken, input: Skip) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    let after = match s {
        Session::Active(a) => input.by < 0 || a.at + 1 < a.queue.len(),
        Session::Idle => false,
    };
    if !after {
        refuse!("nothing comes after this");
    }
    if !(reachable(s) || can_play(db, ctx.user, ctx.session)) {
        refuse!("nothing here can play it");
    }
    let session = match s {
        Session::Active(a) => moved_to(a, (a.at + input.by).max(0), ctx.session, ctx.now),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// A track of the queue, from its start. A press that means a sound.
#[ark::live("listening.jump")]
pub fn jump(ctx: Ctx, db: Harken, input: Jump) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    if !in_queue(s, input.index) {
        refuse!("the track is not in the queue");
    }
    if !(reachable(s) || can_play(db, ctx.user, ctx.session)) {
        refuse!("nothing here can play it");
    }
    let session = match s {
        Session::Active(a) => moved_to(a, input.index, ctx.session, ctx.now),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// Move the sound to a device that is here and can play: `moving` until it
/// says it has started, the clock standing where it stopped meanwhile.
/// Handing it to the device that already has it, settled, changes nothing.
#[ark::live("listening.transfer")]
pub fn transfer(ctx: Ctx, db: Harken, input: Transfer) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is playing to move");
    }
    if !can_play(db, ctx.user, input.to) {
        refuse!("that device is not here, or cannot play");
    }
    // A speaker that was following another leads its group now.
    led_by(db, ctx.user, input.to);
    let session = match s {
        Session::Active(a) => {
            let same = a.output == here(input.to);
            Session::Active(Active {
                clock: if same { a.clock } else { clock_at(a.clock, standing(a.clock, a.output, ctx.now), ctx.now) },
                output: if same { a.output } else { Output::Device { id: input.to, moving: true } },
                ..a
            })
        }
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// Nothing queued, nothing playing, nowhere.
#[ark::live("listening.stop")]
pub fn stop(ctx: Ctx, db: Harken) {
    put_session(db, ctx.user, Session::Idle);
}

/// Tracks put after the one playing ("Play next") or at the end ("Add to
/// queue"); into nothing queued it is `start` of them from the first.
#[ark::live("listening.enqueue")]
pub fn enqueue(ctx: Ctx, db: Harken, input: Enqueue) {
    let s = session_of(db, ctx.user);
    if !(is_active(s) || can_play(db, ctx.user, ctx.session)) {
        refuse!("nothing here can play it");
    }
    let session = match s {
        Session::Active(a) => {
            let n = a.queue.len();
            let cut = (if input.next { a.at + 1 } else { n }).min(n);
            Session::Active(Active { queue: a.queue.slice(0, cut).concat(input.tracks).concat(a.queue.slice(cut, n)), ..a })
        }
        Session::Idle => Session::Active(Active {
            at: 0,
            clock: Clock::Playing { at_ms: 0, since: ctx.now },
            from: "",
            output: here(ctx.session),
            queue: input.tracks,
        }),
    };
    put_session(db, ctx.user, session);
}

/// A track out of the queue. The one playing taken out is the next one
/// playing, from its start — or with nothing after it, the one before,
/// paused at its start; the last track of all taken out is nothing queued.
#[ark::live("listening.remove")]
pub fn remove(ctx: Ctx, db: Harken, input: Remove) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    if !in_queue(s, input.index) {
        refuse!("the track is not in the queue");
    }
    let session = match s {
        Session::Active(a) => {
            let n = a.queue.len();
            let current = input.index == a.at;
            let last = a.at + 1 >= n;
            let playing = match a.clock {
                Clock::Playing { .. } => true,
                Clock::Paused { .. } => false,
            };
            let start = if playing && !last { Clock::Playing { at_ms: 0, since: ctx.now } } else { Clock::Paused { at_ms: 0 } };
            if n <= 1 {
                Session::Idle
            } else {
                Session::Active(Active {
                    at: if input.index < a.at || (current && last) { a.at - 1 } else { a.at },
                    clock: if current { start } else { a.clock },
                    queue: a.queue.remove_at(input.index),
                    ..a
                })
            }
        }
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// A track of the queue somewhere else in it; the one playing plays on.
#[ark::live("listening.move")]
pub fn move_track(ctx: Ctx, db: Harken, input: Move) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    if !(in_queue(s, input.from) && in_queue(s, input.to)) {
        refuse!("the track is not in the queue");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active {
            at: moved_place(a.at, input.from, input.to),
            queue: a.queue.move_at(input.from, input.to),
            ..a
        }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// Everything after the track playing, gone; it plays on.
#[ark::live("listening.clear_upcoming")]
pub fn clear_upcoming(ctx: Ctx, db: Harken) {
    let s = session_of(db, ctx.user);
    if !is_active(s) {
        refuse!("nothing is queued");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active { queue: a.queue.slice(0, (a.at + 1).min(a.queue.len())), ..a }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// The output has the hand-off, at this point in the track.
#[ark::live("listening.started")]
pub fn started(ctx: Ctx, db: Harken, input: At) {
    let s = session_of(db, ctx.user);
    if !is_output(s, ctx.session) {
        refuse!("not the output");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active { clock: clock_at(a.clock, input.at_ms, ctx.now), output: here(ctx.session), ..a }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// The output's track ended: the next one, or paused at the start of the
/// last when there is none. About a track no longer playing, nothing.
#[ark::live("listening.advance")]
pub fn advance(ctx: Ctx, db: Harken, input: Advance) {
    let s = session_of(db, ctx.user);
    if !is_output(s, ctx.session) {
        refuse!("not the output");
    }
    let session = match s {
        Session::Active(a) => {
            if a.at != input.from_track {
                s
            } else if a.at + 1 < a.queue.len() {
                Session::Active(Active { at: a.at + 1, clock: Clock::Playing { at_ms: 0, since: ctx.now }, ..a })
            } else {
                Session::Active(Active { clock: Clock::Paused { at_ms: 0 }, ..a })
            }
        }
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// The output stopped making a sound by itself, here.
#[ark::live("listening.stalled")]
pub fn stalled(ctx: Ctx, db: Harken, input: At) {
    let s = session_of(db, ctx.user);
    if !is_output(s, ctx.session) {
        refuse!("not the output");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active { clock: Clock::Paused { at_ms: input.at_ms }, ..a }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// The output, playing, says where it is; the clock is re-anchored there.
/// About a clock that has moved since, or a hand-off in flight, nothing.
#[ark::live("listening.report_position")]
pub fn report_position(ctx: Ctx, db: Harken, input: Report) {
    let s = session_of(db, ctx.user);
    if !is_output(s, ctx.session) {
        refuse!("not the output");
    }
    let session = match s {
        Session::Active(a) => {
            let current = match a.clock {
                Clock::Playing { since, .. } => since == input.since,
                Clock::Paused { .. } => false,
            };
            let settled = match a.output {
                Output::Device { moving, .. } => !moving,
                _ => false,
            };
            if current && settled {
                Session::Active(Active { clock: Clock::Playing { at_ms: input.at_ms, since: ctx.now }, ..a })
            } else {
                s
            }
        }
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// The output lets the sound go: the queue stays, paused, and nothing is
/// the output.
#[ark::live("listening.release")]
pub fn release(ctx: Ctx, db: Harken) {
    let s = session_of(db, ctx.user);
    if !is_output(s, ctx.session) {
        refuse!("not the output");
    }
    let session = match s {
        Session::Active(a) => Session::Active(Active { clock: halted(a.clock, a.output, ctx.now), output: Output::Nowhere, ..a }),
        Session::Idle => Session::Idle,
    };
    put_session(db, ctx.user, session);
}

/// What a device says of itself once it stands here. A family that moves
/// takes it out of any group it was in.
#[ark::live("listening.equip")]
pub fn equip(ctx: Ctx, db: Harken, input: Equip) {
    if db.listening_device.get((ctx.user, ctx.session)).is_none() {
        refuse!("not here");
    }
    if present_of(db, ctx.user, ctx.session).family != input.family {
        dissolved(db, ctx.user, ctx.session);
    }
    let me = present_of(db, ctx.user, ctx.session);
    put_present(
        db,
        ctx.user,
        ctx.session,
        Present { can_volume: input.can_volume, family: input.family, muted: input.muted, volume: input.volume, ..me },
    );
}

/// A device's volume, from anywhere. Moving the volume unmutes.
#[ark::live("listening.set_volume")]
pub fn set_volume(ctx: Ctx, db: Harken, input: SetVolume) {
    if !has_volume(db, ctx.user, input.device) {
        refuse!("that device is not here, or has no volume");
    }
    let d = present_of(db, ctx.user, input.device);
    put_present(db, ctx.user, input.device, Present { volume: input.level, muted: false, ..d });
}

/// …moved by so much, clamped: what a volume button sends.
#[ark::live("listening.nudge_volume")]
pub fn nudge_volume(ctx: Ctx, db: Harken, input: NudgeVolume) {
    if !has_volume(db, ctx.user, input.device) {
        refuse!("that device is not here, or has no volume");
    }
    let d = present_of(db, ctx.user, input.device);
    put_present(db, ctx.user, input.device, Present { volume: (d.volume + input.delta).clamp(0, 100), muted: false, ..d });
}

#[ark::live("listening.set_mute")]
pub fn set_mute(ctx: Ctx, db: Harken, input: SetMute) {
    if !has_volume(db, ctx.user, input.device) {
        refuse!("that device is not here, or has no volume");
    }
    let d = present_of(db, ctx.user, input.device);
    put_present(db, ctx.user, input.device, Present { muted: input.muted, ..d });
}

/// The device's own word on its volume: its own entry only.
#[ark::live("listening.report_volume")]
pub fn report_volume(ctx: Ctx, db: Harken, input: ReportVolume) {
    if !has_volume(db, ctx.user, ctx.session) {
        refuse!("not here, or no volume");
    }
    let d = present_of(db, ctx.user, ctx.session);
    put_present(db, ctx.user, ctx.session, Present { volume: input.level, muted: input.muted, ..d });
}

/// A speaker joins the output's group; whatever followed it follows nobody.
#[ark::live("listening.join")]
pub fn join(ctx: Ctx, db: Harken, input: Member) {
    let leader = output_id(session_of(db, ctx.user));
    if leader.is_none() {
        refuse!("nothing is playing to join");
    }
    if !leader.map_or(false, |l| joinable(db, ctx.user, l, input.device)) {
        refuse!("that device cannot join this one");
    }
    followed(db, ctx.user, input.device, leader.unwrap());
}

/// …and leaves whatever group it is in.
#[ark::live("listening.unjoin")]
pub fn unjoin(ctx: Ctx, db: Harken, input: Member) {
    if !db.listening_device.get((ctx.user, input.device)).map_or(false, |d| d.present.follows.is_some()) {
        refuse!("that device is not in a group");
    }
    let d = present_of(db, ctx.user, input.device);
    put_present(db, ctx.user, input.device, Present { follows: None, ..d });
}

/// A leader's word on who follows it, changed outside harken.
#[ark::live("listening.report_group")]
pub fn report_group(ctx: Ctx, db: Harken, input: ReportGroup) {
    if !db.listening_device.get((ctx.user, ctx.session)).map_or(false, |d| d.present.family.is_some()) {
        refuse!("not here, or not a speaker that groups");
    }
    regrouped(db, ctx.user, ctx.session, input.members);
}

/// The output's group at a level, every member in proportion, unmuted.
#[ark::live("listening.set_group_volume")]
pub fn set_group_volume(ctx: Ctx, db: Harken, input: GroupVolume) {
    if !group_has_volume(db, ctx.user) {
        refuse!("nothing playing has a volume");
    }
    group_at(db, ctx.user, leader(db, ctx.user), input.level);
}

#[ark::live("listening.nudge_group_volume")]
pub fn nudge_group_volume(ctx: Ctx, db: Harken, input: GroupNudge) {
    if !group_has_volume(db, ctx.user) {
        refuse!("nothing playing has a volume");
    }
    let l = leader(db, ctx.user);
    group_at(db, ctx.user, l, (group_level(db, ctx.user, l) + input.delta).clamp(0, 100));
}

#[ark::live("listening.set_group_mute")]
pub fn set_group_mute(ctx: Ctx, db: Harken, input: GroupMute) {
    if !group_has_volume(db, ctx.user) {
        refuse!("nothing playing has a volume");
    }
    group_muted(db, ctx.user, leader(db, ctx.user), input.muted);
}

/// Ephemeral: where this device's scrubber is being dragged to, or nowhere
/// once it is let go. It reads nothing ordered and writes its own row.
#[ark::live("listening.scrub")]
pub fn scrub(ctx: Ctx, db: Harken, input: Scrub) {
    db.listening_scrub.delete((ctx.user, ctx.session));
    match input.at_ms {
        Some(at) => db.listening_scrub.insert(ListeningScrub { user: ctx.user, device: ctx.session, at_ms: at }),
        None => {}
    }
}

/// A device arriving takes nothing; one the sound stayed with gets it back,
/// still paused.
#[ark::live("listening.on_join")]
pub fn on_join(db: Harken, device: Device) {
    let user = device.user;
    let id = device.id;
    db.listening_device.delete((user, id));
    db.listening_device.insert(ListeningDevice { user, device: id, present: arrived(device) });
    let session = match session_of(db, user) {
        Session::Active(a) => Session::Active(Active { output: returned(a.output, id), ..a }),
        Session::Idle => Session::Idle,
    };
    if db.listening.get(user).is_some() {
        put_session(db, user, session);
    }
}

/// A device leaving is gone, and so is its group; the output leaving keeps
/// the sound, away, the clock stopped.
#[ark::live("listening.on_leave")]
pub fn on_leave(ctx: Ctx, db: Harken, device: Device) {
    let user = device.user;
    let id = device.id;
    dissolved(db, user, id);
    db.listening_device.delete((user, id));
    db.listening_scrub.delete((user, id));
    let s = session_of(db, user);
    let session = match s {
        Session::Active(a) => {
            if is_output(s, id) {
                Session::Active(Active {
                    clock: halted(a.clock, a.output, ctx.now),
                    output: Output::Away { id, name: device.name },
                    ..a
                })
            } else {
                s
            }
        }
        Session::Idle => Session::Idle,
    };
    if db.listening.get(user).is_some() {
        put_session(db, user, session);
    }
}

/// A restart: every queue as it was, paused where it had got to (never
/// past the end of the track), with no output and nobody here.
#[ark::live("listening.on_restore")]
pub fn on_restore(ctx: Ctx, db: Harken) {
    db.listening_device.map(|d| db.listening_device.delete((d.user, d.device)));
    db.listening_scrub.map(|d| db.listening_scrub.delete((d.user, d.device)));
    db.listening.map(|r| {
        let session = match r.session {
            Session::Active(a) => Session::Active(Active {
                clock: Clock::Paused { at_ms: standing(a.clock, a.output, ctx.now).min(duration_at(a.queue, a.at)) },
                output: Output::Nowhere,
                ..a
            }),
            Session::Idle => Session::Idle,
        };
        put_session(db, r.user, session)
    });
}

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

/// A track was played: authored by the device making the sound once it has
/// played long enough to count, dated by its own clock.
#[ark::mutator("listening.record_play")]
pub fn record_play(ctx: Ctx, db: Harken, input: RecordPlay) {
    db.play.insert(Play {
        id: ctx.new_id(),
        user_id: ctx.user,
        track: input.track.text_of_id(),
        title: input.title,
        creator: input.creator,
        album: input.album,
        duration_ms: input.duration_ms,
        file: input.file,
        played_ms: ctx.now,
        from: input.from,
    });
}

/// The caller's newest play.
#[ark::query("listening.last_played")]
pub fn last_played(ctx: Ctx, db: Harken) -> Vec<Play> {
    db.play.filter(|p| p.user_id == ctx.user).sort(|p| (Desc(p.played_ms), Desc(p.id))).take(1)
}

/// The caller's plays, newest first, the last [`RECENT`] of them.
#[ark::query("listening.recently_played")]
pub fn recently_played(ctx: Ctx, db: Harken) -> Vec<Play> {
    db.play.filter(|p| p.user_id == ctx.user).sort(|p| (Desc(p.played_ms), Desc(p.id))).take(50)
}

// * Reading it

/// The state as plain Rust: what a player, a speaker or a screen is driven
/// from — the rows of the three tables, decoded.
pub mod read {
    use super::*;
    use ark5::store::{Read, Scan};

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
        assert_eq!(url("http://10.0.0.2:8787", "music/Bach/air.flac"), "http://10.0.0.2:8787/media/music/Bach/air.flac");
        assert_eq!(url("https://harken.example.com/", "music/a.mp3"), "https://harken.example.com/media/music/a.mp3");
        assert_eq!(url("http://h", "music/Boléro & co/no #1.mp3"), "http://h/media/music/Bol%C3%A9ro%20%26%20co/no%20%231.mp3");
        assert_eq!(url("http://h", "https://upload.wikimedia.org/x.mp3"), "https://upload.wikimedia.org/x.mp3");
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
