//! money's server: an `ark-peer` that listens, as harken's is.
//!
//! The log on the hub, the socket at `/sync`, `/healthz`, the sign-in
//! routes under `/auth` and the browser client at `/` are `ark_peer`'s and
//! `ark_auth`'s, generic over any app; the domain is `money/domain`'s, in
//! plain Rust. What is money's is the bank importer ([`importer`]: Plaid
//! over [`plaid`], inside the server, the banks' logins in [`store`]) and
//! [`start`], the wiring: a [`Config`] read from the environment (what the
//! NixOS module sets) and the server built from it.
//!
//! **Everything to keep is in the data directory** (`MONEY_DATA`): the log
//! (the budget and the sign-ins), the session key, and `banks.json`, the
//! banks the household connected and their logins. A backup of that
//! directory, restored here or on another machine with the same Plaid
//! keys, is the whole server: every bank still connected.
//!
//! **One budget, the household's.** Whoever the OpenID provider lets in
//! shares it: every connection is fed every row (`money_domain::feed`), and
//! the server is the only OpenID Connect client — it holds the secret,
//! talks to the provider, and hands each peer a session of its own, so the
//! desktop, the browser and the phones all sign in the same way: to
//! `/auth/login?redirect=…`, back with a one-minute code, and the code
//! traded at `/auth/exchange` for a login. A phone's redirect is its
//! scheme, [`PHONE_SCHEME`]. Without a provider the server refuses to start,
//! unless told it is on a laptop (`MONEY_DEV_AUTH=1`).
//!
//! **One role**, `bank`, held by the importer's own sign-in (the account
//! [`importer::ACCOUNT`]) and by nobody else unless `MONEY_ROLES` says so:
//! `banks.import` asks for it, and so do the importer's answers about
//! connections (`banks.answer`, `banks.failed`).

pub mod importer;
pub mod plaid;
pub mod store;

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::Running;

use importer::{Banks, Importer};

pub use ark_peer::{Keepalive, Retention, SignIn};
/// The phones' URL scheme: a login may always send its code back there.
pub const PHONE_SCHEME: &str = "money://";

/// Where the server listens unless told: harken's port, and one more.
pub const LISTEN: &str = "127.0.0.1:8788";

/// Days of a bank's history asked for the first time it is pulled.
pub const HISTORY: u32 = 90;

/// Everything the server is told, from the environment or a test: any
/// server's (`ark_peer::Serve`, read under `MONEY_`, which it derefs to)
/// and the importer's beside it.
#[derive(Clone, Debug)]
pub struct Config {
    pub serve: ark_peer::Serve,
    /// The banks, when the server holds Plaid's keys.
    pub banks: Option<Banks>,
}

impl std::ops::Deref for Config {
    type Target = ark_peer::Serve;
    fn deref(&self) -> &ark_peer::Serve {
        &self.serve
    }
}

impl std::ops::DerefMut for Config {
    fn deref_mut(&mut self) -> &mut ark_peer::Serve {
        &mut self.serve
    }
}

impl Config {
    /// A dev server on `listen` keeping its data in `data`, importing nothing.
    pub fn dev(listen: &str, data: &Path) -> Config {
        Config {
            serve: ark_peer::Serve::dev(listen, data),
            banks: None,
        }
    }

    /// Read the environment: what the NixOS module sets. Every
    /// `ark_peer::Serve` reads, under `MONEY_` (`MONEY_DATA`,
    /// `MONEY_OIDC_*`, `MONEY_PUBLIC_URL`, `MONEY_WEB`, `MONEY_ROLES`, …),
    /// and the importer's:
    ///
    /// ```text
    /// MONEY_PLAID_CLIENT_ID_FILE, MONEY_PLAID_SECRET_FILE
    ///                         Plaid's keys, a file each; both or neither,
    ///                         and neither runs no importer
    /// MONEY_PLAID_ENV         production (the default) or sandbox
    /// MONEY_PLAID_URL         Plaid elsewhere: a mock's address
    /// MONEY_PLAID_HISTORY     days of history at a bank's first pull (90)
    /// ```
    ///
    /// The banks themselves are not configuration: they are connected from
    /// the app and kept in the data directory ([`store`]).
    pub fn from_env(listen: &str) -> Result<Config> {
        Config::from_vars(listen, |k| std::env::var(k).ok())
    }

    /// [`Config::from_env`] over any lookup; blank is unset.
    pub fn from_vars(listen: &str, var: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let serve = ark_peer::Serve::from_vars("MONEY", listen, &var)?;
        let env = |k: &str| var(k).filter(|v| !v.trim().is_empty());
        Ok(Config { serve, banks: banks(&env)? })
    }
}

