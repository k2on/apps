//! The admin page's host (`docs/plan-guards.md` D4): the explorer over a
//! server's authority, reached over HTTP ([`crate::wire`]) — compiled to
//! wasm and served by the server on its admin listener, or run natively
//! against one (`ark-admin http://127.0.0.1:8788`).
//!
//! What it is shown is one [`State`] per request: the module, the store,
//! the log's last lines, the connections and the `Verify` answers, decoded
//! into a store the explorer reads like any other. What it writes it asks
//! the server to write — a raw write, or a domain mutation through the
//! CRUD the module exposes — and the server authors it as itself. A
//! request is answered after the write, and the state is read again.
//!
//! Bound to loopback the server asks nothing; bound wider it asks for a
//! login holding `admin`, and the page signs in the way the browser peer
//! does — through the server's `/auth/login`, back here with a code it
//! trades for a token — and carries the token as a bearer.

use std::time::Duration;

use ark::eval::{Args, Ctx};
use ark::ir::Module;
use ark::log::Seq;
use ark::store::{Change, MemoryStore};
use ark::value::TableName;
use iced::widget::{column, container, text};
use iced::{Element, Length, Subscription, Task};

use crate::data::{Connection, CrudVerbs, Line, LogView, Source, Verified, Writer};
use crate::model::{Explorer, Msg, Outcome};
use crate::wire::{author_body, raw_body, State};

/// The state as the explorer reads it: the module decoded, the store laid
/// out under its schema, the rest as it came.
pub struct Loaded {
    pub module: Module,
    pub store: MemoryStore,
    pub state: State,
}

impl Loaded {
    /// A state as the server sent it, read.
    pub fn of(state: State) -> Result<Loaded, String> {
        let v = ark::canon::decode(&state.module).map_err(|e| format!("the module: {e}"))?;
        let module = ark::ir::module_from_value(&v).map_err(|e| format!("the module: {e}"))?;
        let store = MemoryStore::from_value(module.schema.clone(), &state.store);
        Ok(Loaded { module, store, state })
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
    fn verifies(&self) -> Vec<Verified> {
        self.state.verifies.clone()
    }
}

/// A write the page asks the server for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Raw(Change),
    Author(String, Args),
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
    exposed: Vec<(TableName, CrudVerbs)>,
    raw: bool,
    who: String,
    pub queued: Vec<Request>,
}

impl Writer for Outbox {
    fn exposed(&self) -> Vec<(TableName, CrudVerbs)> {
        self.exposed.clone()
    }
    fn author(&mut self, function: &str, args: Args) -> Result<(), String> {
        self.queued.push(Request::Author(function.into(), args));
        Ok(())
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
    /// The writer a state says this page is.
    pub fn of(state: &State) -> Outbox {
        Outbox {
            exposed: state.exposed.clone(),
            raw: state.raw,
            who: state.who.clone(),
            queued: vec![],
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
    Answered(Result<String, String>),
    Explorer(Msg),
    Refresh,
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
}

impl Page {
    pub fn new(base: String, token: Option<String>) -> Page {
        Page {
            base: base.trim_end_matches('/').to_string(),
            token,
            loaded: None,
            explorer: Explorer::new(),
            note: "reading the server\u{2026}".into(),
            busy: false,
            sent: vec![],
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
        self.sent.extend(queued.iter().cloned());
        Task::batch(queued.into_iter().map(|r| {
            let (path, body) = r.to();
            Task::perform(
                fetch("POST", format!("{}{path}", self.base), self.token.clone(), Some(body)),
                Message::Answered,
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
                match r.and_then(|t| State::from_json(&t)).and_then(Loaded::of) {
                    Ok(l) => {
                        self.loaded = Some(l);
                        if self.note.starts_with("reading") {
                            self.note.clear();
                        }
                    }
                    Err(why) => self.note = format!("the state did not read: {why}"),
                }
                Task::none()
            }
            Message::Answered(r) => {
                self.explorer.note = match r {
                    Ok(seq) => format!("written, at {seq}"),
                    Err(why) => format!("refused: {why}"),
                };
                self.get_state()
            }
            Message::Explorer(msg) => {
                let Some(l) = &self.loaded else { return Task::none() };
                let src = Source {
                    store: &l.store,
                    schema: &l.module.schema,
                    module: &l.module,
                    log: l,
                    ctx: Ctx::new(l.state.who.clone(), "admin"),
                };
                let mut w = Outbox::of(&l.state);
                // Esc from the top: there is nothing behind the page to go
                // back to, so the explorer stays.
                let _: Outcome = self.explorer.update(msg, &src, &mut w);
                let queued = std::mem::take(&mut w.queued);
                self.send(queued)
            }
            Message::Refresh => self.get_state(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.loaded {
            None => text(self.note.clone()).size(13).into(),
            Some(l) => {
                let src = Source {
                    store: &l.store,
                    schema: &l.module.schema,
                    module: &l.module,
                    log: l,
                    ctx: Ctx::new(l.state.who.clone(), "admin"),
                };
                let w = Outbox::of(&l.state);
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
        ])
    }
}

/// `hello`'s answer: whether the page is open, and where to sign in.
pub fn hello_of(text: &str) -> Result<(bool, Option<String>), String> {
    let v = ark::json::decode(text).map_err(|e| e.to_string())?;
    let open = matches!(v.as_struct().get("open"), Some(ark::value::Value::Bool(true)));
    let server = match v.as_struct().get("server") {
        Some(ark::value::Value::Text(s)) => Some(s.to_string()),
        _ => None,
    };
    Ok((open, server))
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
