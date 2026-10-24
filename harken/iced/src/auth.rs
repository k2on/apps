//! Where the server is, where this device keeps its replica of it, and who is
//! signed in there.
//!
//! **Nobody has to be.** A window opens on its own replica whether or not
//! anybody has signed in, and works: the library it has is there, and what is
//! done is kept pending on this device, authored as nobody. Signing in later
//! moves that same replica on — `Replica::sign_in` makes every pending
//! intent the signer's, and dials — so everything done before is
//! theirs, synced. That is why the replica is one per *server* and not one
//! per person: it is the device's copy of that server's log, whoever is
//! using it.
//!
//! **Nobody has to have a server, either** (`docs/plan-alone.md` §4). Given
//! no server — no `--server`, no `?server=` — the window opens alone, in a
//! fixed place of its own (`local`), for real: the peer is its own
//! authority and keeps what it does as its local history. "Connect" is a
//! join, in place: that same replica is handed to the server named, the
//! local history pending on it, and the server remembered for this place
//! ([`joined`]) so the next start opens there again rather than alone. The
//! sign-in that follows is the one below, unchanged.
//!
//! Who you are is what the server says. Signing in is ark-auth's flow — open
//! one URL, get one code back — and differs between the targets only in where
//! the code comes back to: a loopback port the desktop listens on, or this
//! page's own address.
use ark::peer::Who;
use ark_auth::Login;
use ark_peer::Logins;

use crate::client::Replica;

/// What the connect entry offers before anything is typed: a dev server
/// on this machine.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8787";

/// The place a window with no server keeps its replica: one per device,
/// whatever it later joins.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub const LOCAL: &str = "local";

/// The logins this device remembers, one per server.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn logins() -> Logins {
    Logins::new("harken")
}

/// Where the server is, if anybody said, and a name to offer a dev server.
/// On the desktop, `--server` and `--user`; in a browser, the query string.
/// No server named is a window alone — or the one [`joined`] remembers.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn config() -> (Option<String>, Option<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
        (flag("--server").map(|s| s.trim_end_matches('/').to_string()), flag("--user"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let location = web_sys::window().map(|w| w.location());
        let query = location.as_ref().and_then(|l| l.search().ok()).unwrap_or_default();
        (
            ark_auth::query_value(&query, "server").map(|s| s.trim_end_matches('/').to_string()),
            ark_auth::query_value(&query, "user"),
        )
    }
}

/// Where a window starts (`docs/plan-alone.md` §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// A server named, or the one `local` joined.
    Server(String),
    /// A page with no server named: ask whoever served it whether it is a
    /// harken server — a household opens its server's address and is
    /// connected, as it always was — and start alone if it is not (GitHub
    /// Pages, a file).
    Ask(String),
    /// Nothing named and nothing to ask: a desktop given no `--server`.
    Alone,
}

/// Where a window starts, from what it was told (`--server`, `?server=`),
/// what `local` joined, and the page's origin where there is one: named,
/// then joined, then ask the origin, then alone.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn start(named: Option<String>, joined: Option<String>, origin: Option<String>) -> Start {
    match (named.or(joined), origin) {
        (Some(server), _) => Start::Server(server),
        (None, Some(origin)) => Start::Ask(origin),
        (None, None) => Start::Alone,
    }
}

/// [`start`] carried through: `answers` is whether a URL answers, asked of
/// `{origin}/healthz` only when there is nothing else to go on. The server
/// to open, or `None` for alone.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn decide(named: Option<String>, joined: Option<String>, origin: Option<String>, answers: impl FnOnce(&str) -> bool) -> Option<String> {
    match start(named, joined, origin) {
        Start::Server(server) => Some(server),
        Start::Ask(origin) => answers(&format!("{origin}/healthz")).then_some(origin),
        Start::Alone => None,
    }
}

/// The page's origin, where it has one a fetch can reach; `None` on the
/// desktop and for a page opened from a file.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn origin() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
    #[cfg(target_arch = "wasm32")]
    {
        let origin = web_sys::window()?.location().origin().ok()?;
        origin.starts_with("http").then(|| origin.trim_end_matches('/').to_string())
    }
}

