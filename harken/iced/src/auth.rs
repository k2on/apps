//! Where the server is, where this device keeps its replica of it, and who is
//! signed in there.
//!
//! **Nobody has to be.** A window opens on its own replica whether or not
//! anybody has signed in, and works: the library it has is there, and what is
//! done is kept pending on this device, authored as nobody. Signing in later
//! moves that same replica on — `ark_client::Peer::sign_in` makes every
//! pending intent the signer's, and dials — so everything done before is
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
use ark_auth::remember::Logins;
use ark_auth::Login;
use ark_client::{Domain, Error, Options};

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
/// nobody until somebody signs in at a server it joins
/// (`ark_client::Options::alone_as_nobody`).
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn open_alone(domain: Domain) -> Result<ark_client::Peer, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ark_client::Peer::open_path(domain, data_dir(LOCAL), Options::alone_as_nobody())
    }
    #[cfg(target_arch = "wasm32")]
    {
        ark_client::Peer::open_local(domain, &format!("harken:{LOCAL}"), Options::alone_as_nobody())
    }
}

/// How the replica is opened: as the remembered login, or signed out.
///
/// The token is never on disk in the replica, so a remembered login hands it
/// over here; signed out, the peer authors as whoever last used this replica
/// (nobody, on a device where nobody ever signed in) and dials nothing.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn options(login: Option<&Login>) -> Options {
    match login {
        Some(l) => Options::server(l.user.id.clone(), l.session.clone(), Some(l.token.clone())),
        None => Options::signed_out(),
    }
}

/// This server's replica on this device: a directory natively, the page's
/// `localStorage` in a browser — the `local` one when that is what joined
/// this server.
#[cfg_attr(feature = "demo", allow(dead_code))]
pub fn open(domain: Domain, server: &str, opts: Options) -> Result<ark_client::Peer, Error> {
    let place = match joined().as_deref() == Some(server) {
        true => LOCAL.to_string(),
        false => server.to_string(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        ark_client::Peer::open_path(domain, data_dir(&place), opts)
    }
    #[cfg(target_arch = "wasm32")]
    {
        ark_client::Peer::open_local(domain, &format!("harken:{place}"), opts)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One server is one directory whatever it is spelt with, and two servers
    /// are two. Falsified by keeping the trailing slash: the first fails.
    #[test]
    fn a_server_is_one_directory() {
        assert_eq!(server_key("http://127.0.0.1:8787/"), server_key("http://127.0.0.1:8787"));
        assert_eq!(server_key("http://127.0.0.1:8787"), "http___127_0_0_1_8787");
        assert_ne!(server_key("https://a.example"), server_key("https://b.example"));
    }
}
