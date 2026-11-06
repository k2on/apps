//! Signing in, as every app's shell walks it: one machine, the same on a
//! phone, a desktop and a page.
//!
//! The server's address, its sign-in, the app once both are behind it — or
//! the demo, which needs neither. Every transition is [`Onboarding::next`],
//! which returns the effects to run; the shell carries them out — asks the
//! server's `/healthz`, opens the browser, trades the code, opens the
//! session, writes down what is remembered — and sends back what came of
//! them as another [`Event`]. Nothing here touches the network, a browser,
//! a disk or a clock.
//!
//! **There is no need of a sign-in button.** A server with nobody signed in
//! goes to sign in by itself, at most once a launch: a sign-in that came
//! back refused, closed or empty, or a login turned away again straight
//! after it was made, is said once on the page and not tried again, so it
//! can never go round and round. What starts it again is what reloading
//! the page is on the web: the app coming back to the front after it was
//! away ([`Event::Foreground`]), another server, or a press of a button
//! where a shell has one ([`Event::OpenSignIn`]).
//!
//! What a person reads is written here too, once ([`Said`]): the errors and
//! what the step is waiting on, in the app's own words ([`AppInfo`]).

use std::sync::Mutex;

use crate::util::login_url;
use crate::Account;

/// A login as a shell holds it: the token, the session, and who it is, the
/// account flattened. The same login as [`crate::Login`], which converts
/// to and from it; this is the one a phone is handed through the bridge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Login {
    pub token: String,
    pub session: String,
    pub user: String,
    pub name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub expires_ms: i64,
    /// The provider's picture of them (`Account::picture`), or empty. Last
    /// and defaulted, so a login made before it still builds.
    #[cfg_attr(feature = "ffi", uniffi(default = ""))]
    pub picture: String,
}

impl From<crate::Login> for Login {
    fn from(l: crate::Login) -> Login {
        Login {
            token: l.token,
            session: l.session,
            user: l.user.id,
            name: l.user.name,
            email: l.user.email,
            roles: l.user.roles,
            expires_ms: l.expires_ms,
            picture: l.user.picture,
        }
    }
}

impl From<Login> for crate::Login {
    fn from(l: Login) -> crate::Login {
        crate::Login {
            token: l.token,
            session: l.session,
            user: Account {
                id: l.user,
                name: l.name,
                email: l.email,
                picture: l.picture,
                roles: l.roles,
            },
            expires_ms: l.expires_ms,
        }
    }
}

/// A login as [`crate::Login`]'s JSON: what a shell keeps between runs.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn login_json(login: Login) -> String {
    serde_json::to_string(&crate::Login::from(login)).unwrap_or_default()
}

/// A login kept as [`login_json`] wrote it, or none — including one that no
/// longer reads as a login, which is forgotten rather than trusted.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn login_parse(json: String) -> Option<Login> {
    serde_json::from_str::<crate::Login>(&json).ok().map(Login::from)
}

/// What to call whoever is signed in: their name, or their id when the
/// provider had none.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn login_name(login: Login) -> String {
    match login.name.is_empty() {
        true => login.user,
        false => login.name,
    }
}

/// Who the app is, as the machine words it and builds its sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct AppInfo {
    /// Its name as a person reads it: `Money`.
    pub name: String,
    /// The scheme its sign-in comes back on: `money`, for `money://auth`.
    pub scheme: String,
    /// Whether it has a demo to try with no server.
    pub demo: bool,
    /// What it keeps, as a sentence says it: `your budget`.
    pub what: String,
}

impl AppInfo {
    /// Where its sign-in sends the code back to: `<scheme>://auth`.
    pub fn redirect(&self) -> String {
        format!("{}://auth", self.scheme)
    }
}

/// What a device remembers between launches, so that it is asked once: the
/// server, the login there, and whether it is showing the demo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Remembered {
    pub server: Option<String>,
    pub login: Option<Login>,
    pub demo: bool,
}

