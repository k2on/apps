//! An app's host on the phones: what every Ark app's native shell used to
//! own by hand (money's `Model.swift` and `Start.kt`), written once, in
//! Rust. It runs the first run every app shares (`ark_auth::first_run`
//! over the sign-in machine) and carries out its effects — the server's
//! `/healthz` asked ([`ark_auth::probe`]), the code traded, what is
//! remembered kept (the login in the secure store, the rest in a file of
//! the app's), the session opened over the process's one runtime
//! ([`ArkProcess`]) — and watches the session's link: the denial said to
//! the machine once, the server's refusals said, the first sync told.
//!
//! What only a native shell can do is two things, asked of its
//! [`HostShell`]: open the sign-in in the system's browser session
//! (`ASWebAuthenticationSession`, a Custom Tab) and come back with where it
//! landed ([`ArkHost::returned`]), and read again what it shows when told
//! something moved. The secure store is the phone's module
//! (ark-secure-store), handed over at construction.
//!
//! The app's own part is a [`HostApp`] in its bridge: how its replica is
//! opened, its demo, and what it makes of a session (its screens'
//! runtimes, its month, its session's facts), as the desktop's `live.rs`
//! is money's on the desktop. Its bridge exports the object the phone
//! holds (money's `MoneyHost`), which hands this one out.

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use ark_auth::first_run::FirstRun;
use ark_auth::keep::LoginKeep;
use ark_auth::onboarding::{AppInfo, Effect, Event, Found, Remembered, Step};
use ark_module::Modules;

use crate::{ArkProcess, Login, Peer, Refusal};

/// What a native shell does for its app's host.
#[uniffi::export(with_foreign)]
pub trait HostShell: Send + Sync {
    /// Something the shell shows moved (the step, a page, a notice): read
    /// it again. Told on any thread; the shell hops to its main one.
    fn changed(&self);
    /// Open the server's sign-in at `url` in the system's browser session,
    /// never an ephemeral one, and answer [`ArkHost::returned`] with where
    /// it came back (`<scheme>://auth?code=…`) or why it did not.
    fn browse(&self, url: String);
}

/// How the phone launched the app, as its native shell read it.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct Launch {
    /// The app's own directory on the device (Application Support, the
    /// files directory): its replicas and what its first run remembers.
    pub root: String,
    /// The app's version as the device says it: `0.1 (12)`.
    pub version: String,
    /// The demo's household for this launch alone, whatever sign-in is
    /// remembered (`-demo`: the screenshots'), on this day when one is
    /// given (`2026-10-26`), today otherwise.
    pub demo: bool,
    pub demo_day: Option<String>,
    /// A screenshot's launch: past the app's pages, as a returning
    /// person's would be.
    pub shots: bool,
    /// A camera reads another device's code here ("Scan a Code").
    pub scan: bool,
    /// The launch's own arguments (`-screen budget`), for the app's shell.
    pub args: Vec<String>,
}

/// How a session was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// The household on `server`, as `login`.
    Server { server: String, login: Login },
    /// The demo's household, on its day (today when none).
    Demo { today: Option<String> },
}

/// The app's own part of its host: its bridge's.
pub trait HostApp: Send + Sync + 'static {
    /// Who the app is, to the sign-in machine.
    fn info(&self) -> AppInfo;
    /// The app's words on its first run's pages (`welcome::copy`).
    fn copy(&self, scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy;
    /// The domain it carries, as a server's `/healthz` names it.
    fn domain(&self) -> String;
    /// Its replica on the disk at `dir`, as `login`.
    fn open(&self, dir: &str, login: Login) -> Result<Arc<Peer>, Refusal>;
    /// Its demo's household, on `today` (today when none).
    fn demo(&self, today: Option<String>) -> Result<Arc<Peer>, Refusal>;
    /// A session open over `peer`: the app's runtimes made over it. What it
    /// asks of the host goes through `host`.
    fn opened(&self, peer: Arc<Peer>, how: Opened, host: Handle);
    /// The session closed (signed out, another server): its runtimes let go.
    fn closed(&self);
    /// The peer said something moved, after the host read its link.
    fn moved(&self) {}
    /// A link for the app itself, once it shows: a launch's arguments
    /// (`-screen budget`) or an address (`money:///budget`).
    fn link(&self, args: Option<Vec<String>>, address: String);
    /// The launch's own arguments, once its runtimes run.
    fn launched(&self, args: Vec<String>);
    /// Whether nothing of the app's is left to come.
    fn idle(&self) -> bool {
        true
    }
    /// A number that grows with every message the app's runtimes take.
    fn version(&self) -> u64 {
        0
    }
}

