//! `docs/plan-live.md` Live state through a server in this process, over
//! real sockets: three devices of one account in the demo's room, one of
//! them partitioned while the others call; on the heal all three agree,
//! and the partitioned device's pending calls are sequenced or refused.
//! And a participant the server stands in for, and a restart that keeps
//! what was ordered.

mod common;

use ark::live::state::{DeviceDecl, Instance, Order};
use ark::protocol::{Identity, ServerMsg};
use ark::value::Value;
use ark_client::{args, demo, Options, Peer};
use common::*;

fn device(running: &ark_server::Running, name: &str) -> Peer {
    let opts = Options::dev("alice")
        .with_timing(quick())
        .with_device(DeviceDecl::new(name, "phone", name, true));
    let mut p = Peer::open_memory(demo::live_domain(), opts).unwrap();
    p.connect(&running.sync_url());
    p
}

fn serve_live(rt: &tokio::runtime::Runtime, data: Option<&std::path::Path>) -> ark_server::Running {
    let mut b = ark_server::builder(demo::live_domain()).name("live").trusting();
    if let Some(d) = data {
        b = b.data(d);
    }
    rt.block_on(b.build().expect("builds").serve("127.0.0.1:0")).expect("listens")
}

fn count(p: &Peer) -> Value {
    p.live("room").map(|v| v.field("count")).unwrap_or(Value::Null)
}

/// Falsified by dropping a device's pending calls on reconnect
/// (`Watch::connected` returning none): the partitioned laptop's calls
/// never reached the server and the three never agreed.
#[test]
fn three_devices_agree_after_a_partition() {
    let rt = runtime();
    let running = serve_live(&rt, None);
    let mut phone = device(&running, "phone");
    let mut laptop = device(&running, "laptop");
    let mut tv = device(&running, "tv");
    pump_until(&mut [&mut phone, &mut laptop, &mut tv], |ps| {
        ps.iter()
            .all(|p| p.live("room").is_some_and(|v| v.field("devices").as_struct().len() == 3))
    });

    // The laptop goes dark, and calls: bump by 5, and stop — the room is not
    // counting yet, so its own preview refuses the stop at once.
    laptop.disconnect();
    laptop.call("room", "bump", args([("by", Value::Int(5))])).unwrap();
    assert_eq!(count(&laptop), Value::Int(5), "its view moves at once");
    // Meanwhile the phone starts the room counting and bumps, the tv bumps.
    phone.call("room", "start", args([])).unwrap();
    phone.call("room", "bump", args([("by", Value::Int(1))])).unwrap();
    tv.call("room", "bump", args([("by", Value::Int(2))])).unwrap();
    pump_until(&mut [&mut phone, &mut tv], |ps| ps.iter().all(|p| count(p) == Value::Int(3)));
    // The laptop, still dark, asks to start too — previewed over its own
    // view, where the room is idle — which the server will refuse, the room
    // counting by then.
    laptop.call("room", "start", args([])).unwrap();
    assert_eq!(laptop.live("room").unwrap().field("mode").field("t"), Value::text("counting"));
    // The phone stops it, so the laptop's start is taken after all.
    phone.call("room", "stop", args([])).unwrap();

    laptop.reconnect();
    pump_until(&mut [&mut phone, &mut laptop, &mut tv], |ps| {
        ps.iter().all(|p| count(p) == Value::Int(8)) && ps[0].live("room") == ps[1].live("room") && ps[1].live("room") == ps[2].live("room")
    });
    let room = laptop.live("room").unwrap();
    assert_eq!(room.field("devices").as_struct().len(), 3, "the laptop is back, once");
    assert_eq!(
        room.field("mode").field("t"),
        Value::text("counting"),
        "its start was sequenced after the phone's stop"
    );
    assert_eq!(room.field("mode").field("by"), Value::text("laptop"));
    assert!(laptop.take_live_refusals().is_empty());

    // A call the server refuses reaches its author as a verdict: the tv and
    // the laptop both stop the room, and the second to arrive is refused.
    tv.disconnect();
    tv.call("room", "stop", args([])).unwrap();
    laptop.call("room", "stop", args([])).unwrap();
    pump_until(&mut [&mut laptop], |ps| {
        ps[0].live("room").is_some_and(|v| v.field("mode").field("t") == Value::text("idle"))
    });
    tv.reconnect();
    let mut refused = vec![];
    pump_until(&mut [&mut phone, &mut laptop, &mut tv], |ps| {
        refused.extend(ps[2].take_live_refusals());
        !refused.is_empty() && ps[0].live("room") == ps[2].live("room")
    });
    assert_eq!(refused[0].reason, "not counting");
}

/// A participant the server stands in for — a speaker — is in the room as
/// a device, hears what is applied, and calls as itself; and a restart
/// keeps the room's ordered part, its pointer gone and `on_restore` run —
/// which empties who is here, the speaker never having left.
/// Falsified by not restoring the kept file: the restarted room counted 0.
#[test]
fn a_participant_calls_and_a_restart_keeps_the_room() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let running = serve_live(&rt, Some(dir.path()));
    let (tx, heard) = std::sync::mpsc::channel::<ServerMsg>();
    let speaker = running
        .hub
        .participate(
            Identity::new("alice", "kitchen-bridge"),
            DeviceDecl::new("media_player.kitchen", "speaker", "Kitchen", true),
            vec![Instance::new("room", "alice")],
            move |m| {
                let _ = tx.send(m);
            },
        )
        .unwrap();
    let mut phone = device(&running, "phone");
    pump_until(&mut [&mut phone], |ps| {
        ps[0]
            .live("room")
            .is_some_and(|v| v.field("devices").field("media_player.kitchen") != Value::Null)
    });
    phone.call("room", "start", args([])).unwrap();
    phone.call("room", "point", args([("at", Value::Int(4))])).unwrap();
    pump_until(&mut [&mut phone], |ps| ps[0].pending_len() == 0 && count(ps[0]) == Value::Int(0));
    let call = ark::live::state::Call {
        instance: Instance::new("room", "alice"),
        transition: "bump".into(),
        input: args([("by", Value::Int(7))]),
        id: [7; 16],
        counter: None,
    };
    running.hub.call(speaker, call).unwrap();
    pump_until(&mut [&mut phone], |ps| count(ps[0]) == Value::Int(7));
    let mut kinds = vec![];
    while let Ok(m) = heard.try_recv() {
        if let ServerMsg::Applied(a) = m {
            kinds.push((a.transition.clone(), matches!(a.order, Order::Ephemeral { .. })));
        }
    }
    assert!(kinds.contains(&("start".into(), false)), "{kinds:?}");
    assert!(
        kinds.contains(&("point".into(), true)),
        "an ephemeral call reaches a participant: {kinds:?}"
    );
    drop(phone);
    rt.block_on(running.stop());

    let running = serve_live(&rt, Some(dir.path()));
    let mut phone = device(&running, "phone");
    pump_until(&mut [&mut phone], |ps| {
        ps[0].live("room").is_some_and(|v| v.field("devices").as_struct().len() == 1)
    });
    let room = phone.live("room").unwrap();
    assert_eq!(room.field("count"), Value::Int(7), "the count survived");
    assert_eq!(room.field("mode").field("t"), Value::text("idle"), "on_restore stopped it");
    assert_eq!(room.field("pointer").as_struct().len(), 0, "no pointer survives");
    rt.block_on(running.stop());
}
