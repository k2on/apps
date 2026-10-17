//! One account, one thing playing, however many devices are watching it —
//! as a live state (`docs/plan-live.md`), and the plays that are worth
//! keeping, as the log.
//!
//! A person with a phone, a laptop and a browser tab has **one** listening
//! session, not three. Any of them can change it; exactly one of them is
//! making the sound; and moving the sound from one to another is a
//! transition rather than a track started again somewhere else.
//!
//! **The state is the instruction.** Nothing here routes a command to the
//! device making the sound. Pause on the phone makes the session paused;
//! the laptop is the output, its player is reconciled against the session,
//! and it pauses. The output reports what happens to it — `started` when a
//! hand-off lands, `advance` when a track ends, `stalled` when the sound
//! stops by itself, `release` when it is no longer the session's to make —
//! and nothing else: no heartbeat, because `Playing { at_ms, since }` is
//! enough for every device to count the position for itself against the
//! server's clock (`ark_client::Peer::server_now`).
//!
//! **None of the session is in the log, and none of it ever will be.** An
//! afternoon of listening is thousands of pauses, seeks and skips, and not
//! one of them is worth replaying tomorrow. It is held by the server, one
//! instance per account (`scope(|ctx| ctx.user)`), on the sync socket, and
//! kept across a restart — paused, with no output (`on_restore`). What *is*
//! worth replaying is that a track was played, once: `record_play`, a
//! mutation like any other, authored by the output.
//!
//! The four rules the old hand-written desk was built around are properties
//! of the transitions below rather than code anybody calls:
//!
//! - **One output.** `Active.output` is one value: a present device, a
//!   device that has gone (`away`), or nowhere.
//! - **The output survives its socket.** `on_leave` keeps the sound with the
//!   device that left — `away`, the clock paused, because nothing is on the
//!   other end to be making a sound — and `on_join` gives it back to that
//!   device, still paused. A lid closing is not a decision to move the music.
//! - **A command is a state change, which the output obeys.** `pause`,
//!   `seek` and `transfer` move nothing but the session.
//! - **A press that means a sound takes an output that cannot answer; a
//!   device arriving takes nothing.** `play`, `skip` and `jump` hand the
//!   sound to the caller when the output is away or nowhere and the caller
//!   can play; `start` — a track pressed — does when nothing is sounding.
//!   `on_join` only adds the device to `devices`.
//!
//! **Volume is state, per device, and so is a group.** Every device that
//! has a volume keeps it in its own entry of `devices` — a level and
//! whether it is muted — and anybody can move it (`set_volume`,
//! `nudge_volume`, `set_mute`); the device obeys it as the output obeys the
//! clock, and says what was done to it by hand (`report_volume`). Speakers
//! of one family (a Sonos is a `sonos`) can be joined: a member *follows* a
//! leader, which plays and is followed, Sonos's coordinator model — so the
//! output is a group when anything follows it (`join`, `unjoin`,
//! `report_group`), and the group's volume moves every member together,
//! keeping their proportions (`set_group_volume`, `nudge_group_volume`,
//! `set_group_mute`). A phone, a browser and a desktop have no family: their
//! audio cannot be kept in step with anybody else's, so nothing joins them.
//!
//! Beside the declaration is a plain-Rust reading of it ([`read`]) for the
//! code that drives a player or a speaker from it, and [`url`], which every
//! device joins a track's file to its own server with.

use std::collections::BTreeMap;

use ark::authoring::*;

use crate::schema::*;

// ---------------------------------------------------------------------------
// The state

/// The live state: who is here, where each of them is dragging a scrubber,
/// and the session.
pub struct Listening {
    /// Who is here, kept by `on_join` and `on_leave`, with each device's
    /// volume and group: ordered — the server runs the hooks as its
    /// connections come and go, and every change of a volume or a group is a
    /// transition like any other.
    pub devices: PerDevice<Present>,
    /// Where each device's scrubber is being dragged to, in milliseconds
    /// into the current track: ephemeral, each device writing its own slot
    /// many times a second (`scrub`), never ordered and never kept.
    pub scrub: PerDevice<Int>,
    pub session: Sum<Session>,
}
impl Record for Listening {
    fn fields() -> Fields<Self> {
        fields()
            .named("listening")
            .field("devices", typed::<PerDevice<Present>>())
            .ephemeral("scrub", typed::<PerDevice<Int>>())
            .field("session", typed::<Sum<Session>>())
    }
}

/// A device standing in the session: what it said it is at `hello`, what it
/// can do, and where its volume and its group are.
///
/// Written in place in `devices` rather than named in the type section, so
/// a runtime's printed type keeps the name it had (`ListeningDevices` in
/// Kotlin).
#[derive(Clone, Copy)]
pub struct Present {
    pub can_play: Bool,
    /// Whether anybody can move its volume: what it said of itself
    /// (`equip`). Nothing has a volume until it says so.
    pub can_volume: Bool,
    /// Which speakers it can be joined with: the same family, present on
    /// both. None for anything that is not a speaker that groups — a phone,
    /// a browser, a desktop — whose audio cannot be kept in step with
    /// another's.
    pub family: Opt<Text>,
    /// The leader whose playback it follows, while it is joined to one.
    /// A device follows one leader at most, and a leader follows nobody.
    pub follows: Opt<Text>,
    pub id: Text,
    pub kind: Text,
    pub muted: Bool,
    pub name: Text,
    pub user: Text,
    /// 0 to 100.
    pub volume: Int,
}
impl Record for Present {
    fn fields() -> Fields<Self> {
        fields()
            .field("can_play", bool_())
            .field("can_volume", bool_())
            .field("family", opt(text()))
            .field("follows", opt(text()))
            .field("id", text())
            .field("kind", text())
            .field("muted", bool_())
            .field("name", text())
            .field("user", text())
            .field("volume", int())
    }
}

/// The level a device that has said nothing of its volume is drawn at.
pub const UNSAID_VOLUME: i64 = 100;

/// Nothing queued, or a queue with somewhere in it.
pub struct Session;
impl SumType for Session {
    const NAME: &'static str = "session";
    fn variants() -> Cases<Self> {
        cases().of::<Active>("active").of::<Idle>("idle")
    }
}

pub struct Idle {}
impl Record for Idle {
    fn fields() -> Fields<Self> {
        fields()
    }
}

/// A queue, where in it, whether it is playing, and on which device.
pub struct Active {
    /// The track playing: always inside the queue.
    pub at: Int,
    pub clock: Sum<Clock>,
    /// Where the queue was taken from, as the client that pressed it names
    /// it (a route: `album/Water Music`); empty for nowhere in particular.
    pub from: Text,
    pub output: Sum<Output>,
    pub queue: NonEmpty<Track>,
}
impl Record for Active {
    fn fields() -> Fields<Self> {
        fields()
            .field("at", int())
            .field("clock", typed::<Sum<Clock>>())
            .field("from", text())
            .field("output", typed::<Sum<Output>>())
            .field("queue", typed::<NonEmpty<Track>>())
    }
}

/// How far into the track, and whether that is moving: paused at a point,
/// or playing from a point since a moment of the server's clock.
pub struct Clock;
impl SumType for Clock {
    const NAME: &'static str = "clock";
    fn variants() -> Cases<Self> {
        cases().of::<Paused>("paused").of::<Playing>("playing")
    }
}

pub struct Paused {
    pub at_ms: Int,
}
impl Record for Paused {
    fn fields() -> Fields<Self> {
        fields().field("at_ms", int())
    }
}

pub struct Playing {
    pub at_ms: Int,
    pub since: Int,
}
impl Record for Playing {
    fn fields() -> Fields<Self> {
        fields().field("at_ms", int()).field("since", int())
    }
}

