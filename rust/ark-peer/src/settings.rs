//! What a server is told (§1.11; the `server` feature): where it listens
//! and keeps its log, how people sign in, where a browser reaches it, its
//! web build, its keepalive and retention, who holds which role, and its
//! admin page — read from the environment under an app's prefix
//! ([`Serve::from_env`]: `HARKEN_DATA`, `HARKEN_OIDC_ISSUER`, …), so that
//! what is any app's server is configured once, here, and an app reads
//! only what is its own beside it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};

/// How people sign in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignIn {
    /// Anyone is whoever they say. A laptop, and nowhere else.
    Dev,
    /// Through an OpenID Connect provider, with the client secret in a file
    /// — a systemd credential — so it is never in a process listing, a unit
    /// file or the store.
    Oidc {
        issuer: String,
        client_id: String,
        secret_file: PathBuf,
        scopes: Vec<String>,
    },
}

/// How often the sync socket pings, and how many may go unanswered before
/// a connection is closed: three, and only three ([`Serve::from_vars`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keepalive {
    pub every: Duration,
    pub missed: u32,
}

impl Default for Keepalive {
    fn default() -> Keepalive {
        Keepalive {
            every: Duration::from_secs(20),
            missed: 3,
        }
    }
}

/// How much of the log the server keeps: `entries` above the snapshot the
/// log stands on (`Config::keep`); a device further behind is sent the
/// snapshot and rebases onto it. `days` is read and checked, and kept
/// nothing by: the horizon is a count of entries, not a device's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    pub entries: usize,
    pub days: u64,
}

impl Default for Retention {
    fn default() -> Retention {
        Retention { entries: 10_000, days: 30 }
    }
}

/// Everything any app's server is told.
#[derive(Clone, Debug)]
pub struct Serve {
    /// `host:port`; port 0 takes an ephemeral one.
    pub listen: String,
    /// The log (sign-ins among its rows), the kept live state, the key
    /// tokens are signed with, and whatever else the app keeps.
    pub data: PathBuf,
    /// Whether every connection is fed `session.feed` (`_SCOPED=1`): its
    /// person's sign-ins and roles only. Off, every connection holds the log
    /// whole.
    pub scoped: bool,
    /// Where a browser reaches this server — the proxy's address behind one.
    /// The provider sends people back under it, and the page it serves
    /// signs in against it. `http://{listen}` when unset.
    pub public_url: Option<String>,
    pub sign_in: SignIn,
    /// Where else a login may send its code, besides loopback, an app's
    /// own scheme and `public_url`.
    pub redirects: Vec<String>,
    /// A built browser client to serve at `/`.
    pub web: Option<PathBuf>,
    /// The file whose change is a rebuild off the store; unset, every file is.
    pub web_module: Option<String>,
    /// How often the sync socket pings (`_KEEPALIVE_MS`).
    pub keepalive: Keepalive,
    /// How much of the log the server keeps (`_RETAIN_ENTRIES`, 10,000;
    /// `_RETAIN_DAYS`, 30, read and unused: [`Retention`]).
    pub retain: Retention,
    /// Who holds which role: role → account ids (`_ROLES`). Asked at every
    /// connection, so a restart with another configuration grants or
    /// revokes at once.
    pub roles: BTreeMap<String, Vec<String>>,
    /// Where the admin page listens (`_ADMIN_BIND`; none without it).
    pub admin_bind: Option<String>,
    /// The admin page's static build (`_ADMIN_WEB`).
    pub admin_web: Option<PathBuf>,
}

impl Serve {
    /// A dev server on `listen` keeping its data in `data`, and nothing else.
    pub fn dev(listen: &str, data: &Path) -> Serve {
        Serve {
            listen: listen.into(),
            data: data.into(),
            scoped: false,
            public_url: None,
            sign_in: SignIn::Dev,
            redirects: vec![],
            web: None,
            web_module: None,
            keepalive: Keepalive::default(),
            retain: Retention::default(),
            roles: BTreeMap::new(),
            admin_bind: None,
            admin_web: None,
        }
    }

    /// Read the environment under `prefix` (`HARKEN`):
    ///
    /// ```text
    /// _DATA                     where the state lives (default: the temp dir)
    /// _SCOPED=1                 each connection fed its person's sign-ins only
    /// _PUBLIC_URL               where a browser reaches this server
    /// _DEV_AUTH=1               anyone is whoever they say
    /// _OIDC_ISSUER, _OIDC_CLIENT_ID, _OIDC_CLIENT_SECRET_FILE
    ///                           all three, or none; _OIDC_SCOPES
    /// _REDIRECTS                comma-separated prefixes
    /// _WEB, _WEB_MODULE         the browser client
    /// _KEEPALIVE_MS             the sync socket's ping interval (20000)
    /// _KEEPALIVE_MISSED         pings unanswered before it closes (3, and only 3)
    /// _RETAIN_DAYS              read and checked; kept nothing by
    /// _RETAIN_ENTRIES           entries kept above the snapshot (10000)
    /// _ROLES                    who holds which role: `library=alice,bob;admin=alice`
    /// _ADMIN_BIND               where the admin page listens (none without it)
    /// _ADMIN_WEB                the admin page's build
    /// ```
    pub fn from_env(prefix: &str, listen: &str) -> Result<Serve> {
        Serve::from_vars(prefix, listen, |k| std::env::var(k).ok())
    }