/// Where asking a server's `/healthz` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Found {
    /// The base URL to sign in at and sync with; `newer` when it runs
    /// another schema of the domain.
    Server { url: String, newer: bool },
    /// Why not, as a sentence.
    Failed { why: String },
}

/// Where a sign-in is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Phase {
    /// Nothing under way: a sign-in already tried this launch, its error said.
    Idle,
    /// The server's sign-in is open in a browser.
    Browser,
    /// A code came back and is being exchanged.
    Exchanging,
}

/// Where the first run stands.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Step {
    /// The server's address: what was typed, whether it is being checked,
    /// and why it was not.
    Server { typed: String, checking: bool, error: Option<String> },
    /// Signing in at `server`.
    SignIn { server: String, phase: Phase, error: Option<String> },
    /// Signed in just now: the session is open and the server's snapshot on
    /// its way, until [`Event::Synced`].
    Opening { server: String, login: Login },
    /// The app, over a session at `server` as `login`.
    Running { server: String, login: Login },
    /// The demo, on this device alone.
    Demo,
    /// A server that came from outside — a scanned code, an opened link —
    /// asked about before it is used: [`Event::Accept`] or
    /// [`Event::Decline`]. With a `code`, accepting signs in with it. An
    /// `error` is a link that cannot be used, said, and only declined.
    Confirm {
        server: String,
        code: Option<String>,
        error: Option<String>,
    },
}

impl Step {
    /// The first page: an empty address.
    pub fn start() -> Step {
        Step::Server {
            typed: String::new(),
            checking: false,
            error: None,
        }
    }
}

/// What happened.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Event {
    /// The app started, with what it remembered, and the sign-in's code if
    /// it was opened with one (a page that came back from signing in);
    /// `returned` when this load is the way back from a sign-in it went to
    /// (a page's reload is a launch), which with no code came back empty.
    Launched {
        remembered: Remembered,
        code: Option<String>,
        returned: bool,
    },
    /// The address field, as typed.
    Typed { text: String },
    /// "Continue" on the server step.
    Check,
    /// What [`Effect::FindServer`] found.
    Checked { found: Found },
    /// "Try the Demo", from the server step.
    TryDemo,
    /// "Use Another Server", from signing in or the demo.
    ChangeServer,
    /// "Sign In" pressed, where a shell has the button.
    OpenSignIn,
    /// `<scheme>://auth?code=…` came back, from the browser or as a link.
    Code { code: String },
    /// The browser ended with no code: closed (`why` none), or refused or
    /// broken (`why` says how).
    CameBack { why: Option<String> },
    /// What [`Effect::Exchange`] got.
    SignedIn { login: Login },
    /// Why [`Effect::Exchange`] got nothing.
    SignInFailed { why: String },
    /// The server turned the session away.
    Denied { reason: String },
    /// The session could not be opened on this device.
    OpenFailed { why: String },
    /// The session holds what the server had — or cannot reach it, and
    /// nothing more is coming soon.
    Synced,
    /// "Sign Out", or "Leave the Demo".
    SignOut,
    /// A join link, scanned or opened (`<scheme>://join?server=…&code=…`,
    /// [`join_link`]): a server, and a one-time code that signs in there.
    /// Asked about first ([`Step::Confirm`]), wherever the machine stands.
    Joined { server: String, code: Option<String> },
    /// "Join" on [`Step::Confirm`].
    Accept,
    /// "Cancel" on [`Step::Confirm`]: back to where it was.
    Decline,
    /// The app came back to the front after it was away: a sign-in may be
    /// tried again — or, with the browser still said to be open, the person
    /// came back from it without a code.
    Foreground,
}