/// Which device the sound belongs to: one that is here (and, until it says
/// it has started, `moving`), one that has gone and keeps it, or none.
pub struct Output;
impl SumType for Output {
    const NAME: &'static str = "output";
    fn variants() -> Cases<Self> {
        cases().of::<Away>("away").of::<OutputDevice>("device").of::<Nowhere>("nowhere")
    }
}

/// The device the sound belongs to, gone: its name is kept beside its id,
/// because "Laptop, not answering" is the one row a picker most needs to
/// draw and the device is no longer in `devices` to be named from.
pub struct Away {
    pub id: Text,
    pub name: Text,
}
impl Record for Away {
    fn fields() -> Fields<Self> {
        fields().field("id", text()).field("name", text())
    }
}

pub struct OutputDevice {
    pub id: Text,
    pub moving: Bool,
}
impl Record for OutputDevice {
    fn fields() -> Fields<Self> {
        fields().field("id", text()).field("moving", bool_())
    }
}

pub struct Nowhere {}
impl Record for Nowhere {
    fn fields() -> Fields<Self> {
        fields()
    }
}

/// A track, carried rather than looked up: everything needed to play it,
/// so a device handed the session need not find it in its own replica
/// first. `file` is the log's path, which each device joins to its own
/// server ([`url`]).
pub struct Track {
    pub album: Text,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub title: Text,
}
impl Record for Track {
    fn fields() -> Fields<Self> {
        fields()
            .named("track")
            .field("album", text())
            .field("creator", text())
            .field("duration_ms", int())
            .field("file", text())
            .field("id", id::<Media>())
            .field("title", text())
    }
}

// The inputs -------------------------------------------------------------------

pub struct Start {
    pub at: Int,
    pub from: Text,
    pub position: Int,
    pub queue: NonEmpty<Track>,
}
impl Input for Start {
    fn schema() -> Object<Self> {
        object()
            .field("at", int().at_least(0))
            .field("from", text())
            .field("position", int().at_least(0))
            .field("queue", typed::<NonEmpty<Track>>())
    }
}

pub struct Seek {
    pub ms: Int,
}
impl Input for Seek {
    fn schema() -> Object<Self> {
        object().field("ms", int().at_least(0))
    }
}

pub struct Skip {
    pub by: Int,
}
impl Input for Skip {
    fn schema() -> Object<Self> {
        object().field("by", int().range(-1, 1).refine(|by| by.ne(0)).why("a skip is one track either way"))
    }
}

pub struct Jump {
    pub index: Int,
}
impl Input for Jump {
    fn schema() -> Object<Self> {
        object().field("index", int().at_least(0))
    }
}

pub struct Transfer {
    pub to: Text,
}
impl Input for Transfer {
    fn schema() -> Object<Self> {
        object().field("to", text())
    }
}

/// What the output says of where it is: `started` and `stalled`.
pub struct At {
    pub at_ms: Int,
}
impl Input for At {
    fn schema() -> Object<Self> {
        object().field("at_ms", int().at_least(0))
    }
}

pub struct Advance {
    pub from_track: Int,
}
impl Input for Advance {
    fn schema() -> Object<Self> {
        object().field("from_track", int())
    }
}

pub struct Scrub {
    pub at_ms: Opt<Int>,
}
impl Input for Scrub {
    fn schema() -> Object<Self> {
        object().field("at_ms", opt(int().at_least(0)))
    }
}

/// What a device says of itself once it stands in the session: whether its
/// volume can be moved, which family of speakers it joins, and where its
/// volume is now.
pub struct Equip {
    pub can_volume: Bool,
    pub family: Opt<Text>,
    pub muted: Bool,
    pub volume: Int,
}
impl Input for Equip {
    fn schema() -> Object<Self> {
        object()
            .field("can_volume", bool_())
            .field("family", opt(text().min(1)))
            .field("muted", bool_())
            .field("volume", int().range(0, 100))
    }
}

/// `set_volume`: a device's level, from anywhere.
pub struct SetVolume {
    pub device: Text,
    pub level: Int,
}
impl Input for SetVolume {
    fn schema() -> Object<Self> {
        object().field("device", text()).field("level", int().range(0, 100))
    }
}

/// `nudge_volume`: a device's level moved by `delta`, clamped — what a
/// volume button sends, since it knows which way and not where to.
pub struct NudgeVolume {
    pub delta: Int,
    pub device: Text,
}
impl Input for NudgeVolume {
    fn schema() -> Object<Self> {
        object()
            .field("delta", int().range(-100, 100).refine(|d| d.ne(0)).why("a nudge moves the volume"))
            .field("device", text())
    }
}

pub struct SetMute {
    pub device: Text,
    pub muted: Bool,
}
impl Input for SetMute {
    fn schema() -> Object<Self> {
        object().field("device", text()).field("muted", bool_())
    }
}

/// `report_volume`: the device's own word on its volume, changed on the
/// speaker or by the operating system.
pub struct ReportVolume {
    pub level: Int,
    pub muted: Bool,
}
impl Input for ReportVolume {
    fn schema() -> Object<Self> {
        object().field("level", int().range(0, 100)).field("muted", bool_())
    }
}

/// `join` and `unjoin`: which speaker.
pub struct Member {
    pub device: Text,
}
impl Input for Member {
    fn schema() -> Object<Self> {
        object().field("device", text())
    }
}

/// `report_group`: the leader's word on who follows it, changed outside
/// harken — the Sonos app, Home Assistant.
pub struct ReportGroup {
    pub members: List<Text>,
}
impl Input for ReportGroup {
    fn schema() -> Object<Self> {
        object().field("members", list(text()))
    }
}

pub struct GroupVolume {
    pub level: Int,
}
impl Input for GroupVolume {
    fn schema() -> Object<Self> {
        object().field("level", int().range(0, 100))
    }
}

pub struct GroupNudge {
    pub delta: Int,
}
impl Input for GroupNudge {
    fn schema() -> Object<Self> {
        object().field("delta", int().range(-100, 100).refine(|d| d.ne(0)).why("a nudge moves the volume"))
    }
}

pub struct GroupMute {
    pub muted: Bool,
}
impl Input for GroupMute {
    fn schema() -> Object<Self> {
        object().field("muted", bool_())
    }
}

// The helpers ------------------------------------------------------------------

/// How far into the track a clock is at `now`: where it was paused, or
/// where it was playing from and for how long since.
pub fn position(clock: Sum<Clock>, now: Int) -> Int {
    helper("position", (("clock", clock), ("now", now)), |clock: Sum<Clock>, now: Int| {
        clock
            .case::<Int>()
            .on::<Paused>("paused", |p| p.at_ms)
            .on::<Playing>("playing", |p| p.at_ms.add(now.sub(p.since)).max(p.at_ms))
            .end()
    })
}

/// The clock paused where it is at `now`.
pub fn paused_at(clock: Sum<Clock>, now: Int) -> Sum<Clock> {
    helper("paused_at", (("clock", clock), ("now", now)), |clock: Sum<Clock>, now: Int| {
        Sum::of("paused", Paused { at_ms: position(clock, now) })
    })
}

/// The clock at `ms`, playing on from there if it was playing.
pub fn clock_at(clock: Sum<Clock>, ms: Int, now: Int) -> Sum<Clock> {
    helper(
        "clock_at",
        (("clock", clock), ("ms", ms), ("now", now)),
        |clock: Sum<Clock>, ms: Int, now: Int| {
            clock
                .case::<Sum<Clock>>()
                .on::<Paused>("paused", |_| Sum::of("paused", Paused { at_ms: ms }))
                .on::<Playing>("playing", |_| Sum::of("playing", Playing { at_ms: ms, since: now }))
                .end()
        },
    )
}