/// The host as an app's part reaches it: weakly, so neither keeps the
/// other.
#[derive(Clone)]
pub struct Handle(Weak<ArkHost>);

impl Handle {
    /// A sentence for the shell to say once (a refusal, the bank's news).
    pub fn notice(&self, said: String) {
        if let Some(h) = self.0.upgrade() {
            h.notice(said)
        }
    }
    /// Something the shell shows moved.
    pub fn changed(&self) {
        if let Some(h) = self.0.upgrade() {
            h.changed()
        }
    }
    /// Signed out, or out of the demo.
    pub fn sign_out(&self) {
        if let Some(h) = self.0.upgrade() {
            h.sign_out()
        }
    }
    /// The app's version, as its launch said it.
    pub fn version(&self) -> String {
        self.0.upgrade().map(|h| h.launch.version.clone()).unwrap_or_default()
    }
}

struct State {
    run: FirstRun,
    shell: Option<Arc<dyn HostShell>>,
    /// The session's peer and how it was opened, while one is.
    open: Option<(Arc<Peer>, Opened)>,
    notice: Option<String>,
    scanning: bool,
    /// The server's denial of this login, said to the machine once.
    denied: bool,
    /// The launch's arguments, handed to the app once.
    args: Option<Vec<String>>,
    version: u64,
    started: bool,
}

/// The app's host: its first run, its session, and what its shell shows
/// beside its screens.
#[derive(uniffi::Object)]
pub struct ArkHost {
    app: Arc<dyn HostApp>,
    launch: Launch,
    keep: LoginKeep,
    state: Mutex<State>,
    /// Where the peer's word that something moved is carried, to a thread
    /// of the host's own: never inside the call that moved it (a screen's
    /// runtime, mid-message).
    told: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    me: Weak<ArkHost>,
}

/// What a launch remembers that is no secret: the server, whether the demo
/// shows, whether the app's pages were seen. A few lines in the app's own
/// directory, as the desktop keeps its pages seen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Kept {
    server: Option<String>,
    demo: bool,
    seen: bool,
}

impl Kept {
    fn read(text: &str) -> Kept {
        let mut k = Kept::default();
        for line in text.lines() {
            match line.split_once(' ').unwrap_or((line, "")) {
                ("server", s) if !s.is_empty() => k.server = Some(s.into()),
                ("demo", "1") => k.demo = true,
                ("seen", "1") => k.seen = true,
                _ => {}
            }
        }
        k
    }

    fn write(&self) -> String {
        let mut out = String::new();
        if let Some(s) = &self.server {
            out += &format!("server {s}\n");
        }
        if self.demo {
            out += "demo 1\n";
        }
        if self.seen {
            out += "seen 1\n";
        }
        out
    }
}

