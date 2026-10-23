//! Causality (docs/plan-observe.md): an intent is authored under its
//! peer's span, every peer holds that span in the entry, a device's
//! answer to what it was told is a child of the entry that told it, and a
//! job runs — its claim, its handler, its finish — in the trace of the
//! entry that made it due. A server and two devices, every frame carried
//! by hand.

use std::sync::{Arc, Mutex};

use ark::effect::Job;
use ark::log::{Entry, Span};
use ark::prelude::*;
use ark::schema::Space;
use ark_peer::{dev, now_ms, Config, ConnId, Memory, Peer};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row, Effect)]
#[effect(lease_ms = 150, retry_ms = 150, owner = owner)]
pub struct Fetch {
    #[ark(key)]
    pub id: String,
    pub owner: String,
    pub state: Job<String>,
}

#[derive(Schema)]
pub struct Jobs {
    pub fetch: Fetch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Ask {
    pub id: String,
}

fn ask(ctx: &Ctx, db: &mut Db<Jobs>, i: Ask) -> Result<(), Refusal> {
    let state = Job::Pending { due_ms: ctx.now };
    db.fetch.insert(Fetch {
        id: i.id,
        owner: ctx.user().into(),
        state,
    })
}

fn config() -> Config {
    let d = Domain::new::<Jobs>("jobs", "1").mutator("fetch.ask", ask);
    Config::of(Arc::new(d.with(Fetch::register::<Jobs>)))
}

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
            let mut d = Peer::open(config(), dev(&format!("user{i}@device{i}")).unwrap(), Box::new(Memory::default()), false);
            let c = net.hub.accept(d.me().clone());
            d.connected();
            net.devices.push((d, c));
        }
        net.carry();
        net
    }

    fn carry(&mut self) {
        for _ in 0..8 {
            for i in 0..self.devices.len() {
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
        }
    }

    fn d(&mut self, i: usize) -> &mut Peer {
        &mut self.devices[i].0
    }

    /// The server's logged entries naming `func`, in order.
    fn logged(&self, func: &str) -> Vec<Entry> {
        let f = self.hub.binding(func).unwrap_or_else(|| func.to_string());
        self.hub.log.log().entries().into_iter().map(|l| l.entry).filter(|e| e.func == f).collect()
    }
}

fn asks(net: &mut Net, by: usize, id: &str) {
    net.d(by).mutate("fetch.ask", &Ask { id: id.into() }).unwrap();
    net.carry();
}

/// A tap's span is the cause of what is authored under it, on the server
/// and on the other device alike; ended, the next intent is not under it.
/// What a device authors after being told an entry is that entry's child.
/// Falsified by `Peer::cause` saying none: every entry began a trace of
/// its own.
#[test]
fn an_intent_is_a_child_of_its_tap_and_an_answer_of_what_it_answered() {
    let mut net = Net::new(2);
    let tap = net.d(0).begin("tap");
    asks(&mut net, 0, "a");
    net.d(0).end();
    let a = net.logged("fetch.ask").pop().unwrap();
    assert_eq!(a.cause, Some(tap), "the server's entry holds the tap");
    assert_eq!(a.span().trace, tap.trace);
    let theirs = net.devices[1].0.log.log().entries().into_iter().find(|l| l.entry.id == a.id).unwrap();
    assert_eq!(theirs.entry.cause, Some(tap), "and so does the other device's");
    // The other device was told `a`: what it does next answers it.
    asks(&mut net, 1, "b");
    let b = net.logged("fetch.ask").pop().unwrap();
    assert_eq!(b.cause, Some(a.span()), "an answer is a child of what it answered");
    assert_eq!(b.span().trace, tap.trace, "in the tap's trace");
}

