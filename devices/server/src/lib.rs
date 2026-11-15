//! devices' server: ark-peer's hub over devices' domain (devices-core),
//! signed in to through the household's OpenID provider, and holding what
//! the household's config declares ([`config`]). Phones sync their person's
//! policy from it and report to it; a push wakes the ones not connected when
//! the rows change (ark-peer's hub, through UnifiedPush).

pub mod config;
pub mod enroll;

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use ark_auth::server::Auth;
use ark_peer::Running;

/// Where the server listens unless told otherwise.
pub const LISTEN: &str = "127.0.0.1:8790";
/// The scheme the phones' sign-in comes back on (`devices://auth`).
pub const PHONE_SCHEME: &str = "devices://";

/// What the server is told: ark-peer's settings under `DEVICES_`, and the
/// household's policies.
pub struct Config {
    pub serve: ark_peer::Serve,
    /// The household's policies and who each applies to (`DEVICES_POLICIES`,
    /// a JSON file Nix writes); none, the store is left as it is.
    pub policies: Option<PathBuf>,
}

impl Deref for Config {
    type Target = ark_peer::Serve;
    fn deref(&self) -> &ark_peer::Serve {
        &self.serve
    }
}

impl Config {
    /// `ark_peer::Serve`'s, under `DEVICES_` (`DEVICES_DATA`,
    /// `DEVICES_OIDC_ISSUER`, `DEVICES_OIDC_CLIENT_ID`,
    /// `DEVICES_OIDC_CLIENT_SECRET_FILE`, `DEVICES_PUBLIC_URL`,
    /// `DEVICES_ROLES`, `DEVICES_DEV_AUTH`, …), and `DEVICES_POLICIES`.
    pub fn from_env(listen: &str) -> Result<Config> {
        Config::from_vars(listen, |k| std::env::var(k).ok())
    }

    /// [`Config::from_env`] over any lookup; blank is unset.
    pub fn from_vars(listen: &str, var: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let serve = ark_peer::Serve::from_vars("DEVICES", listen, &var)?;
        let policies = var("DEVICES_POLICIES").filter(|v| !v.trim().is_empty()).map(PathBuf::from);
        Ok(Config { serve, policies })
    }
}

/// The server running.
pub struct Server {
    pub running: Running,
}

impl Server {
    pub async fn stop(self) {
        self.running.stop().await
    }
}

/// What this build runs: its domain's schema hash and version.
pub fn version() -> String {
    let d = devices_core::shared();
    format!("{} {}", d.schema_hash(), d.version)
}

/// Build the server from `config`, listen, and hold the household's config.
pub async fn start(config: Config) -> Result<Server> {
    std::fs::create_dir_all(&config.data).with_context(|| format!("creating {}", config.data.display()))?;
    eprintln!("devices-server: data in {}", config.data.display());
    let declared = match &config.policies {
        Some(p) => Some(config::Declared::read(p)?),
        None => None,
    };
    // The roles the configuration grants, and the config's own admin, by
    // construction.
    let roles = (config.roles.iter().map(|(r, a)| (r.clone(), a.clone()))).chain([(ark::auth::ADMIN.to_string(), vec![config::ACCOUNT.to_string()])]);
    let mode = config.sign_in.mode().await?;
    eprintln!("devices-server: {}", mode.announce());
    let mut auth = Auth::open::<devices_core::Devices>(mode, &config.public_url(), &config.data)
        .context("the session key")?
        .allow_redirect(PHONE_SCHEME)
        .with_roles(roles);
    for prefix in &config.redirects {
        auth = auth.allow_redirect(prefix);
    }
    let auth = Arc::new(auth);
    let mut hub = ark_peer::Config {
        keep: Some(config.retain.entries),
        ..ark_peer::Config::of(devices_core::shared())
    };
    if config.scoped {
        hub.feed = Arc::new(|_| ark::peer::Feed::Scoped("session.feed".into()));
    }
    // A phone not connected when the rows change is woken by push (its
    // session's endpoint, UnifiedPush through the household's distributor),
    // at most once a minute: it syncs and holds itself to the policy.
    let sender = Arc::new(ark_push::send::Sender::new(None));
    let mut server = ark_peer::Server::new(hub).auth(auth.clone()).push(sender, ark_push::Wake::of("devices"));
    (server.data, server.keepalive) = (Some(config.data.clone()), config.keepalive.every);
    let (hub, router) = server.build()?;
    let running = Running::listen(hub.clone(), router, &config.listen).await?;
    eprintln!("devices-server: listening on http://{} ({})", running.addr, version());
    match declared {
        Some(d) => {
            let (auth, hub) = (auth.clone(), hub.clone());
            let made = tokio::task::spawn_blocking(move || config::hold(&d, &auth, &hub)).await?;
            match made {
                Ok(n) => eprintln!("devices-server: the household's policies held ({n} changes)"),
                Err(e) => eprintln!("devices-server: the household's policies not held: {e:#}"),
            }
        }
        None => eprintln!("devices-server: no DEVICES_POLICIES: the policies are left as they are"),
    }
    Ok(Server { running })
}
