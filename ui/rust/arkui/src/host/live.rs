//! The window on its server: where it is, this machine's replica of its
//! domain, and who is signed in there.
//!
//! **Where the server is:** `--server URL`, else a dev server on this
//! machine ([`DEFAULT_SERVER`]); `--user NAME` is the name a dev server's
//! sign-in is offered.
//!
//! **Signing in** is the machine every app's shell walks
//! (`ark_auth::onboarding`, under the first run every app shares): the
//! system's browser opens the server's sign-in, which comes back to a
//! loopback port with a one-minute code, traded at `/auth/exchange` for a
//! login. A dev server with its own user sends the code straight back, with
//! no browser (what the screenshots' walk signs in by). A login is kept per
//! server in the desktop's secure store (the Secret Service: `devices`), so
//! the next launch is signed in already.
//!
//! **The replica** is one per server, a directory in the app's data
//! (`$XDG_DATA_HOME/<scheme>/<server>`), read on a thread at launch. It
//! dials the server's socket whenever it has a login and rings the window's
//! bell whenever the socket has something to say; the window pumps it then,
//! on the thread it draws on.

use ark_auth::first_run::FirstRun;
use ark_auth::keep::ServerLogins;
use ark_auth::onboarding::{AppInfo, Effect, Event, Remembered, Step};
use ark_auth::Login;
use ark_peer::{Config, Disk, Logins, Memory, Peer};
use iced::Task;

use ark_app::host::{HostedApp, Session};
use ark_app::window::Windowed;

use super::{Desktop, DesktopHost, Msg};

type AppMsg<W> = <<W as Desktop>::App as Windowed>::Msg;

/// A dev server on this machine: every app's server's default port is its
/// own, so none is guessed but money-server's, the first app's.
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8788";

/// What the window holds on its server beside the replica.
pub struct Live {
    /// The server's base URL.
    pub server: String,
    /// Who is signed in, once the session is open.
    pub login: Option<Login>,
    /// The logins this machine keeps; none in a test.
    pub logins: Option<ServerLogins>,
    /// A name to offer a dev server's sign-in (`--user`).
    pub user: Option<String>,
    /// The sign-in machine under the first run's pages, every app's.
    pub first_run: FirstRun,
    /// What the socket rings.
    pub bell: crate::wake::Bell,
}

impl Live {
    /// Whether the app is shown: the session open.
    pub fn running(&self) -> bool {
        matches!(self.first_run.machine().step(), Step::Running { .. })
    }

    /// Whether nothing of signing in is on its way.
    pub fn settled(&self) -> bool {
        self.running()
    }
}

/// Who the app is, to the sign-in machine: its server is its launch's,
/// so no other and no demo is offered.
pub fn app<W: Desktop>() -> AppInfo {
    AppInfo {
        name: W::NAME.into(),
        scheme: W::SCHEME.into(),
        demo: false,
        what: <W::App as HostedApp>::KEEPS.into(),
    }
}

/// Where the server is, and a name to offer a dev server.
pub fn config() -> (String, Option<String>) {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let server = flag("--server").unwrap_or_else(|| DEFAULT_SERVER.into());
    (server.trim_end_matches('/').to_string(), flag("--user"))
}

/// The logins this machine keeps, one per server, in the secure store
/// where it has one (the Secret Service on Linux); with none a login lasts
/// the run.
pub fn logins<W: Desktop>() -> ServerLogins {
    Logins::new(W::SCHEME).secured(super::devices::secure_store(W::SCHEME))
}

/// An empty replica in memory: the window before its own is read, or
/// when its own will not open.
pub fn memory<W: Desktop>() -> Peer {
    Peer::open_as(Config::of(W::domain()), Box::new(Memory::default()), None)
}

/// This server's replica on this machine, as `login` or as whoever last
/// used it, dialling the server whenever there is a login.
pub fn open<W: Desktop>(server: &str, login: Option<&Login>) -> Result<Peer, String> {
    let login = login.map(|l| (ark_peer::who(l), l.token.clone()));
    let dir = data_dir(W::SCHEME, server);
    let disk = ark_peer::Files::new(&dir)
        .map(|f| Box::new(f) as Box<dyn Disk>)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut p = Peer::open_as(Config::of(W::domain()), disk, login);
    p.connect(&ark_auth::socket_url(server));
    Ok(p)
}

/// `$XDG_DATA_HOME/<scheme>/<server>`, or `~/.local/share/…`, or the
/// temporary directory on a machine with neither.
pub fn data_dir(scheme: &str, server: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join(scheme).join(server_key(server))
}

