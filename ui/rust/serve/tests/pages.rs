//! arkui-serve with script off, over money's domain as it is: a fixture
//! app's page of the budget's groups and the person's bank asks — its part
//! of its host, as every app's is, in arkui-web's `WebHost` — written by a
//! server that is ark-peer's and ark-auth's as money-server's is, beside a
//! socket client of the same hub. Forms are sent as a browser without
//! script sends them: the page's own fields, its CSRF cookie and token.

use std::sync::Arc;
use std::time::Duration;

use ark::format::Encode;
use ark_app::host::{Cx, HostedApp, Link, Place, Running, Session};
use ark_app::view::{card_section, form_button, list_row, name_field, View};
use ark_app::window::Windowed;
use ark_auth::server::{Auth, Mode as SignIn};
use ark_auth::Account;
use ark_peer::{Memory, Peer};
use arkui_serve::Look;
use arkui_web::page;
use money_core::{AddGroup, AskLine, GroupLine, Import, MoneyView};

/// The fixture, an app's part of its host as every app's is: the groups,
/// a name typed, Add Group, the bank's Import (the `bank` role's alone),
/// Connect a Bank and the asks waiting, each a row.
struct Groups {
    name: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Name(String),
    Add,
    Import,
    Connect,
}

/// What the fixture authors, as the domain's mutators take it.
#[derive(Clone, Debug)]
enum Act {
    AddGroup(String),
    Import,
    Connect,
}

impl HostedApp for Groups {
    type Key = MoneyView;
    type Change = Act;
    type Route = ();
    type Request = ();

    fn encode(change: &Act) -> (&'static str, Vec<u8>) {
        match change {
            Act::AddGroup(name) => ("categories.add_group", AddGroup { name: name.clone() }.to_bytes()),
            Act::Import => (
                "plaid.import",
                Import {
                    bank: "chase".into(),
                    ..Import::default()
                }
                .to_bytes(),
            ),
            Act::Connect => ("banks.connect", ().to_bytes()),
        }
    }

    fn open(_session: &Session, _cx: &mut Cx<'_, Groups>) -> Groups {
        Groups { name: String::new() }
    }

