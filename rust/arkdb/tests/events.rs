//! `arkdb trace` over a system's events (docs/plan-observe.md): a server
//! and two devices, every frame carried by hand, every event kept in
//! memory and read back as the lines of JSON a server writes; the tree a
//! tap's trace prints names the tap, the entry it caused, who authored
//! it, the server's verdict and that the other device saw it.

use std::sync::Arc;

use arkdb::demo::{demo, CreatePlaylist};
use arkdb::events::{line, render_trace};
use ark_peer::{dev, Config, ConnId, Memory, Peer};

fn config() -> Config {
    Config::of(Arc::new(demo()))
}

fn carry(hub: &mut Peer, ds: &mut [(Peer, ConnId)]) {
    for _ in 0..8 {
        for i in 0..ds.len() {
            let (frames, c) = (ds[i].0.take_frames(), ds[i].1);
            for f in frames {
                for (to, b) in hub.frame(c, f, 0) {
                    if let Some((d, _)) = ds.iter_mut().find(|(_, x)| *x == to) {
                        d.receive(b);
                    }
                }
            }
        }
    }
}

/// Falsified by the verdicts read from the trace's own events: a verdict
/// names its entry and not its trace, and the server's `ok` was missing.
#[test]
fn a_taps_trace_prints_what_it_caused_everywhere() {
    let kept = ark_peer::observe::memory();
    let mut hub = Peer::serve(config(), Box::new(Memory::default()), 0);
    let mut ds: Vec<(Peer, ConnId)> = ["alice@phone", "bob@laptop"]
        .iter()
        .map(|t| {
            let mut d = Peer::open(config(), dev(t).unwrap(), Box::new(Memory::default()), false);
            let c = hub.accept(d.me().clone());
            d.connected();
            (d, c)
        })
        .collect();
    carry(&mut hub, &mut ds);
    let tap = ds[0].0.begin("tap new playlist");
    ds[0].0.mutate("create_playlist", &CreatePlaylist { name: "Road".into() }).unwrap();
    ds[0].0.end();
    carry(&mut hub, &mut ds);
    // As a server writes them, and as `arkdb` reads them.
    let text: String = kept.lock().unwrap().iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect();
    let all: Vec<ark::frames::Event> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(!all.is_empty());
    let tree = render_trace(&all, &ark::hash::hex(&tap.trace.0)[..8]).unwrap();
    let lines: Vec<&str> = tree.lines().collect();
    assert!(lines[0].starts_with("trace "), "{tree}");
    assert!(lines[1].contains("tap new playlist"), "the root is the tap:\n{tree}");
    let made = lines.iter().position(|l| l.contains("create_playlist")).expect(&tree);
    assert!(lines[made].starts_with("  "), "under the tap:\n{tree}");
    assert!(lines[made].contains("by alice"), "authored by alice:\n{tree}");
    assert!(tree.contains(": ok "), "the server's verdict:\n{tree}");
    assert!(tree.contains("seen by"), "and who else saw it:\n{tree}");
    // And one line per event, for `tail`.
    assert!(all.iter().map(line).any(|l| l.contains("create_playlist")));
}
