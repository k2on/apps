//! The listening session, built (`crate::listening`'s functions, one for
//! one).

use ark::author::{Int, Text};
use ark::build::*;

use crate::listening::*;
use crate::schema::*;

fn db() -> HarkenDb {
    Harken::db()
}

// * The helpers

pub static POSITION: Def2<Clock, Int, Int> = Def2 {
    name: "listening.position",
    kind: Kind::Helper,
    body: |clock, now| ClockEx::match_(&clock, |at_ms| at_ms, |at_ms, since| (&at_ms + (&now - since)).max(&at_ms)),
};

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

pub static PAUSED_AT: Def2<Clock, Int, Clock> = Def2 {
    name: "listening.paused_at",
    kind: Kind::Helper,
    body: |clock, now| ClockEx::paused(POSITION.call(clock, now)),
};

pub static HALTED: Def3<Clock, Output, Int, Clock> = Def3 {
    name: "listening.halted",
    kind: Kind::Helper,
    body: |clock, output, now| ClockEx::paused(STANDING.call(clock, output, now)),
};

pub static CLOCK_AT: Def3<Clock, Int, Int, Clock> = Def3 {
    name: "listening.clock_at",
    kind: Kind::Helper,
    body: |clock, ms, now| ClockEx::match_(&clock, |_| ClockEx::paused(&ms), |_, _| ClockEx::playing(&ms, &now)),
};

pub static IS_HERE: Def1<Output, bool> = Def1 {
    name: "listening.is_here",
    kind: Kind::Helper,
    body: |output| OutputEx::match_(&output, |_, _| bool(false), |_, _| bool(true), || bool(false)),
};

pub static IS_ACTIVE: Def1<Session, bool> = Def1 {
    name: "listening.is_active",
    kind: Kind::Helper,
    body: |session| SessionEx::match_(&session, |_| bool(true), || bool(false)),
};

pub static REACHABLE: Def1<Session, bool> = Def1 {
    name: "listening.reachable",
    kind: Kind::Helper,
    body: |session| SessionEx::match_(&session, |a| IS_HERE.call(a.f().output), || bool(false)),
};

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

pub static SESSION_OF: Def1<Text, Session> = Def1 {
    name: "listening.session_of",
    kind: Kind::Helper,
    body: |user| db().listening.get(user).map_or(SessionEx::idle(), |r| r.f().session),
};

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

pub static DEVICES_OF: Def1<Text, Vec<ListeningDevice>> = Def1 {
    name: "listening.devices_of",
    kind: Kind::Helper,
    body: |user| db().listening_device.filter(|r| r.f().user.eq(&user)),
};

pub static PRESENT_OF: Def2<Text, Text, Present> = Def2 {
    name: "listening.present_of",
    kind: Kind::Helper,
    body: |user, id| db().listening_device.get((user, id)).unwrap().f().present,
};

pub static PUT_PRESENT: Def3<Text, Text, Present, ()> = Def3 {
    name: "listening.put_present",
    kind: Kind::Helper,
    body: |user, id, present| {
        db().listening_device
            .update((user, id), |r| ListeningDeviceEx { present, ..r.f() }.into())
            .s()
    },
};

pub static CAN_PLAY: Def2<Text, Text, bool> = Def2 {
    name: "listening.can_play",
    kind: Kind::Helper,
    body: |user, id| db().listening_device.get((user, id)).map_or(false, |d| d.f().present.f().can_play),
};

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

pub static HERE: Def1<Text, Output> = Def1 {
    name: "listening.here",
    kind: Kind::Helper,
    body: |id| OutputEx::device(id, false),
};

pub static PRESSED: Def2<Output, Text, Output> = Def2 {
    name: "listening.pressed",
    kind: Kind::Helper,
    body: |output, caller| if_(IS_HERE.call(&output), output, HERE.call(caller)),
};

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

pub static MOVED_PLACE: Def3<Int, Int, Int, Int> = Def3 {
    name: "listening.moved_place",
    kind: Kind::Helper,
    body: |i, from, to| {
        if_(
            i.eq(&from),
            to.clone(),
            if_(
                from.lt(&to).and(i.gt(&from)).and(i.le(&to)),
                &i - 1,
                if_(to.lt(&from).and(i.ge(&to)).and(i.lt(&from)), &i + 1, i.clone()),
            ),
        )
    },
};

pub static IN_QUEUE: Def2<Session, Int, bool> = Def2 {
    name: "listening.in_queue",
    kind: Kind::Helper,
    body: |session, index| SessionEx::match_(&session, |a| index.lt(a.f().queue.len()), || bool(false)),
};

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

pub static OUTPUT_ID: Def1<Session, Option<Text>> = Def1 {
    name: "listening.output_id",
    kind: Kind::Helper,
    body: |session| SessionEx::match_(&session, |a| OutputEx::match_(&a.f().output, |_, _| none(), |id, _| some(id), none), none),
};

pub static HAS_VOLUME: Def2<Text, Text, bool> = Def2 {
    name: "listening.has_volume",
    kind: Kind::Helper,
    body: |user, id| db().listening_device.get((user, id)).map_or(false, |d| d.f().present.f().can_volume),
};

pub static IN_GROUP: Def2<Present, Text, bool> = Def2 {
    name: "listening.in_group",
    kind: Kind::Helper,
    body: |d, leader| d.f().id.eq(&leader).or(d.f().follows.eq(some(leader))),
};

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

pub static SCALED: Def3<Int, Int, Int, Int> = Def3 {
    name: "listening.scaled",
    kind: Kind::Helper,
    body: |volume, from, to| if_(from.eq(0), to.clone(), ((&volume * &to * 2 + &from) / (from.max(1) * 2)).clamp(0, 100)),
};

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

pub static GROUP_HAS_VOLUME: Def1<Text, bool> = Def1 {
    name: "listening.group_has_volume",
    kind: Kind::Helper,
    body: |user| {
        OUTPUT_ID
            .call(SESSION_OF.call(&user))
            .map_or(false, |leader| GROUP_TALLY.call(&user, leader).f().n.gt(0))
    },
};

pub static LEADER: Def1<Text, Text> = Def1 {
    name: "listening.leader",
    kind: Kind::Helper,
    body: |user| OUTPUT_ID.call(SESSION_OF.call(user)).unwrap(),
};

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

pub static STOP: Def0<()> = Def0 {
    name: "listening.stop",
    kind: Kind::Live,
    body: || PUT_SESSION.call(Ctx.user(), SessionEx::idle()),
};

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
