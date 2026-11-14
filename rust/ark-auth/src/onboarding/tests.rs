//! Every transition of the sign-in machine, with the effects each asks for,
//! as both apps' phones and windows walk it: where a launch starts from what
//! was remembered, the server step and its failures, the sign-in that goes
//! by itself once a launch, the demo, and the ways back out of a session.

use super::*;

const H: &str = "https://h";

fn money() -> AppInfo {
    AppInfo {
        name: "Money".into(),
        scheme: "money".into(),
        demo: true,
        what: "your budget".into(),
    }
}

fn harken() -> AppInfo {
    AppInfo {
        name: "Harken".into(),
        scheme: "harken".into(),
        demo: false,
        what: "your library".into(),
    }
}

fn ana() -> Login {
    Login {
        token: "t".into(),
        session: "s1".into(),
        user: "ana".into(),
        name: "Ana".into(),
        roles: vec!["library".into()],
        expires_ms: 9,
        ..Login::default()
    }
}

fn browse() -> Effect {
    Effect::Browse {
        server: H.into(),
        url: "https://h/auth/login?redirect=harken%3A%2F%2Fauth".into(),
    }
}

fn launched(server: Option<&str>, login: Option<Login>, demo: bool) -> Event {
    Event::Launched {
        remembered: Remembered {
            server: server.map(Into::into),
            login,
            demo,
        },
        code: None,
        returned: false,
    }
}

fn remembered(server: Option<&str>, login: Option<Login>, demo: bool) -> Effect {
    Effect::Remember {
        remembered: Remembered {
            server: server.map(Into::into),
            login,
            demo,
        },
    }
}

fn sign_in(phase: Phase, error: Option<&str>) -> Step {
    Step::SignIn {
        server: H.into(),
        phase,
        error: error.map(Into::into),
    }
}

fn server(typed: &str, checking: bool, error: Option<&str>) -> Step {
    Step::Server {
        typed: typed.into(),
        checking,
        error: error.map(Into::into),
    }
}

fn code(c: &str) -> Event {
    Event::Code { code: c.into() }
}

fn found(url: &str) -> Event {
    Event::Checked {
        found: Found::Server {
            url: url.into(),
            newer: false,
        },
    }
}

/// Signed in at `H` just now, as ana, the session open.
fn signed_in(m: &Onboarding) {
    m.next(code("c"));
    m.next(Event::SignedIn { login: ana() });
}

/// Where a launch starts: nothing remembered is the server's address; a
/// server and a login is the app at once, over the replica already here; a
/// server alone is signing in; the demo is the demo — in an app that has one.
#[test]
fn a_launch_starts_from_what_was_remembered() {
    let m = Onboarding::new(harken());
    assert_eq!(m.next(launched(None, None, false)), vec![]);
    assert_eq!(m.step(), Step::start());
    // A login with no server is nothing to open.
    assert_eq!(m.next(launched(None, Some(ana()), false)), vec![]);
    assert_eq!(m.step(), Step::start());
    let r = Onboarding::new(harken());
    assert_eq!(
        r.next(launched(Some(H), Some(ana()), false)),
        vec![Effect::OpenSession {
            server: H.into(),
            login: ana()
        }]
    );
    assert_eq!(
        r.step(),
        Step::Running {
            server: H.into(),
            login: ana()
        }
    );
    let d = Onboarding::new(money());
    assert_eq!(d.next(launched(Some(H), None, true)), vec![Effect::OpenDemo]);
    assert_eq!(d.step(), Step::Demo);
    // harken has no demo: one remembered is the server's sign-in.
    let h = Onboarding::new(harken());
    assert_eq!(h.next(launched(Some(H), None, true)), vec![browse()]);
}

/// No sign-in button: a server with nobody signed in goes to sign in at
/// launch, on the app's own scheme. Falsified by starting it idle: no browse.
#[test]
fn a_sign_in_goes_by_itself_at_launch() {
    let m = Onboarding::new(harken());
    assert_eq!(m.next(launched(Some(H), None, false)), vec![browse()]);
    assert_eq!(m.step(), sign_in(Phase::Browser, None));
    assert_eq!(m.said().status.as_deref(), Some("Waiting for the sign-in…"));
}

