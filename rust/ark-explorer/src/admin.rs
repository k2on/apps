//! The admin page's host (`docs/plan-guards.md` D4): the explorer over a
//! server's authority, reached over HTTP ([`crate::wire`]) — compiled to
//! wasm and served by the server on its admin listener, or run natively
//! against one (`ark-admin http://127.0.0.1:8788`).
//!
//! What it is shown is one [`State`] per request: the module and its
//! functions, the store, the log's last lines and the connections, read
//! into a store and a library the explorer reads like any other. What it writes it asks
//! the server to write — a raw write, or a domain mutation through the
//! CRUD the module exposes — and the server authors it as itself. A
//! request is answered after the write, and the state is read again.
//!
//! Bound to loopback the server asks nothing; bound wider it asks for a
//! login holding `admin`, and the page signs in the way the browser peer
//! does — through the server's `/auth/login`, back here with a code it
//! trades for a token — and carries the token as a bearer.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use ark::author::Node;
use ark::log::{EntryId, Seq};
use ark::module::{Lib, Module};
use ark::peer::Ctx;
use ark::store::{Change, Store};
use iced::widget::{column, container, text};
use iced::{Element, Length, Subscription, Task};

use crate::data::{Connection, CrudVerbs, Line, LogView, Source, Ticket, Verdict, Writer};
use crate::model::{Explorer, Msg, Outcome};
use crate::wire::{author_body, raw_body, Hello, State};

/// The state as the explorer reads it: the module decoded, its functions
/// checked into a library, the store laid out under its schema, the rest as
/// it came.
pub struct Loaded {
    pub module: Module,
    pub lib: Lib,
    pub store: Store,
    pub state: State,
}

impl Loaded {
    /// A state as the server sent it, read: every body checked, callees
    /// before their callers, as a peer loads what it is sent.
    pub fn of(state: State) -> Result<Loaded, String> {
        let module = Module::from_bytes(&state.module).map_err(|e| format!("the module: {e}"))?;
        let lib = Lib::default();
        let mut left: Vec<&Vec<u8>> = state.bodies.iter().collect();
        while !left.is_empty() {
            let before = left.len();
            left.retain(|b| lib.load(&module.schema, b).is_err());
            if left.len() == before {
                return Err(format!("{} function bodies do not check", left.len()));
            }
        }
        ark::module::verify(&module, &lib).map_err(|e| format!("the module: {e}"))?;
        let store = Store::from_rows(Arc::new(module.schema.clone()), &state.rows).map_err(|e| format!("the store: {e}"))?;
        Ok(Loaded { module, lib, store, state })
    }

    fn source(&self) -> Source<'_> {
        let ctx = Ctx {
            id: EntryId::default(),
            actor: self.state.who.clone(),
            session: "admin".into(),
            roles: vec![],
            now: 0,
        };
        Source {
            store: &self.store,
            module: &self.module,
            lib: &self.lib,
            log: self,
            ctx,
        }
    }
}

impl LogView for Loaded {
    fn head(&self) -> Seq {
        self.state.head
    }
    fn lines(&self) -> Vec<Line> {
        self.state.lines.clone()
    }
    fn connections(&self) -> Vec<Connection> {
        self.state.connections.clone()
    }
}

/// A write the page asks the server for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Raw(Change),
    Author(String, Vec<u8>),
}

impl Request {
    /// Where it goes and what it carries.
    pub fn to(&self) -> (&'static str, String) {
        match self {
            Request::Raw(c) => ("/admin/api/raw", raw_body(c)),
            Request::Author(f, a) => ("/admin/api/author", author_body(f, a)),
        }
    }
}

/// The page as a writer: what it is asked for is queued, to be sent once
/// the explorer is done with the message, and answered later.
pub struct Outbox {
    exposed: Vec<(String, CrudVerbs)>,
    raw: bool,
    who: String,
    pub queued: Vec<Request>,
    /// Where the next request goes among every one the page has sent: a
    /// ticket is that place.
    next: usize,
    /// The server's answers so far, by a request's place.
    answers: BTreeMap<usize, Result<String, String>>,
}

impl Writer for Outbox {
    fn exposed(&self) -> Vec<(String, CrudVerbs)> {
        self.exposed.clone()
    }
    fn author(&mut self, function: &str, input: Vec<u8>) -> Result<Ticket, String> {
        let ticket = Ticket((self.next + self.queued.len()).to_string());
        self.queued.push(Request::Author(function.into(), input));
        Ok(ticket)
    }
    fn verdict(&self, ticket: &Ticket) -> Verdict {
        let Ok(i) = ticket.0.parse::<usize>() else { return Verdict::Unknown };
        match self.answers.get(&i) {
            None => Verdict::Pending,
            Some(Ok(seq)) => Verdict::Logged(seq.trim().parse().ok()),
            Some(Err(why)) => Verdict::Refused(why.clone()),
        }
    }
    fn raw(&mut self, change: Change) -> Result<(), String> {
        if !self.raw {
            return Err("this page may not write raw".into());
        }
        self.queued.push(Request::Raw(change));
        Ok(())
    }
    fn can_raw(&self) -> bool {
        self.raw
    }
    fn who(&self) -> String {
        self.who.clone()
    }
}

