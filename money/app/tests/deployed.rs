//! money's phones against the server the owner runs (ui/docs/app.md,
//! Servers Already Deployed): this build's phone host — its part of its
//! host under ark-ffi's `Hosted` and the app host every Ark app has, as
//! tests/phone.rs puts them together — dialling a money server built from
//! an older revision, which `ARK_DEPLOYED_SERVER` names (its base URL, a dev
//! server: `MONEY_DEV_AUTH=1`, `ARK_DEV_USER` set, the demo's household
//! seeded). `.github/compat-check.sh` builds that server from the revision
//! k2on/os pins, starts it and runs this; with no server named the test
//! says so and passes.
//!
//! The walk is a person's: the first page, the address typed, the server
//! found to be money and this domain, the sign-in (the dev server sends
//! the browser straight back, which the shell here does for it), the
//! household synced; then a transaction entered on the phone, confirmed by
//! the server, and seen on a second device signed in after it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark_ffi::hosted::{Hosted, PhoneApp};
use ark_ffi::{ArkHost, HostShell, Launch, ShotsTook, Standing};
use money_app::budget::Budget;
use money_app::host::Money;
use money_core::{AccountLine, TxnLine};

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

/// A phone's shell: what it is asked to open in the browser.
#[derive(Default)]
struct Shell {
    browsed: Mutex<Vec<String>>,
}

impl HostShell for Shell {
    fn changed(&self) {}
    fn browse(&self, url: String) {
        self.browsed.lock().unwrap().push(url);
    }
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while !f() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A new install of money, walked through its first run to `server` and
/// signed in there: its host, its app and its shell.
fn signed_in(root: &std::path::Path, server: &str) -> (Arc<ArkHost>, Arc<Hosted<MoneyPhone>>) {
    let launch = Launch {
        root: root.to_string_lossy().into(),
        version: "0.1 (1)".into(),
        demo: false,
        demo_day: None,
        shots: true,
        walk: true,
        scan: false,
        args: vec![],
    };
    let app = Hosted::<MoneyPhone>::new();
    let host = ArkHost::new(app.clone(), launch, None);
    let shell = Arc::new(Shell::default());
    host.start(shell.clone());
    for step in [
        r#"{"tap": "welcome.next"}"#.to_string(),
        r#"{"tap": "welcome.typed"}"#.to_string(),
        serde_json::json!({ "type": server }).to_string(),
        r#"{"tap": "welcome.check"}"#.to_string(),
    ] {
        until(&format!("{step} taken"), || host.shots_step(step.clone()) == ShotsTook::Done);
    }
    // The sign-in, in the browser: a dev server sends it straight back to
    // the app's scheme, as the phone's browser sheet would.
    until("the sign-in opened in the browser", || {
        if host.shows_app() {
            return true;
        }
        let Some(url) = shell.browsed.lock().unwrap().pop() else {
            return false;
        };
        let back = ark_ffi::ark_auth::client::straight_back(&url, MoneyPhone::SCHEME)
            .unwrap_or_else(|| panic!("{url} is not a dev server's sign-in that comes straight back"));
        host.returned(Some(back), None);
        true
    });
    until("the app shown, signed in", || host.shows_app());
    (host, app)
}

#[test]
fn the_phones_sync_with_the_deployed_server() {
    let Some(server) = std::env::var("ARK_DEPLOYED_SERVER").ok().filter(|s| !s.is_empty()) else {
        eprintln!("ARK_DEPLOYED_SERVER names no server: nothing to check (.github/compat-check.sh starts one)");
        return;
    };
    let server = server.trim_end_matches('/').to_string();

    // The server runs this domain: its /healthz names the schema this build
    // has, so the phone finds it the same, not newer.
    let health = match ark_ffi::ark_auth::probe::get(&format!("{server}/healthz")) {
        ark_ffi::ark_auth::probe::Answer::Answered { status: 200, body } => body,
        other => panic!("{server}/healthz: {other:?}"),
    };
    let line = ark_ffi::hosted::domain_line::<MoneyPhone>();
    let ours = line.split(' ').next().unwrap();
    assert!(
        health.contains(&format!("domain {ours} ")),
        "the server runs another schema:\n{health}\nthis build: {line}"
    );

    // One phone: the first run, the sign-in, the household synced. Its
    // screen is up (a UI attached to the process's runtime), so it dials.
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let _ui = ark_ffi::ArkProcess::shared().attach();
    let (alex, app) = signed_in(a_dir.path(), &server);
    let peer = alex.peer().expect("a session open");
    let st = peer.status();
    assert!(!st.alone && !st.user.is_empty(), "{st:?}");
    until("the household's accounts", || {
        !peer.view_typed::<AccountLine>("accounts", "null").unwrap().rows().is_empty()
    });
    until("the budget has its lines", || !app.page::<Budget>().groups.is_empty());
    let accounts = peer.view_typed::<AccountLine>("accounts", "null").unwrap().rows();
    let checking = accounts.iter().find(|a| a.name == "Checking").expect("the demo's Checking");

    // An edit, judged by the server: confirmed, not held or refused.
    let payee = format!("Compat {}", ark_peer::now_ms());
    let entry = serde_json::json!({
        "account_id": checking.id.hex(),
        "date": "2026-11-01",
        "payee": payee,
        "category_id": null,
        "memo": "",
        "amount": -1234,
        "cleared": false,
        "transfer_to": null,
    });
    let id = peer.mutate("add_transaction".into(), entry.to_string()).unwrap();
    until("the server confirms the transaction", || {
        let s = peer.standing(id.clone());
        assert!(!matches!(s, Standing::Rejected { .. }), "refused: {s:?}");
        s == Standing::Confirmed
    });

    // A second device, signed in after it — its own sign-in at the server
    // (the desktop's: the dev server answers the loopback with a code) and
    // its own replica, the peer a phone opens — has it from the server.
    // (The phone host's runtime is the process's one, so a second phone is
    // its peer rather than a second host here.)
    let login = ark_ffi::ark_auth::client::login(&server, None, |_| {}).expect("the second device's sign-in");
    let other = ark_ffi::hosted::open::<MoneyPhone>(b_dir.path().to_str().unwrap(), Some(login.into())).unwrap();
    other.connect(ark_ffi::ark_auth::socket_url(&server));
    other.set_online(true);
    let register = |p: &ark_ffi::Peer| p.view_typed::<TxnLine>("register", r#"{"account_id":null}"#).unwrap().rows();
    until("the second device has the transaction", || {
        register(&other).iter().any(|t| t.payee == payee)
    });
    assert_eq!(peer.status().pending, 0, "nothing of the first phone's held");
}
