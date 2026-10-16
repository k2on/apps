//! The demo module of `spec/AUTHORING.md` Appendix B, authored in the
//! vocabulary exactly as `rust/ark/tests/demo_authoring.rs` has it: the tables
//! `playlist(id, name, user_id)` and `item(playlist_id →
//! playlist, track_id, pos)`, the mutators `create_playlist` and
//! `add_to_playlist`, and the query `items`.
//!
//! What this crate's tests and `ark-server`'s run against — never an app's
//! domain — and a small example of what an app hands [`crate::Domain::new`].

use ark::authoring::*;

pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Tables for Demo {
    fn open() -> Self {
        Demo {
            playlist: table(),
            item: table(),
        }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const item: Rel<Self, Item> = rel("item");
}

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .unique((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name"))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
    }
}

pub struct PlaylistId {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistId {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

pub fn demo() -> Router<Demo> {
    let demo = router::<Demo>("demo");
    demo.routes((
        demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
        demo.input::<PlaylistId>().query("items", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc())
        }),
    ))
}

/// The module, as an app would write its own `module()`.
pub fn module() -> Module {
    Module::new((demo(),))
}

/// The module as a peer holds it, every procedure native.
pub fn domain() -> crate::Domain {
    crate::Domain::new(&module())
}

/// `docs/plan-live.md` The demo's live state, a room per account: the third
/// copy of `rust/ark/src/bin/vectors/demo_live.rs`, held to the same bytes
/// by `module/live.json` (`live_tests`).
pub mod room {
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
}

/// The demo with its room: the demo's router and `now`, which declares it.
pub fn live_module() -> Module {
    Module::new((demo(), room::live::<Demo>()))
}

/// The module with its room as a peer holds it, every procedure and the
/// room native.
pub fn live_domain() -> crate::Domain {
    crate::Domain::new(&live_module())
}