/// Whether the session has an output that is here to answer.
pub fn reachable(session: Sum<Session>) -> Bool {
    helper("reachable", ("session", session), |session: Sum<Session>| {
        session
            .case::<Bool>()
            .on::<Active>("active", |a| a.output.is("device"))
            .on::<Idle>("idle", |_| false.into())
            .end()
    })
}

/// Whether something is making a sound: a clock running on an output that
/// is here. An output that has gone is always paused (`on_leave`).
pub fn sounding(session: Sum<Session>) -> Bool {
    helper("sounding", ("session", session), |session: Sum<Session>| {
        session
            .case::<Bool>()
            .on::<Active>("active", |a| a.output.is("device").and(a.clock.is("playing")))
            .on::<Idle>("idle", |_| false.into())
            .end()
    })
}

/// Whether the device is here and can make a sound.
pub fn can_play(devices: PerDevice<Present>, id: Text) -> Bool {
    helper("can_play", (("devices", devices), ("id", id)), |devices: PerDevice<Present>, id: Text| {
        devices.get(id).map_or(false, |d| d.can_play)
    })
}

/// Whether the device is the session's output and here.
pub fn is_output(session: Sum<Session>, id: Text) -> Bool {
    helper("is_output", (("session", session), ("id", id)), |session: Sum<Session>, id: Text| {
        session
            .case::<Bool>()
            .on::<Active>("active", |a| {
                a.output
                    .case::<Bool>()
                    .on::<Away>("away", |_| false.into())
                    .on::<OutputDevice>("device", |d| d.id.eq(id))
                    .on::<Nowhere>("nowhere", |_| false.into())
                    .end()
            })
            .on::<Idle>("idle", |_| false.into())
            .end()
    })
}

/// The output a press that means a sound leaves: the one there is, where it
/// can answer, or the caller.
pub fn pressed(output: Sum<Output>, caller: Text) -> Sum<Output> {
    helper(
        "pressed",
        (("output", output), ("caller", caller)),
        |output: Sum<Output>, caller: Text| pick(output.is("device"), output, here(caller)),
    )
}

/// The sound on this device, which has it.
pub fn here(id: Text) -> Sum<Output> {
    helper("here", ("id", id), |id: Text| {
        Sum::of("device", OutputDevice { id, moving: false.into() })
    })
}

/// The output once `id` has come back: given back to it, if it was the one
/// that went.
pub fn returned(output: Sum<Output>, id: Text) -> Sum<Output> {
    helper("returned", (("output", output), ("id", id)), |output: Sum<Output>, id: Text| {
        output
            .case::<Sum<Output>>()
            .on::<Away>("away", |a| pick(a.id.eq(id), here(a.id), output))
            .on::<OutputDevice>("device", |_| output)
            .on::<Nowhere>("nowhere", |_| output)
            .end()
    })
}

/// How long the track at `at` is: the queue counted through once, since a
/// list is not indexed.
pub fn duration_at(queue: List<Track>, at: Int) -> Int {
    helper("duration_at", (("queue", queue), ("at", at)), |queue: List<Track>, at: Int| {
        queue
            .fold(Counted { i: 0.into(), ms: 0.into() }, |acc: Counted, t| Counted {
                i: acc.i.add(1),
                ms: pick(acc.i.eq(at), t.duration_ms, acc.ms),
            })
            .ms
    })
}

/// The fold of [`duration_at`]: how many tracks counted, and the one found.
pub struct Counted {
    pub i: Int,
    pub ms: Int,
}
impl Record for Counted {
    fn fields() -> Fields<Self> {
        fields().field("i", int()).field("ms", int())
    }
}

/// A session at a new place in its queue, playing from the start of it, on
/// whichever device a press leaves the sound with.
fn moved_to(a: Active, at: Int, caller: Text, now: Int) -> Sum<Session> {
    Sum::of(
        "active",
        Active {
            at,
            clock: Sum::of("playing", Playing { at_ms: 0.into(), since: now }),
            output: pressed(a.output, caller),
            ..a
        },
    )
}

/// The session as it is, for the arm that changes nothing.
fn idle() -> Sum<Session> {
    Sum::of("idle", Idle {})
}

// Volumes and groups ------------------------------------------------------------

/// A device as `on_join` first writes it: what it declared, and nothing it
/// has not said — no volume anybody can move, no family, following nobody.
pub fn arrived(device: Device) -> Present {
    Present {
        can_play: device.can_play,
        can_volume: false.into(),
        family: none(),
        follows: none(),
        id: device.id,
        kind: device.kind,
        muted: false.into(),
        name: device.name,
        user: device.user,
        volume: UNSAID_VOLUME.into(),
    }
}

/// The output, where it is a device that is here: the one a group is led
/// by.
pub fn output_id(session: Sum<Session>) -> Opt<Text> {
    helper("output_id", ("session", session), |session: Sum<Session>| {
        session
            .case::<Opt<Text>>()
            .on::<Active>("active", |a| {
                a.output
                    .case::<Opt<Text>>()
                    .on::<Away>("away", |_| none())
                    .on::<OutputDevice>("device", |d| some(d.id))
                    .on::<Nowhere>("nowhere", |_| none())
                    .end()
            })
            .on::<Idle>("idle", |_| none())
            .end()
    })
}

/// Whether the device can have its volume moved: here, and said it has one.
pub fn has_volume(devices: PerDevice<Present>, id: Text) -> Bool {
    helper(
        "has_volume",
        (("devices", devices), ("id", id)),
        |devices: PerDevice<Present>, id: Text| devices.get(id).map_or(false, |d| d.can_volume),
    )
}

/// Whether `d` is in the group `leader` leads: the leader, or following it.
pub fn in_group(d: Present, leader: Text) -> Bool {
    helper("in_group", (("d", d), ("leader", leader)), |d: Present, leader: Text| {
        d.id.eq(leader).or(d.follows.eq(some(leader)))
    })
}

/// Whether `id` can be joined to the group `leader` leads now: a speaker
/// that is here, can play, and is of the leader's family — which both have
/// — and not the leader itself, which follows nobody.
pub fn joinable(devices: PerDevice<Present>, leader: Text, id: Text) -> Bool {
    helper(
        "joinable",
        (("devices", devices), ("leader", leader), ("id", id)),
        |devices: PerDevice<Present>, leader: Text, id: Text| {
            let family = devices
                .get(leader)
                .map_or(none::<Text>(), |l| -> Opt<Text> { pick(l.follows.is_none(), l.family, none::<Text>()) });
            devices.get(id).map_or(false, |d| {
                d.id.ne(leader).and(d.can_play).and(d.family.is_some()).and(d.family.eq(family))
            })
        },
    )
}

/// The fold of [`group_level`]: how many members have a volume, and its sum.
#[derive(Clone, Copy)]
pub struct Tally {
    pub n: Int,
    pub sum: Int,
}
impl Record for Tally {
    fn fields() -> Fields<Self> {
        fields().field("n", int()).field("sum", int())
    }
}

/// How many of the group `leader` leads have a volume, and their sum.
pub fn group_tally(devices: PerDevice<Present>, leader: Text) -> Tally {
    helper(
        "group_tally",
        (("devices", devices), ("leader", leader)),
        |devices: PerDevice<Present>, leader: Text| {
            devices.slots().fold(Tally { n: 0.into(), sum: 0.into() }, |acc: Tally, s| {
                pick(
                    in_group(s.value, leader).and(s.value.can_volume),
                    Tally {
                        n: acc.n.add(1),
                        sum: acc.sum.add(s.value.volume),
                    },
                    acc,
                )
            })
        },
    )
}

/// The group's volume: the mean of its members', as Sonos reads a group's
/// level — rounded down, and 0 for a group with no volume at all.
pub fn group_level(devices: PerDevice<Present>, leader: Text) -> Int {
    helper(
        "group_level",
        (("devices", devices), ("leader", leader)),
        |devices: PerDevice<Present>, leader: Text| {
            let t = group_tally(devices, leader);
            t.sum.div(t.n.max(1))
        },
    )
}