    /// [`Serve::from_env`] over any lookup; blank is unset.
    pub fn from_vars(prefix: &str, listen: &str, var: impl Fn(&str) -> Option<String>) -> Result<Serve> {
        let name = |k: &str| format!("{prefix}_{k}");
        let env = |k: &str| var(&name(k)).filter(|v| !v.trim().is_empty());
        let (issuer, client, secret) = (name("OIDC_ISSUER"), name("OIDC_CLIENT_ID"), name("OIDC_CLIENT_SECRET_FILE"));
        let sign_in = match (env("OIDC_ISSUER"), env("OIDC_CLIENT_ID"), env("OIDC_CLIENT_SECRET_FILE")) {
            (Some(issuer), Some(client_id), Some(file)) => SignIn::Oidc {
                issuer,
                client_id,
                secret_file: file.into(),
                scopes: env("OIDC_SCOPES")
                    .unwrap_or_else(|| "openid profile email".into())
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
            },
            (None, None, None) if env("DEV_AUTH").is_some() => SignIn::Dev,
            (None, None, None) => bail!(
                "nobody can sign in: set {issuer}, {client} and {secret}, or {}=1 on a laptop",
                name("DEV_AUTH")
            ),
            _ => bail!("{issuer}, {client} and {secret} go together; set all three"),
        };
        let mut keepalive = Keepalive::default();
        if let Some(ms) = env("KEEPALIVE_MS") {
            let ms: u64 = (ms.trim().parse().ok().filter(|n| *n > 0))
                .ok_or_else(|| anyhow!("{} is milliseconds, more than none: {ms}", name("KEEPALIVE_MS")))?;
            keepalive.every = Duration::from_millis(ms);
        }
        if let Some(n) = env("KEEPALIVE_MISSED") {
            let k = name("KEEPALIVE_MISSED");
            keepalive.missed = n.trim().parse().map_err(|_| anyhow!("{k} is a count: {n}"))?;
            if keepalive.missed != 3 {
                bail!("{k}: the sync socket closes after 3 unanswered pings, not {n}");
            }
        }
        let mut retain = Retention::default();
        if let Some(d) = env("RETAIN_DAYS") {
            retain.days = d.trim().parse().map_err(|_| anyhow!("{} is a count of days: {d}", name("RETAIN_DAYS")))?;
        }
        if let Some(n) = env("RETAIN_ENTRIES") {
            retain.entries = (n.trim().parse()).map_err(|_| anyhow!("{} is a count of entries: {n}", name("RETAIN_ENTRIES")))?;
        }
        let list = |k: &str| -> Vec<String> {
            let v = env(k).unwrap_or_default();
            v.split(',').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect()
        };
        Ok(Serve {
            listen: listen.into(),
            data: env("DATA").map_or_else(|| std::env::temp_dir().join(format!("{}-server", prefix.to_lowercase())), PathBuf::from),
            scoped: env("SCOPED").is_some(),
            public_url: env("PUBLIC_URL"),
            sign_in,
            redirects: list("REDIRECTS"),
            web: env("WEB").map(PathBuf::from),
            web_module: env("WEB_MODULE"),
            keepalive,
            retain,
            roles: roles_of(&name("ROLES"), &env("ROLES").unwrap_or_default())?,
            admin_bind: env("ADMIN_BIND"),
            admin_web: env("ADMIN_WEB").map(PathBuf::from),
        })
    }

    pub fn public_url(&self) -> String {
        self.public_url.clone().unwrap_or_else(|| format!("http://{}", self.listen))
    }
}

/// `library=alice,bob;admin=alice` — each role, then the account ids
/// holding it — as role → accounts, `var` naming where it was read in a
/// refusal; empty is no role at all.
pub fn roles_of(var: &str, text: &str) -> Result<BTreeMap<String, Vec<String>>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((role, accounts)) = part.split_once('=') else {
            bail!("{var}: `{part}` is not role=account,account");
        };
        let role = role.trim();
        if role.is_empty() {
            bail!("{var}: a role with no name in `{part}`");
        }
        let accounts = accounts.split(',').map(str::trim).filter(|a| !a.is_empty());
        out.entry(role.to_string()).or_default().extend(accounts.map(str::to_string));
    }
    Ok(out)
}
