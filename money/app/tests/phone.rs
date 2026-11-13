//! money as its phones run it: its part of its host (src/host.rs) under
//! ark-ffi's `Hosted` and the app host every Ark app has (`ArkHost`), as
//! its generated library (ui/prebuild.py's `ffi`, from app.toml's `[ffi]`)
//! puts them together, here without UniFFI's twins: the first run's launch,
//! the demo's household opened and every screen's runtime over it, what one
//! runtime asks of another carried out in Rust, a link taken as a launch,
//! the bank asked, what a launch remembers and the way out; the replica
//! opened alone, the demo seeded, and two devices of one household synced
//! through a server in the process.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark_app::welcome::WelcomeMsg;
use ark_ffi::ark_auth::onboarding::Step;
use ark_ffi::hall::Hall;
use ark_ffi::hosted::{Hosted, PhoneApp};
use ark_ffi::{ArkHost, HostShell, Launch, Peer, ShotsTook, Standing, Taken};
use money_app::budget::Budget;
use money_app::host::{Money, ShellAsk};
use money_app::shell::{MoneyShell, ShellMsg};
use money_core::accounts::AccountKind;
use money_core::{AccountLine, BudgetLine, MoneyChange, OpenAccount, Summary, TxnLine};

const TODAY: &str = "2026-10-26";

/// money on the phones, as its generated library says it (app.toml's
/// `[ffi]`).
struct MoneyPhone;

impl PhoneApp for MoneyPhone {
    type App = Money;
    const NAME: &'static str = "Money";
    const SCHEME: &'static str = "money";
    const DEMO: bool = true;
    fn domain() -> Arc<ark::domain::Domain> {
        money_core::shared()
    }
    fn demo(today: &str, now_ms: i64) -> ark_peer::Peer {
        money_seed::open(money_seed::seeded_disk(today, now_ms))
    }
}

/// A phone's shell: counts what it is told, and what it is asked to browse.
#[derive(Default)]
struct Shell {
    told: Mutex<u32>,
    browsed: Mutex<Vec<String>>,
}

impl HostShell for Shell {
    fn changed(&self) {
        *self.told.lock().unwrap() += 1;
    }
    fn browse(&self, url: String) {
        self.browsed.lock().unwrap().push(url);
    }
}

fn launch(root: &std::path::Path, demo: bool) -> Launch {
    Launch {
        root: root.to_string_lossy().into(),
        version: "0.1 (1)".into(),
        demo,
        demo_day: demo.then(|| TODAY.into()),
        shots: false,
        walk: false,
        scan: false,
        args: vec![],
    }
}

/// money's library as the phone holds it: the app host over its part.
fn library(launch: Launch) -> (Arc<ArkHost>, Arc<Hosted<MoneyPhone>>, Arc<Shell>) {
    let app = Hosted::<MoneyPhone>::new();
    let host = ArkHost::new(app.clone(), launch, None);
    let shell = Arc::new(Shell::default());
    host.start(shell.clone());
    (host, app, shell)
}

/// Wait (a few seconds at most) until `f` holds: the host opens off the
/// caller's thread.
fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn month(app: &Hosted<MoneyPhone>) -> String {
    app.read(|m, _| m.month().to_string()).unwrap_or_default()
}