/// What a transition asks the shell for.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Effect {
    /// Ask `/healthz` of what was typed; answer with [`Event::Checked`].
    FindServer { typed: String },
    /// Write down what is remembered now, all of it.
    Remember { remembered: Remembered },
    /// Open `server`'s sign-in, `url`, in a browser; answer with
    /// [`Event::Code`] or [`Event::CameBack`]. `url` comes back on the app's
    /// scheme: a desktop or a page, whose code comes back elsewhere, builds
    /// its own from `server`.
    Browse { server: String, url: String },
    /// Trade the code at `/auth/exchange`; answer with [`Event::SignedIn`]
    /// or [`Event::SignInFailed`].
    Exchange { server: String, code: String },
    /// Trade a join link's one-time code at `/auth/redeem`; answer with
    /// [`Event::SignedIn`] or [`Event::SignInFailed`], whose `why` is the
    /// server's own sentence ("This code has expired. …").
    Redeem { server: String, code: String },
    /// Open the session at `server` as `login`; answer [`Event::Synced`]
    /// when it has what the server had, [`Event::Denied`] or
    /// [`Event::OpenFailed`].
    OpenSession { server: String, login: Login },
    /// Open the demo.
    OpenDemo,
    /// Close the session, written down; its storage stays.
    CloseSession,
}

/// What the page says now: what it is waiting on, and why the last thing
/// did not go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Said {
    /// The question [`Step::Confirm`] asks: "Join money.koon.us?"
    pub question: Option<String>,
    /// "Checking…", "Waiting for the sign-in…", "Signing in…", "Getting
    /// your budget from money.koon.us…"; none when nothing is under way.
    pub status: Option<String>,
    pub error: Option<String>,
}

/// The machine's sentences, each written once.
pub mod words {
    use super::AppInfo;

    /// A sign-in page that will not go again by itself.
    pub fn again(app: &AppInfo) -> String {
        format!(
            "Signing in didn't finish. Leave {} and come back to try again, or use another server.",
            app.name
        )
    }

    /// A page back from signing in with no code.
    pub const EMPTY: &str = "The sign-in came back without a login.";

    /// The browser closed with no code.
    pub const CLOSED: &str = "The sign-in was closed before it finished.";

    /// The sign-in, or the code's exchange, went wrong.
    pub fn failed(why: &str) -> String {
        format!("Signing in didn't finish: {}.", why.trim_end_matches('.'))
    }

    /// The server turned the session away.
    pub fn signed_out(reason: &str) -> String {
        format!("The server signed you out ({reason}). Sign in again — what's here is kept, and syncs once you do.")
    }

    /// The session would not open.
    pub fn broken(app: &AppInfo, why: &str) -> String {
        let what = capitalised(&app.what);
        format!("{what} couldn't be opened here ({why}). Sign in to try again.")
    }

    /// What joining a server from a link asks.
    pub fn join(server: &str) -> String {
        format!("Join {}?", super::display(server))
    }

    /// What it says under the question: who should be trusted.
    pub fn join_why(app: &AppInfo, code: bool) -> String {
        match code {
            true => format!(
                "You'll be signed in to {} there with the code you were given. Only join a server you know.",
                app.name
            ),
            false => format!("{} will use that server and ask you to sign in. Only join a server you know.", app.name),
        }
    }

    /// A join link that cannot be used.
    pub fn bad_link(why: &str) -> String {
        format!("This link can't be used: {why}.")
    }

    pub const CHECKING: &str = "Checking…";
    pub const WAITING: &str = "Waiting for the sign-in…";
    pub const SIGNING_IN: &str = "Signing in…";

    /// Signed in, and what the app keeps on its way.
    pub fn getting(app: &AppInfo, server: &str) -> String {
        format!("Getting {} from {}…", app.what, super::display(server))
    }

    fn capitalised(s: &str) -> String {
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_uppercase().chain(c).collect(),
            None => String::new(),
        }
    }
}