/// A page that comes back from signing in with a code trades it, and does
/// not go again. Falsified by browsing when a code came with the launch.
#[test]
fn a_launch_with_a_code_trades_it() {
    let m = Onboarding::new(harken());
    let e = Event::Launched {
        remembered: Remembered {
            server: Some(H.into()),
            ..Remembered::default()
        },
        code: Some("c0de".into()),
        returned: false,
    };
    assert_eq!(
        m.next(e),
        vec![Effect::Exchange {
            server: H.into(),
            code: "c0de".into()
        }]
    );
    assert_eq!(m.step(), sign_in(Phase::Exchanging, None));
    assert_eq!(m.next(Event::SignInFailed { why: "no such code".into() }), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Idle, Some("Signing in didn't finish: no such code.")));
}

/// A sign-in that comes back empty is said, and not tried again until the
/// app comes back to the front. Falsified by browsing again on `CameBack`.
#[test]
fn a_sign_in_that_comes_back_empty_is_said_once_and_not_looped() {
    let m = Onboarding::new(harken());
    m.next(launched(Some(H), None, false));
    assert_eq!(m.next(Event::CameBack { why: None }), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Idle, Some(words::CLOSED)));
    // Nothing here starts it again: a code that is late, a check, a sync.
    assert_eq!(m.next(Event::Synced), vec![]);
    assert_eq!(m.next(Event::Check), vec![]);
    assert_eq!(m.next(Event::CameBack { why: Some("again".into()) }), vec![]);
    // The app away and back is the web's reload: once more.
    assert_eq!(m.next(Event::Foreground), vec![browse()]);
    assert_eq!(m.next(Event::CameBack { why: Some("refused".into()) }), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Idle, Some("Signing in didn't finish: refused.")));
}

/// Back in front with the browser still said to be open is back from it
/// with nothing (a browser tab is another activity): said, and not opened
/// again until the next time. Falsified by browsing again: a loop.
#[test]
fn back_in_front_from_the_browser_is_the_browser_closed() {
    let m = Onboarding::new(harken());
    m.next(launched(Some(H), None, false));
    assert_eq!(m.next(Event::Foreground), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Idle, Some(words::CLOSED)));
    assert_eq!(m.next(Event::Foreground), vec![browse()]);
    // A code that arrived first is traded, and the return changes nothing.
    m.next(code("c"));
    assert_eq!(m.next(Event::Foreground), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Exchanging, None));
}

/// "Sign In", where a shell has the button, opens the browser whatever this
/// launch has tried — but never over a code being traded.
#[test]
fn the_button_opens_the_browser() {
    let m = Onboarding::new(harken());
    m.next(launched(Some(H), None, false));
    m.next(Event::CameBack { why: None });
    assert_eq!(m.next(Event::OpenSignIn), vec![browse()]);
    assert_eq!(m.step(), sign_in(Phase::Browser, None));
    assert_eq!(m.next(Event::OpenSignIn), vec![browse()]);
    m.next(code("c"));
    assert_eq!(m.next(Event::OpenSignIn), vec![]);
}

/// A code is traded, remembered with the server, and the session opens; the
/// page says what it is getting until the session has what the server had.
#[test]
fn a_code_is_exchanged_then_the_app_opens() {
    let m = Onboarding::new(harken());
    m.next(launched(Some(H), None, false));
    assert_eq!(
        m.next(code("c0de")),
        vec![Effect::Exchange {
            server: H.into(),
            code: "c0de".into()
        }]
    );
    assert_eq!(m.said().status.as_deref(), Some("Signing in…"));
    // A second code while one is traded is nothing.
    assert_eq!(m.next(code("again")), vec![]);
    assert_eq!(
        m.next(Event::SignedIn { login: ana() }),
        vec![
            remembered(Some(H), Some(ana()), false),
            Effect::OpenSession {
                server: H.into(),
                login: ana()
            }
        ]
    );
    assert_eq!(
        m.step(),
        Step::Opening {
            server: H.into(),
            login: ana()
        }
    );
    assert_eq!(m.said().status.as_deref(), Some("Getting your library from h…"));
    // Nothing but the sync moves it on.
    for e in [code("c"), Event::Check, Event::OpenSignIn, Event::CameBack { why: None }] {
        assert_eq!(m.next(e), vec![]);
    }
    assert_eq!(m.next(Event::Synced), vec![]);
    assert_eq!(
        m.step(),
        Step::Running {
            server: H.into(),
            login: ana()
        }
    );
    assert_eq!(m.said(), Said::default());
}