/// A `-demo` launch opens the demo's household on its day, past the first
/// run, every screen's runtime made; the budget's month moved from the
/// shell's toolbar reaches Budget's runtime through the host (StepMonth);
/// Sync asks the demo's bank and says what it brought; a link is taken as a
/// launch (the shell on its place, the phone asked to put its own pages
/// away). Falsified by a request left for the phone (the month stayed), and
/// by a launch remembered for the next (it wrote what it remembered).
#[test]
fn a_demo_launch_runs_every_screen() {
    let dir = tempfile::tempdir().unwrap();
    let (host, app, shell) = library(launch(dir.path(), true));
    until("the demo shows", || host.shows_app());
    assert_eq!(host.step(), Step::Demo);
    assert!(*shell.told.lock().unwrap() > 0);
    assert!(app.is_open() && app.generation() > 0);
    assert_eq!(app.today(), TODAY);
    assert_eq!(month(&app), "2026-10");
    until("the budget has its lines", || !app.page::<Budget>().groups.is_empty());

    // The shell's toolbar asks for the month before: Budget's runtime told.
    app.send::<MoneyShell>(ShellMsg::PreviousMonth);
    assert_eq!(month(&app), "2026-09");
    let title = app.page::<MoneyShell>().month_title;
    assert!(title.contains("September"), "{title}");
    app.send::<MoneyShell>(ShellMsg::ThisMonth);
    assert_eq!(month(&app), "2026-10");

    // Sync with Banks: the demo's bank, said once.
    app.send::<MoneyShell>(ShellMsg::Refresh);
    assert_eq!(
        host.take_notice().as_deref(),
        Some("Chase: 2 new from the bank \u{2014} one still pending")
    );
    assert_eq!(host.take_notice(), None);

    // Who is using it, for the account sheet: the shell's page says.
    let h = app.page::<MoneyShell>().household;
    assert!(h.demo && h.who == "Alex" && h.server.is_none(), "{h:?}");

    // A link: the shell on Accounts, the phone asked to reset its pages.
    host.open_url("money:///accounts".into());
    until("the shell on Accounts", || app.page::<MoneyShell>().section == "accounts");
    assert!(app.take_natives().contains(&ShellAsk::Reset));

    // Nothing of a `-demo` launch is remembered.
    assert!(!dir.path().join("money").join("first-run").exists());
    until("idle", || host.idle());
}

/// "Try the Demo" from the first page opens the demo and is remembered: the
/// next launch opens it again by itself; leaving it is back to the first
/// page with no runtime left, and remembered too. Falsified by keeping the
/// demo in the phone's preferences (nothing read it back here).
#[test]
fn the_demo_is_remembered_and_left() {
    let dir = tempfile::tempdir().unwrap();
    let (host, app, _shell) = library(launch(dir.path(), false));
    until("the first page", || matches!(host.step(), Step::Server { .. }));
    assert!(!host.shows_app() && !app.is_open());
    host.press(WelcomeMsg::Skip);
    host.press(WelcomeMsg::TryDemo);
    until("the demo shows", || host.shows_app());
    let kept = std::fs::read_to_string(dir.path().join("money").join("first-run")).unwrap();
    assert!(kept.contains("demo 1") && kept.contains("seen 1"), "{kept}");
    drop((host, app));

    let (again, app, _shell) = library(launch(dir.path(), false));
    until("the demo shows again", || again.shows_app());
    assert_eq!(again.step(), Step::Demo);

    again.sign_out();
    until("the first page again", || matches!(again.step(), Step::Server { .. }));
    assert!(!app.is_open() && app.generation() == 0);
    let kept = std::fs::read_to_string(dir.path().join("money").join("first-run")).unwrap();
    assert!(!kept.contains("demo 1"), "{kept}");
}

/// A server typed is asked whether it runs money (its `/healthz`): one
/// that answers nothing is said, and nothing is browsed. Falsified by the
/// probe left to the phone (the step never moved past checking).
#[test]
fn a_server_that_is_not_there_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let (host, _app, shell) = library(launch(dir.path(), false));
    host.press(WelcomeMsg::Skip);
    host.press(WelcomeMsg::Typed {
        text: "http://127.0.0.1:9".into(),
    });
    host.press(WelcomeMsg::Check);
    until("the probe answered", || {
        matches!(
            host.step(),
            Step::Server {
                checking: false,
                error: Some(_),
                ..
            }
        )
    });
    assert!(shell.browsed.lock().unwrap().is_empty());
}

/// A launch's arguments open nothing until the shell says its window is up
/// (`ArkHost::shown`), then open once: the Mac's main window never showed
/// when a launch's sheet or Settings was asked for before it was on the
/// screen (app-screens 43159033586). Falsified by handing the arguments to
/// the app as soon as the session opens (Settings asked before `shown`).
#[test]
fn a_launch_opens_once_the_window_is_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = launch(dir.path(), true);
    l.args = ["-demo", TODAY, "-screen", "settings", "-settingsTab", "banks"]
        .map(String::from)
        .to_vec();
    let (host, app, _shell) = library(l);
    until("the demo shows", || host.shows_app());
    until("idle", || host.idle());
    assert!(app.take_natives().is_empty(), "nothing asked before the window is up");
    host.shown();
    until("Settings asked", || {
        app.take_natives().iter().any(|n| matches!(n, ShellAsk::Settings { .. }))
    });
    host.shown();
    until("idle", || host.idle());
    assert!(app.take_natives().is_empty(), "the launch opened once");
}

