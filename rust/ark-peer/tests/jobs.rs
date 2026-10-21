//! Handlers and reconcilers (`ark_peer::jobs`), over a server and devices
//! whose frames are carried by hand: a claim and a cancel decided by the
//! order the server takes them in; a handler run once across a restart; a
//! handler run only where it is registered; a reconciler told every
//! change.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ark::effect::{Cancel, Job};
use ark::prelude::*;
use ark::schema::Space;
use ark_peer::{dev, now_ms, Config, ConnId, Memory, Peer};

/// Something to fetch for its owner: the job, its outcome the body.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row, Effect)]
#[effect(lease_ms = 150, retry_ms = 150, owner = owner)]
pub struct Fetch {
    #[ark(key)]
    pub id: String,
    pub owner: String,
    pub url: String,
    pub state: Job<String>,
}

#[derive(Schema)]
pub struct Jobs {
    pub fetch: Fetch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Ask {
    pub id: String,
    pub url: String,
}

fn ask(ctx: &Ctx, db: &mut Db<Jobs>, i: Ask) -> Result<(), Refusal> {
    let state = Job::Pending { due_ms: ctx.now };
    db.fetch.insert(Fetch {
        id: i.id,
        owner: ctx.user().into(),
        url: i.url,
        state,
    })
}

fn all(_: &Ctx, _: &Db<Jobs>, _: ()) -> Result<Plan<Jobs, Fetch>, Refusal> {
    Ok(scan::<Jobs, Fetch>())
}

fn config() -> Config {
    let d = Domain::new::<Jobs>("jobs", "1").mutator("fetch.ask", ask).view("fetch.all", all);
    Config::of(Arc::new(d.with(Fetch::register::<Jobs>)))
}

/// A server and its devices, every frame carried at once.
struct Net {
    hub: Peer,
    devices: Vec<(Peer, ConnId)>,
}

impl Net {
    fn new(n: usize) -> Net {
        let mut net = Net {
            hub: Peer::serve(config(), Box::new(Memory::default()), 0),
            devices: vec![],
        };
        for i in 0..n {
            let who = dev(&format!("user{i}@device{i}")).unwrap();
            net.join(Peer::open(config(), who, Box::new(Memory::default()), false));
        }
        net
    }

    fn join(&mut self, mut d: Peer) -> usize {
        let c = self.hub.accept(d.me().clone());
        d.connected();
        self.devices.push((d, c));
        self.carry();
        self.devices.len() - 1
    }

    /// Device `i`'s frames to the server, until nothing moves.
    fn carry_from(&mut self, i: usize) {
        let (d, c) = &mut self.devices[i];
        let (frames, c) = (d.take_frames(), *c);
        for f in frames {
            for (to, b) in self.hub.frame(c, f, now_ms()) {
                if let Some((d, _)) = self.devices.iter_mut().find(|(_, x)| *x == to) {
                    d.receive(b);
                }
            }
        }
    }

    fn carry(&mut self) {
        for _ in 0..8 {
            (0..self.devices.len()).for_each(|i| self.carry_from(i));
        }
    }

    fn d(&mut self, i: usize) -> &mut Peer {
        &mut self.devices[i].0
    }

    fn state(&self) -> Job<String> {
        self.hub
            .log
            .store
            .rows("fetch")
            .map(|r| Fetch::from_bytes(r).unwrap().state)
            .next()
            .unwrap()
    }
}

fn counting(n: &Arc<AtomicUsize>) -> impl FnMut(&Fetch) -> Result<String, String> + Send + 'static {
    let n = n.clone();
    move |f: &Fetch| {
        n.fetch_add(1, Ordering::Relaxed);
        Ok(format!("body of {}", f.url))
    }
}

fn asked(net: &mut Net, by: usize) {
    net.d(by)
        .mutate(
            "fetch.ask",
            &Ask {
                id: "a".into(),
                url: "u".into(),
            },
        )
        .unwrap();
    net.carry();
}

