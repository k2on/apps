//! A hub opened on the demo's seeded library (`harken_seed::seeded_disk`, a
//! log standing at its start on a snapshot of rows), as the screenshots'
//! server is: a device that has nothing is filled with the library when it
//! says hello. Falsified by the kernel taking "at the chain's start" for
//! "has it all" on either side (`Peer::serve`, a fill's first part): the
//! device's library stayed empty until the log had an entry of its own.

use std::sync::Arc;

use ark::format::Encode;
use ark_peer::store::Store;
use ark_peer::{dev, Config, Deliver, Live, Memory, Module, Peer};
use harken_domain::HarkenView;

/// Every frame carried both ways until neither has more.
fn carry(device: &mut Peer, hub: &mut Peer, c: u64) {
    loop {
        let mut q: Vec<Deliver> = vec![];
        device.flush();
        for f in device.take_frames() {
            q.extend(hub.frame(c, f, 0));
        }
        q.extend(hub.release());
        if q.is_empty() {
            break;
        }
        for (to, b) in q {
            if to == c {
                device.receive(b);
            }
        }
    }
}

#[test]
fn a_seeded_hub_fills_a_device_that_has_nothing() {
    let d: Arc<ark::domain::Domain> = harken_domain::shared();
    let get = |n: &str| Some(format!("listening.{n}"));
    let hub_config = Config {
        live: Some(Live {
            on_join: get("on_join"),
            on_leave: get("on_leave"),
            on_restore: get("on_restore"),
            arrive: Arc::new(harken_domain::host::arrive),
            ..Live::new(Module::of(&d))
        }),
        ..Config::of(d.clone())
    };
    let said = harken_domain::Device {
        can_play: true,
        id: String::new(),
        kind: "phone".into(),
        name: "Phone".into(),
        user: String::new(),
    };
    let config = Config {
        live: Some(Live {
            device: Some(said.to_bytes()),
            ..Live::new(Module::of(&d))
        }),
        ..Config::of(d)
    };
    let mut hub = Peer::serve(hub_config, Box::new(harken_seed::seeded_disk(1)), 0);
    let mut who = dev("alice@phone").unwrap();
    who.roles = vec![harken_domain::LIBRARY.into()];
    let mut device = Peer::open(config, who, Box::new(Memory::default()), false);
    let c = hub.accept(device.me().clone());
    device.linked = true;
    device.connected();
    carry(&mut device, &mut hub, c);
    let mut seen = Store::<HarkenView>::new();
    seen.hold(&mut device, HarkenView::Albums);
    seen.settle(&mut device);
    let mut kept = Store::<HarkenView>::new();
    kept.hold(&mut hub, HarkenView::Albums);
    kept.settle(&mut hub);
    let albums = kept.rows::<Vec<u8>>(&HarkenView::Albums).len();
    assert!(albums > 0);
    assert_eq!(seen.rows::<Vec<u8>>(&HarkenView::Albums).len(), albums);
}
