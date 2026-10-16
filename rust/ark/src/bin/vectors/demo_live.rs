//! The demo's live state (`docs/plan-live.md`, `spec/AUTHORING.md`
//! Appendix B.2): a `room` per account, which every live vector is over. A
//! router of its own, `now`, so that the demo's module — and every vector
//! of it — is untouched; [`module`] is the demo's routers with this one
//! after them.
//!
//! The room counts, which is ordered; can be set counting by a device and
//! stopped, which reads the time and a guard; keeps who is here in an
//! ordered `PerDevice`, by its hooks; and carries each device's pointer in
//! an ephemeral one — `point` is the ephemeral transition, and `drop_at`
//! the ordered one that also clears the caller's pointer — and a label
//! beside it, an ephemeral text, which is what a slot's size is held on.
//!
//! Generic over the tables, so that `rust/ark/tests/live.rs` declares the
//! same state over its own copy of the demo's (`#[path]`), and
//! `rust/ark-client/src/demo.rs` is the third copy, held to the same bytes
//! by `module/live.json`.

// The records are the vocabulary's: a body reads their fields only
// natively, and the generator only ever emits.
#![allow(dead_code)]

use ark::authoring::*;

/// The room: a record, named in the type section.
pub struct Room {
    pub count: Int,
    pub devices: PerDevice<Device>,
    pub label: PerDevice<Text>,
    pub mode: Sum<Mode>,
    pub pointer: PerDevice<Int>,
}
impl Record for Room {
    fn fields() -> Fields<Self> {
        fields()
            .named("room")
            .field("count", int())
            .field("devices", typed::<PerDevice<Device>>())
            .ephemeral("label", typed::<PerDevice<Text>>())
            .field("mode", typed::<Sum<Mode>>())
            .ephemeral("pointer", typed::<PerDevice<Int>>())
    }
}

/// Whether the room is counting, and since when, by whom.
pub struct Mode;
impl SumType for Mode {
    const NAME: &'static str = "mode";
    fn variants() -> Cases<Self> {
        cases().of::<Idle>("idle").of::<Counting>("counting")
    }
}

pub struct Idle {}
impl Record for Idle {
    fn fields() -> Fields<Self> {
        fields()
    }
}

pub struct Counting {
    pub by: Text,
    pub since: Int,
}
impl Record for Counting {
    fn fields() -> Fields<Self> {
        fields().field("by", text()).field("since", int())
    }
}

pub struct Bump {
    pub by: Int,
}
impl Input for Bump {
    fn schema() -> Object<Self> {
        object().field("by", int().range(1, 10))
    }
}

pub struct Say {
    pub text: Text,
}
impl Input for Say {
    fn schema() -> Object<Self> {
        object().field("text", text())
    }
}

pub struct At {
    pub at: Int,
}
impl Input for At {
    fn schema() -> Object<Self> {
        object().field("at", int())
    }
}

/// The router that declares the room.
pub fn live<S: Tables>() -> Router<S> {
    let now = router::<S>("now");
    now.live::<Room>("room")
        .scope(|ctx| ctx.user)
        .init(|| Room {
            count: 0.into(),
            devices: PerDevice::empty(),
            label: PerDevice::empty(),
            mode: Sum::of("idle", Idle {}),
            pointer: PerDevice::empty(),
        })
        // Ordered: reads the count.
        .transition::<Bump>("bump", |s, _ctx, input| Room {
            count: s.count.add(input.by),
            ..s
        })
        // Ordered: reads the server's clock and the caller's device.
        .transition::<()>("start", |s, ctx, ()| Room {
            mode: Sum::of(
                "counting",
                Counting {
                    by: ctx.device,
                    since: ctx.now,
                },
            ),
            ..s
        })
        .transition::<()>("stop", |s, _ctx, ()| Room {
            mode: Sum::of("idle", Idle {}),
            ..s
        })
        .guard::<()>("not counting", |s, _ctx, ()| s.mode.is("counting"))
        // Ephemeral: the caller's own pointer, and nothing read.
        .transition::<At>("point", |s, ctx, input| Room {
            pointer: s.pointer.put(ctx.device, input.at),
            ..s
        })
        // Ephemeral too: what the device says beside its pointer, held to
        // the slot's size.
        .transition::<Say>("label", |s, ctx, input| Room {
            label: s.label.put(ctx.device, input.text),
            ..s
        })
        // Ordered, and writes the caller's pointer: a drop clears it.
        .transition::<At>("drop_at", |s, ctx, input| Room {
            count: input.at,
            pointer: s.pointer.clear(ctx.device),
            ..s
        })
        .on_join(|s, _ctx, device| Room {
            devices: s.devices.put(device.id, device),
            ..s
        })
        .on_leave(|s, _ctx, device| Room {
            devices: s.devices.clear(device.id),
            ..s
        })
        // What a room restored at start is: still counted, not counting, and
        // nobody in it — whoever was here when the server stopped never left,
        // and only the app knows `devices` is who is here.
        .on_restore(|s, _ctx| Room {
            devices: PerDevice::empty(),
            mode: Sum::of("idle", Idle {}),
            ..s
        });
    now
}