/// Of a worker's claim and the owner's cancel of one pending job, the one
/// the server takes first wins, in either order: the claim first and the
/// cancel is refused, the job run; the cancel first and the claim is
/// refused, the handler never run. Falsified by carrying the second
/// device's frames before the first's in both rounds: the first round's
/// claim was refused, and nothing ran.
#[test]
fn a_claim_and_a_cancel_race_in_the_servers_order() {
    for claim_first in [true, false] {
        let mut net = Net::new(2);
        let n = Arc::new(AtomicUsize::new(0));
        net.d(0).handle::<Fetch>(counting(&n));
        asked(&mut net, 1);
        net.d(0).jobs(now_ms());
        net.d(1).mutate("fetch.cancel", &Cancel { id: ("a".to_string(),) }).unwrap();
        let order = if claim_first { [0, 1] } else { [1, 0] };
        order.into_iter().for_each(|i| net.carry_from(i));
        net.carry();
        net.d(0).jobs(now_ms());
        net.carry();
        match claim_first {
            true => assert_eq!(net.state(), Job::Done("body of u".into())),
            false => assert_eq!(net.state(), Job::Cancelled),
        }
        assert_eq!(n.load(Ordering::Relaxed), claim_first as usize, "claim first: {claim_first}");
    }
}

/// A claim confirmed whose handler had not run when the device stopped
/// is not run again by its next process while the lease holds, and is
/// once it has run out; done, it is never run again. Falsified by a
/// claimed job being due again at its claim, its lease not counted
/// (`Job::due`): the restarted device claimed it and ran it at once.
#[test]
fn a_handler_runs_once_across_a_restart() {
    let mut net = Net::new(1);
    let n = Arc::new(AtomicUsize::new(0));
    net.d(0)
        .handle::<Fetch>(|_: &Fetch| -> Result<String, String> { panic!("this process stopped before it ran") });
    asked(&mut net, 0);
    net.d(0).jobs(now_ms());
    net.carry();
    assert!(matches!(net.state(), Job::Claimed { .. }), "claimed, confirmed");
    let (d, _) = net.devices.remove(0);
    let mut d = d.reopen();
    d.handle::<Fetch>(counting(&n));
    let i = net.join(d);
    for _ in 0..2 {
        net.d(i).jobs(now_ms());
        net.carry();
    }
    assert_eq!(n.load(Ordering::Relaxed), 0, "the lease holds");
    std::thread::sleep(std::time::Duration::from_millis(160));
    for _ in 0..3 {
        net.d(i).jobs(now_ms());
        net.carry();
    }
    assert_eq!(n.load(Ordering::Relaxed), 1, "run once the lease ran out");
    assert_eq!(net.state(), Job::Done("body of u".into()));
}

/// A job is run only by the peer its handler is registered on: another
/// device and the server, pumped and past its due, run nothing and it
/// stays pending; the registering device's turn runs it. Falsified by
/// keeping handlers in one registry of the process's, which every peer's
/// turn runs: the second device ran the first's handler.
#[test]
fn a_handler_runs_only_where_it_is_registered() {
    let mut net = Net::new(2);
    let n = Arc::new(AtomicUsize::new(0));
    net.d(0).handle::<Fetch>(counting(&n));
    asked(&mut net, 0);
    for _ in 0..3 {
        net.d(1).pump();
        net.d(1).jobs(now_ms() + 10_000);
        net.hub.jobs(now_ms() + 10_000);
        net.carry();
    }
    assert_eq!(n.load(Ordering::Relaxed), 0);
    assert!(matches!(net.state(), Job::Pending { .. }));
    for _ in 0..2 {
        net.d(0).jobs(now_ms());
        net.carry();
    }
    assert_eq!((n.load(Ordering::Relaxed), net.state()), (1, Job::Done("body of u".into())));
}

/// A reconciler is told the view's rows at the start and on each change:
/// the ask, the claim, the finish, each as the server confirmed it.
/// Falsified by telling it only at the start: it saw no rows, once.
#[test]
fn a_reconciler_is_told_every_change() {
    let mut net = Net::new(2);
    let seen: Arc<Mutex<Vec<Vec<Job<String>>>>> = Arc::default();
    let s = seen.clone();
    let tell = move |rows: Vec<Fetch>| s.lock().unwrap().push(rows.into_iter().map(|f| f.state).collect());
    net.d(1).reconcile::<Fetch>(Space::Durable, "fetch.all", &().to_bytes(), tell).unwrap();
    net.d(0).handle::<Fetch>(|f: &Fetch| Ok(f.url.clone()));
    asked(&mut net, 0);
    for _ in 0..2 {
        net.d(0).jobs(now_ms());
        net.carry();
    }
    let seen = seen.lock().unwrap();
    assert_eq!(seen.first(), Some(&vec![]));
    assert!(matches!(seen[1][..], [Job::Pending { .. }]), "{seen:?}");
    assert!(matches!(seen[2][..], [Job::Claimed { .. }]), "{seen:?}");
    assert_eq!(seen.last(), Some(&vec![Job::Done("u".into())]));
}
