//! harken's server, on `ark-server`.
//!
//! Almost nothing here is about sync, and that is the point: the log, the
//! socket at `/sync`, `/healthz`, the sign-in routes, `/media` and the web
//! build's validator are `ark_server`'s and `ark_auth`'s, generic over any
//! app. What is harken's is the four things only harken has, each a module a
//! test can reach:
//!
//! - [`library`]: the media directory as a peer — walked, watched, and
//!   authored as `add_song`;
//! - [`listening`]: one audio session per account, as a live room;
//! - [`assistant`]: the house's speakers, as devices in those sessions;
//! - and [`start`], which is the wiring: a [`Config`] read from the
//!   environment (what the NixOS module sets) and the server built from it.
//!
//! It signs people in, because the engine holds every entry to who pushed
//! it and somebody has to say who that is. The server is the only OpenID
//! Connect client: it holds the secret, talks to the provider, and hands
//! each peer a session token of its own — so the desktop, the browser and
//! the phone all sign in the same way. Without a provider it refuses to
//! start, unless told it is on a laptop (`HARKEN_DEV_AUTH=1`).
//!
//! Nothing here assumes a peer arrived signed in. A client used before
//! anybody signed in authors as nobody and dials nothing; when somebody
//! signs in, it re-stamps that work as theirs and pushes it under the new
//! login — which the engine takes like any other entry. An entry authored
//! under an *older* login of the same person is taken too: `.auth(..)` hands
//! the engine `Auth::owns`.

pub mod assistant;
pub mod library;
pub mod listening;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use ark_auth::oidc::Provider;
use ark_auth::server::{Auth, Mode};
use ark_auth::session::SessionStore;
use ark_auth::Account;
use ark_client::Domain;
use ark_server::Running;

use assistant::ha;
use library::Scanner;
use listening::Desk;

/// The phone's URL scheme: a login may always send its code back there.
pub const PHONE_SCHEME: &str = "harken://";

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

/// Everything the server is told, from the environment or a test.
#[derive(Clone, Debug)]
pub struct Config {
    /// `host:port`; port 0 takes an ephemeral one.
    pub listen: String,
    /// The log, the rooms' kept sessions, the sign-in sessions and the
    /// scanner's replica.
    pub data: PathBuf,
    /// An `.ark` module to host instead of harken's own: functions whose
    /// hashes this build holds run natively, the rest interpreted.
    pub module: Option<PathBuf>,
    /// Where a browser reaches this server — the proxy's address behind one.
    /// The provider sends people back under it, and the page it serves
    /// signs in against it. `http://{listen}` when unset.
    pub public_url: Option<String>,
    pub sign_in: SignIn,
    /// Where else a login may send its code, besides loopback, the phone's
    /// scheme and `public_url`.
    pub redirects: Vec<String>,
    /// The media root: `music/` under it is scanned, all of it is served at
    /// `/media`.
    pub media: Option<PathBuf>,
    /// A built browser client to serve at `/`.
    pub web: Option<PathBuf>,
    /// The file whose change is a rebuild off the store
    /// (`pkg/harken_iced_bg.wasm`); unset, every file is.
    pub web_module: Option<String>,
    /// The house, if there is one.
    pub house: Option<ha::Config>,
}

impl Config {
    /// A dev server on `listen` keeping its data in `data`, and nothing else.
    pub fn dev(listen: &str, data: &Path) -> Config {
        Config {
            listen: listen.into(),
            data: data.into(),
            module: None,
            public_url: None,
            sign_in: SignIn::Dev,
            redirects: vec![],
            media: None,
            web: None,
            web_module: None,
            house: None,
        }
    }

