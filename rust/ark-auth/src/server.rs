//! The routes — `/auth/login`, `/auth/callback`, `/auth/exchange`, the
//! `server` feature's — and the hub's side of who is signed in (S1,
//! `crate::rows`).
//!
//! A sign-in is a [`SignIn`] row the hub writes as its authority
//! ([`Auth::issue`]), and its token is the row's id with this server's HMAC
//! of it ([`Auth::mint`]): the server keeps nothing but its key
//! (`session.key` in the data directory it is handed, [`key_file`]). The
//! hub judges a token by the HMAC and the row it names ([`Auth::judge`],
//! [`Auth::authenticate`]) and closes the connections of a session revoked
//! or expired, or of a person whose roles changed ([`Auth::watcher`]).
//!
//! ```ignore
//! let auth = Arc::new(Auth::open::<App>(Mode::Dev, "http://127.0.0.1:8787", &data)?.allow_redirect("myapp://"));
//! // ark-peer does the rest: `.auth(auth)` mounts these routes, hands the
//! // hub over (`Auth::serve`) and plugs in the authenticator and watcher.
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use hmac::{Hmac, Mac};

use ark::db::read;
use ark::format::Encode;
use ark::hash::{hex, unhex};
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::row::Row;
use ark::store::{Change, Store};

use crate::oidc::{Challenge, Provider};
use crate::rows::{current, live, AuthTables, Role, SignIn};
use crate::{Account, Login};

#[cfg(feature = "server")]
mod routes;
#[cfg(feature = "server")]
pub use routes::router;

/// Thirty days. A phone that has not spoken to the server in a month signs
/// in again, and loses nothing by it.
pub const DEFAULT_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// How people sign in.
#[derive(Debug)]
pub enum Mode {
    /// Through an OpenID Connect provider.
    Oidc(Provider),
    /// As whatever name they type. For a laptop and nowhere else; a server
    /// running this way should say so every time it starts.
    Dev,
}

impl Mode {
    /// What a server says about this at startup, every time.
    pub fn announce(&self) -> String {
        match self {
            Mode::Oidc(p) => format!("signing in through {}", p.issuer()),
            Mode::Dev => "*** DEV AUTH: anyone is whoever they say. A name is a login, `name:role,role` a login \
                          holding those roles, and nothing is checked. Fine on a laptop; never anywhere else. ***"
                .into(),
        }
    }
}

/// What sign-in asks of the hub it serves (`ark-peer`'s `Handle`).
pub trait Hub: Send + Sync {
    /// `f` over the authority's store, on its thread, waited for.
    fn read(&self, f: Box<dyn FnOnce(&Store) + Send>);
    /// One change written as an entry the authority authors: a raw write,
    /// which no client's intent can name.
    fn write(&self, c: Change) -> Result<(), String>;
    /// The domain's function `func` run on `input`, as an entry the
    /// authority authors.
    fn author(&self, func: &str, input: Vec<u8>) -> Result<(), String>;
}

/// The hub's watch over who is signed in: given what an entry changed — or
/// `None`, the clock's tick — and the sessions connected, the sessions
/// whose connections to close.
pub type Watch = Box<dyn FnMut(&Store, Option<&[Change]>, &[Who], i64) -> Vec<String> + Send>;

/// Who signs in, as a token proves it.
pub type Authenticate = Arc<dyn Fn(&str) -> Option<Who> + Send + Sync>;

/// A login started and not finished: what the provider's answer has to
/// match, where the code goes, and what is signing in.
#[derive(Debug)]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
struct Pending {
    challenge: Challenge,
    redirect: String,
    device: String,
    started_ms: i64,
}

/// A login finished and not collected: a one-minute, single-use code
/// standing in for the token in the URL.
#[derive(Debug)]
struct Issued {
    login: Login,
    issued_ms: i64,
}

/// A login has a minute to go from the provider to the exchange, and a
/// person ten minutes to sign in at the provider.
const CODE_TTL_MS: i64 = 60 * 1000;
#[cfg_attr(not(feature = "server"), allow(dead_code))]
const PENDING_TTL_MS: i64 = 10 * 60 * 1000;

/// The schema's reads, its type erased: a session live at `ctx.now` with
/// its granted roles, a sign-in by id, and a person's sign-ins.
struct Reads {
    current: fn(&Store, &Ctx) -> Option<SignIn>,
    get: fn(&Store, &str) -> Option<SignIn>,
    of: fn(&Store, &str) -> Vec<SignIn>,
}

