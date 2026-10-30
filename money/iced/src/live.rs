//! The window on a server: where it is, this device's replica of its
//! budget, and who is signed in there — what the build without `demo` is.
//!
//! **Where the server is:** on the desktop, `--server URL` (a dev server on
//! this machine, [`DEFAULT_SERVER`], when none is named); in a browser,
//! `?server=URL`, or else whoever served the page — money-server serves
//! this page at `/`, so a household opens their server's address and is
//! on it.
//!
//! **Signing in** is ark-auth's flow, as harken's window does it: open the
//! server's `/auth/login`, come back with a one-minute code, trade it at
//! `/auth/exchange` for a login. The desktop listens on a loopback port and
//! hands the page to the browser; the page goes there itself and comes
//! back to its own address. The server is the only OpenID Connect client,
//! so this needs nothing but its address. A login is remembered per server
//! ([`logins`]), so the next start is signed in already.
//!
//! **The replica** is one per server: a directory natively, in memory in a
//! browser (the page fills from its server at every load). It dials the
//! server's socket whenever it has a token, and rings the window's bell
//! whenever the socket has something to say; the window pumps it then, on
//! the thread it draws on — a household's budget is small.
#![cfg_attr(feature = "demo", allow(dead_code))]

use ark_auth::Login;
use ark_peer::{Config, Disk, Logins};

use crate::client::Replica;

/// A dev server on this machine: money-server's own default.
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8788";

/// What a window on a server holds beside its replica.
pub struct Server {
    pub url: String,
    /// Who is signed in; none until somebody is.
    pub login: Option<Login>,
    /// Where logins are remembered — none in a test, which must not write
    /// into the config directory of whoever runs it.
    pub logins: Option<Logins>,
    /// A name to offer a dev server's sign-in (`--user`, `?user=`).
    pub user: Option<String>,
    /// This load has gone to sign in once, and does not go again by itself:
    /// a sign-in that came back empty would otherwise go round for ever.
    pub tried: bool,
    /// What the socket rings.
    pub bell: arkui::wake::Bell,
}

/// The logins this device remembers, one per server.
pub fn logins() -> Logins {
    Logins::new("money")
}

/// Where the server is, and a name to offer a dev server.
pub fn config() -> (String, Option<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
        let server = flag("--server").unwrap_or_else(|| DEFAULT_SERVER.into());
        (server.trim_end_matches('/').to_string(), flag("--user"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let location = web_sys::window().map(|w| w.location());
        let query = location.as_ref().and_then(|l| l.search().ok()).unwrap_or_default();
        let origin = location.as_ref().and_then(|l| l.origin().ok()).unwrap_or_else(|| DEFAULT_SERVER.into());
        let server = ark_auth::query_value(&query, "server").unwrap_or(origin);
        (server.trim_end_matches('/').to_string(), ark_auth::query_value(&query, "user"))
    }
}

/// This server's replica on this device, as `login` or as whoever last
/// used it, dialling the server whenever there is a token.
pub fn open(server: &str, login: Option<&Login>) -> Result<Replica, String> {
    let login = login.map(|l| (ark_peer::who(l), l.token.clone()));
    let mut r = Replica::open_as(Config::of(crate::client::domain()), disk(server)?, login);
    r.connect(&ark_auth::socket_url(server));
    Ok(r)
}

/// Where a server's replica is kept.
fn disk(server: &str) -> Result<Box<dyn Disk>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = data_dir(server);
        ark_peer::Files::new(&dir)
            .map(|f| Box::new(f) as Box<dyn Disk>)
            .map_err(|e| format!("{}: {e}", dir.display()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = server;
        Ok(Box::new(ark_peer::Memory::default()))
    }
}

/// `$XDG_DATA_HOME/money/<server>`, or `~/.local/share/…`, or the temp
/// directory on a machine with neither.
#[cfg(not(target_arch = "wasm32"))]
pub fn data_dir(server: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("money").join(server_key(server))
}

/// A server's address as one path segment: `http://127.0.0.1:8788` is
/// `http___127_0_0_1_8788`.
pub fn server_key(server: &str) -> String {
    let server = server.trim_end_matches('/');
    server.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// The socket's wake: ring the window's bell, from the socket's thread
/// natively and from the `WebSocket`'s callbacks in a browser.
pub fn waker(bell: &arkui::wake::Bell) -> ark_peer::Wake {
    let bell = bell.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::sync::Arc::new(move || bell.ring())
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::rc::Rc::new(move || bell.ring())
    }
}

/// The name a status line gives a login.
pub fn who(login: &Login) -> String {
    match login.user.name.is_empty() {
        true => login.user.id.clone(),
        false => login.user.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_is_one_directory() {
        assert_eq!(server_key("http://127.0.0.1:8788/"), server_key("http://127.0.0.1:8788"));
        assert_eq!(server_key("http://127.0.0.1:8788"), "http___127_0_0_1_8788");
        assert_ne!(server_key("https://a.example"), server_key("https://b.example"));
    }
}