/// A failed exchange is said and not retried by itself. Falsified by `go`
/// on failure.
#[test]
fn a_failed_exchange_is_said_once() {
    let m = Onboarding::new(harken());
    m.next(launched(Some(H), None, false));
    m.next(code("c"));
    assert_eq!(m.next(Event::SignInFailed { why: "no answer".into() }), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Idle, Some("Signing in didn't finish: no answer.")));
    assert_eq!(m.said().error.as_deref(), Some("Signing in didn't finish: no answer."));
    // Only a code being traded can fail.
    assert_eq!(m.next(Event::SignInFailed { why: "late".into() }), vec![]);
    assert_eq!(m.next(Event::SignedIn { login: ana() }), vec![]);
}

/// Turned away goes back to sign in by itself, the login forgotten and the
/// server kept — once a launch: a login turned away straight after it was
/// made is not sent round again. Falsified by resetting `tried` on a denial.
#[test]
fn a_denial_signs_in_again_once_a_launch() {
    let r = Onboarding::new(harken());
    r.next(launched(Some(H), Some(ana()), false));
    assert_eq!(
        r.next(Event::Denied { reason: "revoked".into() }),
        vec![Effect::CloseSession, remembered(Some(H), None, false), browse()]
    );
    let Step::SignIn {
        phase: Phase::Browser,
        error: Some(e),
        ..
    } = r.step()
    else {
        panic!("{:?}", r.step())
    };
    assert_eq!(
        e,
        "The server signed you out (revoked). Sign in again — what's here is kept, and syncs once you do."
    );
    signed_in(&r);
    r.next(Event::Synced);
    assert_eq!(
        r.next(Event::Denied { reason: "again".into() }),
        vec![Effect::CloseSession, remembered(Some(H), None, false)]
    );
    let Step::SignIn {
        phase: Phase::Idle,
        error: Some(e),
        ..
    } = r.step()
    else {
        panic!("{:?}", r.step())
    };
    assert!(e.contains("(again)"), "{e}");
    // While opening, too.
    signed_in(&r);
    assert_eq!(
        r.next(Event::Denied { reason: "x".into() }),
        vec![Effect::CloseSession, remembered(Some(H), None, false)]
    );
}

/// Signing out keeps the server, forgets the login, and waits on the page:
/// it is not a sign-in to start again at once. Coming back is.
#[test]
fn signing_out_keeps_the_server_and_does_not_sign_in_again() {
    let r = Onboarding::new(harken());
    r.next(launched(Some(H), Some(ana()), false));
    assert_eq!(r.next(Event::SignOut), vec![Effect::CloseSession, remembered(Some(H), None, false)]);
    assert_eq!(r.step(), sign_in(Phase::Idle, None));
    assert_eq!(r.next(Event::Foreground), vec![browse()]);
    // From opening, the same.
    signed_in(&r);
    assert_eq!(r.next(Event::SignOut), vec![Effect::CloseSession, remembered(Some(H), None, false)]);
    assert_eq!(r.step(), sign_in(Phase::Idle, None));
}

/// Nothing but leaving moves a running app.
#[test]
fn nothing_else_moves_a_running_app() {
    let r = Onboarding::new(money());
    r.next(launched(Some(H), Some(ana()), false));
    let running = r.step();
    for e in [
        Event::Check,
        Event::OpenSignIn,
        Event::CameBack { why: None },
        code("c"),
        Event::Synced,
        Event::ChangeServer,
        Event::TryDemo,
        Event::Foreground,
    ] {
        assert_eq!(r.next(e.clone()), vec![], "{e:?}");
        assert_eq!(r.step(), running, "{e:?}");
    }
}

/// A session that will not open is said, the login kept for the next try,
/// and nothing goes by itself. Falsified by forgetting the login.
#[test]
fn a_session_that_will_not_open_keeps_the_login() {
    let r = Onboarding::new(money());
    r.next(launched(Some(H), Some(ana()), false));
    assert_eq!(r.next(Event::OpenFailed { why: "disk full".into() }), vec![Effect::CloseSession]);
    assert_eq!(
        r.step(),
        sign_in(
            Phase::Idle,
            Some("Your budget couldn't be opened here (disk full). Sign in to try again.")
        )
    );
    assert_eq!(r.remembered().login, Some(ana()));
}