/// Whether `url` answers a GET with a success within a second and a half.
/// One fetch, and a timeout, because a page from somewhere that is not a
/// server must not sit waiting to find out.
#[cfg(target_arch = "wasm32")]
pub fn answers(url: String) -> impl std::future::Future<Output = bool> {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(inline_js = r#"
export function probe(url, ms, done) {
  const stop = new AbortController();
  const timer = setTimeout(() => stop.abort(), ms);
  fetch(url, { signal: stop.signal, cache: "no-store" })
    .then((r) => { clearTimeout(timer); done(r.ok); })
    .catch(() => { clearTimeout(timer); done(false); });
}
"#)]
    extern "C" {
        fn probe(url: &str, ms: u32, done: &JsValue);
    }
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    let done = Closure::once_into_js(move |ok: bool| {
        let _ = tx.send(ok);
    });
    probe(&url, 1_500, &done);
    async move { rx.await.unwrap_or(false) }
}

/// The desktop never asks: it has no origin ([`origin`]).
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn answers(_: String) -> impl std::future::Future<Output = bool> {
    std::future::ready(false)
}

/// The server the `local` replica joined, if it has: the next start opens
/// there, over the same place, rather than alone — opening it alone again
/// would be leaving it.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn joined() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read_to_string(data_dir(LOCAL).join("joined"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
    #[cfg(target_arch = "wasm32")]
    {
        ark_auth::web::storage().and_then(|s| s.get_item("harken:local:joined").ok().flatten())
    }
}

/// Remember that the `local` replica now belongs to `server`.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn remember_joined(server: &str) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = data_dir(LOCAL);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tmp = dir.join(".joined.tmp");
        std::fs::write(&tmp, server).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dir.join("joined")).map_err(|e| format!("{}: {e}", dir.display()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        ark_auth::web::storage()
            .ok_or_else(|| "this page has no localStorage".to_string())?
            .set_item("harken:local:joined", server)
            .map_err(|e| format!("{e:?}"))
    }
}

/// This device's replica with no server: its own authority, authoring as
/// nobody until somebody signs in at a server it joins.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn open_alone() -> Result<Replica, String> {
    let mut r = crate::client::open(disk(LOCAL)?, None, crate::listening::device());
    r.alone();
    Ok(r)
}

/// What a window on a server, with nobody signed in, does as it opens.
///
/// **Nobody is asked.** There is no sign-in button: a window that has a
/// server and no login goes to sign in — the page to the server's
/// `/auth/login` and back, the desktop through its browser hand-off — and
/// the only question is whether this load is already the way back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arrival {
    /// Back from signing in with a code: trade it for the login.
    Exchange(String),
    /// Back from signing in without one — the provider refused, or the back
    /// button. Said, and not tried again until a reload: going again would
    /// come back the same way, for ever.
    CameBack,
    /// Neither: go.
    Go,
}

/// The code in the address, if any, and whether this load is one that went
/// to sign in and came back ([`came_back`]).
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn arrival(code: Option<String>, returned: bool) -> Arrival {
    match (code, returned) {
        (Some(code), _) => Arrival::Exchange(code),
        (None, true) => Arrival::CameBack,
        (None, false) => Arrival::Go,
    }
}

/// The mark a page leaves as it goes to sign in: in the tab's own
/// `sessionStorage`, so a reload in another tab is not mistaken for coming
/// back, and taken by the load that comes back.
#[cfg(target_arch = "wasm32")]
const LEAVING: &str = "harken:signing-in";

#[cfg(target_arch = "wasm32")]
fn session() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok()?
}

/// This page is going to sign in.
#[cfg(all(target_arch = "wasm32", not(feature = "demo")))]
pub fn leaving() {
    if let Some(s) = session() {
        let _ = s.set_item(LEAVING, "1");
    }
}

/// Whether this load is the way back from signing in — taken, so the reload
/// after it is not.
#[cfg(target_arch = "wasm32")]
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn came_back() -> bool {
    let Some(s) = session() else { return false };
    let back = s.get_item(LEAVING).ok().flatten().is_some();
    let _ = s.remove_item(LEAVING);
    back
}

/// Who a login is, as the server verifies it and an entry carries it.
pub fn who_of(l: &Login) -> Who {
    ark_peer::who(l)
}