/// A server's address as one path segment: `http://127.0.0.1:8788` is
/// `http___127_0_0_1_8788`.
pub fn server_key(server: &str) -> String {
    server
        .trim_end_matches('/')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Where this machine keeps that the app's pages were seen.
fn seen_file(scheme: &str) -> std::path::PathBuf {
    data_dir(scheme, "x").with_file_name("intro-seen")
}

fn remember_intro(scheme: &str) {
    let f = seen_file(scheme);
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, "1\n");
}

/// A sign-in link (`money://join?server=…&code=…`): what the sign-in
/// machine is told; any other link is a place.
pub fn join_of(link: &str) -> Option<Event> {
    let rest = link.split_once("://")?.1;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    if path.trim_matches('/') != "join" {
        return None;
    }
    let get = |k: &str| {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(key, _)| *key == k)
            .map(|(_, v)| ark_router::url::decode(v))
    };
    Some(Event::Joined {
        server: get("server")?,
        code: get("code"),
    })
}

/// Who is signed in where, as the window knows it.
pub fn session_of<W: Desktop>(peer: &Peer, live: Option<&Live>, today: &str) -> Session {
    let version = W::VERSION.to_string();
    let Some(live) = live else {
        return Session {
            demo: true,
            today: today.into(),
            version,
            ..Default::default()
        };
    };
    let status = peer.status();
    let user = live.login.as_ref().map(|l| l.user.clone()).unwrap_or_default();
    Session {
        demo: false,
        today: today.into(),
        name: user.name.clone(),
        user: user.id.clone(),
        email: user.email.clone(),
        picture: ark_auth::picture_url(&live.server, &user.id, &user.picture).unwrap_or_default(),
        server: ark_auth::onboarding::display(&live.server),
        linked: Some(status.linked),
        pending: status.pending as i64,
        version,
        leaves: live.login.is_some(),
        // A code for another device is the phones' to make.
        adds_devices: false,
    }
}

/// The window on its server over `peer`, as the login this machine keeps
/// there; the sign-in machine is told by [`DesktopHost::signing_in`].
pub fn on_server<W: Desktop>(mut peer: Peer, today: String) -> DesktopHost<W>
where
    AppMsg<W>: Send,
{
    let (server, user) = config();
    let logins = logins::<W>();
    let login = logins.recall(&server);
    let bell = crate::wake::Bell::new();
    {
        let bell = bell.clone();
        peer.set_waker(std::sync::Arc::new(move || bell.ring()));
    }
    let live = Live {
        server,
        login,
        logins: Some(logins),
        user,
        first_run: FirstRun::new(app::<W>(), W::App::welcome(false, false)),
        bell,
    };
    DesktopHost::over(peer, today, false, Some(live))
}