    /// Read the environment: what the NixOS module sets.
    ///
    /// ```text
    /// HARKEN_DATA                   where the state lives (default: the temp dir)
    /// HARKEN_MODULE                 an .ark to host instead of harken's own
    /// HARKEN_PUBLIC_URL             where a browser reaches this server
    /// HARKEN_DEV_AUTH=1             anyone is whoever they say
    /// HARKEN_OIDC_ISSUER, HARKEN_OIDC_CLIENT_ID, HARKEN_OIDC_CLIENT_SECRET_FILE
    ///                               all three, or none; HARKEN_OIDC_SCOPES
    /// HARKEN_REDIRECTS              comma-separated prefixes
    /// HARKEN_MEDIA                  the media root
    /// HARKEN_WEB, HARKEN_WEB_MODULE the browser client
    /// HARKEN_HA_URL, HARKEN_HA_TOKEN_FILE, HARKEN_HA_PLAYERS
    ///                               all three, or none; HARKEN_HA_MEDIA
    /// ```
    pub fn from_env(listen: &str) -> Result<Config> {
        Config::from_vars(listen, |k| std::env::var(k).ok())
    }

    /// [`Config::from_env`] over any lookup; blank is unset.
    pub fn from_vars(listen: &str, var: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let env = |k: &str| var(k).filter(|v| !v.trim().is_empty());
        let sign_in = match (env("HARKEN_OIDC_ISSUER"), env("HARKEN_OIDC_CLIENT_ID"), env("HARKEN_OIDC_CLIENT_SECRET_FILE")) {
            (Some(issuer), Some(client_id), Some(file)) => SignIn::Oidc {
                issuer,
                client_id,
                secret_file: file.into(),
                scopes: env("HARKEN_OIDC_SCOPES")
                    .unwrap_or_else(|| "openid profile email".into())
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
            },
            (None, None, None) if env("HARKEN_DEV_AUTH").is_some() => SignIn::Dev,
            (None, None, None) => bail!(
                "nobody can sign in: set HARKEN_OIDC_ISSUER, HARKEN_OIDC_CLIENT_ID and \
                 HARKEN_OIDC_CLIENT_SECRET_FILE, or HARKEN_DEV_AUTH=1 on a laptop"
            ),
            _ => bail!("HARKEN_OIDC_ISSUER, HARKEN_OIDC_CLIENT_ID and HARKEN_OIDC_CLIENT_SECRET_FILE go together; set all three"),
        };
        let house = house(&env)?;
        Ok(Config {
            listen: listen.into(),
            data: env("HARKEN_DATA")
                .map_or_else(|| std::env::temp_dir().join("harken-server"), PathBuf::from),
            module: env("HARKEN_MODULE").map(PathBuf::from),
            public_url: env("HARKEN_PUBLIC_URL"),
            sign_in,
            redirects: env("HARKEN_REDIRECTS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
            media: env("HARKEN_MEDIA").map(PathBuf::from),
            web: env("HARKEN_WEB").map(PathBuf::from),
            web_module: env("HARKEN_WEB_MODULE"),
            house,
        })
    }

    pub fn public_url(&self) -> String {
        self.public_url
            .clone()
            .unwrap_or_else(|| format!("http://{}", self.listen))
    }
}

/// The house, from the environment, or nothing.
///
/// All three together or none of them, the way the OpenID Connect three
/// are: half a configuration is a server that starts and quietly does not do
/// the thing it was configured for. The token is a *file* for the reason the
/// client secret is one.
fn house(env: &impl Fn(&str) -> Option<String>) -> Result<Option<ha::Config>> {
    match (
        env("HARKEN_HA_URL"),
        env("HARKEN_HA_TOKEN_FILE"),
        env("HARKEN_HA_PLAYERS"),
    ) {
        (Some(url), Some(file), Some(players)) => {
            let token = std::fs::read_to_string(&file)
                .with_context(|| format!("cannot read HARKEN_HA_TOKEN_FILE {file}"))?
                .trim()
                .to_string();
            if token.is_empty() {
                bail!("HARKEN_HA_TOKEN_FILE {file} is empty");
            }
            let players = players_of(&players);
            if players.is_empty() {
                bail!("HARKEN_HA_PLAYERS names no players");
            }
            // Where a *speaker* fetches from, which is not necessarily where
            // a phone does: the phone may be on a public address while the
            // speaker only knows one on the LAN.
            let media = env("HARKEN_HA_MEDIA")
                .or_else(|| env("HARKEN_PUBLIC_URL"))
                .ok_or_else(|| {
                    anyhow!("HARKEN_HA_MEDIA: speakers need an address they can fetch bytes from")
                })?;
            Ok(Some(ha::Config {
                url,
                token,
                players,
                media,
                tick: ha::TICK,
            }))
        }
        (None, None, None) => Ok(None),
        _ => bail!(
            "HARKEN_HA_URL, HARKEN_HA_TOKEN_FILE and HARKEN_HA_PLAYERS go together; set all three"
        ),
    }
}

/// `media_player.kitchen=Kitchen,media_player.study` — a name after an `=`
/// when the entity id is not what you would call it out loud, and a guess
/// from the id when it is.
pub fn players_of(list: &str) -> Vec<(String, String)> {
    list.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((id, name)) => (id.trim().to_string(), name.trim().to_string()),
            None => (p.to_string(), pretty(p)),
        })
        .collect()
}

