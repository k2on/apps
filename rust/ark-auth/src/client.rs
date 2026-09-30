//! Getting a login, on a desktop.
//!
//! Listen on a loopback port, send the person to the server's login page
//! with that port as the redirect, and wait for the code to come back. In
//! dev mode with a name given there is no page: the server answers the first
//! request with the redirect, and no browser opens at all — which is how
//! a desktop peer given a name stays one command.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use crate::util::{login_url, query_value};
use crate::Login;

/// How long a request to the server waits for its TCP connection
/// (`docs/plan-perf.md` R6). Ten seconds is three of a connect's SYN
/// retransmissions (at one, three and seven seconds), so a lossy network
/// gets through, where the kernel's own limit is two minutes and more; an
/// address nothing answers from gives up in ten.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a request waits on each read of the server's answer, and on
/// each write of its own, once connected. Every request here is answered
/// from the server's own memory and its session file — the login's first
/// redirect, a code traded for a session, `me`, `logout` — and none waits
/// on a provider, so an answer twenty seconds late is a server that is
/// not answering: a black hole, a proxy holding the socket, a process
/// stopped under it. That is what used to wait for ever, on a thread the
/// desktop signs in on and before a headless peer's first frame. Twenty
/// is room for a server starting cold behind a proxy. The person's own
/// time — the browser, the provider's page — is not bounded: that is
/// [`wait_for_code`], which waits on a person rather than on a server.
pub const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a request waits: the connection, and each read or write of
/// it. [`Patience::DEFAULT`] is [`CONNECT_TIMEOUT`] and [`READ_TIMEOUT`];
/// a test, or a headless peer told to, may be shorter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    pub connect: Duration,
    pub read: Duration,
}

impl Patience {
    pub const DEFAULT: Patience = Patience {
        connect: CONNECT_TIMEOUT,
        read: READ_TIMEOUT,
    };

    /// The same bound on the connection and on each read.
    pub fn of(d: Duration) -> Patience {
        Patience { connect: d, read: d }
    }

    // An agent held to it, following redirects or not.
    fn agent(self, redirects: u32) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .redirects(redirects)
            .timeout_connect(self.connect)
            .timeout_read(self.read)
            .timeout_write(self.read)
            .build()
    }
}

impl Default for Patience {
    fn default() -> Patience {
        Patience::DEFAULT
    }
}

/// Sign in to `server`, opening a browser with `open` if the server needs a
/// person to. Blocking, for as long as the person takes; call it off the UI
/// thread. The server is waited on for [`Patience::DEFAULT`] at most.
pub fn login(server: &str, user: Option<&str>, open: impl FnOnce(&str)) -> Result<Login, String> {
    login_with(server, user, open, Patience::DEFAULT)
}

/// [`login`], waiting on the server for `patience` at most.
pub fn login_with(server: &str, user: Option<&str>, open: impl FnOnce(&str), patience: Patience) -> Result<Login, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/");
    let url = login_url(server, &redirect, user);

    // Ask without following: a dev server answers with the code straight
    // away, and anything else is a page for a person.
    let first = patience.agent(0).get(&url).call();
    let code = match first {
        Ok(resp) if resp.status() / 100 == 3 => {
            let location = resp.header("Location").unwrap_or_default().to_string();
            match location.strip_prefix(&redirect) {
                Some(rest) => query_value(rest, "code").ok_or("the server sent no code")?,
                None => {
                    open(&location);
                    wait_for_code(&listener)?
                }
            }
        }
        Ok(_) => {
            open(&url);
            wait_for_code(&listener)?
        }
        Err(ureq::Error::Status(status, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            return Err(format!("{server} answered {status}: {body}"));
        }
        Err(e) => return Err(format!("cannot reach {server}: {e}")),
    };
    exchange_with(server, &code, patience)
}

/// The code, for the login it stands for.
pub fn exchange(server: &str, code: &str) -> Result<Login, String> {
    exchange_with(server, code, Patience::DEFAULT)
}

/// [`exchange`], waiting on the server for `patience` at most.
pub fn exchange_with(server: &str, code: &str, patience: Patience) -> Result<Login, String> {
    let url = format!("{}/auth/exchange", server.trim_end_matches('/'));
    let resp = patience
        .agent(5)
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({ "code": code }).to_string())
        .map_err(|e| match e {
            ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
            other => other.to_string(),
        })?;
    let body = resp.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| format!("the login did not parse: {e}"))
}

/// Whether `token` still proves a login at `server`, and whose; the
/// server is waited on for [`Patience::DEFAULT`] at most.
pub fn whoami(server: &str, token: &str) -> Result<Option<Login>, String> {
    let url = format!("{}/auth/me", server.trim_end_matches('/'));
    match Patience::DEFAULT
        .agent(5)
        .get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(resp) => {
            let body = resp.into_string().map_err(|e| e.to_string())?;
            serde_json::from_str(&body).map(Some).map_err(|e| format!("the login did not parse: {e}"))
        }
        Err(ureq::Error::Status(401, _)) => Ok(None),
        Err(e) => Err(format!("cannot reach {server}: {e}")),
    }
}

/// End the login `token` proves; the server is waited on for
/// [`Patience::DEFAULT`] at most.
pub fn logout(server: &str, token: &str) -> Result<(), String> {
    let url = format!("{}/auth/logout", server.trim_end_matches('/'));
    Patience::DEFAULT
        .agent(5)
        .post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// One request on the loopback listener, which the server's redirect makes
/// the browser send: `GET /?code=… HTTP/1.1`.
fn wait_for_code(listener: &TcpListener) -> Result<String, String> {
    loop {
        let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let path = line.split_whitespace().nth(1).unwrap_or("");
        let code = path.split_once('?').and_then(|(_, q)| query_value(q, "code"));
        let (status, body) = match &code {
            Some(_) => ("200 OK", "signed in — you can close this tab"),
            None => ("404 Not Found", "nothing here"),
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if let Some(code) = code {
            return Ok(code);
        }
    }
}

/// Open `url` in whatever the desktop calls a browser.
pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = ("open", vec![url]);
    #[cfg(target_os = "windows")]
    let cmd = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let cmd = ("xdg-open", vec![url]);
    let _ = std::process::Command::new(cmd.0)
        .args(cmd.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