impl ArkHost {
    /// `app`'s host as `launch` says, its login kept in `store` (none: for
    /// this run alone).
    pub fn new(app: Arc<dyn HostApp>, launch: Launch, store: Option<Arc<dyn ark_secure_store::phone::SecureStore>>) -> Arc<ArkHost> {
        let mut m = Modules::new();
        m.set(store.map(ark_secure_store::phone::port));
        let run = FirstRun::new(app.info(), app.copy(launch.scan, app.info().demo));
        Arc::new_cyclic(|me| ArkHost {
            app,
            keep: LoginKeep::new("login", m, None),
            state: Mutex::new(State {
                run,
                shell: None,
                open: None,
                notice: None,
                scanning: false,
                denied: false,
                args: Some(launch.args.clone()),
                version: 0,
                started: false,
            }),
            launch,
            told: Mutex::new(None),
            me: me.clone(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn handle(&self) -> Handle {
        Handle(self.me.clone())
    }

    /// The app's own directory, under the launch's root.
    fn dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.launch.root).join(&self.app.info().scheme)
    }

    fn kept(&self) -> Kept {
        Kept::read(&std::fs::read_to_string(self.dir().join("first-run")).unwrap_or_default())
    }

    fn keep_file(&self, k: &Kept) {
        let dir = self.dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("first-run"), k.write());
    }

    /// Where `server`'s replica is kept: one directory per server, not per
    /// person — the household's log is the server's, and a person signing in
    /// again finds their pending intents. The phones' own, as before.
    pub fn replica_dir(root: &str, scheme: &str, server: &str) -> std::path::PathBuf {
        let place: String = server.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        std::path::Path::new(root).join(scheme).join(format!("server-{place}")).join("replica")
    }

    fn changed(&self) {
        let shell = {
            let mut s = self.lock();
            s.version += 1;
            s.shell.clone()
        };
        if let Some(shell) = shell {
            shell.changed()
        }
    }

    fn notice(&self, said: String) {
        self.lock().notice = Some(said);
        self.changed()
    }

    /// The first run told `e`, its effects carried out.
    fn event(&self, e: Event) {
        let fx = self.lock().run.event(e);
        self.carry(fx);
    }

    fn carry(&self, fx: Vec<Effect>) {
        for e in fx {
            self.effect(e);
        }
        self.offer_link();
        self.changed();
    }

    /// The link the first run kept, handed to the app once it shows.
    fn offer_link(&self) {
        let link = {
            let mut s = self.lock();
            if s.open.is_none() {
                return;
            }
            s.run.take_link()
        };
        if let Some(url) = link {
            let scheme = self.app.info().scheme;
            self.app.link(ark_auth::probe::launch_args(&url, &scheme), url);
        }
    }

    fn effect(&self, e: Effect) {
        match e {
            Effect::FindServer { typed } => {
                let domain = self.app.domain();
                self.off(move |h| {
                    let found = ark_auth::probe::find(&typed, &domain, ark_auth::probe::get);
                    h.event(Event::Checked { found });
                });
            }
            Effect::Remember { remembered } => {
                if self.launch.demo {
                    return;
                }
                let mut k = self.kept();
                k.server = remembered.server;
                k.demo = remembered.demo;
                self.keep_file(&k);
                self.keep.keep(remembered.login.as_ref());
            }
            Effect::Browse { url, .. } => {
                if let Some(shell) = self.lock().shell.clone() {
                    shell.browse(url)
                }
            }
            Effect::Exchange { server, code } => self.trade(move || ark_auth::client::exchange(&server, &code)),
            Effect::Redeem { server, code } => self.trade(move || ark_auth::client::redeem(&server, &code)),
            Effect::OpenSession { server, login } => {
                self.close();
                let dir = ArkHost::replica_dir(&self.launch.root, &self.app.info().scheme, &server);
                self.off(move |h| {
                    let opened = std::fs::create_dir_all(&dir)
                        .map_err(|e| Refusal::from(format!("{}: {e}", dir.display())))
                        .and_then(|_| {
                            let peer = h.app.open(&dir.to_string_lossy(), login.clone())?;
                            peer.connect(ark_auth::socket_url(&server));
                            Ok(peer)
                        });
                    match opened {
                        Ok(peer) => h.opened(peer, Opened::Server { server, login }),
                        Err(Refusal::Refused { why }) => h.event(Event::OpenFailed { why }),
                    }
                });
            }
            Effect::OpenDemo => {
                self.close();
                let today = self.launch.demo_day.clone();
                self.off(move |h| match h.app.demo(today.clone()) {
                    Ok(peer) => h.opened(peer, Opened::Demo { today }),
                    Err(Refusal::Refused { why }) => h.event(Event::OpenFailed { why }),
                });
            }
            Effect::CloseSession => self.close(),
        }
    }

    /// `f` on a thread of its own, with the host.
    fn off(&self, f: impl FnOnce(Arc<ArkHost>) + Send + 'static) {
        if let Some(h) = self.me.upgrade() {
            std::thread::spawn(move || f(h));
        }
    }

    /// A code traded off the shell's thread: the login, or the sentence why not.
    fn trade(&self, f: impl FnOnce() -> Result<ark_auth::Login, String> + Send + 'static) {
        self.off(move |h| {
            let e = match f() {
                Ok(login) => Event::SignedIn { login: login.into() },
                Err(why) => Event::SignInFailed { why },
            };
            h.event(e);
        });
    }

    /// The session open: the app's runtimes made over it, the peer
    /// watched. Online while a UI is attached to the process's runtime
    /// (the phone's app library attaches for as long as the app runs).
    fn opened(&self, peer: Arc<Peer>, how: Opened) {
        // The app's runtimes first: the host says it is open (and the app
        // shows) only once they are made.
        self.app.opened(peer.clone(), how.clone(), self.handle());
        {
            let mut s = self.lock();
            s.open = Some((peer.clone(), how));
            s.denied = false;
        }
        peer.set_listener(Some(Arc::new(Told(self.teller()))));
        if let Some(args) = self.lock().args.take() {
            self.app.launched(args);
        }
        self.drain();
        self.offer_link();
        self.changed();
    }

    fn close(&self) {
        let gone = self.lock().open.take();
        if let Some((peer, _)) = gone {
            peer.set_listener(None);
            self.app.closed();
        }
    }

    /// What the peer said moved: its link read — a denial told the machine
    /// once, the server's refusals said, the first sync told — and the app
    /// told.
    fn drain(&self) {
        let (peer, opening, denied) = {
            let s = self.lock();
            let Some((peer, _)) = s.open.clone() else { return };
            (peer, matches!(s.run.machine().step(), Step::Opening { .. }), s.denied)
        };
        let st = peer.status();
        if let (Some(d), false) = (st.denied.clone(), denied) {
            self.lock().denied = true;
            self.event(Event::Denied { reason: d });
        }
        if let Some(last) = peer.take_rejections().last() {
            self.lock().notice = Some(format!("Not saved: {}", last.why));
        }
        // Signed in just now: opening, until the replica has what the server
        // had — or the server cannot be reached, and nothing more is coming.
        if opening && (st.linked || st.cursor > 0 || st.link == "waiting to dial" || st.link == "offline") {
            self.event(Event::Synced);
        }
        self.app.moved();
        self.changed();
    }
}

/// The peer's word that something moved, to the host's thread.
struct Told(std::sync::mpsc::Sender<()>);

impl crate::Listener for Told {
    fn changed(&self) {
        let _ = self.0.send(());
    }
}

impl ArkHost {
    /// Where the peer's word goes: the host's thread, made the first time,
    /// which drains once for every burst.
    fn teller(&self) -> std::sync::mpsc::Sender<()> {
        let mut t = self.told.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = t.as_ref() {
            return tx.clone();
        }
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let me = self.me.clone();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                while rx.try_recv().is_ok() {}
                match me.upgrade() {
                    Some(h) => h.drain(),
                    None => return,
                }
            }
        });
        *t = Some(tx.clone());
        tx
    }
}