/// The server's address: an empty or broken one said under the field and
/// never asked; one that reads is asked, once at a time; what was found is
/// remembered and signed in at, by itself even though this launch has.
#[test]
fn the_server_step() {
    let m = Onboarding::new(harken());
    m.next(Event::Typed { text: "  ".into() });
    assert_eq!(m.next(Event::Check), vec![]);
    assert_eq!(m.step(), server("  ", false, Some("Type the address of your server.")));
    m.next(Event::Typed { text: "h".into() });
    assert_eq!(m.step(), server("h", false, None));
    assert_eq!(m.next(Event::Check), vec![Effect::FindServer { typed: "h".into() }]);
    assert_eq!(m.said().status.as_deref(), Some("Checking…"));
    assert_eq!(m.next(Event::Check), vec![]);
    assert_eq!(
        m.next(Event::Checked {
            found: Found::Failed { why: "down".into() }
        }),
        vec![]
    );
    assert_eq!(m.step(), server("h", false, Some("down")));
    // An answer nobody is waiting for is nothing.
    assert_eq!(m.next(found(H)), vec![]);
    m.next(Event::Check);
    assert_eq!(m.next(found(H)), vec![remembered(Some(H), None, false), browse()]);
    // Another server, from signing in: the field holds this one.
    m.next(Event::CameBack { why: None });
    assert_eq!(m.next(Event::ChangeServer), vec![]);
    assert_eq!(m.step(), server("h", false, None));
    // A new server is a new sign-in: it goes by itself though this launch has.
    m.next(Event::Check);
    assert_eq!(m.next(found(H)), vec![remembered(Some(H), None, false), browse()]);
    // Not while a code is traded.
    m.next(code("c"));
    assert_eq!(m.next(Event::ChangeServer), vec![]);
}

/// A server that answers forgets the login of the last one.
#[test]
fn another_server_is_another_login() {
    let m = Onboarding::new(money());
    m.next(Event::Launched {
        remembered: Remembered {
            server: None,
            login: Some(ana()),
            demo: false,
        },
        code: None,
        returned: false,
    });
    m.next(Event::Typed {
        text: "money.koon.us".into(),
    });
    m.next(Event::Check);
    let fx = m.next(found("https://money.koon.us"));
    assert_eq!(fx[0], remembered(Some("https://money.koon.us"), None, false));
    assert_eq!(
        fx[1],
        Effect::Browse {
            server: "https://money.koon.us".into(),
            url: "https://money.koon.us/auth/login?redirect=money%3A%2F%2Fauth".into(),
        }
    );
}

/// The demo needs no server, is remembered, and leaving it — signed out of,
/// or another server asked for — is the first page again, the demo
/// forgotten. An app with no demo never goes there.
#[test]
fn the_demo_needs_no_server() {
    let m = Onboarding::new(money());
    m.next(launched(None, None, false));
    assert_eq!(m.next(Event::TryDemo), vec![remembered(None, None, true), Effect::OpenDemo]);
    assert_eq!(m.step(), Step::Demo);
    for e in [Event::Synced, Event::Foreground, code("c"), Event::Denied { reason: "x".into() }] {
        assert_eq!(m.next(e), vec![]);
    }
    assert_eq!(m.next(Event::SignOut), vec![Effect::CloseSession, remembered(None, None, false)]);
    assert_eq!(m.step(), Step::start());
    m.next(Event::TryDemo);
    assert_eq!(m.next(Event::ChangeServer), vec![Effect::CloseSession, remembered(None, None, false)]);
    // Not while a server is being asked.
    m.next(Event::Typed { text: "h".into() });
    m.next(Event::Check);
    assert_eq!(m.next(Event::TryDemo), vec![]);
    let h = Onboarding::new(harken());
    assert_eq!(h.next(Event::TryDemo), vec![]);
    assert_eq!(h.step(), Step::start());
}