/// The importer's configuration, or none when there are no Plaid keys.
fn banks(env: &impl Fn(&str) -> Option<String>) -> Result<Option<Banks>> {
    // What a server was told before its banks were connected from the app:
    // said, rather than quietly importing nothing.
    for gone in ["MONEY_PLAID_ITEMS", "MONEY_ACCOUNTS"] {
        if env(gone).is_some() {
            bail!(
                "{gone} is no longer read: banks are connected from the app and kept in the data directory's banks.json \
                 (money/server/src/store.rs says how to move a connection there by hand); unset it"
            );
        }
    }
    let (id, secret) = match (env("MONEY_PLAID_CLIENT_ID_FILE"), env("MONEY_PLAID_SECRET_FILE")) {
        (Some(id), Some(secret)) => (id, secret),
        (None, None) => return Ok(None),
        _ => bail!("MONEY_PLAID_CLIENT_ID_FILE and MONEY_PLAID_SECRET_FILE go together; set both"),
    };
    let url = match (env("MONEY_PLAID_URL"), env("MONEY_PLAID_ENV")) {
        (Some(url), _) => url,
        (None, None) => plaid::base_of("production"),
        (None, Some(e)) if e == "production" || e == "sandbox" => plaid::base_of(&e),
        (None, Some(e)) => bail!("MONEY_PLAID_ENV is production or sandbox, not {e}"),
    };
    let history = match env("MONEY_PLAID_HISTORY") {
        Some(d) => d
            .trim()
            .parse()
            .ok()
            .filter(|d| (1..=730).contains(d))
            .ok_or_else(|| anyhow!("MONEY_PLAID_HISTORY is a count of days, 1 to 730: {d}"))?,
        None => HISTORY,
    };
    Ok(Some(Banks {
        history,
        ..Banks::new(&url, id.into(), secret.into())
    }))
}

/// What this build runs, in a word: its domain's schema hash and version,
/// as a phone's build of the same domain says it (and as harken's server
/// does).
pub fn version() -> String {
    let d = money_domain::shared();
    format!("{} {}", d.schema_hash(), d.version)
}

/// What `/healthz` says, a line each: `ok`; the connections open; the
/// log's head and its horizon; and the domain it runs (`domain <hash>
/// <version> current`), which a phone holds its own build to. Answered in
/// front of `ark-peer`'s own `/healthz`, which says `ok` alone.
pub async fn health(hub: &ark_peer::Handle) -> Option<String> {
    let hub = hub.clone();
    let domain = format!("domain {} current\n", version());
    let read = move || {
        hub.with(move |h| {
            let (connections, log) = (h.connections().count(), h.log.log());
            format!(
                "ok\nconnections {connections}\nhead {}\nhorizon {}\n{domain}",
                log.head().0,
                log.base_seq()
            )
        })
    };
    tokio::task::spawn_blocking(read).await.ok().flatten()
}

async fn healthz(
    axum::extract::State(hub): axum::extract::State<ark_peer::Handle>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if req.uri().path() != "/healthz" {
        return next.run(req).await;
    }
    match health(&hub).await {
        Some(text) => text.into_response(),
        None => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "the hub is gone\n").into_response(),
    }
}

/// A server that is up, and the importer beside it.
pub struct Server {
    pub running: Running,
    pub auth: Arc<Auth>,
    pub importer: Option<Importer>,
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://{}", self.running.addr)
    }

    /// Stop the importer, then the listener.
    pub async fn stop(self) {
        let Server { running, importer, .. } = self;
        // It joins a thread that talks to the hub; that may not block a worker.
        let _ = tokio::task::spawn_blocking(move || drop(importer)).await;
        running.stop().await;
    }
}