#[uniffi::export]
impl ArkHost {
    /// The launch: what was remembered read, and the first run started;
    /// `shell` told from now on. Once; a second start only sets the shell.
    pub fn start(&self, shell: Arc<dyn HostShell>) {
        let first = {
            let mut s = self.lock();
            s.shell = Some(shell);
            !std::mem::replace(&mut s.started, true)
        };
        if !first {
            return self.changed();
        }
        let k = self.kept();
        let remembered = match self.launch.demo {
            true => Remembered {
                server: None,
                login: None,
                demo: true,
            },
            false => Remembered {
                server: k.server.clone(),
                login: self.keep.recall(),
                demo: k.demo,
            },
        };
        self.lock().run.launch(k.seen, self.launch.shots || self.launch.demo);
        self.event(Event::Launched {
            remembered,
            code: None,
            returned: false,
        });
    }

    /// Where the browser's sign-in came back (`<scheme>://auth?code=…`), or
    /// why it did not (none for closed).
    pub fn returned(&self, url: Option<String>, error: Option<String>) {
        let scheme = self.app.info().scheme;
        match url.as_deref().and_then(|u| ark_auth::probe::code_of(u, &scheme)) {
            Some(code) => self.event(Event::Code { code }),
            None => self.event(Event::CameBack {
                why: if url.is_none() {
                    error
                } else {
                    Some("it came back without a code".into())
                },
            }),
        }
    }

    /// A link the system opened the app with. `<scheme>://auth?code=…` is a
    /// sign-in coming back; `<scheme>://welcome` the app's pages; a join
    /// link (`<scheme>://join?…`, or a page's `https://…/join?…`) the
    /// sign-in machine's, asked about first; any other of the app's scheme
    /// a page or a launch, kept until the app shows and then the app's.
    pub fn open_url(&self, url: String) {
        let scheme = self.app.info().scheme;
        if let Some(code) = ark_auth::probe::code_of(&url, &scheme) {
            return self.event(Event::Code { code });
        }
        if let Some(j) = ark_auth::onboarding::join_of(url.clone()) {
            self.lock().run.skip_intro();
            return self.event(Event::Joined {
                server: j.server,
                code: j.code,
            });
        }
        let Some(rest) = url.strip_prefix(&format!("{scheme}://")) else { return };
        if rest.trim_matches('/').split(['?', '#']).next() == Some("welcome") {
            self.lock().run.show_intro();
            return self.changed();
        }
        self.lock().run.open_link(url);
        self.offer_link();
        self.changed();
    }

    /// The app came back to the front after it was away: the link dialled
    /// now, and a sign-in that came back empty may go again.
    pub fn foreground(&self) {
        if let Some((peer, _)) = self.lock().open.clone() {
            peer.dial_now();
        }
        self.event(Event::Foreground);
    }