/// The link that sets a device's server and, with a one-time `code`, signs
/// it in there: `<scheme>://join?server=<url>&code=<code>`. What "Add a
/// Device" shows as a QR code, and what scanning or opening it sends
/// ([`Event::Joined`], after [`join_of`]).
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn join_link(scheme: String, server: String, code: Option<String>) -> String {
    let code = code.map_or(String::new(), |c| format!("&code={}", crate::percent_encode(&c)));
    format!("{scheme}://join?server={}{code}", crate::percent_encode(server.trim_end_matches('/')))
}

/// What a join link names, scanned or opened: its server and its code; none
/// for any other text. The scheme is any app's (a phone reads the link its
/// own scheme brings it), or a page's own `https://…/join?…`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Join {
    pub server: String,
    pub code: Option<String>,
}

/// The [`Join`] in `link`, if it is one.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn join_of(link: String) -> Option<Join> {
    let link = link.trim();
    let (_, rest) = link.split_once("://")?;
    let (path, query) = rest.split_once('?')?;
    let path = path.split('#').next().unwrap_or_default();
    // `money://join?…` names `join` as its host; a page's `https://h/join?…` as its path.
    if path != "join" && !path.ends_with("/join") {
        return None;
    }
    let query = query.split('#').next().unwrap_or_default();
    let server = crate::query_value(query, "server").filter(|s| !s.is_empty())?;
    let code = crate::query_value(query, "code").filter(|c| !c.is_empty());
    Some(Join { server, code })
}

/// A QR code: `size` modules a side, and whether each is dark, row by row
/// from the top left. Drawn by each platform's ArkUI (`QrCode`) with the
/// quiet zone around it that a scanner needs.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Qr {
    pub size: u32,
    pub dark: Vec<bool>,
}

/// `text` as a QR code ([`Qr`]): a join link, for "Add a Device". Medium
/// error correction, which a phone's camera reads off a screen; none for
/// text too long for any QR code.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn qr_code(text: String) -> Option<Qr> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M).ok()?;
    let size = code.width() as u32;
    let dark = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some(Qr { size, dark })
}

/// A server from a link, as it may be used: `https`, or `http` only to this
/// machine itself; its base URL, or why not.
pub fn join_server(server: &str) -> Result<String, String> {
    let text = server.trim();
    let Some((scheme, _)) = text.split_once("://") else {
        return Err("it doesn't name a server".into());
    };
    let urls = address(text).map_err(|e| {
        let e = e.trim_end_matches('.');
        let mut c = e.chars();
        c.next().map_or(String::new(), |f| f.to_lowercase().chain(c).collect())
    })?;
    let url = urls.into_iter().next().ok_or("it doesn't name a server")?;
    match scheme.to_ascii_lowercase().as_str() {
        "https" => Ok(url),
        _ if loopback(&url) => Ok(url),
        _ => Err(format!("{} isn't reached over https", display(&url))),
    }
}

// Whether a base URL is this machine itself.
fn loopback(url: &str) -> bool {
    let rest = display(url);
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    host == "localhost" || host.ends_with(".localhost") || host == "::1" || host.starts_with("127.")
}

/// A URL as a person writes it: the host and port, and any path.
pub fn display(url: &str) -> String {
    match url.split_once("://") {
        Some((_, rest)) => rest.to_string(),
        None => url.to_string(),
    }
}

