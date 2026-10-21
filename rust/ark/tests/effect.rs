//! Effects as rows (`ark::effect`): a job's three functions, judged in the
//! order the log gives them — a claim and a cancel of one pending row,
//! either first; a lease run out and claimed again; a finish by whoever
//! did not claim it.

use std::sync::Arc;

use ark::effect::{Cancel, Claim, Finish, Job};
use ark::prelude::*;
use ark::store::Store;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row, Effect)]
#[effect(lease_ms = 100, retry_ms = 50, role = "worker", owner = owner)]
pub struct Fetch {
    #[ark(key)]
    pub id: String,
    pub owner: String,
    pub state: Job<i64>,
}

#[derive(Schema)]
pub struct Jobs {
    pub fetch: Fetch,
}

fn domain() -> Domain {
    Domain::new::<Jobs>("jobs", "1").with(Fetch::register::<Jobs>)
}

fn store() -> Store {
    let mut s = Store::new(Arc::new(Jobs::schema()));
    let row = Fetch {
        id: "a".into(),
        owner: "alice".into(),
        state: Job::Pending { due_ms: 10 },
    };
    s.put("fetch", row.to_bytes()).unwrap();
    s
}

fn ctx(actor: &str, roles: &[&str], now: i64) -> Ctx {
    Ctx {
        id: Default::default(),
        actor: actor.into(),
        session: actor.into(),
        roles: roles.iter().map(|r| r.to_string()).collect(),
        now,
        draws: Default::default(),
    }
}

/// One function run as the authority would in log order: its verdict.
fn run(d: &Domain, s: &mut Store, f: &str, c: &Ctx, input: Vec<u8>) -> Result<(), String> {
    let p = d.get(&format!("fetch.{f}")).and_then(|f| f.proc.clone()).unwrap();
    p.apply(s, c, &input).map(|_| ()).map_err(|r| r.0)
}

fn claim(by: &str) -> Vec<u8> {
    Claim::<Fetch> {
        id: "a".into(),
        by: by.into(),
    }
    .to_bytes()
}

fn state(s: &Store) -> Job<i64> {
    let rows: Vec<Vec<u8>> = s.rows("fetch").map(<[u8]>::to_vec).collect();
    Fetch::from_bytes(&rows[0]).unwrap().state
}

const CANCEL: fn() -> Vec<u8> = || Cancel::<Fetch> { id: "a".into() }.to_bytes();

/// Of a claim and a cancel of one pending job, the first in the log wins
/// and the second is refused, whichever it is. Falsified by `cancel` not
/// asking whether the job is pending: the cancel after the claim was
/// taken, and the job ended `Cancelled` under its worker.
#[test]
fn a_claim_and_a_cancel_are_decided_by_log_order() {
    let d = domain();
    let (worker, alice) = (ctx("w", &["worker"], 20), ctx("alice", &[], 20));
    let mut s = store();
    assert_eq!(run(&d, &mut s, "claim", &worker, claim("w1")), Ok(()));
    assert!(run(&d, &mut s, "cancel", &alice, CANCEL()).is_err(), "claimed first");
    assert_eq!(state(&s), Job::Claimed { by: "w1".into(), at_ms: 20 });
    let mut s = store();
    assert!(run(&d, &mut s, "cancel", &ctx("bob", &[], 20), CANCEL()).is_err(), "not bob's");
    assert_eq!(run(&d, &mut s, "cancel", &alice, CANCEL()), Ok(()));
    assert!(run(&d, &mut s, "claim", &worker, claim("w1")).is_err(), "cancelled first");
    assert_eq!(state(&s), Job::Cancelled);
}

/// A job is claimed only when due and by its role; a claim holds for its
/// lease and is taken by another after it. Falsified by `Job::due` giving
/// a claimed job no time (`None`): the second worker's claim after the
/// lease was refused.
#[test]
fn a_lease_run_out_is_claimed_again() {
    let d = domain();
    let mut s = store();
    assert!(run(&d, &mut s, "claim", &ctx("w", &["worker"], 5), claim("w1")).is_err(), "not due");
    assert!(run(&d, &mut s, "claim", &ctx("x", &[], 20), claim("x")).is_err(), "not a worker");
    assert_eq!(run(&d, &mut s, "claim", &ctx("w", &["worker"], 20), claim("w1")), Ok(()));
    assert!(run(&d, &mut s, "claim", &ctx("w", &["worker"], 119), claim("w2")).is_err(), "held");
    assert_eq!(run(&d, &mut s, "claim", &ctx("w", &["worker"], 120), claim("w2")), Ok(()));
    assert_eq!(state(&s), Job::Claimed { by: "w2".into(), at_ms: 120 });
}

/// A finish by whoever does not hold the claim is refused; the holder's
/// is done with its outcome, or failed and due again at its retry.
/// Falsified by `finish` not comparing `by`: w1, whose lease w2 took, set
/// the outcome.
#[test]
fn a_finish_by_the_wrong_claimant_is_refused() {
    let d = domain();
    let w = |now| ctx("w", &["worker"], now);
    let fin = |by: &str, outcome: Result<i64, String>| {
        Finish::<Fetch> {
            id: "a".into(),
            by: by.into(),
            outcome,
        }
        .to_bytes()
    };
    let mut s = store();
    run(&d, &mut s, "claim", &w(20), claim("w1")).unwrap();
    run(&d, &mut s, "claim", &w(200), claim("w2")).unwrap();
    assert!(run(&d, &mut s, "finish", &w(201), fin("w1", Ok(1))).is_err(), "w2 holds it");
    assert_eq!(run(&d, &mut s, "finish", &w(202), fin("w2", Err("down".into()))), Ok(()));
    assert_eq!(
        state(&s),
        Job::Failed {
            why: "down".into(),
            retry_ms: 252
        }
    );
    assert!(run(&d, &mut s, "claim", &w(251), claim("w3")).is_err(), "before its retry");
    run(&d, &mut s, "claim", &w(252), claim("w3")).unwrap();
    assert_eq!(run(&d, &mut s, "finish", &w(253), fin("w3", Ok(7))), Ok(()));
    assert_eq!(state(&s), Job::Done(7));
    assert!(run(&d, &mut s, "claim", &w(9999), claim("w4")).is_err(), "done is over");
}

/// A job's and a finish's bytes read as their described types say, and
/// back: a done job and a failure each a record of one field. Falsified by
/// describing `Done` by its position (`0`): its bytes did not read.
#[test]
fn a_job_reads_as_its_type_says() {
    let done = Fetch {
        id: "a".into(),
        owner: "alice".into(),
        state: Job::Done(7),
    };
    let text = ark::json::to_text(&Fetch::describe(), &done.to_bytes()).unwrap();
    assert!(text.contains(r#""Done":{"value":7}"#), "{text}");
    assert_eq!(ark::json::from_text(&Fetch::describe(), &text).unwrap(), done.to_bytes());
    let failed = Finish::<Fetch> {
        id: "a".into(),
        by: "w".into(),
        outcome: Err("down".into()),
    };
    let text = ark::json::to_text(&Finish::<Fetch>::describe(), &failed.to_bytes()).unwrap();
    assert!(text.contains(r#""Err":{"why":"down"}"#), "{text}");
}