/// This server's replica on this device: a directory natively, the page's
/// `localStorage` in a browser — the `local` one when that is what joined
/// this server — as the remembered login, or signed out. The token is never
/// on disk in the replica: a remembered login hands it over here; signed
/// out, the peer authors as whoever last used this replica (nobody, on a
/// device where nobody ever signed in) and dials nothing.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn open(server: &str, login: Option<&Login>) -> Result<Replica, String> {
    let place = match joined().as_deref() == Some(server) {
        true => LOCAL.to_string(),
        false => server.to_string(),
    };
    let login = login.map(|l| (who_of(l), l.token.clone()));
    Ok(crate::client::open(disk(&place)?, login, crate::listening::device()))
}

/// Where a place's replica is kept.
#[cfg_attr(feature = "demo", allow(dead_code))]
fn disk(place: &str) -> Result<Box<dyn ark_peer::Disk>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = data_dir(place);
        ark_peer::Files::new(&dir)
            .map(|f| Box::new(f) as Box<dyn ark_peer::Disk>)
            .map_err(|e| format!("{}: {e}", dir.display()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        Ok(Box::new(ark_peer::disk::Local(format!("harken:{place}"))) as Box<dyn ark_peer::Disk>)
    }
}

/// `$XDG_DATA_HOME/harken/<server>`, or `~/.local/share/…`, or the temp
/// directory on a machine with neither. One directory per server: the
/// replica is a copy of that server's log.
#[cfg(not(target_arch = "wasm32"))]
pub fn data_dir(server: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("harken").join(server_key(server))
}

/// A server's address as one path segment: its letters and digits, the rest
/// as `_`. `http://127.0.0.1:8787` is `http___127_0_0_1_8787`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn server_key(server: &str) -> String {
    server
        .trim_end_matches('/')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// The name a status line gives a login: the provider's display name, or
/// the id when it had none.
pub fn who(login: &Login) -> String {
    match login.user.name.is_empty() {
        true => login.user.id.clone(),
        false => login.user.name.clone(),
    }
}

/// This window is staying: the splash over it goes (`web/index.html` fades
/// `#boot.done`). Not before — a page still asking whether it has a server,
/// or about to go to sign in, shows the mark rather than a player it is
/// about to leave. Nothing on the desktop.
pub fn reveal() {
    #[cfg(target_arch = "wasm32")]
    {
        let boot = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("boot"));
        if let Some(boot) = boot {
            let _ = boot.class_list().add_1("done");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One server is one directory whatever it is spelt with, and two servers
    /// are two. Falsified by keeping the trailing slash: the first fails.
    /// Where a window starts: a server named wins without asking anybody,
    /// then the one `local` joined; a page with neither asks its own origin
    /// and opens there when `/healthz` answers — a harken server serving
    /// its page — and alone when it does not (GitHub Pages, a file); a
    /// desktop with neither, and no origin, is alone without asking.
    /// Falsified by `decide` opening the origin whatever it answered: the
    /// page from elsewhere opens a server that is not there.
    #[test]
    fn a_page_opens_on_the_server_that_served_it_and_alone_elsewhere() {
        let s = |x: &str| Some(x.to_string());
        let never = |_: &str| -> bool { panic!("nothing to ask") };
        assert_eq!(decide(s("http://a"), s("http://b"), s("http://c"), never), s("http://a"));
        assert_eq!(decide(None, s("http://b"), s("http://c"), never), s("http://b"));
        let mut asked = vec![];
        let served = decide(None, None, s("http://home.lan"), |u| {
            asked.push(u.to_string());
            true
        });
        assert_eq!(served, s("http://home.lan"));
        assert_eq!(asked, ["http://home.lan/healthz"]);
        assert_eq!(decide(None, None, s("https://pages.example"), |_| false), None);
        assert_eq!(decide(None, None, None, never), None);
        assert_eq!(start(None, None, None), Start::Alone);
    }

    #[test]
    fn a_server_is_one_directory() {
        assert_eq!(server_key("http://127.0.0.1:8787/"), server_key("http://127.0.0.1:8787"));
        assert_eq!(server_key("http://127.0.0.1:8787"), "http___127_0_0_1_8787");
        assert_ne!(server_key("https://a.example"), server_key("https://b.example"));
    }
}