    fn screens(&mut self) -> Vec<&mut dyn Running<Groups>> {
        vec![]
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Groups>> {
        vec![]
    }

    fn holds(&self) -> Vec<MoneyView> {
        vec![MoneyView::Groups, MoneyView::Asks]
    }

    fn route(&mut self, _route: (), _cx: &mut Cx<'_, Groups>) {}

    fn request(&mut self, _request: (), _cx: &mut Cx<'_, Groups>) {}

    fn link(&mut self, link: Link, _cx: &mut Cx<'_, Groups>) {
        if let Link::Address(url) = link {
            let a = arkui_web::Address::of_url(&url, "/");
            self.name = a.get("name").unwrap_or_default().to_string();
        }
    }

    fn launched(&mut self, _args: Vec<String>, _cx: &mut Cx<'_, Groups>) {}
}

type Seen<'a> = &'a dyn Place<MoneyView, Act>;

fn rows<T: Clone + 'static>(place: Seen<'_>, key: &MoneyView) -> Vec<T> {
    let mut out = vec![];
    place.read(&mut |v| out = v.rows::<T>(key).to_vec());
    out
}

ark_app::app! {
    type _App = Groups {}
}

impl Windowed for Groups {
    type Msg = Msg;

    fn take(&mut self, msg: Msg, cx: &mut Cx<'_, Groups>) {
        match msg {
            Msg::Name(n) => self.name = n,
            // The name is kept when its group is refused, to be put right.
            Msg::Add => {
                if cx.place.send(Act::AddGroup(self.name.clone())).is_ok() {
                    self.name.clear();
                }
            }
            Msg::Import => {
                let _ = cx.place.send(Act::Import);
            }
            Msg::Connect => {
                let _ = cx.place.send(Act::Connect);
            }
        }
    }

    fn window(&self, place: Seen<'_>, _now_ms: i64) -> View<Msg> {
        let groups: Vec<View<Msg>> = rows::<GroupLine>(place, &MoneyView::Groups)
            .into_iter()
            .map(|g| list_row(format!("group: {}", g.name)).into())
            .collect();
        let asks: Vec<View<Msg>> = rows::<AskLine>(place, &MoneyView::Asks)
            .into_iter()
            .map(|a| list_row(format!("ask: {} {:?}", a.what, a.state)).into())
            .collect();
        View::List(vec![
            card_section(groups).into(),
            name_field("Name", self.name.clone(), None::<String>, Msg::Name),
            form_button("Add Group", true, Msg::Add),
            form_button("Import", true, Msg::Import),
            form_button("Connect a Bank", true, Msg::Connect),
            card_section(asks).into(),
        ])
    }

    fn hosted(&self, _name: &str, _place: Seen<'_>, _now_ms: i64) -> View<Msg> {
        View::Empty
    }

    fn address(&self, _place: Seen<'_>, _now_ms: i64) -> String {
        arkui_web::Address::default().with("name", self.name.clone()).url()
    }
}

/// The fixture as its generated entry point would say it.
struct Web;

impl arkui_web::WebHosted for Web {
    type App = Groups;
    const ID: &'static str = "groups";
    const NAME: &'static str = "Groups";
    const SCHEME: &'static str = "groups";
    const VERSION: &'static str = "0.1";
    const ACCENT: &'static str = "green";
    const MARK: &'static str = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>";

    fn domain() -> Option<Arc<ark::domain::Domain>> {
        Some(money_core::shared())
    }
}

struct Site {
    rt: tokio::runtime::Runtime,
    running: Option<ark_peer::Running>,
    auth: Arc<Auth>,
    url: String,
    _data: tempfile::TempDir,
}

fn site() -> Site {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let data = tempfile::tempdir().unwrap();
    let auth = Auth::open::<money_core::Money>(SignIn::Dev, "http://127.0.0.1", data.path())
        .unwrap()
        .with_roles([(money_core::BANK, vec!["bob"])]);
    let auth = Arc::new(auth);
    let config = || ark_peer::Config::of(money_core::shared());
    let look = Look {
        accent: "green".into(),
        ..Look::default()
    };
    let server = ark_peer::Server::new(config())
        .auth(auth.clone())
        .site(arkui_serve::site::<Web>(auth.clone(), Arc::new(config), look, None));
    let running = rt.block_on(server.listen("127.0.0.1:0")).unwrap();
    let url = format!("http://{}", running.addr);
    Site {
        rt,
        running: Some(running),
        auth,
        url,
        _data: data,
    }
}

/// A browser without script, signed in as `user`: its cookies, and the
/// page it is on.
struct Browser {
    cookies: Vec<(String, String)>,
    page: String,
    at: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).build()
}

impl Browser {
    fn of(site: &Site, user: &str) -> Browser {
        // Signed in as a browser is: a sign-in's code traded at /web/session.
        let account = Account {
            id: user.into(),
            ..Account::default()
        };
        let code = site.auth.finish(&account, "a browser").unwrap();
        let mut b = Browser {
            cookies: vec![],
            page: String::new(),
            at: "/".into(),
        };
        let r = b.send(agent().get(&format!("{}/web/session?code={code}&to=/", site.url)));
        assert_eq!(r.0, 303);
        b
    }

    fn cookie_header(&self) -> String {
        self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    fn send(&mut self, req: ureq::Request) -> (u16, String, Option<String>) {
        let req = req.set("Cookie", &self.cookie_header());
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        for c in resp.all("set-cookie") {
            let (k, v) = c.split(';').next().unwrap().split_once('=').unwrap();
            self.cookies.retain(|(n, _)| n != k);
            self.cookies.push((k.into(), v.into()));
        }
        let (status, location) = (resp.status(), resp.header("location").map(String::from));
        (status, resp.into_string().unwrap_or_default(), location)
    }

    fn open(&mut self, site: &Site, path: &str) -> u16 {
        let (s, body, _) = self.send(agent().get(&format!("{}{path}", site.url)));
        (self.page, self.at) = (body, path.into());
        s
    }

    fn hidden(&self, name: &str) -> String {
        let at = self
            .page
            .find(&format!("name=\"{name}\" value=\""))
            .unwrap_or_else(|| panic!("no {name}"));
        let rest = &self.page[at + name.len() + 15..];
        rest[..rest.find('"').unwrap()].replace("&amp;", "&")
    }

    /// The button labelled `label` pressed, `typed` in the fields named by
    /// their labels' order: the form as the page wrote it.
    fn press(&mut self, site: &Site, label: &str, typed: &[(&str, &str)]) -> (u16, Option<String>) {
        let form = self.form(label, typed);
        self.post(site, &form)
    }

    fn form(&self, label: &str, typed: &[(&str, &str)]) -> String {
        let at = self
            .page
            .find(&format!(">{label}</button>"))
            .unwrap_or_else(|| panic!("no {label} on {}", self.page));
        let tag = &self.page[self.page[..at].rfind("<button").unwrap()..at];
        let value = &tag[tag.find("value=\"").unwrap() + 7..];
        let press = &value[..value.find('"').unwrap()];
        let mut form = vec![
            ("ark".to_string(), press.to_string()),
            ("route".into(), self.hidden("route")),
            ("csrf".into(), self.hidden("csrf")),
            ("once".into(), self.hidden("once")),
        ];
        for (name, text) in typed {
            form.push((name.to_string(), text.to_string()));
        }
        let enc = |s: &str| {
            s.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        };
        form.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
    }

    fn post(&mut self, site: &Site, form: &str) -> (u16, Option<String>) {
        let req = agent()
            .post(&format!("{}{}", site.url, self.at))
            .set("Content-Type", "application/x-www-form-urlencoded");
        let (s, body, location) = self.send_body(req, form);
        if s == 422 {
            self.page = body;
        }
        (s, location)
    }

    fn send_body(&mut self, req: ureq::Request, form: &str) -> (u16, String, Option<String>) {
        let req = req.set("Cookie", &self.cookie_header());
        let resp = match req.send_string(form) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        let (status, location) = (resp.status(), resp.header("location").map(String::from));
        (status, resp.into_string().unwrap_or_default(), location)
    }

    /// The groups the page shows, in its first band (each band draws them).
    fn band(&self) -> &str {
        let p = &self.page;
        let from = p.find("ark-at-compact\"").unwrap_or(0);
        &p[from..p[from..].find("ark-at-medium\"").map_or(p.len(), |e| from + e)]
    }

    fn groups(&self) -> Vec<String> {
        self.band()
            .split("class=\"ark-row-title\">group: ")
            .skip(1)
            .map(|s| s[..s.find('<').unwrap()].to_string())
            .collect()
    }
}

/// A socket client of the same hub, as a phone is.
struct Phone {
    peer: Peer,
}

fn phone(site: &Site, user: &str) -> Phone {
    let account = Account {
        id: user.into(),
        ..Account::default()
    };
    let login = site.auth.issue(&account, "a phone").unwrap();
    let mut peer = Peer::open(
        ark_peer::Config::of(money_core::shared()),
        ark_peer::who(&login),
        Box::new(Memory::default()),
        false,
    );
    peer.set_token(Some(login.token.clone()));
    peer.connect(&ark_auth::socket_url(&site.url));
    Phone { peer }
}

impl Phone {
    fn until(&mut self, mut f: impl FnMut(&Peer) -> bool) -> bool {
        ark_peer::link::until(Duration::from_secs(10), Duration::from_millis(5), || {
            let _ = self.peer.pump();
            f(&self.peer)
        })
    }

    fn groups(&self) -> Vec<String> {
        let b = self.peer.ask("budget.groups", &().to_bytes()).unwrap();
        Vec::<GroupLine>::from_bytes(&b).unwrap().into_iter().map(|g| g.name).collect()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            self.rt.block_on(r.stop());
        }
    }
}

/// A group added with script off is the hub's, and a phone on the socket
/// sees it; sent again (the back button) it is authored once; a form with
/// no CSRF token, or another session's, is refused and runs nothing.
#[test]
fn a_form_is_the_apps_update_through_the_domain_once() {
    let s = site();
    let mut alice = Browser::of(&s, "alice");
    assert_eq!(alice.open(&s, "/"), 200);
    assert!(alice.page.contains("data-ark-mode=\"ssr\""));
    let before = alice.groups().len();
    let form = alice.form("Add Group", &[("t0", "Gifts")]);
    let (status, to) = alice.post(&s, &form);
    assert_eq!((status, to.as_deref()), (303, Some("/")));
    alice.open(&s, "/");
    assert_eq!(alice.groups().len(), before + 1);
    assert!(alice.groups().contains(&"Gifts".to_string()));

    let mut p = phone(&s, "sam");
    assert!(p.until(|_| true) && p.until(|peer| peer.linked));
    assert!(
        p.until(|peer| Vec::<GroupLine>::from_bytes(&peer.ask("budget.groups", &().to_bytes()).unwrap())
            .unwrap()
            .iter()
            .any(|g| g.name == "Gifts"))
    );

    // The same form again: nothing authored, sent where it went.
    let (status, to) = alice.post(&s, &form);
    assert_eq!((status, to.as_deref()), (303, Some("/")));
    alice.open(&s, "/");
    assert_eq!(alice.groups().iter().filter(|g| *g == "Gifts").count(), 1, "authored once");

    // No token, or a token that is not this browser's cookie's.
    let fresh = alice.form("Add Group", &[("t0", "Forged")]);
    let csrf = alice.hidden("csrf");
    for forged in [
        fresh.replace(&format!("csrf={}", csrf), "csrf="),
        fresh.replace(&format!("csrf={}", csrf), "csrf=elsewhere"),
    ] {
        assert_eq!(alice.post(&s, &forged).0, 403);
    }
    alice.open(&s, "/");
    assert!(!alice.groups().contains(&"Forged".to_string()));
    assert!(!p.groups().contains(&"Forged".to_string()));

    // A press the page could not make: refused, nothing runs.
    let stale = alice.form("Add Group", &[("t0", "Stale")]).replacen("ark=", "ark=9", 1);
    assert_eq!(alice.post(&s, &stale).0, 409);
    // Nobody signed in: sent to sign in, nothing drawn.
    let (status, body, _) = Browser {
        cookies: vec![],
        page: String::new(),
        at: "/".into(),
    }
    .send(agent().get(&format!("{}/", s.url)));
    assert_eq!(status, 401);
    assert!(body.contains("Sign In") && body.contains("/auth/login"));
}

/// Refused as over the socket, in the domain's words: a group's name too
/// long (its input's refinement), and the bank's Import by somebody
/// without the `bank` role. The page is drawn again with 422, the name as
/// it was typed.
#[test]
fn a_refusal_is_the_sockets_with_the_input_kept() {
    let s = site();
    let mut alice = Browser::of(&s, "alice");
    alice.open(&s, "/");
    let long = "g".repeat(81);
    let (status, _) = alice.press(&s, "Add Group", &[("t0", &long)]);
    assert_eq!(status, 422);
    let mut p = phone(&s, "alice");
    assert!(p.until(|peer| peer.linked));
    let on_socket = p.peer.mutate("categories.add_group", &AddGroup { name: long.clone() }).unwrap_err();
    assert!(
        alice.page.contains(&format!("role=\"alert\">{}</p>", page::escape(&on_socket))),
        "{on_socket}"
    );
    assert!(alice.page.contains(&format!("value=\"{long}\"")), "the name kept");

    alice.open(&s, "/");
    let (status, _) = alice.press(&s, "Import", &[]);
    assert_eq!(status, 422);
    let on_socket = p
        .peer
        .mutate(
            "plaid.import",
            &Import {
                bank: "chase".into(),
                ..Import::default()
            },
        )
        .unwrap_err();
    assert!(alice.page.contains(&page::escape(&on_socket)), "{on_socket}: {}", alice.page);
}

/// What a page reads is the socket's: bank asks are each person's own,
/// so alice's Connect a Bank is on her page and not on sam's.
#[test]
fn a_page_reads_what_the_socket_would() {
    let s = site();
    let mut alice = Browser::of(&s, "alice");
    alice.open(&s, "/");
    assert_eq!(alice.press(&s, "Connect a Bank", &[]).0, 303);
    alice.open(&s, "/");
    assert!(alice.page.contains("class=\"ark-row-title\">ask: connect"), "{}", alice.page);
    let mut sam = Browser::of(&s, "sam");
    sam.open(&s, "/");
    assert!(!sam.page.contains("class=\"ark-row-title\">ask: "));
}