fn reads<S: AuthTables>() -> Reads {
    Reads {
        current: |store, ctx| read::<S, _>(store, |db| current::<S>(ctx, db, ())).ok().flatten(),
        get: |store, id| {
            read::<S, _>(store, |db| Ok::<_, ark::Refusal>(S::sign_in(db).get(id.to_string())))
                .ok()
                .flatten()
        },
        of: |store, user| {
            read::<S, _>(store, |db| {
                Ok::<_, ark::Refusal>(S::sign_in(db).by(SignIn::USER).from(user.to_string()).all())
            })
            .unwrap_or_default()
        },
    }
}

/// Everything the routes share. `Arc` it; the same one is what the hub is
/// opened with, so the socket and the routes agree on who is who.
pub struct Auth {
    mode: Mode,
    /// Where this server is reachable from a browser, with no trailing
    /// slash: the provider sends people back to `{public_url}/auth/callback`.
    public_url: String,
    /// Prefixes a redirect may have, besides the loopback ones every server
    /// allows and its own `public_url`.
    redirects: Vec<String>,
    /// What tokens are signed with.
    key: Vec<u8>,
    ttl_ms: i64,
    #[cfg_attr(not(feature = "server"), allow(dead_code))]
    pending: Mutex<HashMap<String, Pending>>,
    codes: Mutex<HashMap<String, Issued>>,
    hub: OnceLock<Box<dyn Hub>>,
    reads: Reads,
    /// `docs/plan-auth.md` The roles the configuration grants, by account
    /// id ([`Auth::with_roles`]), merged at every judging.
    roles: BTreeMap<String, BTreeSet<String>>,
    /// The function a person's first sign-in runs ([`Auth::welcome`]).
    welcome: Option<String>,
}

impl Auth {
    /// Signing in by `mode` at `public_url` to an app of schema `S`,
    /// tokens signed with `key`.
    pub fn new<S: AuthTables>(mode: Mode, public_url: &str, key: &[u8]) -> Self {
        Auth {
            mode,
            public_url: public_url.trim_end_matches('/').to_string(),
            redirects: Vec::new(),
            key: key.to_vec(),
            ttl_ms: DEFAULT_TTL_MS,
            pending: Mutex::new(HashMap::new()),
            codes: Mutex::new(HashMap::new()),
            hub: OnceLock::new(),
            reads: reads::<S>(),
            roles: BTreeMap::new(),
            welcome: None,
        }
    }

    /// [`Auth::new`] with the key kept in `data` ([`key_file`]).
    pub fn open<S: AuthTables>(mode: Mode, public_url: &str, data: &Path) -> std::io::Result<Self> {
        Ok(Auth::new::<S>(mode, public_url, &key_file(&data.join("session.key"))?))
    }

    /// How long a session lasts from issue.
    pub fn with_ttl(mut self, ttl_ms: i64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    /// `docs/plan-auth.md` Grant roles from the configuration: role → the
    /// account ids holding it, as `services.harken.roles` says them.
    pub fn with_roles<R: Into<String>, A: Into<String>>(mut self, roles: impl IntoIterator<Item = (R, Vec<A>)>) -> Self {
        for (role, accounts) in roles {
            let role = role.into();
            for a in accounts {
                self.roles.entry(a.into()).or_default().insert(role.clone());
            }
        }
        self
    }

    /// `roles` joined by those the configuration grants `account`, sorted,
    /// once each.
    fn merged(&self, account: &str, roles: &[String]) -> Vec<String> {
        let mut all: BTreeSet<String> = roles.iter().cloned().collect();
        all.extend(self.roles.get(account).into_iter().flatten().cloned());
        all.into_iter().collect()
    }

    /// Run the domain's `func` when a person signs in for the first time:
    /// once their first sign-in row is written, the authority authors it on
    /// a [`crate::Welcome`] — what an app gives everybody, made in one
    /// place rather than by whichever device is first to look. Two first
    /// sign-ins at once both run it, so it makes what it makes once
    /// whatever the count. A peer the server runs itself ([`Auth::own`]) is
    /// not welcomed.
    pub fn welcome(mut self, func: &str) -> Self {
        self.welcome = Some(func.to_string());
        self
    }

    /// Let a login code go back to anything starting with `prefix` — an
    /// app's URL scheme, `harken://`, or a web client served elsewhere.
    pub fn allow_redirect(mut self, prefix: &str) -> Self {
        self.redirects.push(prefix.to_string());
        self
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Whether a code may be sent to `url`. Loopback is always fine — it is
    /// how a desktop program listens, and nothing off the machine can be
    /// there — and so is this server's own origin, where the browser client
    /// it serves lives.
    pub fn redirect_allowed(&self, url: &str) -> bool {
        // The origin, not a prefix: `https://h.example.evil/` starts with
        // `https://h.example` and is somebody else's.
        if let Some(rest) = url.strip_prefix(&self.public_url) {
            if rest.is_empty() || rest.starts_with(['/', '?', '#']) {
                return true;
            }
        }
        if let Some(rest) = url.strip_prefix("http://") {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
            if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
                return true;
            }
        }
        self.redirects.iter().any(|p| url.starts_with(p))
    }

    /// The hub every sign-in's row is written to and every token judged
    /// over. Once; a sign-in before it is refused.
    pub fn serve(&self, hub: Box<dyn Hub>) {
        let _ = self.hub.set(hub);
    }

    fn ask<T: Send + 'static>(&self, f: impl FnOnce(&Store) -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        self.hub.get()?.read(Box::new(move |s| drop(tx.send(f(s)))));
        rx.recv().ok()
    }

