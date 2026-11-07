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

pub mod browser;
pub mod costco;
pub mod importer;
pub mod paper;
pub mod photos;
pub mod plaid;
pub mod setup;
pub mod store;
pub mod stores;
pub mod vault;

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::{Running, ServerConfig};

use importer::{Banks, Importer};

pub use ark_peer::SignIn;
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
    /// The stores' receipts: `amazon-orders`, Costco, and a folder of exports.
    pub stores: stores::Stores,
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
            stores: stores::Stores::default(),
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
    /// MONEY_AMAZON_ORDERS     the amazon-orders program: Amazon's orders,
    ///                         and how they were charged, each run ([`stores`])
    /// MONEY_AMAZON_ARGS       what it is run with for the orders
    ///                         (`orders --last-30-days --full-details -o json`)
    /// MONEY_AMAZON_TRANSACTIONS_ARGS
    ///                         and for the charges (`transactions --days 60 -o json`)
    /// MONEY_AMAZON_USERNAME_FILE, MONEY_AMAZON_PASSWORD_FILE
    ///                         its sign-in, a file each; neither, and it is the
    ///                         Settings'
    /// MONEY_AMAZON_OTP_SECRET_FILE
    ///                         the one-time code's secret, when the account
    ///                         has one
    /// MONEY_BROWSER           a browser's DevTools address (`http://host:port`):
    ///                         Costco's receipts are read in it, and Amazon's
    ///                         sign-in challenge is run in it ([`browser`])
    /// MONEY_BROWSER_PAGE      the page where people see that browser and sign in
    /// MONEY_COSTCO_DAYS       how far back Costco's receipts are asked for (90)
    /// MONEY_RECEIPT_OCR       receipt-ocr, which reads a paper receipt's photo ([`paper`])
    /// MONEY_RECEIPTS_DIR      a folder of exports: `<store>/*.json`
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
        Ok(Config {
            serve,
            banks: banks(&env)?,
            stores: stores::Stores::from_env(&env)?,
        })
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
    // The roles the configuration grants, and the importer's own `bank`,
    // by construction: asked at every connection and stamped on every
    // entry that connection pushes.
    let auth = config.auth::<money_domain::Money>(PHONE_SCHEME).await?;
    eprintln!("money-server: data in {}", config.data.display());
    eprintln!("money-server: {}", auth.mode().announce());
    let auth = Arc::new(auth.with_roles([(money_domain::BANK, vec![importer::ACCOUNT])]));
    let public_url = config.public_url();

    // The receipts folder anybody's exports wait in: the server's own
    // unless told another.
    let mut stores = config.stores.clone();
    stores.dir.get_or_insert_with(|| config.data.join("receipts"));
    let importer_slot = Arc::new(std::sync::Mutex::new(None));
    let hub = ServerConfig {
        // Settings' stores, called over the socket and kept off the log.
        calls: setup::calls(setup::Setup {
            data: config.data.clone(),
            amazon_program: stores.amazon.is_some(),
            costco: stores.costco.clone(),
            ocr: stores.ocr.clone(),
            importer: importer_slot.clone(),
        }),
        ..ServerConfig::of(money_domain::shared())
    };
    let running = config.server(hub, auth.clone()).listen(&config.listen).await?;
    let hub = running.hub.clone();
    eprintln!("money-server: listening on http://{}", running.addr);

    match &config.web {
        Some(dir) => eprintln!("money-server: browser client on {public_url}/ from {}", dir.display()),
        None => eprintln!("money-server: no browser client: set MONEY_WEB to a built one"),
    }

    match &config.banks {
        Some(banks) => eprintln!(
            "money-server: importing from {}, the banks kept in {}",
            banks.url,
            store::file(&config.data).display()
        ),
        // Said out loud: a server with no keys looks exactly like one whose
        // keys are misconfigured.
        None => eprintln!("money-server: no bank importer: set MONEY_PLAID_CLIENT_ID_FILE and MONEY_PLAID_SECRET_FILE"),
    }
    if let Some(a) = &config.stores.amazon {
        eprintln!("money-server: importing Amazon's orders with {}", a.program.display());
    }
    if let Some(d) = &config.stores.dir {
        eprintln!("money-server: importing receipts dropped in {}", d.display());
    }
    let importer = match config.banks.is_some() || stores.any() {
        true => {
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
            Some(Importer::start(
                config.banks.clone(),
                stores.clone(),
                hub.clone(),
                config.data.clone(),
                login,
            )?)
        }
        false => None,
    };

    if let Some(i) = &importer {
        *importer_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(i.handle());
    }

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