/// What a sign-in's step came to, off the window's thread.
fn off<W: Desktop>(work: impl FnOnce() -> Result<Event, String> + Send + 'static) -> Task<Msg<AppMsg<W>>>
where
    AppMsg<W>: Send,
{
    crate::later::off_thread(
        move || work().unwrap_or_else(|why| Event::SignInFailed { why }),
        |slot: crate::later::Slot<Event>| match slot.take() {
            Some(e) => Msg::Onboard(Box::new(e)),
            None => Msg::Swallow,
        },
    )
}

impl<W: Desktop> DesktopHost<W>
where
    AppMsg<W>: Send,
{
    /// The sign-in machine told where this launch starts: signed in
    /// already, or on its way to sign in; and the app's pages from the
    /// first until this machine has seen them, not on a launch that goes
    /// straight somewhere (a screenshot's, a link's, launch arguments').
    pub(super) fn signing_in(&mut self) -> Task<Msg<AppMsg<W>>> {
        let Some(live) = &self.live else { return Task::none() };
        let remembered = Remembered {
            server: Some(live.server.clone()),
            login: live.login.clone().map(Into::into),
            demo: false,
        };
        let task = self.onboard(Event::Launched {
            remembered,
            code: None,
            returned: false,
        });
        let args: Vec<String> = std::env::args().collect();
        let straight = arkui_shots::asked().is_some() || crate::navigator::link_arg(W::SCHEME).is_some() || args.iter().any(|a| a == "-screen");
        let seen = seen_file(W::SCHEME).exists();
        if let Some(live) = &mut self.live {
            live.first_run.launch(seen, straight);
        }
        task
    }

    /// An event for the sign-in machine, and its effects carried out.
    pub fn onboard(&mut self, event: Event) -> Task<Msg<AppMsg<W>>> {
        let Some(live) = &mut self.live else { return Task::none() };
        let effects = live.first_run.event(event);
        let said = live.first_run.machine().said();
        if let Some(t) = said.error {
            self.toast.heard(&t);
        }
        Task::batch(effects.into_iter().map(|e| self.effect(e)).collect::<Vec<_>>())
    }

    /// A press on the first run's page, and the machine's effects carried
    /// out; the pages walked or skipped remembered by this machine.
    pub(super) fn first_run(&mut self, msg: ark_app::welcome::WelcomeMsg) -> Task<Msg<AppMsg<W>>> {
        let Some(live) = &mut self.live else { return Task::none() };
        let asked = live.first_run.send(msg);
        if asked.seen {
            remember_intro(W::SCHEME);
        }
        Task::batch(asked.effects.into_iter().map(|e| self.effect(e)).collect::<Vec<_>>())
    }

    /// One of the machine's effects: the login remembered or forgotten, the
    /// browser sent to sign in, the code traded, the session opened or
    /// closed.
    fn effect(&mut self, effect: Effect) -> Task<Msg<AppMsg<W>>> {
        let Some(live) = &mut self.live else { return Task::none() };
        match effect {
            Effect::Remember { remembered } => {
                if let Some(logins) = &live.logins {
                    match remembered.login {
                        Some(l) => logins.remember(&live.server, &l.into()),
                        None => logins.forget(&live.server),
                    }
                }
                Task::none()
            }
            Effect::Browse { server, .. } => {
                let user = live.user.clone();
                off::<W>(move || {
                    let code = ark_auth::client::code(&server, user.as_deref(), ark_auth::client::open_browser)?;
                    Ok(Event::Code { code })
                })
            }
            Effect::Exchange { server, code } => off::<W>(move || {
                let login = ark_auth::client::exchange(&server, &code)?;
                Ok(Event::SignedIn { login: login.into() })
            }),
            Effect::Redeem { server, code } => off::<W>(move || {
                let login = ark_auth::client::redeem(&server, &code)?;
                Ok(Event::SignedIn { login: login.into() })
            }),
            Effect::OpenSession { login, .. } => {
                let login: Login = login.into();
                if live.login.as_ref() != Some(&login) {
                    self.peer.login(ark_peer::who(&login), login.token.clone());
                    live.login = Some(login);
                }
                // Shown once the server's data is here, or the server cannot
                // be reached (`wake`): a new machine's replica is empty until
                // the server fills it.
                self.wake()
            }
            Effect::CloseSession => {
                live.login = None;
                Task::none()
            }
            Effect::FindServer { .. } | Effect::OpenDemo => Task::none(),
        }
    }

    /// What the socket brought, taken in: the views brought up to it, a
    /// session the server turned away handed to the machine, and the
    /// session that was opening shown once the server's data is here (or
    /// the server cannot be reached, and nothing more is coming soon).
    pub(super) fn wake(&mut self) -> Task<Msg<AppMsg<W>>> {
        let p = self.peer.pump();
        for (_, why) in self.peer.take_rejections() {
            self.said.push(format!("The server refused a change: {why}"));
        }
        self.store.settle(&mut self.peer);
        if let Some(reason) = p.denied {
            return self.onboard(Event::Denied { reason });
        }
        let opening = self
            .live
            .as_ref()
            .is_some_and(|l| matches!(l.first_run.machine().step(), Step::Opening { .. }));
        let filling = self.peer.machine(ark::schema::Space::Durable).is_some_and(|m| m.fill.is_some());
        let here = self.peer.linked && !filling && self.peer.backlog() == 0 && self.peer.cursor() > 0;
        match opening && (here || !p.closed.is_empty()) {
            true => self.onboard(Event::Synced),
            false => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server is one directory, whatever its trailing slash. Falsified by
    /// two servers sharing a key.
    #[test]
    fn a_server_is_one_directory() {
        assert_eq!(server_key("http://127.0.0.1:8788/"), server_key("http://127.0.0.1:8788"));
        assert_eq!(server_key("http://127.0.0.1:8788"), "http___127_0_0_1_8788");
        assert_ne!(server_key("https://a.example"), server_key("https://b.example"));
    }

    /// A join link is the sign-in machine's; any other link is a place.
    #[test]
    fn a_join_link_signs_in() {
        assert_eq!(
            join_of("money://join?server=https%3A%2F%2Fm.example&code=abc"),
            Some(Event::Joined {
                server: "https://m.example".into(),
                code: Some("abc".into())
            })
        );
        assert_eq!(join_of("money://account/1/checking"), None);
    }
}