/// What the address field came to: the base URLs to try, in order (no
/// trailing slash, `https` first), or why there are none.
///
/// One field takes `host`, `host:port` or a whole URL. A URL with a scheme
/// is taken at its word. Anything else is tried over `https` first, and
/// over `http` as well only where the address is one a certificate is
/// unlikely to cover and nobody outside can reach: loopback, the private
/// ranges, link-local, the carrier-grade range a tailnet uses, and a name
/// with no dot or one under `.local`, `.lan`, `.home.arpa` or `.internal`.
pub fn address(typed: &str) -> Result<Vec<String>, String> {
    let text = typed.trim();
    if text.is_empty() {
        return Err("Type the address of your server.".into());
    }
    if text.chars().any(char::is_whitespace) {
        return Err("An address has no spaces in it.".into());
    }
    let (scheme, rest) = match text.split_once("://") {
        Some((s, r)) => {
            if !["http", "https"].contains(&s.to_ascii_lowercase().as_str()) {
                return Err(format!("A server is reached over http or https, not {s}."));
            }
            (s.to_ascii_lowercase(), r)
        }
        None => (String::new(), text),
    };
    let authority: String = rest.chars().take_while(|c| !matches!(c, '/' | '?' | '#')).collect();
    let mut path: String = rest[authority.len()..].chars().take_while(|c| !matches!(c, '?' | '#')).collect();
    while path.ends_with('/') {
        path.pop();
    }
    let bad = || format!("That isn't an address a server can have: {text}");
    if authority.is_empty() || authority.contains('@') {
        return Err(bad());
    }
    let host = host_of(&authority).ok_or_else(bad)?;
    let skip = match authority.starts_with('[') {
        true => host.len() + 2,
        false => host.len(),
    };
    let port = &authority[skip..];
    if !port.is_empty() {
        let n = port.strip_prefix(':').and_then(|p| p.parse::<u32>().ok());
        if !matches!(n, Some(1..=65535)) {
            return Err("The port has to be a number from 1 to 65535.".into());
        }
    }
    let base = authority.to_lowercase() + &path;
    Ok(match (scheme.is_empty(), is_local(&host)) {
        (false, _) => vec![format!("{scheme}://{base}")],
        (true, true) => vec![format!("https://{base}"), format!("http://{base}")],
        (true, false) => vec![format!("https://{base}")],
    })
}

// The host of `host`, `host:port`, `[v6]` or `[v6]:port`, or none when it
// has characters no host has.
fn host_of(authority: &str) -> Option<String> {
    if let Some(inner) = authority.strip_prefix('[') {
        let v6 = &inner[..inner.find(']')?];
        let ok = !v6.is_empty() && v6.chars().all(|c| c.is_alphanumeric() || c == ':' || c == '.');
        return ok.then(|| v6.to_string());
    }
    let h: String = authority.chars().take_while(|&c| c != ':').collect();
    let ok = !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') && !h.starts_with('.') && !h.starts_with('-');
    ok.then_some(h)
}

/// Whether `host` is one nobody outside this network reaches, where a
/// server is as likely to speak plain http as https.
pub fn is_local(host: &str) -> bool {
    let h = host.to_lowercase();
    let h = h.trim_end_matches('.');
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    if !h.contains('.') && !h.contains(':') {
        return true;
    }
    if [".local", ".lan", ".home.arpa", ".internal"].iter().any(|s| h.ends_with(s)) {
        return true;
    }
    if h.contains(':') {
        return h == "::1" || h.starts_with("fc") || h.starts_with("fd") || h.starts_with("fe80:");
    }
    let parts: Vec<Option<u8>> = h.split('.').map(|p| p.parse().ok()).collect();
    let [Some(a), Some(b), Some(_), Some(_)] = parts[..] else { return false };
    a == 127
        || a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
}

#[derive(Clone, Debug)]
struct State {
    step: Step,
    /// Where [`Step::Confirm`] goes back to when declined.
    before: Option<Box<Step>>,
    /// The code being traded is a join link's: its failure is the server's
    /// own sentence.
    redeeming: bool,
    /// A sign-in went this launch: not by itself again until the app comes
    /// back to the front.
    tried: bool,
    remembered: Remembered,
}

/// The machine: where it stands, what is remembered, and whether this
/// launch has already gone to sign in.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
#[derive(Debug)]
pub struct Onboarding {
    app: AppInfo,
    state: Mutex<State>,
}

impl Clone for Onboarding {
    fn clone(&self) -> Onboarding {
        Onboarding {
            app: self.app.clone(),
            state: Mutex::new(self.lock().clone()),
        }
    }
}