fn change(c: MoneyChange) -> (&'static str, Vec<u8>) {
    (c.name(), c.input())
}

/// money's replica opened alone on a disk, the budget set up and an account
/// opened by name, seen in a typed view, and a refusal said as a value; the
/// domain as a server's `/healthz` names it. Falsified by opening the
/// replica with no domain: there was no function `open_account`.
#[test]
fn money_opens_and_is_called_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let p = ark_ffi::hosted::open::<MoneyPhone>(dir.path().to_str().unwrap(), None).unwrap();
    p.alone();
    p.mutate("setup".into(), "null".into()).unwrap();
    let accounts = p.view_typed::<AccountLine>("accounts", "null").unwrap();
    assert!(accounts.rows().is_empty());
    let open = |name: &str, balance: i64| {
        change(MoneyChange::OpenAccount(OpenAccount {
            name: name.into(),
            kind: AccountKind::Checking,
            on_budget: true,
            balance,
            date: "2026-10-01".into(),
        }))
    };
    let (name, input) = open("Checking", 120_000);
    let id = p.mutate_encoded(name, input).unwrap();
    assert_eq!(p.standing(id), Standing::Confirmed, "alone, its own authority");
    let (update, rows) = accounts.take();
    assert!(matches!(update, Taken::Patched { .. }), "{update:?}");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].name.as_str(), rows[0].balance), ("Checking", 120_000));

    let (name, input) = open(" ", 0);
    assert!(p.mutate_encoded(name, input).is_err(), "an account needs a name");
    let line = ark_ffi::hosted::domain_line::<MoneyPhone>();
    let (hash, version) = line.split_once(' ').unwrap();
    assert_eq!((hash.len(), version), (64, money_core::VERSION));
}

fn demo() -> Arc<Peer> {
    ark_ffi::hosted::demo::<MoneyPhone>(Some(TODAY.into())).unwrap()
}

/// The demo's household as the phones open it: seeded up to its day, its
/// own authority, holding the importer's role. Falsified by seeding for the
/// clock rather than the day given: the month had no lines for 2026-10.
#[test]
fn the_demo_is_the_seeded_household() {
    let p = demo();
    let st = p.status();
    assert!(st.alone, "{st:?}");
    assert_eq!(st.user, money_seed::ME);
    let accounts = p.view_typed::<AccountLine>("accounts", "null").unwrap().rows();
    let names: Vec<&str> = accounts.iter().map(|a| a.name.as_str()).collect();
    for n in ["Checking", "Savings", "Cash", "Mortgage", "Retirement", "Chase Sapphire"] {
        assert!(names.contains(&n), "{names:?}");
    }
    let month = r#"{"month":"2026-10"}"#;
    let lines = p.view_typed::<BudgetLine>("month", month).unwrap().rows();
    let dining = lines.iter().find(|l| l.name == "Dining Out").unwrap();
    assert!(dining.available < 0, "the seed overspends it: {}", dining.available);
    let summary = p.view_typed::<Summary>("summary", month).unwrap().rows();
    assert!(summary.len() == 1 && summary[0].ready_to_assign > 0, "{summary:?}");
    assert!(
        !p.view_typed::<TxnLine>("inbox", "null").unwrap().rows().is_empty(),
        "the card's last days wait for a look"
    );
}