/// `media_player.the_kitchen` as `The kitchen`. A guess, overridden by
/// writing the name out — but a picker full of entity ids is a picker for
/// somebody who already knows what is in their house.
pub fn pretty(entity: &str) -> String {
    let tail = entity
        .rsplit('.')
        .next()
        .unwrap_or(entity)
        .replace('_', " ");
    let mut chars = tail.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => entity.to_string(),
    }
}

/// The module this server hosts.
pub fn domain(module: Option<&Path>) -> Result<Domain> {
    match module {
        None => Ok(Domain::new(&harken_domain::module())),
        Some(p) => {
            let bytes =
                std::fs::read(p).with_context(|| format!("reading the module {}", p.display()))?;
            Domain::from_bytes(&bytes, harken_domain::module().procedures())
                .map_err(|e| anyhow!("the module {}: {e}", p.display()))
        }
    }
}

/// A server that is up, and the things running beside it.
pub struct Server {
    pub running: Running,
    pub auth: Arc<Auth>,
    pub domain: Domain,
    pub scanner: Option<Scanner>,
    pub house: Option<ha::Assistant>,
}

impl Server {
    pub fn url(&self) -> String {
        self.running.url()
    }

    /// Stop the scanner and the bridge, then the listener.
    pub async fn stop(self) {
        let Server {
            running,
            scanner,
            house,
            ..
        } = self;
        // Both join threads that talk to the hub; neither may block a worker.
        let _ = tokio::task::spawn_blocking(move || {
            drop(scanner);
            drop(house);
        })
        .await;
        running.stop().await;
    }
}

/// How people sign in, made real: the provider's discovery is one blocking
/// request, once.
async fn mode(sign_in: &SignIn) -> Result<Mode> {
    Ok(match sign_in {
        SignIn::Dev => Mode::Dev,
        SignIn::Oidc {
            issuer,
            client_id,
            secret_file,
            scopes,
        } => {
            let (issuer, client_id, file, scopes) = (
                issuer.clone(),
                client_id.clone(),
                secret_file.clone(),
                scopes.clone(),
            );
            let provider = tokio::task::spawn_blocking(move || {
                let scopes: Vec<&str> = scopes.iter().map(String::as_str).collect();
                Provider::discover_with_secret_file(&issuer, &client_id, &file, &scopes)
            })
            .await?
            .map_err(|e| anyhow!("signing in: {e}"))?;
            Mode::Oidc(provider)
        }
    })
}