impl Onboarding {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg_attr(feature = "ffi", uniffi::export)]
impl Onboarding {
    /// A machine for `app`, on the first page until [`Event::Launched`].
    #[cfg_attr(feature = "ffi", uniffi::constructor)]
    pub fn new(app: AppInfo) -> Onboarding {
        Onboarding {
            app,
            state: Mutex::new(State {
                step: Step::start(),
                before: None,
                redeeming: false,
                tried: false,
                remembered: Remembered::default(),
            }),
        }
    }

    /// Where it stands.
    pub fn step(&self) -> Step {
        self.lock().step.clone()
    }

    /// What it remembers, as it last asked for it to be written down.
    pub fn remembered(&self) -> Remembered {
        self.lock().remembered.clone()
    }

    /// What the page says now.
    pub fn said(&self) -> Said {
        let s = self.lock();
        let app = &self.app;
        match &s.step {
            Step::Server { checking, error, .. } => Said {
                question: None,
                status: checking.then(|| words::CHECKING.to_string()),
                error: error.clone(),
            },
            Step::SignIn { phase, error, .. } => Said {
                question: None,
                status: match phase {
                    Phase::Idle => None,
                    Phase::Browser => Some(words::WAITING.into()),
                    Phase::Exchanging => Some(words::SIGNING_IN.into()),
                },
                error: error.clone(),
            },
            Step::Opening { server, .. } => Said {
                question: None,
                status: Some(words::getting(app, server)),
                error: None,
            },
            Step::Confirm { server, code, error } => Said {
                question: error.is_none().then(|| words::join(server)),
                status: error.is_none().then(|| words::join_why(app, code.is_some())),
                error: error.clone(),
            },
            Step::Running { .. } | Step::Demo => Said::default(),
        }
    }

    /// One event; an event that means nothing where the machine stands
    /// leaves it as it was, and asks for nothing.
    pub fn next(&self, event: Event) -> Vec<Effect> {
        let mut s = self.lock();
        step(&self.app, &mut s, event)
    }
}

// To sign in, by itself, unless this launch already has: then the page
// says why it is waiting.
fn go(app: &AppInfo, s: &mut State, server: String, error: Option<String>) -> Vec<Effect> {
    if s.tried {
        let error = error.unwrap_or_else(|| words::again(app));
        s.step = Step::SignIn {
            server,
            phase: Phase::Idle,
            error: Some(error),
        };
        return vec![];
    }
    s.tried = true;
    browse(app, s, server, error)
}

fn browse(app: &AppInfo, s: &mut State, server: String, error: Option<String>) -> Vec<Effect> {
    let url = login_url(&server, &app.redirect(), None);
    s.step = Step::SignIn {
        server: server.clone(),
        phase: Phase::Browser,
        error,
    };
    vec![Effect::Browse { server, url }]
}

fn remember(s: &mut State, change: impl FnOnce(&mut Remembered)) -> Effect {
    change(&mut s.remembered);
    Effect::Remember {
        remembered: s.remembered.clone(),
    }
}

// Back to signing in from a session: the server kept, the session closed.
fn out(app: &AppInfo, s: &mut State, server: String, event: Event) -> Vec<Effect> {
    match event {
        // The login forgotten, and signing in again by itself — once a
        // launch: a login turned away straight after it was made is not
        // sent round again.
        Event::Denied { reason } => {
            let mut fx = vec![Effect::CloseSession, remember(s, |r| r.login = None)];
            fx.extend(go(app, s, server, Some(words::signed_out(&reason))));
            fx
        }
        // Signing out on purpose is not a sign-in to start again at once:
        // the page waits.
        Event::SignOut => {
            s.tried = true;
            s.step = Step::SignIn {
                server,
                phase: Phase::Idle,
                error: None,
            };
            vec![Effect::CloseSession, remember(s, |r| r.login = None)]
        }
        // A session that cannot be opened is no reason to lose the login:
        // signing in again opens it again, and so does the next launch.
        Event::OpenFailed { why } => {
            s.tried = true;
            s.step = Step::SignIn {
                server,
                phase: Phase::Idle,
                error: Some(words::broken(app, &why)),
            };
            vec![Effect::CloseSession]
        }
        _ => vec![],
    }
}