    fn mac(&self, id: &str) -> Hmac<sha2::Sha256> {
        let mut m = Hmac::<sha2::Sha256>::new_from_slice(&self.key).expect("an HMAC takes any key");
        m.update(id.as_bytes());
        m
    }

    /// The token for session `id`: `id.hmac(key, id)`, hex.
    pub fn mint(&self, id: &str) -> String {
        format!("{id}.{}", hex(&self.mac(id).finalize().into_bytes()))
    }

    /// The session a token names, when this server signed it.
    pub fn verify<'t>(&self, token: &'t str) -> Option<&'t str> {
        let (id, sig) = token.rsplit_once('.')?;
        let sig = unhex(sig)?;
        self.mac(id).verify_slice(&sig).ok().map(|_| id)
    }

    /// Who session `id` proves over `store` at `now`: its row live, its
    /// person holding the roles it was opened with, those the role table
    /// grants and those the configuration does.
    pub fn judge(&self, store: &Store, id: &str, now: i64) -> Option<Who> {
        let s = (self.reads.current)(store, &at(id, now))?;
        Some(Who {
            roles: self.merged(&s.user, &s.roles),
            actor: s.user,
            session: id.into(),
        })
    }

    /// A token judged: signed by this server, and [`Auth::judge`]d over the
    /// hub's store. Asked on the caller's thread, of the hub's.
    pub fn authenticate(self: &Arc<Self>) -> Authenticate {
        let auth = self.clone();
        Arc::new(move |token| {
            let id = auth.verify(token)?.to_string();
            let a = auth.clone();
            auth.ask(move |store| a.judge(store, &id, now_ms())).flatten()
        })
    }

    /// The hub's [`Watch`]: at a tick, every connected session whose row is
    /// no longer live; at an entry, those of its changed sign-ins no longer
    /// live or gone, and every session of a person whose roles changed, to
    /// be judged again. A connection no row names is not a sign-in's (one
    /// the server's process connected itself) and is left be.
    pub fn watcher(self: &Arc<Self>) -> Watch {
        let auth = self.clone();
        Box::new(move |store, changes, open, now| {
            let (mut ids, mut users) = (BTreeSet::new(), BTreeSet::new());
            for c in changes.unwrap_or_default() {
                let row = match c {
                    Change::Add { row, .. } | Change::Remove { row, .. } | Change::Edit { new: row, .. } => row,
                };
                match c.table() {
                    t if t == SignIn::TABLE => ids.extend(SignIn::from_bytes(row).ok().map(|s| s.id)),
                    t if t == Role::TABLE => users.extend(Role::from_bytes(row).ok().map(|r| r.user)),
                    _ => {}
                }
            }
            let ended = open.iter().filter(|w| {
                let row = || (auth.reads.get)(store, &w.session);
                users.contains(&w.actor)
                    || match changes {
                        None => row().is_some_and(|s| !live(&s, now)),
                        Some(_) => ids.contains(&w.session) && row().is_none_or(|s| !live(&s, now)),
                    }
            });
            let ended: BTreeSet<String> = ended.map(|w| w.session.clone()).collect();
            ended.into_iter().collect()
        })
    }

    /// The login of row `s` for `account`.
    fn login(&self, account: &Account, s: SignIn) -> Login {
        Login {
            token: self.mint(&s.id),
            user: Account {
                roles: self.merged(&account.id, &account.roles),
                ..account.clone()
            },
            session: s.id,
            expires_ms: s.expires_ms,
        }
    }

    /// Sign `account` in on `device`, with no code and no browser: the
    /// session row written, and the login.
    pub fn issue(&self, account: &Account, device: &str) -> Result<Login, String> {
        let first = match &self.welcome {
            Some(_) => {
                let (user, of) = (account.id.clone(), self.reads.of);
                self.ask(move |store| of(store, &user).is_empty()).unwrap_or(false)
            }
            None => false,
        };
        let login = self.write_sign_in(account, device)?;
        if let (true, Some(func)) = (first, &self.welcome) {
            let hub = self.hub.get().ok_or("no hub writes sessions here")?;
            let input = crate::Welcome {
                user: account.id.clone(),
                name: account.name.clone(),
            };
            // A welcome refused is not a sign-in refused: the person is in.
            if let Err(e) = hub.author(func, input.to_bytes()) {
                eprintln!("ark-auth: {func} for {}: {e}", account.id);
            }
        }
        Ok(login)
    }

    /// The session row written, and the login; nobody welcomed.
    fn write_sign_in(&self, account: &Account, device: &str) -> Result<Login, String> {
        let hub = self.hub.get().ok_or("no hub writes sessions here")?;
        let issued_ms = now_ms();
        let row = SignIn {
            id: random_id(),
            user: account.id.clone(),
            name: account.name.clone(),
            email: account.email.clone(),
            device: device.into(),
            issued_ms,
            expires_ms: issued_ms + self.ttl_ms,
            revoked: false,
            roles: account.roles.clone(),
        };
        let c = Change::Add {
            table: SignIn::TABLE.into(),
            row: row.to_bytes(),
        };
        hub.write(c).map_err(|e| format!("could not record the session: {e}"))?;
        Ok(self.login(account, row))
    }

    /// What a peer the server runs itself signs in with (a scanner
    /// authoring what it finds): its live session on `device` from an
    /// earlier start with a day or more left, minted again — a restart
    /// writes no entry — or a new one.
    pub fn own(&self, account: &Account, device: &str) -> Result<Login, String> {
        let (user, of, now) = (account.id.clone(), self.reads.of, now_ms());
        let day = 24 * 60 * 60 * 1000;
        let earlier = self.ask(move |store| of(store, &user)).unwrap_or_default();
        match earlier.into_iter().rfind(|s| s.device == device && live(s, now + day)) {
            Some(s) => Ok(self.login(account, s)),
            None => self.write_sign_in(account, device),
        }
    }

    /// Sign `account` in and mint the code the redirect carries.
    pub fn finish(&self, account: &Account, device: &str) -> Result<String, String> {
        let login = self.issue(account, device)?;
        let code = random_token();
        let now = now_ms();
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes.retain(|_, c| now - c.issued_ms < CODE_TTL_MS);
        codes.insert(code.clone(), Issued { login, issued_ms: now });
        Ok(code)
    }

    /// The login a code stands for, once.
    pub fn redeem(&self, code: &str) -> Option<Login> {
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        let issued = codes.remove(code)?;
        (now_ms() - issued.issued_ms < CODE_TTL_MS).then_some(issued.login)
    }
}