/// Build the server from `config`, listen, and start the scanner and the
/// bridge beside it.
pub async fn start(config: Config) -> Result<Server> {
    let domain = domain(config.module.as_deref())?;
    std::fs::create_dir_all(&config.data)
        .with_context(|| format!("creating {}", config.data.display()))?;
    eprintln!("harken-server: data in {}", config.data.display());

    // Sessions beside the log, in their own file: they are not part of the
    // log and never leave this machine.
    let sessions = SessionStore::open(config.data.join("sessions.json"))
        .map_err(|e| anyhow!("the sessions: {e}"))?;
    let public_url = config.public_url();
    let mut auth =
        Auth::new(sessions, mode(&config.sign_in).await?, &public_url).allow_redirect(PHONE_SCHEME);
    for prefix in &config.redirects {
        auth = auth.allow_redirect(prefix);
    }
    let auth = Arc::new(auth);

    // What each account is listening to, and where: the hub's live machine,
    // on the same socket as the log, in a room per account, held in memory.
    // The bridge's watch is registered *before* the desk is handed over,
    // because after that it belongs to the hub.
    let mut desk = Desk::new();
    let rooms = config.house.as_ref().map(|_| {
        let (tx, rx) = std::sync::mpsc::channel();
        desk.watch(tx);
        rx
    });

    let mut builder = ark_server::builder(domain.clone())
        .name("harken-server")
        .data(&config.data)
        .auth(auth.clone())
        .live(desk);
    // One root for every kind rather than one per kind, because `file` is
    // on the kind-neutral side of the schema: the scanner writes each
    // track's path relative to this directory, and `/media/` serves that
    // same path back — with range requests, which is what makes seeking
    // work — and with no authentication at all, which is what lets a
    // speaker fetch it.
    if let Some(dir) = &config.media {
        builder = builder.media(dir);
    }
    // Everything the routes above do not match falls through to the page,
    // with the build as its validator: a store path cannot be one, since nix
    // dates every file in it 1970.
    if let Some(dir) = &config.web {
        builder = builder.web(dir);
        if let Some(m) = &config.web_module {
            builder = builder.web_module(m);
        }
    }
    let app = builder.build()?;
    let hub = app.hub.clone();
    let running = app.serve(&config.listen).await?;

    // Every `media_player` named becomes a device in every listening
    // session: it stands in each room as a peer of the live channel, which
    // is how it is a device rather than a client.
    let house = match (config.house.clone(), rooms) {
        (Some(house), Some(rooms)) => {
            eprintln!("harken-server: the house: {house}");
            Some(ha::start(house, hub.clone(), rooms))
        }
        _ => None,
    };

    // The scanner signs in like everything else, as the account `library`.
    let scanner = match &config.media {
        Some(dir) => {
            let login = auth
                .issue(&Account {
                    id: library::ACCOUNT.into(),
                    name: "Library".into(),
                    email: String::new(),
                })
                .map_err(|e| anyhow!("signing the scanner in: {e}"))?;
            Some(Scanner::start(
                dir.clone(),
                domain.clone(),
                hub.clone(),
                config.data.join("library"),
                login,
            )?)
        }
        None => None,
    };

    eprintln!("harken-server: one listening session per account, on the sync socket");
    match &config.media {
        Some(dir) => eprintln!(
            "harken-server: media from {} (music in music/)",
            dir.display()
        ),
        // Said out loud: a server with no media looks exactly like one whose
        // directory is misconfigured.
        None => eprintln!("harken-server: no media: set HARKEN_MEDIA to a directory"),
    }
    match &config.web {
        Some(dir) => eprintln!(
            "harken-server: browser client on {public_url}/ from {}",
            dir.display()
        ),
        None => eprintln!("harken-server: no browser client: set HARKEN_WEB to a built one"),
    }

    Ok(Server {
        running,
        auth,
        domain,
        scanner,
        house,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn a_server_nobody_can_sign_in_to_does_not_start() {
        let e = Config::from_vars("x:1", vars(&[])).unwrap_err().to_string();
        assert!(e.contains("nobody can sign in"), "{e}");
        let c = Config::from_vars("x:1", vars(&[("HARKEN_DEV_AUTH", "1")])).unwrap();
        assert_eq!(c.sign_in, SignIn::Dev);
        assert_eq!(c.public_url(), "http://x:1");
        // Half a provider is a mistake, even beside dev auth.
        let e = Config::from_vars(
            "x:1",
            vars(&[
                ("HARKEN_DEV_AUTH", "1"),
                ("HARKEN_OIDC_ISSUER", "https://auth"),
            ]),
        )
        .unwrap_err();
        assert!(e.to_string().contains("go together"), "{e}");
        let c = Config::from_vars(
            "x:1",
            vars(&[
                ("HARKEN_OIDC_ISSUER", "https://auth"),
                ("HARKEN_OIDC_CLIENT_ID", "harken"),
                (
                    "HARKEN_OIDC_CLIENT_SECRET_FILE",
                    "/run/credentials/harken/oidc-secret",
                ),
                ("HARKEN_REDIRECTS", " https://a/ ,, https://b/"),
                ("HARKEN_PUBLIC_URL", "https://harken.example.com"),
            ]),
        )
        .unwrap();
        assert_eq!(
            c.sign_in,
            SignIn::Oidc {
                issuer: "https://auth".into(),
                client_id: "harken".into(),
                secret_file: "/run/credentials/harken/oidc-secret".into(),
                scopes: vec!["openid".into(), "profile".into(), "email".into()],
            }
        );
        assert_eq!(c.redirects, ["https://a/", "https://b/"]);
        assert_eq!(c.public_url(), "https://harken.example.com");
    }

    #[test]
    fn the_house_is_all_three_or_none_and_the_token_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("ha-token");
        std::fs::write(&token, "sekrit\n").unwrap();
        let token = token.to_str().unwrap();
        let dev = ("HARKEN_DEV_AUTH", "1");
        let e = Config::from_vars("x:1", vars(&[dev, ("HARKEN_HA_URL", "http://ha")])).unwrap_err();
        assert!(e.to_string().contains("go together"), "{e}");
        let three = [
            dev,
            ("HARKEN_HA_URL", "http://ha:8123"),
            ("HARKEN_HA_TOKEN_FILE", token),
            (
                "HARKEN_HA_PLAYERS",
                "media_player.the_kitchen, media_player.study=Study",
            ),
        ];
        let e = Config::from_vars("x:1", vars(&three)).unwrap_err();
        assert!(
            e.to_string().contains("HARKEN_HA_MEDIA"),
            "a speaker needs somewhere to fetch from: {e}"
        );
        let mut with_media = three.to_vec();
        with_media.push(("HARKEN_HA_MEDIA", "http://10.0.0.2:8787"));
        let house = Config::from_vars("x:1", vars(&with_media))
            .unwrap()
            .house
            .unwrap();
        assert_eq!(house.token, "sekrit");
        assert_eq!(house.media, "http://10.0.0.2:8787");
        assert_eq!(
            house.players,
            [
                (
                    "media_player.the_kitchen".to_string(),
                    "The kitchen".to_string()
                ),
                ("media_player.study".to_string(), "Study".to_string())
            ]
        );
        assert!(
            !house.to_string().contains("sekrit"),
            "the token is never printed"
        );
        // The public URL stands in for a speaker's when nothing else is said.
        let mut public = three.to_vec();
        public.push(("HARKEN_PUBLIC_URL", "http://10.0.0.2:8787"));
        assert_eq!(
            Config::from_vars("x:1", vars(&public))
                .unwrap()
                .house
                .unwrap()
                .media,
            "http://10.0.0.2:8787"
        );
        let mut missing = with_media.clone();
        missing[2] = ("HARKEN_HA_TOKEN_FILE", "/nonexistent/ha-token");
        assert!(Config::from_vars("x:1", vars(&missing)).is_err());
    }
}