    /// What the camera read for "Scan a Code" (none: closed).
    pub fn scanned(&self, text: Option<String>) {
        let fx = {
            let mut s = self.lock();
            s.scanning = false;
            s.run.scanned(text)
        };
        self.carry(fx);
    }

    /// Signed out of the server (the session revoked, pushed while linked,
    /// pending otherwise; the replica kept), or out of the demo: back to the
    /// first page, the process's runtime put away.
    pub fn sign_out(&self) {
        let open = self.lock().open.clone();
        if let Some((peer, Opened::Server { .. })) = open {
            peer.logout();
            ArkProcess::shared().sign_out();
        }
        self.event(Event::SignOut);
    }

    /// Where the first run stands.
    pub fn step(&self) -> Step {
        self.lock().run.machine().step()
    }

    /// Whether the app itself shows: a session, or the demo, with no page
    /// of the first run over it.
    pub fn shows_app(&self) -> bool {
        let s = self.lock();
        s.open.is_some() && !s.run.page().shows() && matches!(s.run.machine().step(), Step::Running { .. } | Step::Demo)
    }

    /// Whether the camera's code scanner is up ("Scan a Code").
    pub fn scanning(&self) -> bool {
        self.lock().scanning
    }

    /// The sentence to say once, taken.
    pub fn take_notice(&self) -> Option<String> {
        self.lock().notice.take()
    }

    /// A number that grows whenever anything the shell shows moved.
    pub fn version(&self) -> u64 {
        self.lock().version + self.app.version()
    }

    /// Nothing on its way: the session's peer settled (or no session) and
    /// the app's runtimes idle. A screenshot's ready signal, with the
    /// version unchanged.
    pub fn idle(&self) -> bool {
        let peer = self.lock().open.clone().map(|(p, _)| p);
        peer.is_none_or(|p| p.settled()) && self.app.idle()
    }

    /// "Add a Device": a join link another device signs in with, made as
    /// this login at its server. Blocking: off the shell's thread.
    pub fn device_link(&self) -> Result<crate::DeviceLink, Refusal> {
        let (server, token) = {
            let s = self.lock();
            let r = s.run.machine().remembered();
            (r.server, r.login.map(|l| l.token))
        };
        match (server, token.filter(|t| !t.is_empty())) {
            (Some(server), Some(token)) => crate::device_link(self.app.info().scheme, server, token)
                .map_err(|Refusal::Refused { why }| Refusal::from(format!("Couldn't make a code: {why}."))),
            _ => Err("Sign in first.".into()),
        }
    }
}

impl ArkHost {
    /// The first run, for the app's bridge (its pages' twins).
    pub fn with_run<T>(&self, f: impl FnOnce(&mut FirstRun) -> T) -> T {
        f(&mut self.lock().run)
    }

    /// A press on the first run's page, its effects carried out.
    pub fn press(&self, msg: ark_app::welcome::WelcomeMsg) {
        let (asked, demo) = {
            let mut s = self.lock();
            let asked = s.run.send(msg);
            if asked.scan {
                s.scanning = true;
            }
            (asked, self.launch.demo)
        };
        if asked.seen && !demo {
            let mut k = self.kept();
            k.seen = true;
            self.keep_file(&k);
        }
        self.carry(asked.effects);
    }

    /// The session's peer, while one is open.
    pub fn peer(&self) -> Option<Arc<Peer>> {
        self.lock().open.clone().map(|(p, _)| p)
    }

    /// What the machine says it found, for a test that answers a probe.
    pub fn found(&self, found: Found) {
        self.event(Event::Checked { found })
    }

    /// The login the first run stands on, while it has one.
    pub fn login(&self) -> Option<Login> {
        match self.step() {
            Step::Running { login, .. } | Step::Opening { login, .. } => Some(login),
            _ => None,
        }
    }

    /// The server remembered, while there is one.
    pub fn server(&self) -> Option<String> {
        self.lock().run.machine().remembered().server
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_kept_reads_back() {
        let k = Kept {
            server: Some("https://money.example.com".into()),
            demo: false,
            seen: true,
        };
        assert_eq!(Kept::read(&k.write()), k);
        assert_eq!(Kept::read(""), Kept::default());
    }

    /// The phones' replica directories, as before: an install keeps its
    /// replicas. Falsified by any other spelling of the server.
    #[test]
    fn a_server_is_one_directory() {
        let d = ArkHost::replica_dir("/r", "money", "http://127.0.0.1:8788");
        assert_eq!(d, std::path::Path::new("/r/money/server-http___127_0_0_1_8788/replica"));
    }
}