/// A whole first run, its effects carried out as a shell would, and the
/// launch after it: straight in; and after signing out, signing in.
#[test]
fn a_whole_first_run_and_a_second_launch() {
    let m = Onboarding::new(harken());
    let mut kept = Remembered::default();
    let mut go = |m: &Onboarding, e: Event| {
        for fx in m.next(e) {
            if let Effect::Remember { remembered } = fx {
                kept = remembered;
            }
        }
        kept.clone()
    };
    go(&m, launched(None, None, false));
    go(&m, Event::Typed { text: "h".into() });
    go(&m, Event::Check);
    go(&m, found(H));
    go(&m, code("c"));
    go(&m, Event::SignedIn { login: ana() });
    let kept = go(&m, Event::Synced);
    assert_eq!(
        m.step(),
        Step::Running {
            server: H.into(),
            login: ana()
        }
    );
    let again = Onboarding::new(harken());
    again.next(Event::Launched {
        remembered: kept,
        code: None,
        returned: false,
    });
    assert_eq!(
        again.step(),
        Step::Running {
            server: H.into(),
            login: ana()
        }
    );
    again.next(Event::SignOut);
    let third = Onboarding::new(harken());
    assert_eq!(
        third.next(Event::Launched {
            remembered: again.remembered(),
            code: None,
            returned: false
        }),
        vec![browse()]
    );
}

/// A login is kept whole as `ark_auth::Login`'s JSON, picture and roles
/// too, and what no longer reads as one is none — forgotten, not trusted.
#[test]
fn a_login_is_kept_whole() {
    let mut l = ana();
    l.picture = "https://pictures.example.invalid/ana.png".into();
    let json = login_json(l.clone());
    assert!(json.contains("\"user\":{\"id\":\"ana\""), "{json}");
    assert_eq!(login_parse(json), Some(l.clone()));
    assert_eq!(login_parse("{not json".into()), None);
    // One written before pictures and roles still reads.
    let old = r#"{"token":"t","session":"s","user":{"id":"bo"},"expires_ms":1}"#;
    assert_eq!(login_parse(old.into()).map(|l| l.user), Some("bo".into()));
    assert_eq!(crate::Login::from(l.clone()).user.picture, l.picture);
    assert_eq!(login_name(l.clone()), "Ana");
    l.name.clear();
    assert_eq!(login_name(l), "ana");
}

#[test]
fn addresses() {
    let urls = |t: &str| address(t).unwrap();
    let why = |t: &str| address(t).unwrap_err();
    assert_eq!(urls("music.example.com"), ["https://music.example.com"]);
    assert_eq!(urls("  Music.Example.com/  "), ["https://music.example.com"]);
    assert_eq!(urls("music.example.com:8443"), ["https://music.example.com:8443"]);
    // A scheme is taken at its word, in the clear too.
    assert_eq!(urls("http://music.example.com"), ["http://music.example.com"]);
    assert_eq!(urls("https://music.example.com/harken/"), ["https://music.example.com/harken"]);
    // Private and loopback: https first, then http.
    assert_eq!(urls("192.168.1.20:8787"), ["https://192.168.1.20:8787", "http://192.168.1.20:8787"]);
    assert_eq!(urls("10.0.2.2:8787"), ["https://10.0.2.2:8787", "http://10.0.2.2:8787"]);
    assert_eq!(urls("localhost:8787"), ["https://localhost:8787", "http://localhost:8787"]);
    assert_eq!(urls("nas"), ["https://nas", "http://nas"]);
    assert_eq!(urls("music.local"), ["https://music.local", "http://music.local"]);
    assert_eq!(urls("[::1]:8787"), ["https://[::1]:8787", "http://[::1]:8787"]);
    assert_eq!(urls("100.101.102.103"), ["https://100.101.102.103", "http://100.101.102.103"]);
    // A public address is never tried in the clear.
    assert_eq!(urls("8.8.8.8"), ["https://8.8.8.8"]);
    assert_eq!(urls("172.32.0.1"), ["https://172.32.0.1"]);
    assert_eq!(why("   "), "Type the address of your server.");
    assert_eq!(why("music example"), "An address has no spaces in it.");
    assert_eq!(why("ftp://music.example.com"), "A server is reached over http or https, not ftp.");
    assert_eq!(why("music.example.com:99999"), "The port has to be a number from 1 to 65535.");
    assert_eq!(why("music.example.com:x"), "The port has to be a number from 1 to 65535.");
    assert!(why("user@music.example.com").starts_with("That isn't an address"));
    assert!(why("mu_sic.example.com").starts_with("That isn't an address"));
    assert!(why("https://").starts_with("That isn't an address"));
    assert_eq!(display("https://music.example.com:8443"), "music.example.com:8443");
}

fn joined(server: &str, code: Option<&str>) -> Event {
    Event::Joined {
        server: server.into(),
        code: code.map(Into::into),
    }
}