/// A context as session `id` at `now`: what judging reads under.
fn at(id: &str, now: i64) -> Ctx {
    Ctx {
        id: EntryId::of(id, 0),
        session: id.into(),
        now,
        ..Default::default()
    }
}

/// The key kept in `path`: read, or 32 bytes from the OS written there
/// (readable by its owner alone) when there is none.
pub fn key_file(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind, Write};
    match std::fs::read_to_string(path) {
        Ok(text) => unhex(text.trim())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| Error::other(format!("{}: not a hex key", path.display()))),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let key = random_token();
            std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
            let mut f = std::fs::OpenOptions::new();
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut f, 0o600);
            f.write(true).create_new(true).open(path)?.write_all(key.as_bytes())?;
            Ok(unhex(&key).expect("hex"))
        }
        Err(e) => Err(e),
    }
}

/// Dev auth's reading of a name: `name:role,role` is `name` holding those
/// roles, trimmed, empty ones dropped, sorted and once each.
pub fn dev_account(name: &str) -> Account {
    let (id, roles) = name.split_once(':').unwrap_or((name, ""));
    let roles: BTreeSet<String> = roles.split(',').map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect();
    Account {
        id: id.trim().into(),
        name: id.trim().into(),
        email: String::new(),
        roles: roles.into_iter().collect(),
    }
}

/// 128 bits from the OS, as hex: a session id, which entries carry.
pub fn random_id() -> String {
    random_hex::<16>()
}

/// 256 bits from the OS, as hex.
pub fn random_token() -> String {
    random_hex::<32>()
}

fn random_hex<const N: usize>() -> String {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("the OS gives no randomness");
    hex(&b)
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