fn step(app: &AppInfo, s: &mut State, event: Event) -> Vec<Effect> {
    match event {
        // Where a launch starts, from what was remembered: the demo; a
        // server and a login is the app over the replica already here; a
        // server alone is signing in, gone to at once (or the code it came
        // back with traded); nothing is the server's address.
        Event::Launched { remembered, code, returned } => {
            s.remembered = remembered.clone();
            s.tried = false;
            s.before = None;
            s.redeeming = false;
            match (remembered.server, remembered.login) {
                _ if remembered.demo && app.demo => {
                    s.step = Step::Demo;
                    vec![Effect::OpenDemo]
                }
                (Some(server), Some(login)) => {
                    s.step = Step::Running {
                        server: server.clone(),
                        login: login.clone(),
                    };
                    vec![Effect::OpenSession { server, login }]
                }
                (Some(server), None) => match code {
                    Some(code) => {
                        s.tried = true;
                        s.step = Step::SignIn {
                            server: server.clone(),
                            phase: Phase::Exchanging,
                            error: None,
                        };
                        vec![Effect::Exchange { server, code }]
                    }
                    // Back with nothing: said, and not sent round again.
                    None if returned => {
                        s.tried = true;
                        s.step = Step::SignIn {
                            server,
                            phase: Phase::Idle,
                            error: Some(words::EMPTY.into()),
                        };
                        vec![]
                    }
                    None => go(app, s, server, None),
                },
                (None, _) => {
                    s.step = Step::start();
                    vec![]
                }
            }
        }
        Event::Joined { server, code } => {
            if matches!(
                s.step,
                Step::SignIn {
                    phase: Phase::Exchanging,
                    ..
                }
            ) {
                return vec![];
            }
            let (server, error) = match join_server(&server) {
                Ok(url) => (url, None),
                Err(why) => (server, Some(words::bad_link(&why))),
            };
            let code = code.filter(|c| !c.trim().is_empty());
            if !matches!(s.step, Step::Confirm { .. }) {
                s.before = Some(Box::new(s.step.clone()));
            }
            s.step = Step::Confirm { server, code, error };
            vec![]
        }
        Event::Foreground => match s.step.clone() {
            // Back from the browser with nothing: it was closed. Said, and
            // not opened again until the next time.
            Step::SignIn {
                server,
                phase: Phase::Browser,
                ..
            } => {
                s.step = Step::SignIn {
                    server,
                    phase: Phase::Idle,
                    error: Some(words::CLOSED.into()),
                };
                vec![]
            }
            Step::SignIn {
                server,
                phase: Phase::Idle,
                error,
            } => {
                s.tried = false;
                let error = error.filter(|e| *e != words::again(app));
                go(app, s, server, error)
            }
            _ => {
                s.tried = false;
                vec![]
            }
        },
        event => match s.step.clone() {
            Step::Server { typed, checking, .. } => match event {
                Event::Typed { text } => {
                    s.step = Step::Server {
                        typed: text,
                        checking: false,
                        error: None,
                    };
                    vec![]
                }
                Event::Check if !checking => match address(&typed) {
                    Err(why) => {
                        s.step = Step::Server {
                            typed,
                            checking: false,
                            error: Some(why),
                        };
                        vec![]
                    }
                    Ok(_) => {
                        s.step = Step::Server {
                            typed: typed.clone(),
                            checking: true,
                            error: None,
                        };
                        vec![Effect::FindServer { typed }]
                    }
                },
                Event::Checked { found } if checking => match found {
                    // Another server is another sign-in: it goes by itself.
                    Found::Server { url, .. } => {
                        s.tried = false;
                        let mut fx = vec![remember(s, |r| {
                            r.server = Some(url.clone());
                            r.login = None;
                        })];
                        fx.extend(go(app, s, url, None));
                        fx
                    }
                    Found::Failed { why } => {
                        s.step = Step::Server {
                            typed,
                            checking: false,
                            error: Some(why),
                        };
                        vec![]
                    }
                },
                Event::TryDemo if !checking && app.demo => {
                    s.step = Step::Demo;
                    vec![remember(s, |r| r.demo = true), Effect::OpenDemo]
                }
                _ => vec![],
            },
            Step::SignIn { server, phase, .. } => match event {
                Event::Code { code } if phase != Phase::Exchanging => {
                    s.tried = true;
                    s.redeeming = false;
                    s.step = Step::SignIn {
                        server: server.clone(),
                        phase: Phase::Exchanging,
                        error: None,
                    };
                    vec![Effect::Exchange { server, code }]
                }
                Event::CameBack { why } if phase == Phase::Browser => {
                    s.step = Step::SignIn {
                        server,
                        phase: Phase::Idle,
                        error: Some(why.map_or_else(|| words::CLOSED.into(), |w| words::failed(&w))),
                    };
                    vec![]
                }
                Event::OpenSignIn if phase != Phase::Exchanging => {
                    s.tried = true;
                    browse(app, s, server, None)
                }
                Event::SignedIn { login } if phase == Phase::Exchanging => {
                    s.redeeming = false;
                    s.step = Step::Opening {
                        server: server.clone(),
                        login: login.clone(),
                    };
                    vec![
                        remember(s, |r| {
                            r.server = Some(server.clone());
                            r.login = Some(login.clone());
                        }),
                        Effect::OpenSession { server, login },
                    ]
                }
                Event::SignInFailed { why } if phase == Phase::Exchanging => {
                    let error = match std::mem::take(&mut s.redeeming) {
                        true => why,
                        false => words::failed(&why),
                    };
                    s.step = Step::SignIn {
                        server,
                        phase: Phase::Idle,
                        error: Some(error),
                    };
                    vec![]
                }
                Event::ChangeServer if phase != Phase::Exchanging => {
                    s.step = Step::Server {
                        typed: display(&server),
                        checking: false,
                        error: None,
                    };
                    vec![]
                }
                _ => vec![],
            },
            Step::Opening { server, login } => match event {
                Event::Synced => {
                    s.step = Step::Running { server, login };
                    vec![]
                }
                event => out(app, s, server, event),
            },
            Step::Running { server, .. } => out(app, s, server, event),
            Step::Confirm { server, code, error } => match event {
                Event::Decline => {
                    s.step = *s.before.take().unwrap_or_else(|| Box::new(Step::start()));
                    vec![]
                }
                Event::Accept if error.is_none() => {
                    // Whatever was open is left: a session, or the demo.
                    let before = s.before.take().map(|b| *b);
                    let mut fx = match before {
                        Some(Step::Running { .. } | Step::Opening { .. } | Step::Demo) => vec![Effect::CloseSession],
                        _ => vec![],
                    };
                    fx.push(remember(s, |r| {
                        r.server = Some(server.clone());
                        r.login = None;
                        r.demo = false;
                    }));
                    match code {
                        Some(code) => {
                            s.tried = true;
                            s.redeeming = true;
                            s.step = Step::SignIn {
                                server: server.clone(),
                                phase: Phase::Exchanging,
                                error: None,
                            };
                            fx.push(Effect::Redeem { server, code });
                        }
                        // Another server is another sign-in: it goes by itself.
                        None => {
                            s.tried = false;
                            fx.extend(go(app, s, server, None));
                        }
                    }
                    fx
                }
                _ => vec![],
            },
            Step::Demo => match event {
                Event::ChangeServer | Event::SignOut => {
                    s.step = Step::start();
                    vec![Effect::CloseSession, remember(s, |r| r.demo = false)]
                }
                _ => vec![],
            },
        },
    }
}

#[cfg(test)]
mod tests;