/// Falsified by the day's noon taken in the local zone's sense of a day
/// (`now_ms` itself): two demos for one day differed.
#[test]
fn a_day_is_the_same_household_every_time() {
    let rows = |p: Arc<Peer>| p.view_typed::<TxnLine>("register", r#"{"account_id":null}"#).unwrap().rows();
    assert_eq!(rows(demo()), rows(demo()));
    assert!(ark_ffi::hosted::demo::<MoneyPhone>(Some("26 October".into())).is_err());
}

/// Two people of one household dial a money server in the process, and
/// what one enters is the other's register once carried; a bank import
/// from anybody but the importer is refused, and the importer's is the
/// server's. Falsified by the hall's token dropping its roles: the
/// importer's import was rejected as the household's was.
#[test]
fn a_household_shares_one_budget() {
    let dir = || tempfile::tempdir().unwrap();
    let (a, b, c) = (dir(), dir(), dir());
    let hall = Hall::new(ark_peer::Config::of(money_core::shared()));
    let path = |d: &tempfile::TempDir| d.path().to_str().unwrap().to_string();
    let alex = hall.device(path(&a), "alex".into(), "phone".into(), vec![]).unwrap();
    let sam = hall.device(path(&b), "sam".into(), "phone".into(), vec![]).unwrap();
    alex.mutate("setup".into(), "null".into()).unwrap();
    let open = r#"{"name":"Checking","kind":"checking","on_budget":true,"balance":50000,"date":"2026-10-01"}"#;
    alex.mutate("open_account".into(), open.into()).unwrap();
    hall.carry();
    let accounts = sam.view_typed::<AccountLine>("accounts", "null").unwrap().rows();
    assert_eq!(accounts.len(), 1, "sam sees alex's account");
    let checking = accounts[0].id;

    let register = alex.view_typed::<TxnLine>("register", r#"{"account_id":null}"#).unwrap();
    let before = register.rows().len();
    let coffee = format!(
        r#"{{"account_id":"{}","date":"2026-10-02","payee":"Blue Bottle","category_id":null,"memo":"","amount":-650,"cleared":false,"transfer_to":null}}"#,
        checking.hex()
    );
    let id = sam.mutate("add_transaction".into(), coffee).unwrap();
    hall.carry();
    assert_eq!(sam.standing(id), Standing::Confirmed);
    let rows = register.rows();
    assert_eq!(rows.len(), before + 1);
    assert!(rows.iter().any(|t| t.payee == "Blue Bottle" && t.user_id == "sam"));

    let import = r#"{"bank":"chase","institution":"Chase","cursor":"c1","accounts":[],"upserts":[],"removed":[],"choose":false,"from_start":false,"institution_id":null}"#;
    assert!(alex.mutate("import".into(), import.into()).is_err(), "only the importer imports");
    let importer = hall.device(path(&c), "importer".into(), "plaid".into(), vec!["bank".into()]).unwrap();
    let id = importer.mutate("import".into(), import.into()).unwrap();
    hall.carry();
    assert_eq!(importer.standing(id), Standing::Confirmed);
}

/// A screenshot walk's steps on a new install (ui/docs/shots.md, "In the
/// App"): the first run's pages are a new person's, and each step reaches
/// what it names on them by id — Get Started, the address field typed
/// into, Continue — through the host, as a tap sends it, and the host is
/// not idle while the server is asked. Falsified by a walk's
/// launch passing over the pages (as a `-shots` launch once did), or a
/// step taking nothing.
#[test]
fn a_walk_goes_through_the_first_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = launch(dir.path(), false);
    l.shots = true;
    l.walk = true;
    let (host, _, _) = library(l);
    let tree = host.shots_tree();
    assert!(tree.contains("[button] welcome.next \u{2014} Get Started (taps)"), "{tree}");
    assert_eq!(host.shots_step(r#"{"tap": "welcome.next"}"#.into()), ShotsTook::Done);
    let tree = host.shots_tree();
    assert!(tree.contains("[field] welcome.typed \u{2014} Server Address (types)"), "{tree}");
    assert!(matches!(
        host.shots_step(r#"{"tap": "welcome.nowhere"}"#.into()),
        ShotsTook::NotYet { .. }
    ));
    assert_eq!(host.shots_step(r#"{"tap": "welcome.typed"}"#.into()), ShotsTook::Done);
    assert_eq!(host.shots_step(r#"{"type": "http://127.0.0.1:9"}"#.into()), ShotsTook::Done);
    let typed = host.with_run(|r| r.page()).server.map(|s| s.typed).unwrap_or_default();
    assert_eq!(typed, "http://127.0.0.1:9");
    assert_eq!(host.shots_step(r#"{"tap": "welcome.check"}"#.into()), ShotsTook::Done);
    // The server is asked on a thread of the host's: not idle until it answers.
    until("the server's answer", || host.idle());
}

/// Every screen's view, as the generated library's `phone_host!` hands
/// them to a walk's steps.
fn every_view(app: &Arc<Hosted<MoneyPhone>>) {
    use ark_app::shots::{nodes, Screen};
    use money_app::{accounts, budget, insights, sheets, shell, transactions};
    use std::rc::Rc;
    fn screen<A>(a: &Arc<Hosted<MoneyPhone>>, name: &str, view: fn(&A::Screen) -> ark_app::view::View<A::Msg>) -> Screen
    where
        A: ark_app::App<Key = money_core::MoneyView, Change = MoneyChange, Route = money_app::routes::Route, Request = money_app::MoneyRequest>,
        A::Msg: 'static,
        Money: ark_app::host::Has<A>,
    {
        let to = a.clone();
        Screen {
            name: name.into(),
            nodes: nodes(&view(&a.page::<A>()), Rc::new(move |m| to.send::<A>(m))),
        }
    }
    let weak = Arc::downgrade(app);
    app.set_views(Box::new(move || {
        let Some(a) = weak.upgrade() else { return vec![] };
        vec![
            screen::<accounts::Accounts>(&a, "accounts", accounts::view::accounts::view),
            screen::<budget::Budget>(&a, "budget", budget::view::budget::view),
            screen::<transactions::Transactions>(&a, "transactions", transactions::view::transactions::view),
            screen::<insights::Insights>(&a, "insights", insights::view::insights::view),
            screen::<sheets::Sheets>(&a, "sheets", sheets::view::sheets::view),
            screen::<shell::MoneyShell>(&a, "shell", shell::view::money_shell::view),
        ]
    }));
}

/// What waits for a page to be drawn, taken as a phone takes it once it
/// has drawn: until nothing waits.
fn draw(app: &Hosted<MoneyPhone>) {
    use money_app::{accounts, budget, insights, sheets, shell, transactions};
    for _ in 0..16 {
        let any = [
            app.drawn::<shell::MoneyShell>(),
            app.drawn::<accounts::Accounts>(),
            app.drawn::<budget::Budget>(),
            app.drawn::<transactions::Transactions>(),
            app.drawn::<insights::Insights>(),
            app.drawn::<sheets::Sheets>(),
        ];
        if !any.contains(&true) {
            return;
        }
    }
}

/// The phone's walk of money/demo/shots.json after its sign-in (the demo
/// opened instead): each shot's steps taken, and what each shot's screen
/// could be named by then, by its id.
fn walk(host: &ArkHost, app: &Hosted<MoneyPhone>) -> Vec<(String, String)> {
    // Read where the run finds it (money/demo beside this crate), not
    // compiled in: the crate's build holds only its own directory.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/shots.json");
    let plan: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect(path)).unwrap();
    let shots = plan["walk"]["phone"].as_array().unwrap();
    let first = shots.iter().position(|s| s["signIn"] == true).unwrap() + 1;
    let mut out = vec![];
    for shot in &shots[first..] {
        let id = shot["id"].as_str().unwrap();
        for step in shot["steps"].as_array().unwrap() {
            let took = host.shots_step(step.to_string());
            assert_eq!(took, ShotsTook::Done, "{id}: {step}\n{}", host.shots_tree());
            draw(app);
            app.take_natives();
        }
        out.push((id.to_string(), host.shots_tree()));
    }
    out
}

/// A walk's reset (ui/docs/shots.md, "In the App"): the app put back as a
/// launch already signed in shows it, so the walk after it goes as the
/// first went — every shot's step finds what it names, and every shot's
/// screen holds the same elements — and the platform is asked to put away
/// its own pages. Falsified by a reset that put away the sheets and chose
/// the first tab alone (app-screens 43327637228: Transactions' All kept,
/// the second walk's review was the first's transaction list, and its
/// transaction shot found no sheet to dismiss).
#[test]
fn a_walk_is_reset() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = launch(dir.path(), true);
    l.demo_day = Some("2026-11-12".into());
    let (host, app, _) = library(l);
    every_view(&app);
    until("the demo shows", || host.shows_app());
    until("the budget has its lines", || !app.page::<Budget>().groups.is_empty());
    let fresh = host.shots_tree();
    let first = walk(&host, &app);
    let version = host.version();
    assert_eq!(host.shots_reset(), ShotsTook::Done);
    assert!(app.take_natives().contains(&ShellAsk::Reset));
    assert!(host.version() > version, "a reset is a newer screen");
    assert_eq!(app.page::<MoneyShell>().section, "budget");
    assert_eq!(host.shots_tree(), fresh, "the reset is where the walk began");
    let again = walk(&host, &app);
    for ((id, a), (_, b)) in first.iter().zip(&again) {
        assert_eq!(a, b, "{id} differs on the walk after the reset");
    }
}