impl Outbox {
    /// The writer a state says this page is, `sent` requests in, with the
    /// server's answers to them.
    pub fn of(state: &State, sent: usize, answers: &BTreeMap<usize, Result<String, String>>) -> Outbox {
        let (exposed, raw, who, answers) = (state.exposed.clone(), state.raw, state.who.clone(), answers.clone());
        Outbox {
            exposed,
            raw,
            who,
            queued: vec![],
            next: sent,
            answers,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Where the page is, and whether it must sign in — or why not.
    Hello(Result<(bool, Option<String>), String>),
    /// A login came back from the code in the address.
    Token(Result<String, String>),
    Loaded(Result<String, String>),
    /// The server's answer to the request at this place among those sent:
    /// the sequence it was logged at, or the refusal.
    Answered(usize, Result<String, String>),
    Explorer(Msg),
    Refresh,
    /// The clock, while something lit is fading.
    Tick,
}

/// The page.
pub struct Page {
    /// The admin listener's origin: where the requests go.
    pub base: String,
    pub token: Option<String>,
    pub loaded: Option<Loaded>,
    pub explorer: Explorer,
    pub note: String,
    /// A request is out: no second refresh is started under it.
    busy: bool,
    /// Every write this page has asked of the server, oldest first.
    pub sent: Vec<Request>,
    /// The server's answer to each, by its place in `sent`.
    pub answers: BTreeMap<usize, Result<String, String>>,
    /// Bumped with every state read: the explorer diffs what it shows when
    /// this moves, and lights what changed.
    pub generation: u64,
}

impl Page {
    pub fn new(base: String, token: Option<String>) -> Page {
        let (base, note) = (base.trim_end_matches('/').to_string(), "reading the server\u{2026}".into());
        Page {
            base,
            token,
            loaded: None,
            explorer: Explorer::new(),
            note,
            busy: false,
            sent: vec![],
            answers: BTreeMap::new(),
            generation: 0,
        }
    }

    /// Start: ask whether a login is needed.
    pub fn boot(base: String, token: Option<String>) -> (Page, Task<Message>) {
        let page = Page::new(base, token);
        let task = Task::perform(fetch("GET", format!("{}/admin/api/hello", page.base), None, None), |r| {
            Message::Hello(r.and_then(|t| hello_of(&t)))
        });
        (page, task)
    }

    fn get_state(&mut self) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        Task::perform(
            fetch("GET", format!("{}/admin/api/state", self.base), self.token.clone(), None),
            Message::Loaded,
        )
    }

    /// Send what the explorer queued, one request each, answered in turn.
    fn send(&mut self, queued: Vec<Request>) -> Task<Message> {
        let from = self.sent.len();
        self.sent.extend(queued.iter().cloned());
        Task::batch(queued.into_iter().enumerate().map(|(k, r)| {
            let (path, body) = r.to();
            Task::perform(
                fetch("POST", format!("{}{path}", self.base), self.token.clone(), Some(body)),
                move |answer| Message::Answered(from + k, answer),
            )
        }))
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Hello(Ok((true, _))) => self.get_state(),
            Message::Hello(Ok((false, server))) => {
                if self.token.is_some() {
                    return self.get_state();
                }
                sign_in(server)
            }
            Message::Hello(Err(why)) => {
                self.note = format!("the server did not answer: {why}");
                Task::none()
            }
            Message::Token(Ok(t)) => {
                remember(&t);
                self.token = Some(t);
                self.get_state()
            }
            Message::Token(Err(why)) => {
                self.note = format!("could not sign in: {why}");
                Task::none()
            }
            Message::Loaded(r) => {
                self.busy = false;
                match r.and_then(|t| State::from_body(&t)).and_then(Loaded::of) {
                    Ok(l) => {
                        self.loaded = Some(l);
                        self.generation += 1;
                        self.observe();
                        if self.note.starts_with("reading") {
                            self.note.clear();
                        }
                    }
                    Err(why) => self.note = format!("the state did not read: {why}"),
                }
                Task::none()
            }
            Message::Answered(i, r) => {
                self.explorer.note = match &r {
                    Ok(seq) => format!("written, at {seq}"),
                    Err(why) => format!("refused: {why}"),
                };
                self.answers.insert(i, r);
                self.get_state()
            }
            Message::Explorer(msg) => {
                let Some(l) = &self.loaded else { return Task::none() };
                let src = l.source();
                let mut w = Outbox::of(&l.state, self.sent.len(), &self.answers);
                // Esc from the top: there is nothing behind the page to go
                // back to, so the explorer stays.
                let _: Outcome = self.explorer.update(msg, &src, &mut w);
                let queued = std::mem::take(&mut w.queued);
                // A table or an answer newly on screen is remembered as drawn.
                self.observe();
                self.send(queued)
            }
            Message::Refresh => self.get_state(),
            Message::Tick => {
                self.observe();
                Task::none()
            }
        }
    }

