//! Workers (`Handle::<Authority>::worker`) and their handlers
//! (`ark_peer::jobs`): a worker signed in as its account and linked in
//! the server's process; a handler on its own thread, which holds up
//! neither the worker nor the server; failures put off twice as long each
//! time; a job scheduled for later run then; and a job that recurs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use ark::effect::Job;
use ark::prelude::*;
use ark_auth::server::{Auth, Mode};
use ark_auth::{Account, Role, SignIn};
use ark_peer::{now_ms, Authority, Config, Handle, Memory, Peer, Server, Work};

/// Something to do: the job, its outcome what was said of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[effect(role = "worker", lease_ms = 60_000, retry_ms = 50, retry_max_ms = 200)]
pub struct Task {
    #[ark(key)]
    pub id: String,
    pub state: Job<String>,
}

/// What somebody wrote, and who.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
pub struct Note {
    #[ark(key)]
    pub id: String,
    pub by: String,
}

/// Something done two at a time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[effect(role = "worker", at_once = 2)]
pub struct Pair {
    #[ark(key)]
    pub id: String,
    pub state: Job<String>,
}

#[derive(Schema)]
pub struct App {
    pub task: Task,
    pub note: Note,
    pub pair: Pair,
    pub sign_in: SignIn,
    pub role: Role,
}

ark_auth::install!(App);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Ask {
    pub id: String,
    pub due_ms: i64,
}

