//! money's server: an `ark-peer` that listens, as harken's is.
//!
//! The log on the hub, the socket at `/sync`, `/healthz`, the sign-in
//! routes under `/auth` and the browser client at `/` are `ark_peer`'s and
//! `ark_auth`'s, generic over any app; the domain is `money/domain`'s, in
//! plain Rust. What is money's is the bank importer ([`importer`]: Plaid
//! over [`plaid`], inside the server) and [`start`], the wiring: a
//! [`Config`] read from the environment (what the NixOS module sets) and the
//! server built from it.
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
//! only `banks.import` asks for it.

pub mod importer;
pub mod plaid;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::Running;

use importer::{Banks, Importer, Mapping};

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
    ///                         and neither imports nothing
    /// MONEY_PLAID_ENV         production (the default) or sandbox
    /// MONEY_PLAID_URL         Plaid elsewhere: a mock's address
    /// MONEY_PLAID_ITEMS       each bank and its access token's file:
    ///                         `chase=/run/credentials/money.service/plaid-chase,…`
    /// MONEY_PLAID_HISTORY     days of history at a bank's first pull (90)
    /// MONEY_ACCOUNTS          the household's names for the banks' accounts,
    ///                         as JSON: `{"Chase Checking": {"item": "chase",
    ///                         "mask": "1234"}, …}` (`accountId` for a mask
    ///                         two share)
    /// ```
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
    let (id, secret) = match (env("MONEY_PLAID_CLIENT_ID_FILE"), env("MONEY_PLAID_SECRET_FILE")) {
        (Some(id), Some(secret)) => (id, secret),
        (None, None) if env("MONEY_PLAID_ITEMS").is_some() => {
            bail!("MONEY_PLAID_ITEMS names banks, and there are no Plaid keys: set MONEY_PLAID_CLIENT_ID_FILE and MONEY_PLAID_SECRET_FILE")
        }
        (None, None) => return Ok(None),
        _ => bail!("MONEY_PLAID_CLIENT_ID_FILE and MONEY_PLAID_SECRET_FILE go together; set both"),
    };
    let url = match (env("MONEY_PLAID_URL"), env("MONEY_PLAID_ENV")) {
        (Some(url), _) => url,
        (None, None) => plaid::base_of("production"),
        (None, Some(e)) if e == "production" || e == "sandbox" => plaid::base_of(&e),
        (None, Some(e)) => bail!("MONEY_PLAID_ENV is production or sandbox, not {e}"),
    };
    let mut items = BTreeMap::new();
    for part in env("MONEY_PLAID_ITEMS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let Some((name, file)) = part.split_once('=') else {
            bail!("MONEY_PLAID_ITEMS: `{part}` is not bank=token-file");
        };
        let (name, file) = (name.trim(), file.trim());
        if name.is_empty() || file.is_empty() {
            bail!("MONEY_PLAID_ITEMS: `{part}` is not bank=token-file");
        }
        items.insert(name.to_string(), PathBuf::from(file));
    }
    let accounts: BTreeMap<String, Mapping> = match env("MONEY_ACCOUNTS") {
        Some(json) => serde_json::from_str(&json).map_err(|e| anyhow!("MONEY_ACCOUNTS is not the accounts as JSON: {e}"))?,
        None => BTreeMap::new(),
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
    let banks = Banks {
        url,
        client_id_file: id.into(),
        secret_file: secret.into(),
        items,
        accounts,
        history,
    };
    banks.check()?;
    Ok(Some(banks))
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
    let running = Running::listen(hub.clone(), router, &config.listen).await?;
    eprintln!("money-server: listening on http://{}", running.addr);
    match &config.web {
        Some(dir) => eprintln!("money-server: browser client on {public_url}/ from {}", dir.display()),
        None => eprintln!("money-server: no browser client: set MONEY_WEB to a built one"),
    }

    let importer = match config.banks.clone() {
        Some(banks) => {
            eprintln!(
                "money-server: importing {} bank{} from {}",
                banks.items.len(),
                if banks.items.len() == 1 { "" } else { "s" },
                banks.url
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
        let m: BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
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

    /// The importer's configuration: the keys both or neither, banks only
    /// with keys, the environment's URL unless a mock's is given, and the
    /// accounts as the module writes them. Falsified by reading
    /// `MONEY_PLAID_ENV` after `MONEY_PLAID_URL`: the mock's address lost.
    #[test]
    fn the_importer_is_told_its_banks() {
        let keys = [DEV, ("MONEY_PLAID_CLIENT_ID_FILE", "/c/id"), ("MONEY_PLAID_SECRET_FILE", "/c/secret")];
        let b = Config::from_vars("x:1", vars(&keys)).unwrap().banks.unwrap();
        assert_eq!((b.url.as_str(), b.items.len(), b.history), ("https://production.plaid.com", 0, 90));

        let mut more = keys.to_vec();
        more.extend([
            ("MONEY_PLAID_ENV", "sandbox"),
            ("MONEY_PLAID_ITEMS", " chase=/c/plaid-chase , amex=/c/plaid-amex"),
            ("MONEY_PLAID_HISTORY", "30"),
            (
                "MONEY_ACCOUNTS",
                r#"{"Chase Checking":{"item":"chase","mask":"0000"},"Card":{"item":"amex","accountId":"x9","mask":null}}"#,
            ),
        ]);
        let b = Config::from_vars("x:1", vars(&more)).unwrap().banks.unwrap();
        assert_eq!(b.url, "https://sandbox.plaid.com");
        assert_eq!(b.items["chase"], PathBuf::from("/c/plaid-chase"));
        assert_eq!(b.history, 30);
        assert_eq!(b.accounts["Card"].account_id.as_deref(), Some("x9"));
        assert!(b.accounts["Chase Checking"].is("chase", "whatever", "0000"));
        assert!(!b.accounts["Chase Checking"].is("amex", "whatever", "0000"));
        assert!(b.accounts["Card"].is("amex", "x9", ""));

        more.push(("MONEY_PLAID_URL", "http://127.0.0.1:9"));
        assert_eq!(Config::from_vars("x:1", vars(&more)).unwrap().banks.unwrap().url, "http://127.0.0.1:9");

        for bad in [
            vec![DEV, ("MONEY_PLAID_CLIENT_ID_FILE", "/c/id")],
            vec![DEV, ("MONEY_PLAID_ITEMS", "chase=/c/t")],
            [keys.to_vec(), vec![("MONEY_PLAID_ENV", "development")]].concat(),
            [keys.to_vec(), vec![("MONEY_PLAID_ITEMS", "chase")]].concat(),
            [keys.to_vec(), vec![("MONEY_PLAID_HISTORY", "0")]].concat(),
            [keys.to_vec(), vec![("MONEY_ACCOUNTS", r#"{"X":{"item":"chase","mask":"1"}}"#)]].concat(),
            [
                keys.to_vec(),
                vec![("MONEY_PLAID_ITEMS", "chase=/c/t"), ("MONEY_ACCOUNTS", r#"{"X":{"item":"chase"}}"#)],
            ]
            .concat(),
        ] {
            assert!(Config::from_vars("x:1", vars(&bad)).is_err(), "{bad:?}");
        }
    }
}