    /// The explorer's clock and the state's generation: what changed since
    /// it last drew is lit, and fades.
    pub fn observe(&mut self) {
        let Some(l) = &self.loaded else { return };
        let src = l.source();
        self.explorer.observe(&src, self.generation, now_ms());
    }

    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.loaded {
            None => text(self.note.clone()).size(13).into(),
            Some(l) => {
                let src = l.source();
                let w = Outbox::of(&l.state, self.sent.len(), &self.answers);
                let page = self.explorer.view(&src, &w).map(Message::Explorer);
                if self.note.is_empty() {
                    page
                } else {
                    column![page, text(self.note.clone()).size(12).style(arkui::style::dim)].into()
                }
            }
        };
        container(body).width(Length::Fill).height(Length::Fill).style(arkui::style::page).into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            arkui::vim::presses().map(|(k, m)| Message::Explorer(Msg::Key(k, m))),
            // What the log and the connections are now, every few seconds.
            iced::time::every(Duration::from_secs(5)).map(|_| Message::Refresh),
            // A clock only while something lit is fading.
            match self.explorer.fading() {
                true => iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick),
                false => Subscription::none(),
            },
        ])
    }
}

/// `hello`'s answer: whether the page is open, and where to sign in.
pub fn hello_of(text: &str) -> Result<(bool, Option<String>), String> {
    let h: Hello = crate::wire::read(text)?;
    Ok((h.open, h.server))
}

/// One request: the body of a success, or the status and body of anything
/// else.
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch(method: &'static str, url: String, token: Option<String>, body: Option<String>) -> Result<String, String> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    // Blocking, on a thread of its own: there is no async runtime on the
    // desktop and one request is not a reason to start one.
    std::thread::spawn(move || {
        let req = ureq::request(method, &url);
        let req = match &token {
            Some(t) => req.set("Authorization", &format!("Bearer {t}")),
            None => req,
        };
        let res = match body {
            Some(b) => req.send_string(&b),
            None => req.call(),
        };
        let out = match res {
            Ok(r) => r.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(s, r)) => Err(format!("{s} {}", r.into_string().unwrap_or_default())),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(out);
    });
    rx.await.unwrap_or_else(|_| Err("the request's thread went away".into()))
}

/// One request, through the browser's `fetch`.
#[cfg(target_arch = "wasm32")]
pub async fn fetch(method: &'static str, url: String, token: Option<String>, body: Option<String>) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let js = |e: wasm_bindgen::JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    if let Some(b) = &body {
        init.set_body(&wasm_bindgen::JsValue::from_str(b));
    }
    let headers = web_sys::Headers::new().map_err(js)?;
    if let Some(t) = &token {
        headers.set("Authorization", &format!("Bearer {t}")).map_err(js)?;
    }
    init.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js)?;
    let resp = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| format!("cannot reach {url}"))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(js)?;
    let text = JsFuture::from(resp.text().map_err(js)?)
        .await
        .map_err(js)?
        .as_string()
        .unwrap_or_default();
    if !resp.ok() {
        return Err(format!("{} {text}", resp.status()));
    }
    Ok(text)
}

const TOKEN: &str = "ark-admin-token";

/// Keep a login between loads: the browser's storage, and nowhere natively.
fn remember(token: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = ark_auth::web::storage() {
        let _ = s.set_item(TOKEN, token);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (token, TOKEN);
}

/// A login kept from an earlier load.
pub fn remembered() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        ark_auth::web::storage()?.get_item(TOKEN).ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("ARK_ADMIN_TOKEN").ok().filter(|t| !t.is_empty())
    }
}

/// Sign in, where the page must: in a browser, trade the code it came back
/// with, or go to the server's login and come back; natively, say how.
fn sign_in(server: Option<String>) -> Task<Message> {
    let Some(server) = server else {
        return Task::done(Message::Token(Err("the server did not say where to sign in".into())));
    };
    #[cfg(target_arch = "wasm32")]
    {
        match ark_auth::web::take_code() {
            Some(code) => Task::perform(
                async move { ark_auth::web::exchange(&server, &code).await.map(|l| l.token) },
                Message::Token,
            ),
            None => {
                ark_auth::web::go_sign_in(&server, None);
                Task::none()
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Task::done(Message::Token(Err(format!(
            "this admin page asks for a login holding admin: sign in at {server} and set ARK_ADMIN_TOKEN"
        ))))
    }
}

/// Milliseconds from some start, for what fades: the browser's clock, or a
/// monotonic one natively (`Instant` panics on wasm32-unknown-unknown).
fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }
}