/// A job runs in the trace of the entry that made it due: its claim is a
/// child of that entry, its handler authors under the claim, its finish is
/// the claim's child. Falsified by `Peer::cause` saying none: the claim
/// began a trace of its own, and the handler ran under nothing.
#[test]
fn a_job_runs_in_the_trace_of_what_made_it_due() {
    let mut net = Net::new(2);
    let seen: Arc<Mutex<Option<Span>>> = Arc::default();
    let s = seen.clone();
    net.d(0).handle::<Fetch>(move |p, _| {
        *s.lock().unwrap() = p.cause(Space::Durable, now_ms());
        Ok("body".into())
    });
    let tap = net.d(1).begin("tap");
    asks(&mut net, 1, "a");
    net.d(1).end();
    let a = net.logged("fetch.ask").pop().unwrap();
    for _ in 0..3 {
        net.d(0).jobs(now_ms());
        net.carry();
    }
    let claim = net.logged("fetch.claim").pop().expect("claimed");
    let finish = net.logged("fetch.finish").pop().expect("finished");
    assert_eq!(claim.cause, Some(a.span()), "the claim is a child of the ask");
    assert_eq!(*seen.lock().unwrap(), Some(claim.span()), "the handler ran under the claim");
    assert_eq!(finish.cause, Some(claim.span()), "the finish is the claim's child");
    assert!([a.span(), claim.span(), finish.span()].iter().all(|s| s.trace == tap.trace), "one trace, from the tap");
}

/// A trace is what one hand caused, for [`ark_peer::TRACE_MS`]: an answer
/// made after that begins a trace of its own, where one made inside it
/// joins. Falsified by `Peer::cause` taking the waking span at any age: the
/// late answer was the tap's child.
#[test]
fn an_answer_after_the_trace_is_over_begins_its_own() {
    use ark::format::Encode;
    let mut net = Net::new(2);
    let tap = net.d(0).begin("tap");
    asks(&mut net, 0, "a");
    net.d(0).end();
    let f = net.d(1).binding("fetch.ask").unwrap();
    let late = tap.began + ark_peer::TRACE_MS + 1;
    net.d(1).author(Space::Durable, &f, Ask { id: "late".into() }.to_bytes(), late).unwrap();
    net.carry();
    let b = net.logged("fetch.ask").pop().unwrap();
    assert_eq!(b.cause, None, "late, it is a trace of its own");
    assert_ne!(b.span().trace, tap.trace);
}

/// What a job's own code says is collected inside the job's span, with its
/// peer and the trace it runs in; what is refused is collected with why.
/// Falsified by the layer reading only the event's own fields: the job's
/// line had no peer and no span.
#[test]
fn a_jobs_own_words_are_collected_in_its_trace() {
    let kept = ark_peer::observe::memory();
    let mut net = Net::new(2);
    net.d(0).handle::<Fetch>(|_, f| {
        tracing::info!(id = %f.id, "fetching");
        Ok("body".into())
    });
    let tap = net.d(1).begin("tap");
    asks(&mut net, 1, "words");
    net.d(1).end();
    for _ in 0..3 {
        net.d(0).jobs(now_ms());
        net.carry();
    }
    let me = net.devices[0].0.label();
    let trace = ark::hash::hex(&tap.trace.0);
    let all = kept.lock().unwrap().clone();
    let said = all.iter().find(|e| e.msg == "fetching" && e.fields.iter().any(|(k, v)| k == "id" && v == "words"));
    let said = said.expect("the job's line was collected");
    assert_eq!(said.peer, me, "as the peer that ran it");
    let span = said.fields.iter().find(|(k, _)| k == "span").map(|(_, v)| v.clone()).unwrap();
    assert!(span.contains(&trace[..12]), "inside the tap's trace: {span}");
    // A refusal, said where it was refused.
    let _ = net.d(1).mutate("fetch.ask", &Ask { id: "words".into() });
    let refused = kept.lock().unwrap().iter().any(|e| e.msg == "refused" && e.level == "warn");
    assert!(refused, "the refusal was collected");
}
