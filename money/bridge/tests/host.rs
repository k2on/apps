//! money's host as a phone runs it (src/host.rs, over ark-ffi's `ArkHost`):
//! the first run's launch, the demo's household opened and every screen's
//! runtime made over it, what one runtime asks of another carried out in
//! Rust, a link taken as a launch, the bank asked, what a launch remembers,
//! and the way out. What the phones' `Model.swift`, `Bridge.swift`,
//! `Book.kt` and `Start.kt` did by hand, and their tests said.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark_ffi::ark_auth::onboarding::Step;
use ark_ffi::{HostShell, Launch};
use money_bridge::{MoneyHost, NativeAsk, ShellMsgFfi, WelcomeMsgFfi};

const TODAY: &str = "2026-10-26";

/// A phone's shell: counts what it is told, and what it is asked to browse.
#[derive(Default)]
struct Phone {
    told: Mutex<u32>,
    browsed: Mutex<Vec<String>>,
}

impl HostShell for Phone {
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
        scan: false,
        args: vec![],
    }
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
    let m = MoneyHost::new(launch(dir.path(), true), None);
    let phone = Arc::new(Phone::default());
    m.host().start(phone.clone());
    until("the demo shows", || m.host().shows_app());
    assert_eq!(m.host().step(), Step::Demo);
    assert!(*phone.told.lock().unwrap() > 0);
    for (name, made) in [
        ("shell", m.shell().is_some()),
        ("accounts", m.accounts().is_some()),
        ("budget", m.budget().is_some()),
        ("transactions", m.transactions().is_some()),
        ("insights", m.insights().is_some()),
        ("sheets", m.sheets().is_some()),
    ] {
        assert!(made, "{name}");
    }
    assert_eq!(m.today(), TODAY);
    assert_eq!(m.month(), "2026-10");
    let budget = m.budget().unwrap();
    until("the budget has its lines", || !budget.page().groups.is_empty());

    // The shell's toolbar asks for the month before: Budget's runtime told.
    let shell = m.shell().unwrap();
    shell.send(ShellMsgFfi::PreviousMonth {});
    assert_eq!(m.month(), "2026-09");
    assert!(shell.page().month_title.contains("September"), "{}", shell.page().month_title);
    shell.send(ShellMsgFfi::ThisMonth {});
    assert_eq!(m.month(), "2026-10");

    // Sync: the demo's bank, said once.
    m.sync_bank();
    assert_eq!(
        m.host().take_notice().as_deref(),
        Some("Chase: 2 new from the bank \u{2014} one still pending")
    );
    assert_eq!(m.host().take_notice(), None);

    // Who is using it, for the account sheet.
    let h = m.household();
    assert!(h.demo && h.who == "Alex" && h.server.is_none(), "{h:?}");

    // A link: the shell on Accounts, the phone asked to reset its pages.
    m.host().open_url("money:///accounts".into());
    until("the shell on Accounts", || shell.page().section == "accounts");
    assert!(m.take_asks().contains(&NativeAsk::Reset));

    // Nothing of a `-demo` launch is remembered.
    assert!(!dir.path().join("money").join("first-run").exists());
    until("idle", || m.host().idle());
}

/// "Try the Demo" from the first page opens the demo and is remembered: the
/// next launch opens it again by itself; leaving it is back to the first
/// page with no runtime left, and remembered too. Falsified by keeping the
/// demo in the phone's preferences (nothing read it back here).
#[test]
fn the_demo_is_remembered_and_left() {
    let dir = tempfile::tempdir().unwrap();
    let m = MoneyHost::new(launch(dir.path(), false), None);
    let phone = Arc::new(Phone::default());
    m.host().start(phone.clone());
    until("the first page", || matches!(m.host().step(), Step::Server { .. }));
    assert!(!m.host().shows_app());
    assert!(m.shell().is_none());
    let welcome = m.welcome();
    welcome.send(WelcomeMsgFfi::Skip {});
    welcome.send(WelcomeMsgFfi::TryDemo {});
    until("the demo shows", || m.host().shows_app());
    let kept = std::fs::read_to_string(dir.path().join("money").join("first-run")).unwrap();
    assert!(kept.contains("demo 1") && kept.contains("seen 1"), "{kept}");
    drop(m);

    let again = MoneyHost::new(launch(dir.path(), false), None);
    again.host().start(phone.clone());
    until("the demo shows again", || again.host().shows_app());
    assert_eq!(again.host().step(), Step::Demo);

    again.host().sign_out();
    until("the first page again", || matches!(again.host().step(), Step::Server { .. }));
    assert!(again.shell().is_none() && again.budget().is_none());
    let kept = std::fs::read_to_string(dir.path().join("money").join("first-run")).unwrap();
    assert!(!kept.contains("demo 1"), "{kept}");
}

/// A server typed is asked whether it runs money (its `/healthz`): one
/// that answers nothing is said, and nothing is browsed. Falsified by the
/// probe left to the phone (the step never moved past checking).
#[test]
fn a_server_that_is_not_there_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let m = MoneyHost::new(launch(dir.path(), false), None);
    let phone = Arc::new(Phone::default());
    m.host().start(phone.clone());
    let welcome = m.welcome();
    welcome.send(WelcomeMsgFfi::Skip {});
    welcome.send(WelcomeMsgFfi::Typed {
        text: "http://127.0.0.1:9".into(),
    });
    welcome.send(WelcomeMsgFfi::Check {});
    until("the probe answered", || {
        matches!(
            m.host().step(),
            Step::Server {
                checking: false,
                error: Some(_),
                ..
            }
        )
    });
    assert!(phone.browsed.lock().unwrap().is_empty());
}