/// One member's level once the group's moves from `from` to `to`: in
/// proportion, rounded to the nearest, clamped to 0 and 100 — and, from a
/// group at 0, where there is no proportion to keep, `to` itself.
pub fn scaled(volume: Int, from: Int, to: Int) -> Int {
    helper(
        "scaled",
        (("volume", volume), ("from", from), ("to", to)),
        |volume: Int, from: Int, to: Int| pick(from.eq(0), to, volume.mul(to).mul(2).add(from).div(from.max(1).mul(2)).clamp(0, 100)),
    )
}

/// The group `leader` leads at the level `to`, every member with a volume
/// moved in proportion and unmuted.
pub fn group_at(devices: PerDevice<Present>, leader: Text, to: Int) -> PerDevice<Present> {
    helper(
        "group_at",
        (("devices", devices), ("leader", leader), ("to", to)),
        |devices: PerDevice<Present>, leader: Text, to: Int| {
            let from = group_level(devices, leader);
            devices.slots().fold(devices, |acc: PerDevice<Present>, s| {
                let d = s.value;
                acc.put(
                    s.device,
                    pick::<Present>(
                        in_group(d, leader).and(d.can_volume),
                        Present {
                            volume: scaled(d.volume, from, to),
                            muted: false.into(),
                            ..d
                        },
                        d,
                    ),
                )
            })
        },
    )
}

/// The group `leader` leads, every member with a volume muted or not.
pub fn group_muted(devices: PerDevice<Present>, leader: Text, muted: Bool) -> PerDevice<Present> {
    helper(
        "group_muted",
        (("devices", devices), ("leader", leader), ("muted", muted)),
        |devices: PerDevice<Present>, leader: Text, muted: Bool| {
            devices.slots().fold(devices, |acc: PerDevice<Present>, s| {
                let d = s.value;
                acc.put(
                    s.device,
                    pick::<Present>(in_group(d, leader).and(d.can_volume), Present { muted, ..d }, d),
                )
            })
        },
    )
}

/// `id` follows nobody, and nobody follows it: its group, whichever side of
/// it it was on, is gone.
pub fn dissolved(devices: PerDevice<Present>, id: Text) -> PerDevice<Present> {
    helper(
        "dissolved",
        (("devices", devices), ("id", id)),
        |devices: PerDevice<Present>, id: Text| {
            devices.slots().fold(devices, |acc: PerDevice<Present>, s| {
                let d = s.value;
                acc.put(
                    s.device,
                    pick::<Present>(d.id.eq(id).or(d.follows.eq(some(id))), Present { follows: none(), ..d }, d),
                )
            })
        },
    )
}

/// `id` follows `leader`; whoever followed `id` follows nobody, since a
/// member leads nothing.
pub fn followed(devices: PerDevice<Present>, id: Text, leader: Text) -> PerDevice<Present> {
    helper(
        "followed",
        (("devices", devices), ("id", id), ("leader", leader)),
        |devices: PerDevice<Present>, id: Text, leader: Text| {
            devices.slots().fold(devices, |acc: PerDevice<Present>, s| {
                let d = s.value;
                acc.put(
                    s.device,
                    pick::<Present>(
                        d.id.eq(id),
                        Present { follows: some(leader), ..d },
                        pick::<Present>(d.follows.eq(some(id)), Present { follows: none(), ..d }, d),
                    ),
                )
            })
        },
    )
}

/// The sound handed to `to`: if it was following a leader, it leads that
/// group now — the old leader and every other member follow it — as Sonos
/// makes a member the coordinator when it is picked. A device that follows
/// nobody leads whatever follows it already, which is a leader keeping its
/// group.
pub fn led_by(devices: PerDevice<Present>, to: Text) -> PerDevice<Present> {
    helper("led_by", (("devices", devices), ("to", to)), |devices: PerDevice<Present>, to: Text| {
        let old = devices.get(to).map_or(none::<Text>(), |t| -> Opt<Text> { t.follows });
        devices.slots().fold(devices, |acc: PerDevice<Present>, s| {
            let d = s.value;
            let was = old.is_some().and(some(d.id).eq(old).or(d.follows.eq(old)));
            acc.put(
                s.device,
                pick::<Present>(
                    d.id.eq(to),
                    Present { follows: none(), ..d },
                    pick::<Present>(was, Present { follows: some(to), ..d }, d),
                ),
            )
        })
    })
}

/// The leader `caller`'s word on who follows it: each of `members` that is
/// here and of its family follows it, whoever followed it and is not named
/// follows nobody, and so does whoever followed a device that is a member
/// now. The caller itself leads, and follows nobody.
pub fn regrouped(devices: PerDevice<Present>, caller: Text, members: List<Text>) -> PerDevice<Present> {
    helper(
        "regrouped",
        (("devices", devices), ("caller", caller), ("members", members)),
        |devices: PerDevice<Present>, caller: Text, members: List<Text>| {
            let family = devices.get(caller).map_or(none::<Text>(), |c| -> Opt<Text> { c.family });
            devices.slots().fold(devices, |acc: PerDevice<Present>, s| {
                let d = s.value;
                let named = members
                    .contains(d.id)
                    .and(d.id.ne(caller))
                    .and(d.can_play)
                    .and(family.is_some())
                    .and(d.family.eq(family));
                let behind = d.follows.map_or(false, |l| members.contains(l));
                let follows = pick::<Opt<Text>>(
                    d.id.eq(caller),
                    none::<Text>(),
                    pick::<Opt<Text>>(
                        named,
                        some(caller),
                        pick::<Opt<Text>>(d.follows.eq(some(caller)).or(behind), none::<Text>(), d.follows),
                    ),
                );
                acc.put(s.device, Present { follows, ..d })
            })
        },
    )
}

/// A device's entry with its volume moved: what `set_volume`,
/// `nudge_volume`, `set_mute` and `report_volume` write.
fn tuned(s: Listening, id: Text, f: impl FnOnce(Present) -> Present) -> Listening {
    let d = s.devices.get(id).unwrap();
    Listening {
        devices: s.devices.put(id, f(d)),
        ..s
    }
}

/// The group the output leads, moved by `f`: what the group transitions
/// write.
fn grouped(s: Listening, f: impl FnOnce(PerDevice<Present>, Text) -> PerDevice<Present>) -> Listening {
    let leader = output_id(s.session).unwrap();
    Listening {
        devices: f(s.devices, leader),
        ..s
    }
}

/// Whether the output is a device that is here and something in its group
/// has a volume: what every group transition is guarded by.
fn group_has_volume(s: Listening) -> Bool {
    output_id(s.session).map_or(false, |leader| group_tally(s.devices, leader).n.gt(0))
}

// ---------------------------------------------------------------------------
// The plays

/// What `record_play` takes: the track as the session carried it, and
/// where it was played from.
pub struct RecordPlay {
    pub track: Id<Media>,
    pub title: Text,
    pub creator: Text,
    pub album: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub from: Text,
}
impl Input for RecordPlay {
    fn schema() -> Object<Self> {
        object()
            .field("track", id::<Media>())
            .field("title", text())
            .field("creator", text())
            .field("album", text())
            .field("duration_ms", int().at_least(0))
            .field("file", text())
            .field("from", text())
    }
}

/// How many plays `recently_played` lists.
pub const RECENT: i64 = 50;