fn confirm(server: &str, code: Option<&str>) -> Step {
    Step::Confirm {
        server: server.into(),
        code: code.map(Into::into),
        error: None,
    }
}

/// A join link with a code, from the first page: asked about, then the
/// code redeemed and the session opened — never the browser.
#[test]
fn a_join_link_with_a_code_asks_then_signs_in() {
    let m = Onboarding::new(money());
    m.next(launched(None, None, false));
    assert_eq!(m.next(joined("https://money.koon.us/", Some("c0de"))), vec![]);
    assert_eq!(m.step(), confirm("https://money.koon.us", Some("c0de")));
    let said = m.said();
    assert_eq!(said.question.as_deref(), Some("Join money.koon.us?"));
    assert!(said.status.unwrap().contains("Only join a server you know."));
    let fx = m.next(Event::Accept);
    assert_eq!(
        fx,
        vec![
            remembered(Some("https://money.koon.us"), None, false),
            Effect::Redeem {
                server: "https://money.koon.us".into(),
                code: "c0de".into()
            }
        ]
    );
    let fx = m.next(Event::SignedIn { login: ana() });
    assert_eq!(
        fx[1],
        Effect::OpenSession {
            server: "https://money.koon.us".into(),
            login: ana()
        }
    );
}

/// A code the server will not take — expired, used already — is said in
/// the server's own sentence, and nothing goes by itself after it.
/// Falsified by wrapping it as a browser sign-in's failure.
#[test]
fn an_expired_or_reused_code_is_said_as_the_server_says_it() {
    for why in [
        "This code has expired. Make a new one on a device that's signed in.",
        "This code has been used already. Make a new one on a device that's signed in.",
    ] {
        let m = Onboarding::new(harken());
        m.next(launched(None, None, false));
        m.next(joined(H, Some("c")));
        m.next(Event::Accept);
        assert_eq!(m.next(Event::SignInFailed { why: why.into() }), vec![]);
        assert_eq!(m.step(), sign_in(Phase::Idle, Some(why)));
        // A browser's sign-in after it is worded as one again.
        m.next(Event::OpenSignIn);
        m.next(code("c"));
        m.next(Event::SignInFailed { why: "no".into() });
        assert_eq!(m.step(), sign_in(Phase::Idle, Some("Signing in didn't finish: no.")));
    }
}

/// A link with a server and no code sets the server, then signs in as
/// usual, by itself.
#[test]
fn a_join_link_without_a_code_sets_the_server() {
    let m = Onboarding::new(harken());
    m.next(launched(None, None, false));
    m.next(joined(H, None));
    assert!(m.said().status.unwrap().contains("ask you to sign in"));
    assert_eq!(m.next(Event::Accept), vec![remembered(Some(H), None, false), browse()]);
    // An empty code is no code.
    let e = Onboarding::new(harken());
    e.next(joined(H, Some(" ")));
    assert_eq!(e.step(), confirm(H, None));
}

/// A link that names no server, or one in the clear, is said and can only be
/// declined: https only, but for this machine itself. Falsified by taking
/// `http://` at its word.
#[test]
fn a_bad_link_is_said_and_never_used() {
    for (server, why) in [
        ("http://money.koon.us", "This link can't be used: money.koon.us isn't reached over https."),
        (
            "http://192.168.1.2:8788",
            "This link can't be used: 192.168.1.2:8788 isn't reached over https.",
        ),
        ("money.koon.us", "This link can't be used: it doesn't name a server."),
        ("ftp://h", "This link can't be used: a server is reached over http or https, not ftp."),
    ] {
        let m = Onboarding::new(money());
        m.next(launched(None, None, false));
        m.next(joined(server, Some("c")));
        let Step::Confirm { error: Some(e), .. } = m.step() else {
            panic!("{:?}", m.step())
        };
        assert_eq!(e, why, "{server}");
        assert_eq!(m.said().question, None);
        assert_eq!(m.next(Event::Accept), vec![], "{server}");
        assert_eq!(m.next(Event::Decline), vec![]);
        assert_eq!(m.step(), Step::start());
    }
    for local in ["http://127.0.0.1:8788", "http://localhost:8787", "http://[::1]:8787"] {
        assert_eq!(join_server(local).as_deref(), Ok(local), "{local}");
    }
}