/// Build the server from `config`, listen, and start the importer beside it.
pub async fn start(config: Config) -> Result<Server> {
    std::fs::create_dir_all(&config.data).with_context(|| format!("creating {}", config.data.display()))?;
    eprintln!("money-server: data in {}", config.data.display());
    let public_url = config.public_url();

    // The roles the configuration grants, and the importer's own `bank`,
    // by construction: asked at every connection and stamped on every
    // entry that connection pushes.
    let roles =
        (config.roles.iter().map(|(r, a)| (r.clone(), a.clone()))).chain([(money_domain::BANK.to_string(), vec![importer::ACCOUNT.to_string()])]);
    let mode = config.sign_in.mode().await?;
    eprintln!("money-server: {}", mode.announce());
    let mut auth = Auth::open::<money_domain::Money>(mode, &public_url, &config.data)
        .context("the session key")?
        .allow_redirect(PHONE_SCHEME)
        .with_roles(roles);
    for prefix in &config.redirects {
        auth = auth.allow_redirect(prefix);
    }
    for host in &config.picture_hosts {
        auth = auth.trust_picture_host(host);
    }
    let auth = Arc::new(auth);

    let mut hub = ark_peer::Config {
        keep: Some(config.retain.entries),
        ..ark_peer::Config::of(money_domain::shared())
    };
    if config.scoped {
        hub.feed = Arc::new(|_| ark::peer::Feed::Scoped("session.feed".into()));
    }
    let mut server = ark_peer::Server::new(hub).auth(auth.clone());
    (server.data, server.keepalive) = (Some(config.data.clone()), config.keepalive.every);
    if let Some(dir) = &config.web {
        server.web = Some((dir.clone(), config.web_module.clone()));
    }
    let (hub, router) = server.build()?;
    let router = router.layer(axum::middleware::from_fn_with_state(hub.clone(), healthz));
    let running = Running::listen(hub.clone(), router, &config.listen).await?;
    eprintln!("money-server: listening on http://{}", running.addr);
    match &config.web {
        Some(dir) => eprintln!("money-server: browser client on {public_url}/ from {}", dir.display()),
        None => eprintln!("money-server: no browser client: set MONEY_WEB to a built one"),
    }

    let importer = match config.banks.clone() {
        Some(banks) => {
            eprintln!(
                "money-server: importing from {}, the banks kept in {}",
                banks.url,
                store::file(&config.data).display()
            );
            let account = Account {
                id: importer::ACCOUNT.into(),
                name: "Bank".into(),
                roles: vec![money_domain::BANK.into()],
                ..Account::default()
            };
            let auth = auth.clone();
            let login = tokio::task::spawn_blocking(move || auth.own(&account, importer::DEVICE))
                .await?
                .map_err(|e| anyhow!("signing the importer in: {e}"))?;
            Some(Importer::start(banks, hub.clone(), config.data.clone(), login)?)
        }
        // Said out loud: a server with no keys looks exactly like one whose
        // keys are misconfigured.
        None => {
            eprintln!("money-server: no bank importer: set MONEY_PLAID_CLIENT_ID_FILE and MONEY_PLAID_SECRET_FILE");
            None
        }
    };

    Ok(Server { running, auth, importer })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: std::collections::BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    const DEV: (&str, &str) = ("MONEY_DEV_AUTH", "1");

    /// Nobody can sign in to a server with neither a provider nor dev auth,
    /// and the provider's three go together: `ark_peer::Serve`'s rules,
    /// under money's prefix.
    #[test]
    fn a_server_nobody_can_sign_in_to_does_not_start() {
        let e = Config::from_vars("x:1", vars(&[])).unwrap_err().to_string();
        assert!(e.contains("MONEY_OIDC_ISSUER"), "{e}");
        let c = Config::from_vars(
            "x:1",
            vars(&[
                ("MONEY_OIDC_ISSUER", "https://auth"),
                ("MONEY_OIDC_CLIENT_ID", "money"),
                ("MONEY_OIDC_CLIENT_SECRET_FILE", "/run/credentials/money/oidc-secret"),
                ("MONEY_PUBLIC_URL", "https://money.example.com"),
            ]),
        )
        .unwrap();
        assert!(matches!(c.sign_in, SignIn::Oidc { .. }));
        assert_eq!(c.public_url(), "https://money.example.com");
        assert!(c.banks.is_none(), "no keys, no importer");
    }

    /// The importer's configuration: the keys both or neither, the
    /// environment's URL unless a mock's is given, and what a server was
    /// told before its banks were connected from the app refused out loud.
    /// Falsified by reading `MONEY_PLAID_ENV` after `MONEY_PLAID_URL`: the
    /// mock's address lost.
    #[test]
    fn the_importer_is_told_where_plaid_is() {
        let keys = [DEV, ("MONEY_PLAID_CLIENT_ID_FILE", "/c/id"), ("MONEY_PLAID_SECRET_FILE", "/c/secret")];
        let b = Config::from_vars("x:1", vars(&keys)).unwrap().banks.unwrap();
        assert_eq!((b.url.as_str(), b.history), ("https://production.plaid.com", 90));
        assert_eq!(b.client_id_file, std::path::PathBuf::from("/c/id"));

        let mut more = keys.to_vec();
        more.extend([("MONEY_PLAID_ENV", "sandbox"), ("MONEY_PLAID_HISTORY", "30")]);
        let b = Config::from_vars("x:1", vars(&more)).unwrap().banks.unwrap();
        assert_eq!((b.url.as_str(), b.history), ("https://sandbox.plaid.com", 30));

        more.push(("MONEY_PLAID_URL", "http://127.0.0.1:9"));
        assert_eq!(Config::from_vars("x:1", vars(&more)).unwrap().banks.unwrap().url, "http://127.0.0.1:9");

        for bad in [
            vec![DEV, ("MONEY_PLAID_CLIENT_ID_FILE", "/c/id")],
            [keys.to_vec(), vec![("MONEY_PLAID_ENV", "development")]].concat(),
            [keys.to_vec(), vec![("MONEY_PLAID_HISTORY", "0")]].concat(),
        ] {
            assert!(Config::from_vars("x:1", vars(&bad)).is_err(), "{bad:?}");
        }
        for gone in [
            ("MONEY_PLAID_ITEMS", "chase=/c/t"),
            ("MONEY_ACCOUNTS", r#"{"X":{"item":"chase","mask":"1"}}"#),
        ] {
            let e = Config::from_vars("x:1", vars(&[keys.to_vec(), vec![gone]].concat()))
                .unwrap_err()
                .to_string();
            assert!(e.contains("no longer read") && e.contains("banks.json"), "{e}");
        }
    }
}