/// The router: the listening session, and the plays.
pub fn listening() -> Router<Harken> {
    let r = router::<Harken>("listening");
    r.live::<Listening>("listening")
        // One instance per account.
        .scope(|ctx| ctx.user)
        .init(|| Listening {
            devices: PerDevice::empty(),
            scrub: PerDevice::empty(),
            session: idle(),
        })
        // A track pressed: this queue, from here, at this point, playing.
        // While something is sounding the press goes to the device making
        // it; otherwise the caller takes the sound if it can play, and a
        // paused output that is here keeps it if the caller cannot. This
        // is also what a device that was playing offline says when it comes
        // back to a session with nothing sounding: its queue and position
        // become the session.
        .transition::<Start>("start", |s, ctx, input| {
            let keep = sounding(s.session).or(can_play(s.devices, ctx.device).not());
            let output = s
                .session
                .case::<Sum<Output>>()
                .on::<Active>("active", |a| pick(keep, a.output, here(ctx.device)))
                .on::<Idle>("idle", |_| here(ctx.device))
                .end();
            Listening {
                session: Sum::of(
                    "active",
                    Active {
                        at: input.at,
                        clock: Sum::of(
                            "playing",
                            Playing {
                                at_ms: input.position,
                                since: ctx.now,
                            },
                        ),
                        from: input.from,
                        output,
                        queue: input.queue,
                    },
                ),
                ..s
            }
        })
        .guard::<Start>("the track is not in the queue", |_s, _ctx, input| input.at.lt(input.queue.list().len()))
        .guard::<Start>("nothing here can play it", |s, ctx, _input| {
            sounding(s.session).or(reachable(s.session)).or(can_play(s.devices, ctx.device))
        })
        // Resume, on the output where it can answer, else on the caller.
        .transition::<()>("play", |s, ctx, ()| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: Sum::of(
                                "playing",
                                Playing {
                                    at_ms: position(a.clock, ctx.now),
                                    since: ctx.now,
                                },
                            ),
                            output: pressed(a.output, ctx.device),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<()>("nothing is queued", |s, _ctx, ()| s.session.is("active"))
        .guard::<()>("nothing here can play it", |s, ctx, ()| {
            reachable(s.session).or(can_play(s.devices, ctx.device))
        })
        // Pause, wherever the sound is. Nobody takes anything.
        .transition::<()>("pause", |s, ctx, ()| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: paused_at(a.clock, ctx.now),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<()>("nothing is queued", |s, _ctx, ()| s.session.is("active"))
        // Somewhere else in the track, playing on if it was.
        .transition::<Seek>("seek", |s, ctx, input| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: clock_at(a.clock, input.ms, ctx.now),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<Seek>("nothing is queued", |s, _ctx, _input| s.session.is("active"))
        // The next track or the one before, from its start; before the first
        // is the first again. A press that means a sound.
        .transition::<Skip>("skip", |s, ctx, input| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    let at = a.at.add(input.by).max(0);
                    moved_to(a, at, ctx.device, ctx.now)
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<Skip>("nothing is queued", |s, _ctx, _input| s.session.is("active"))
        .guard::<Skip>("nothing comes after this", |s, _ctx, input| {
            s.session
                .case::<Bool>()
                .on::<Active>("active", |a| input.by.lt(0).or(a.at.add(1).lt(a.queue.list().len())))
                .on::<Idle>("idle", |_| false.into())
                .end()
        })
        .guard::<Skip>("nothing here can play it", |s, ctx, _input| {
            reachable(s.session).or(can_play(s.devices, ctx.device))
        })
        // A track of the queue, from its start. A press that means a sound.
        .transition::<Jump>("jump", |s, ctx, input| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| moved_to(a, input.index, ctx.device, ctx.now))
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<Jump>("nothing is queued", |s, _ctx, _input| s.session.is("active"))
        .guard::<Jump>("the track is not in the queue", |s, _ctx, input| {
            s.session
                .case::<Bool>()
                .on::<Active>("active", |a| input.index.lt(a.queue.list().len()))
                .on::<Idle>("idle", |_| false.into())
                .end()
        })
        .guard::<Jump>("nothing here can play it", |s, ctx, _input| {
            reachable(s.session).or(can_play(s.devices, ctx.device))
        })
        // Move the sound to a device that is here and can play. It is
        // `moving` until it says it has started; handing it to the device
        // that already has it, settled, changes nothing. The device losing
        // it is told nothing: it reads the session and goes quiet.
        .transition::<Transfer>("transfer", |s, _ctx, input| Listening {
            // A speaker that was following another leads its group now;
            // a leader keeps the group it has.
            devices: led_by(s.devices, input.to),
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            output: pick(
                                a.output.eq(here(input.to)),
                                a.output,
                                Sum::of(
                                    "device",
                                    OutputDevice {
                                        id: input.to,
                                        moving: true.into(),
                                    },
                                ),
                            ),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<Transfer>("nothing is playing to move", |s, _ctx, _input| s.session.is("active"))
        .guard::<Transfer>("that device is not here, or cannot play", |s, _ctx, input| can_play(s.devices, input.to))
        // Nothing queued, nothing playing, nowhere.
        .transition::<()>("stop", |s, _ctx, ()| Listening { session: idle(), ..s })
        // The output has the hand-off, at this point in the track.
        .transition::<At>("started", |s, ctx, input| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: clock_at(a.clock, input.at_ms, ctx.now),
                            output: here(ctx.device),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<At>("not the output", |s, ctx, _input| is_output(s.session, ctx.device))
        // The output's track ended: the next one, or paused at the start of
        // the last when there is none. Said about a track that is no longer
        // the one playing — somebody moved the session first — it is nothing.
        .transition::<Advance>("advance", |s, ctx, input| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    pick(
                        a.at.ne(input.from_track),
                        s.session,
                        pick::<Sum<Session>>(
                            a.at.add(1).lt(a.queue.list().len()),
                            Sum::of(
                                "active",
                                Active {
                                    at: a.at.add(1),
                                    clock: Sum::of(
                                        "playing",
                                        Playing {
                                            at_ms: 0.into(),
                                            since: ctx.now,
                                        },
                                    ),
                                    ..a
                                },
                            ),
                            Sum::of(
                                "active",
                                Active {
                                    clock: Sum::of("paused", Paused { at_ms: 0.into() }),
                                    ..a
                                },
                            ),
                        ),
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<Advance>("not the output", |s, ctx, _input| is_output(s.session, ctx.device))
        // The output stopped making a sound by itself, here: a stream that
        // ran out, a speaker paused by hand.
        .transition::<At>("stalled", |s, _ctx, input| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: Sum::of("paused", Paused { at_ms: input.at_ms }),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<At>("not the output", |s, ctx, _input| is_output(s.session, ctx.device))
        // The output lets the sound go: a speaker somebody else took, or one
        // playing something that is not this queue, or not answering. The
        // queue stays, paused, and nothing is the output.
        .transition::<()>("release", |s, ctx, ()| Listening {
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: paused_at(a.clock, ctx.now),
                            output: Sum::of("nowhere", Nowhere {}),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        })
        .guard::<()>("not the output", |s, ctx, ()| is_output(s.session, ctx.device))
        // What a device says of itself once it stands here: whether its
        // volume can be moved, which family of speakers it joins, and where
        // its volume is. Said once a connection, so it describes the device
        // and not whatever `on_join` assumed. A family that moves takes the
        // device out of any group it was in.
        .transition::<Equip>("equip", |s, ctx, input| {
            let me = s.devices.get(ctx.device).unwrap();
            let devices = pick::<PerDevice<Present>>(me.family.eq(input.family), s.devices, dissolved(s.devices, ctx.device));
            let me = devices.get(ctx.device).unwrap();
            Listening {
                devices: devices.put(
                    ctx.device,
                    Present {
                        can_volume: input.can_volume,
                        family: input.family,
                        muted: input.muted,
                        volume: input.volume,
                        ..me
                    },
                ),
                ..s
            }
        })
        .guard::<Equip>("not here", |s, ctx, _input| s.devices.get(ctx.device).is_some())
        // A device's volume, from anywhere: a slider dragged, a level
        // picked. Moving the volume unmutes, as a volume button does.
        .transition::<SetVolume>("set_volume", |s, _ctx, input| {
            tuned(s, input.device, |d| Present {
                volume: input.level,
                muted: false.into(),
                ..d
            })
        })
        .guard::<SetVolume>("that device is not here, or has no volume", |s, _ctx, input| {
            has_volume(s.devices, input.device)
        })
        // …moved by so much, clamped: what a volume button sends.
        .transition::<NudgeVolume>("nudge_volume", |s, _ctx, input| {
            tuned(s, input.device, |d| Present {
                volume: d.volume.add(input.delta).clamp(0, 100),
                muted: false.into(),
                ..d
            })
        })
        .guard::<NudgeVolume>("that device is not here, or has no volume", |s, _ctx, input| {
            has_volume(s.devices, input.device)
        })
        .transition::<SetMute>("set_mute", |s, _ctx, input| {
            tuned(s, input.device, |d| Present { muted: input.muted, ..d })
        })
        .guard::<SetMute>("that device is not here, or has no volume", |s, _ctx, input| {
            has_volume(s.devices, input.device)
        })
        // The device's own word: its volume was changed on the speaker, or
        // by the operating system. Its own entry only — and ordered, because
        // everybody else reads it.
        .transition::<ReportVolume>("report_volume", |s, ctx, input| {
            tuned(s, ctx.device, |d| Present {
                volume: input.level,
                muted: input.muted,
                ..d
            })
        })
        .guard::<ReportVolume>("not here, or no volume", |s, ctx, _input| has_volume(s.devices, ctx.device))
        // A speaker joins the output's group: it follows the output, playing
        // what it plays. Whatever followed the speaker follows nobody.
        .transition::<Member>("join", |s, _ctx, input| {
            grouped(s, |devices, leader| followed(devices, input.device, leader))
        })
        .guard::<Member>("nothing is playing to join", |s, _ctx, _input| output_id(s.session).is_some())
        .guard::<Member>("that device cannot join this one", |s, _ctx, input| {
            output_id(s.session).map_or(false, |leader| joinable(s.devices, leader, input.device))
        })
        // …and leaves whatever group it is in, and goes quiet. A leader
        // follows nobody, so it is not unjoined: the sound handed to a
        // member first makes that member the leader.
        .transition::<Member>("unjoin", |s, _ctx, input| tuned(s, input.device, |d| Present { follows: none(), ..d }))
        .guard::<Member>("that device is not in a group", |s, _ctx, input| {
            s.devices.get(input.device).map_or(false, |d| d.follows.is_some())
        })
        // A leader's word on who follows it, changed outside harken.
        .transition::<ReportGroup>("report_group", |s, ctx, input| Listening {
            devices: regrouped(s.devices, ctx.device, input.members),
            ..s
        })
        .guard::<ReportGroup>("not here, or not a speaker that groups", |s, ctx, _input| {
            s.devices.get(ctx.device).map_or(false, |d| d.family.is_some())
        })
        // The output's group at a level: every member with a volume moved
        // in proportion — Sonos's group volume — and unmuted. A group of one
        // is its leader's volume.
        .transition::<GroupVolume>("set_group_volume", |s, _ctx, input| {
            grouped(s, |devices, leader| group_at(devices, leader, input.level))
        })
        .guard::<GroupVolume>("nothing playing has a volume", |s, _ctx, _input| group_has_volume(s))
        .transition::<GroupNudge>("nudge_group_volume", |s, _ctx, input| {
            grouped(s, |devices, leader| {
                group_at(devices, leader, group_level(devices, leader).add(input.delta).clamp(0, 100))
            })
        })
        .guard::<GroupNudge>("nothing playing has a volume", |s, _ctx, _input| group_has_volume(s))
        .transition::<GroupMute>("set_group_mute", |s, _ctx, input| {
            grouped(s, |devices, leader| group_muted(devices, leader, input.muted))
        })
        .guard::<GroupMute>("nothing playing has a volume", |s, _ctx, _input| group_has_volume(s))
        // Ephemeral: where this device's scrubber is being dragged to, or
        // nowhere once it is let go. It reads nothing of the state.
        .transition::<Scrub>("scrub", |s, ctx, input| Listening {
            scrub: s.scrub.set(ctx.device, input.at_ms),
            ..s
        })
        // A device arriving takes nothing; one the sound stayed with gets it
        // back, still paused.
        .on_join(|s, _ctx, device| {
            let id = device.id;
            Listening {
                devices: s.devices.put(id, arrived(device)),
                session: s
                    .session
                    .case::<Sum<Session>>()
                    .on::<Active>("active", |a| {
                        Sum::of(
                            "active",
                            Active {
                                output: returned(a.output, id),
                                ..a
                            },
                        )
                    })
                    .on::<Idle>("idle", |_| idle())
                    .end(),
                ..s
            }
        })
        // A device leaving is gone from `devices`, and so is its group —
        // whoever followed it follows nobody; the output leaving keeps the
        // sound, away, and the clock stops, because nothing is on the other
        // end to be making one.
        .on_leave(|s, ctx, device| {
            let (id, name) = (device.id, device.name);
            Listening {
                devices: dissolved(s.devices, id).clear(id),
                session: s
                    .session
                    .case::<Sum<Session>>()
                    .on::<Active>("active", |a| {
                        pick(
                            is_output(s.session, id),
                            Sum::of(
                                "active",
                                Active {
                                    clock: paused_at(a.clock, ctx.now),
                                    output: Sum::of("away", Away { id, name }),
                                    ..a
                                },
                            ),
                            s.session,
                        )
                    })
                    .on::<Idle>("idle", |_| idle())
                    .end(),
                ..s
            }
        })
        // A restart: the queue as it was, paused where it had got to (never
        // past the end of the track), with no output and nobody here —
        // whoever was here when the server stopped never left, and nothing
        // is making a sound for a session nobody has rejoined.
        .on_restore(|s, ctx| Listening {
            devices: PerDevice::empty(),
            session: s
                .session
                .case::<Sum<Session>>()
                .on::<Active>("active", |a| {
                    Sum::of(
                        "active",
                        Active {
                            clock: Sum::of(
                                "paused",
                                Paused {
                                    at_ms: position(a.clock, ctx.now).min(duration_at(a.queue.list(), a.at)),
                                },
                            ),
                            output: Sum::of("nowhere", Nowhere {}),
                            ..a
                        },
                    )
                })
                .on::<Idle>("idle", |_| idle())
                .end(),
            ..s
        });
    r.routes((
        // A track was played: authored by the device making the sound, once
        // a track has played thirty seconds or half its length, whichever is
        // first, once per play. When is the author's clock, frozen in the
        // entry as an auto (`played_ms`), so a play made offline is dated
        // when it was made and reaches the server whenever the device does.
        // The track is carried as it was heard, and need not be in the
        // library (`schema::Play`).
        r.input::<RecordPlay>().mutation("record_play", |ctx, db, input| {
            db.play.insert(Play {
                id: ctx.new_id("id"),
                user_id: ctx.user,
                track: input.track.to_text(),
                title: input.title,
                creator: input.creator,
                album: input.album,
                duration_ms: input.duration_ms,
                file: input.file,
                played_ms: ctx.now("played_ms"),
                from: input.from,
            })
        }),
        // The caller's newest play: what Home says was last played when
        // nothing is playing now — the same on every device, and there
        // offline.
        r.query("last_played", |ctx, db, _input: ()| {
            db.play
                .filter(Play::user_id.eq(ctx.user))
                .order_by((Play::played_ms.desc(), Play::id.desc()))
                .limit(1)
        }),
        // The caller's plays, newest first, the last [`RECENT`] of them.
        r.query("recently_played", |ctx, db, _input: ()| {
            db.play
                .filter(Play::user_id.eq(ctx.user))
                .order_by((Play::played_ms.desc(), Play::id.desc()))
                .limit(RECENT)
        }),
    ))
}

// ---------------------------------------------------------------------------
// Reading it

/// The state as plain Rust: what a player, a speaker or a screen is driven
/// from. Decoded from the value `ark_client::Peer::live` hands back; the
/// shapes are the declaration's above, one for one, and a value that is not
/// one is `None` rather than a guess.
pub mod read {
    use super::*;
    use ark::value::{Id, Value};

    /// The name of the live state.
    pub const STATE: &str = "listening";

    /// What sort of thing a device is, for the glyph a picker draws.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Kind {
        #[default]
        Computer,
        Phone,
        /// Something in the house, reached through Home Assistant.
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

        /// A kind this build has never heard of reads as a computer, so an
        /// older client still lists a newer device.
        pub fn parse(s: &str) -> Kind {
            match s {
                "phone" => Kind::Phone,
                "speaker" => Kind::Speaker,
                _ => Kind::Computer,
            }
        }
    }

    /// A device standing in the session.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Device {
        pub id: String,
        pub name: String,
        pub kind: Kind,
        pub can_play: bool,
        pub user: String,
        /// Whether anybody can move its volume.
        pub can_volume: bool,
        /// 0 to 100.
        pub volume: i64,
        pub muted: bool,
        /// Which speakers it joins: none for anything that is not a
        /// speaker that groups.
        pub family: Option<String>,
        /// The leader it follows, while it is joined to one.
        pub follows: Option<String>,
    }

    impl Device {
        /// A device as it declared itself, with nothing said of its volume
        /// or its group: what `on_join` writes.
        pub fn new(id: &str, name: &str, kind: Kind, can_play: bool) -> Device {
            Device {
                id: id.into(),
                name: name.into(),
                kind,
                can_play,
                user: String::new(),
                can_volume: false,
                volume: super::UNSAID_VOLUME,
                muted: false,
                family: None,
                follows: None,
            }
        }
    }

    /// A track of the queue.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Track {
        pub id: Id,
        pub title: String,
        pub creator: String,
        pub album: String,
        pub duration_ms: i64,
        pub file: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Clock {
        Paused {
            at_ms: i64,
        },
        /// Playing from `at_ms` since `since`, the server's clock.
        Playing {
            at_ms: i64,
            since: i64,
        },
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

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Output {
        Device { id: String, moving: bool },
        Away { id: String, name: String },
        Nowhere,
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

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Active {
        pub queue: Vec<Track>,
        pub at: usize,
        pub clock: Clock,
        pub output: Output,
        pub from: String,
    }

    impl Active {
        /// The track playing.
        pub fn now(&self) -> &Track {
            &self.queue[self.at.min(self.queue.len() - 1)]
        }
    }

    /// The whole state.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct Listening {
        pub devices: Vec<Device>,
        /// Where each device is dragging its scrubber, by device id.
        pub scrub: BTreeMap<String, i64>,
        pub session: Option<Active>,
    }

    impl Listening {
        pub fn device(&self, id: &str) -> Option<&Device> {
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

        /// The output, where it is a device that is here: the leader of
        /// whatever group plays.
        pub fn leader(&self) -> Option<&str> {
            match self.output()? {
                Output::Device { id, .. } => Some(id),
                _ => None,
            }
        }

        /// Who follows `leader`, in id order.
        pub fn followers(&self, leader: &str) -> Vec<&Device> {
            let mut out: Vec<&Device> = self.devices.iter().filter(|d| d.follows.as_deref() == Some(leader)).collect();
            out.sort_by(|a, b| a.id.cmp(&b.id));
            out
        }

        /// The group `leader` leads: it, then whoever follows it.
        pub fn group(&self, leader: &str) -> Vec<&Device> {
            self.device(leader).into_iter().chain(self.followers(leader)).collect()
        }

        /// Whether the output is a group: something follows it.
        pub fn grouped(&self) -> bool {
            self.leader().is_some_and(|l| !self.followers(l).is_empty())
        }

        /// The level of the group `leader` leads, as the domain reads it:
        /// the mean of the members' that have a volume, rounded down. `None`
        /// for a group in which nothing has one.
        pub fn group_volume(&self, leader: &str) -> Option<i64> {
            let levels: Vec<i64> = self.group(leader).iter().filter(|d| d.can_volume).map(|d| d.volume).collect();
            (!levels.is_empty()).then(|| levels.iter().sum::<i64>() / levels.len() as i64)
        }

        /// Whether every member of `leader`'s group with a volume is muted.
        pub fn group_muted(&self, leader: &str) -> bool {
            let members: Vec<&Device> = self.group(leader).into_iter().filter(|d| d.can_volume).collect();
            !members.is_empty() && members.iter().all(|d| d.muted)
        }

        /// Whether `id` can be joined to the output's group now: the
        /// domain's `joinable`, read the same way.
        pub fn joinable(&self, id: &str) -> bool {
            let Some(leader) = self.leader().and_then(|l| self.device(l)) else {
                return false;
            };
            let Some(d) = self.device(id) else { return false };
            leader.follows.is_none() && d.id != leader.id && d.can_play && d.family.is_some() && d.family == leader.family
        }
    }

    fn text(v: &Value, k: &str) -> Option<String> {
        match v.as_struct().get(k)? {
            Value::Text(t) => Some(t.to_string()),
            _ => None,
        }
    }

    fn int(v: &Value, k: &str) -> Option<i64> {
        match v.as_struct().get(k)? {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    fn tag(v: &Value) -> Option<String> {
        text(v, ark::types::TAG)
    }

    fn is_struct(v: &Value) -> bool {
        matches!(v, Value::Struct(_))
    }

    impl Track {
        pub fn from_value(v: &Value) -> Option<Track> {
            if !is_struct(v) {
                return None;
            }
            Some(Track {
                id: match v.as_struct().get("id")? {
                    Value::Id(i) => *i,
                    _ => return None,
                },
                title: text(v, "title")?,
                creator: text(v, "creator")?,
                album: text(v, "album")?,
                duration_ms: int(v, "duration_ms")?,
                file: text(v, "file")?,
            })
        }

        pub fn to_value(&self) -> Value {
            Value::record(vec![
                ("album", Value::text(self.album.clone())),
                ("creator", Value::text(self.creator.clone())),
                ("duration_ms", Value::Int(self.duration_ms)),
                ("file", Value::text(self.file.clone())),
                ("id", Value::Id(self.id)),
                ("title", Value::text(self.title.clone())),
            ])
        }
    }

    fn clock(v: &Value) -> Option<Clock> {
        match tag(v)?.as_str() {
            "paused" => Some(Clock::Paused { at_ms: int(v, "at_ms")? }),
            "playing" => Some(Clock::Playing {
                at_ms: int(v, "at_ms")?,
                since: int(v, "since")?,
            }),
            _ => None,
        }
    }

    fn output(v: &Value) -> Option<Output> {
        match tag(v)?.as_str() {
            "device" => Some(Output::Device {
                id: text(v, "id")?,
                moving: matches!(v.as_struct().get("moving")?, Value::Bool(true)),
            }),
            "away" => Some(Output::Away {
                id: text(v, "id")?,
                name: text(v, "name")?,
            }),
            "nowhere" => Some(Output::Nowhere),
            _ => None,
        }
    }

    impl Listening {
        /// The state from its value, or `None` for a value that is not one.
        pub fn from_value(v: &Value) -> Option<Listening> {
            if !is_struct(v) {
                return None;
            }
            let m = v.as_struct();
            let mut devices = vec![];
            if let Value::Struct(ds) = m.get("devices")? {
                for d in ds.values() {
                    if !is_struct(d) {
                        return None;
                    }
                    let flag = |k: &str| matches!(d.as_struct().get(k), Some(Value::Bool(true)));
                    devices.push(Device {
                        id: text(d, "id")?,
                        name: text(d, "name")?,
                        kind: Kind::parse(&text(d, "kind")?),
                        can_play: matches!(d.as_struct().get("can_play")?, Value::Bool(true)),
                        user: text(d, "user").unwrap_or_default(),
                        can_volume: flag("can_volume"),
                        volume: int(d, "volume").unwrap_or(super::UNSAID_VOLUME),
                        muted: flag("muted"),
                        family: text(d, "family"),
                        follows: text(d, "follows"),
                    });
                }
            }
            let mut scrub = BTreeMap::new();
            if let Some(Value::Struct(ss)) = m.get("scrub") {
                for (k, x) in ss.iter() {
                    if let Value::Int(n) = x {
                        scrub.insert(k.to_string(), *n);
                    }
                }
            }
            let s = m.get("session")?;
            let session = match tag(s)?.as_str() {
                "idle" => None,
                "active" => {
                    let queue: Option<Vec<Track>> = match s.as_struct().get("queue")? {
                        Value::List(xs) => xs.iter().map(Track::from_value).collect(),
                        _ => None,
                    };
                    let queue = queue?;
                    let at = usize::try_from(int(s, "at")?).ok()?;
                    if queue.is_empty() || at >= queue.len() {
                        return None;
                    }
                    Some(Active {
                        queue,
                        at,
                        clock: clock(&s.field("clock"))?,
                        output: output(&s.field("output"))?,
                        from: text(s, "from")?,
                    })
                }
                _ => return None,
            };
            Some(Listening { devices, scrub, session })
        }
    }

    /// The inputs, as a call takes them.
    pub mod input {
        use super::*;
        use ark::eval::Args;

        fn args(fields: Vec<(&str, Value)>) -> Args {
            fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
        }

        pub fn start(queue: &[Track], at: usize, position_ms: i64, from: &str) -> Args {
            args(vec![
                ("at", Value::Int(at as i64)),
                ("from", Value::text(from)),
                ("position", Value::Int(position_ms.max(0))),
                ("queue", Value::list(queue.iter().map(Track::to_value).collect())),
            ])
        }

        pub fn none() -> Args {
            Args::new()
        }

        pub fn seek(ms: i64) -> Args {
            args(vec![("ms", Value::Int(ms.max(0)))])
        }

        pub fn skip(by: i64) -> Args {
            args(vec![("by", Value::Int(by))])
        }

        pub fn jump(index: usize) -> Args {
            args(vec![("index", Value::Int(index as i64))])
        }

        pub fn transfer(to: &str) -> Args {
            args(vec![("to", Value::text(to))])
        }

        /// `started` and `stalled`.
        pub fn at(at_ms: i64) -> Args {
            args(vec![("at_ms", Value::Int(at_ms.max(0)))])
        }

        pub fn advance(from_track: usize) -> Args {
            args(vec![("from_track", Value::Int(from_track as i64))])
        }

        pub fn scrub(at_ms: Option<i64>) -> Args {
            args(vec![("at_ms", at_ms.map_or(Value::Null, |n| Value::Int(n.max(0))))])
        }

        pub fn equip(can_volume: bool, family: Option<&str>, volume: i64, muted: bool) -> Args {
            args(vec![
                ("can_volume", Value::Bool(can_volume)),
                ("family", family.map_or(Value::Null, Value::text)),
                ("muted", Value::Bool(muted)),
                ("volume", Value::Int(volume.clamp(0, 100))),
            ])
        }

        pub fn set_volume(device: &str, level: i64) -> Args {
            args(vec![("device", Value::text(device)), ("level", Value::Int(level.clamp(0, 100)))])
        }

        pub fn nudge_volume(device: &str, delta: i64) -> Args {
            args(vec![("delta", Value::Int(delta.clamp(-100, 100))), ("device", Value::text(device))])
        }

        pub fn set_mute(device: &str, muted: bool) -> Args {
            args(vec![("device", Value::text(device)), ("muted", Value::Bool(muted))])
        }

        pub fn report_volume(level: i64, muted: bool) -> Args {
            args(vec![("level", Value::Int(level.clamp(0, 100))), ("muted", Value::Bool(muted))])
        }

        /// `join` and `unjoin`.
        pub fn member(device: &str) -> Args {
            args(vec![("device", Value::text(device))])
        }

        pub fn report_group(members: &[&str]) -> Args {
            args(vec![("members", Value::list(members.iter().map(|m| Value::text(*m)).collect()))])
        }

        pub fn set_group_volume(level: i64) -> Args {
            args(vec![("level", Value::Int(level.clamp(0, 100)))])
        }

        pub fn nudge_group_volume(delta: i64) -> Args {
            args(vec![("delta", Value::Int(delta.clamp(-100, 100)))])
        }

        pub fn set_group_mute(muted: bool) -> Args {
            args(vec![("muted", Value::Bool(muted))])
        }

        /// `record_play`'s arguments.
        pub fn record_play(track: &Track, from: &str) -> Args {
            args(vec![
                ("album", Value::text(track.album.clone())),
                ("creator", Value::text(track.creator.clone())),
                ("duration_ms", Value::Int(track.duration_ms.max(0))),
                ("file", Value::text(track.file.clone())),
                ("from", Value::text(from)),
                ("title", Value::text(track.title.clone())),
                ("track", Value::Id(track.id)),
            ])
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

/// Where a track's file is, from where a server is. The column is a *path*,
/// and every device joins it to its own server — which is what makes a
/// hand-off work between a phone and a speaker that only knows a LAN
/// address. An absolute URL is already an answer and passes through.
pub fn url(base: &str, file: &str) -> String {
    if file.is_empty() || file.starts_with("http://") || file.starts_with("https://") {
        return file.to_string();
    }
    let mut out = format!("{}/media", base.trim_end_matches('/'));
    for part in file.split('/') {
        out.push('/');
        encode_segment(part, &mut out);
    }
    out
}

/// Percent-encode one path segment: everything outside RFC 3986's
/// unreserved set, which covers spaces, `&`, the silently destructive `#`
/// and every non-ASCII byte.
fn encode_segment(part: &str, out: &mut String) {
    for b in part.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path becomes a URL against whichever server is asking, and a URL
    /// is already an answer.
    #[test]
    fn a_file_is_joined_to_the_server_that_is_asking() {
        assert_eq!(
            url("http://10.0.0.2:8787", "music/Bach/air.flac"),
            "http://10.0.0.2:8787/media/music/Bach/air.flac"
        );
        assert_eq!(
            url("https://harken.example.com/", "music/a.mp3"),
            "https://harken.example.com/media/music/a.mp3",
            "a trailing slash is not a second one"
        );
        assert_eq!(
            url("http://h", "music/Boléro & co/no #1.mp3"),
            "http://h/media/music/Bol%C3%A9ro%20%26%20co/no%20%231.mp3"
        );
        assert_eq!(
            url("http://h", "https://upload.wikimedia.org/x.mp3"),
            "https://upload.wikimedia.org/x.mp3"
        );
        assert_eq!(url("http://h", ""), "", "nothing to stream is not a URL");
    }

    /// Thirty seconds or half the track, whichever is first.
    #[test]
    fn a_play_counts_after_thirty_seconds_or_half_the_track() {
        assert_eq!(read::counts_after(300_000), 30_000);
        assert_eq!(read::counts_after(40_000), 20_000);
        assert_eq!(read::counts_after(60_000), 30_000);
        assert_eq!(read::counts_after(0), 30_000, "a length nobody knows is the thirty seconds");
    }
}