/// While running, a link is an offer to switch, never a switch: declined,
/// the app is as it was, its session open; accepted, the session closes and
/// the new server is signed in to.
#[test]
fn a_join_link_while_running_asks_to_switch() {
    let m = Onboarding::new(harken());
    m.next(launched(Some(H), Some(ana()), false));
    let running = m.step();
    assert_eq!(m.next(joined("https://other.example", Some("c"))), vec![]);
    assert_eq!(m.next(Event::Decline), vec![]);
    assert_eq!(m.step(), running);
    m.next(joined("https://other.example", Some("c")));
    // Anything but an answer leaves the question up.
    assert_eq!(m.next(Event::Denied { reason: "x".into() }), vec![]);
    assert_eq!(m.next(Event::SignOut), vec![]);
    assert_eq!(
        m.next(Event::Accept),
        vec![
            Effect::CloseSession,
            remembered(Some("https://other.example"), None, false),
            Effect::Redeem {
                server: "https://other.example".into(),
                code: "c".into()
            }
        ]
    );
    // Not while a code is being traded: that one finishes first.
    assert_eq!(m.next(joined(H, None)), vec![]);
}

/// From the demo: accepted, the demo is left and forgotten.
#[test]
fn a_join_link_leaves_the_demo() {
    let m = Onboarding::new(money());
    m.next(launched(None, None, true));
    m.next(joined(H, None));
    assert_eq!(
        m.next(Event::Accept),
        vec![
            Effect::CloseSession,
            remembered(Some(H), None, false),
            Effect::Browse {
                server: H.into(),
                url: "https://h/auth/login?redirect=money%3A%2F%2Fauth".into()
            }
        ]
    );
    let d = Onboarding::new(money());
    d.next(launched(None, None, true));
    d.next(joined(H, None));
    d.next(Event::Decline);
    assert_eq!(d.step(), Step::Demo);
}

/// The link is written and read the same everywhere: an app's scheme, or a
/// page's own `/join`; anything else is none.
#[test]
fn join_links() {
    let l = join_link("money".into(), "https://money.koon.us/".into(), Some("a b".into()));
    assert_eq!(l, "money://join?server=https%3A%2F%2Fmoney.koon.us&code=a%20b");
    let j = |server: &str, code: Option<&str>| {
        Some(Join {
            server: server.into(),
            code: code.map(Into::into),
        })
    };
    assert_eq!(join_of(l), j("https://money.koon.us", Some("a b")));
    assert_eq!(join_of(join_link("harken".into(), H.into(), None)), j(H, None));
    assert_eq!(
        join_of("https://money.koon.us/join?server=https%3A%2F%2Fh&code=c".into()),
        j(H, Some("c"))
    );
    assert_eq!(join_of("money://auth?code=c".into()), None);
    assert_eq!(join_of("money://join?code=c".into()), None);
    assert_eq!(join_of("hello".into()), None);
}

/// A page that went to sign in and came back with no code — the provider
/// refused, the back button — says so and does not go again: a reload of it
/// would come back the same way, for ever. Falsified by `go` on `returned`.
#[test]
fn a_page_back_without_a_code_stays() {
    let m = Onboarding::new(harken());
    let e = Event::Launched {
        remembered: Remembered {
            server: Some(H.into()),
            ..Remembered::default()
        },
        code: None,
        returned: true,
    };
    assert_eq!(m.next(e), vec![]);
    assert_eq!(m.step(), sign_in(Phase::Idle, Some(words::EMPTY)));
}

/// A join link is a QR code a side square, dark and light, with its three
/// finders where a scanner looks for them.
#[test]
fn a_join_link_is_a_qr_code() {
    let q = qr_code(join_link(
        "money".into(),
        "https://money.koon.us".into(),
        Some("0123456789abcdef0123456789abcdef".into()),
    ))
    .unwrap();
    assert_eq!(q.dark.len(), (q.size * q.size) as usize);
    assert!(q.size >= 21 && (q.size - 17).is_multiple_of(4), "{}", q.size);
    let at = |x: u32, y: u32| q.dark[(y * q.size + x) as usize];
    // Each finder's corner, and the light ring inside it.
    for (x, y) in [(0, 0), (q.size - 1, 0), (0, q.size - 1)] {
        assert!(at(x, y));
    }
    assert!(!at(1, 1) && at(2, 2));
}