#[mutator("task.ask")]
fn ask(_: &Ctx, db: &mut Db<App>, i: Ask) -> Result<(), String> {
    let state = Job::Pending { due_ms: i.due_ms };
    db.task.insert(Task { id: i.id, state })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Write {
    pub id: String,
}

#[mutator("note.write")]
fn write(ctx: &Ctx, db: &mut Db<App>, i: Write) -> Result<(), String> {
    db.note.insert(Note {
        id: i.id,
        by: ctx.user().into(),
    })
}

#[mutator("pair.ask")]
fn pair(ctx: &Ctx, db: &mut Db<App>, i: Write) -> Result<(), String> {
    let state = Job::Pending { due_ms: ctx.now };
    db.pair.insert(Pair { id: i.id, state })
}

fn config() -> Config {
    let d = Domain::new::<App>("app", "1").with(ark_auth::register::<App>);
    let d = d.with(Task::register::<App>).with(Pair::register::<App>);
    Config::of(Arc::new(ark::functions!(d)))
}

/// A server signing people in, and a worker of its: the account `bot`,
/// holding the `worker` role.
fn worker() -> (Handle<Authority>, Handle<Peer>) {
    let auth = Arc::new(Auth::new::<App>(Mode::Dev, "http://localhost", b"key"));
    let (hub, _) = Server::new(config()).auth(auth.clone()).build().unwrap();
    let bot = Account {
        id: "bot".into(),
        roles: vec!["worker".into()],
        ..Account::default()
    };
    let w = hub.worker(&auth, &bot, "a test", Box::new(Memory::default())).unwrap();
    (hub, w)
}

/// What `f` reads of the server's rows, once it says something, within
/// five seconds.
fn eventually<T>(hub: &Handle<Authority>, f: impl Fn(&Db<App>) -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(t) = hub.with(|h| ark::db::read::<App, _>(&h.log.store, |db| f(db))).flatten() {
            return t;
        }
        assert!(Instant::now() < until, "not within five seconds");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn state(hub: &Handle<Authority>, id: &str) -> Option<Job<String>> {
    hub.with(|h| ark::db::read::<App, _>(&h.log.store, |db| db.task.get(&id.to_string()).map(|t| t.state)))
        .flatten()
}

fn asked(w: &Handle<Peer>, id: &str, due_ms: i64) {
    w.lock().mutate("task.ask", &Ask { id: id.into(), due_ms }).unwrap();
}

/// A worker writes as its account, its role the account's, through the
/// server it was made of. Falsified by dialling with no token: the
/// connection was turned away and nothing reached the server.
#[test]
fn a_worker_writes_as_its_account() {
    let (hub, w) = worker();
    w.lock().mutate("note.write", &Write { id: "a".into() }).unwrap();
    let note = eventually(&hub, |db| db.note.get(&"a".to_string()));
    assert_eq!(note.by, "bot");
    assert!(w.lock().me().has_role("worker"));
}

/// While a handler waits, its worker goes on — what it writes confirmed —
/// and the server goes on sequencing; the job is done once the handler
/// is. Falsified by running the handler inside the worker's pump: the
/// note's confirmation waited for the handler, which waited for the note.
#[test]
fn a_slow_handler_holds_up_neither_its_worker_nor_its_server() {
    let (hub, w) = worker();
    let (go, wait) = mpsc::channel::<()>();
    let (started, wait) = (Arc::new(AtomicBool::new(false)), Mutex::new(wait));
    let s = started.clone();
    w.lock().handle::<Task>(move |_: &mut Work, t: &Task| {
        s.store(true, Ordering::SeqCst);
        wait.lock().unwrap().recv_timeout(Duration::from_secs(10)).map_err(|e| e.to_string())?;
        Ok(format!("{} done", t.id))
    });
    asked(&w, "slow", now_ms());
    let until = Instant::now() + Duration::from_secs(5);
    while !started.load(Ordering::SeqCst) {
        assert!(Instant::now() < until, "the handler never ran");
        std::thread::sleep(Duration::from_millis(5));
    }
    let id = w.lock().mutate("note.write", &Write { id: "meanwhile".into() }).unwrap();
    eventually(&hub, |db| db.note.get(&"meanwhile".to_string()));
    let until = Instant::now() + Duration::from_secs(5);
    while w.lock().outcome(&id) != Some(ark_peer::Outcome::Ok) {
        assert!(Instant::now() < until, "the worker's own write was never confirmed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(state(&hub, "slow"), Some(Job::Claimed { .. })), "still running");
    go.send(()).unwrap();
    let done = eventually(&hub, |db| db.task.get(&"slow".to_string()).filter(|t| matches!(t.state, Job::Done(_))));
    assert_eq!(done.state, Job::Done("slow done".into()));
}

/// A failure is retried after the job's retry, and after twice as long at
/// each failure in a row, to its longest: 50, 100, 200, 200 ms. Falsified
/// by finishing with no `again_ms`: every retry came 50 ms after.
#[test]
fn failures_are_put_off_twice_as_long_each_time() {
    let (hub, w) = worker();
    let runs = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let r = runs.clone();
    w.lock().handle::<Task>(move |_: &mut Work, _: &Task| {
        r.lock().unwrap().push(Instant::now());
        Err("not yet".into())
    });
    asked(&w, "flaky", now_ms());
    let until = Instant::now() + Duration::from_secs(5);
    while runs.lock().unwrap().len() < 5 {
        assert!(Instant::now() < until, "ran {} times", runs.lock().unwrap().len());
        std::thread::sleep(Duration::from_millis(5));
    }
    let runs = runs.lock().unwrap().clone();
    let gaps: Vec<u128> = runs.windows(2).map(|w| (w[1] - w[0]).as_millis()).collect();
    for (gap, least) in gaps.iter().zip([50, 100, 200, 200]) {
        assert!(*gap + 5 >= least && *gap < least + 150, "{gaps:?}");
    }
    assert!(matches!(state(&hub, "flaky"), Some(Job::Failed { .. }) | Some(Job::Claimed { .. })));
}

/// A job pending at a time to come is run then, and not before; one whose
/// handler says when it is due again runs again then. Falsified by a
/// handler's `again` not reaching its finish: the second run never came.
#[test]
fn a_job_runs_when_it_is_due_and_again_when_it_says() {
    let (hub, w) = worker();
    let runs = Arc::new(Mutex::new(Vec::<i64>::new()));
    let r = runs.clone();
    w.lock().handle::<Task>(move |work: &mut Work, _: &Task| {
        let mut runs = r.lock().unwrap();
        runs.push(now_ms());
        if runs.len() == 1 {
            work.again(now_ms() + 150);
        }
        Ok(format!("run {}", runs.len()))
    });
    let due = now_ms() + 300;
    asked(&w, "later", due);
    let done = eventually(&hub, |db| {
        db.task.get(&"later".to_string()).filter(|t| t.state == Job::Done("run 2".into()))
    });
    assert_eq!(done.state, Job::Done("run 2".into()));
    let runs = runs.lock().unwrap().clone();
    assert!(runs[0] >= due, "run before it was due: {} < {due}", runs[0]);
    assert!(runs[1] >= runs[0] + 150, "run again before it said: {runs:?}");
}

/// Each job is run on a thread of its own, as many at once as its job
/// says: two tasks of `Pair`'s, each waiting for the other to begin, both
/// done. Falsified by `AT_ONCE` being one whatever the job says: the
/// first waited for the second, which was never claimed.
#[test]
fn as_many_run_at_once_as_the_job_says() {
    let (hub, w) = worker();
    let begun = Arc::new(std::sync::Barrier::new(2));
    w.lock().handle::<Pair>(move |_: &mut Work, p: &Pair| {
        begun.wait();
        Ok(p.id.clone())
    });
    for id in ["a", "b"] {
        w.lock().mutate("pair.ask", &Write { id: id.into() }).unwrap();
    }
    let done = |db: &Db<App>| db.pair.scan().iter().all(|p| matches!(p.state, Job::Done(_))).then_some(());
    eventually(&hub, |db| (db.pair.scan().len() == 2).then_some(()).and(done(db)));
}
