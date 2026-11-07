//! harken's server: an `ark-peer` that listens.
//!
//! Almost nothing here is about sync, and that is the point: the log and
//! the listening session's volatile space on the hub, the socket at
//! `/sync`, `/healthz`, the sign-in routes and the web build's validator
//! are `ark_peer`'s and `ark_auth`'s, generic over any app; the domain
//! is `harken/domain`'s ArkLang, called through the bindings `build.rs`
//! generates ([`domain`]). What is harken's is the things only harken has,
//! each a module a test can reach:
//!
//! - [`library`]: the media directory, the feeds and the catalogue as a
//!   worker of the server's, its work jobs — walked, watched, read,
//!   downloaded, heard, and
//!   authored as `library.add_song` and the rest;
//! - [`covers`]: where a track's cover comes from, and `/media`;
//! - [`assistant`]: the house's speakers, as devices in each account's
//!   listening session — which is the domain's live state
//!   (`harken/domain/listening.ark`), sequenced by the hub's volatile
//!   authority, and needs nothing of this crate;
//! - [`domain`]: the domain's bindings and the host's Rust beside them;
//! - [`hosted`]: the domain the hub runs, harken's own, as this build compiled it;
//! - and [`start`], which is the wiring: a [`Config`] read from the
//!   environment (what the NixOS module sets) and the server built from it.
//!
//! It signs people in, because the hub holds every entry to who pushed it
//! and somebody has to say who that is. The server is the only OpenID
//! Connect client: it holds the secret, talks to the provider, and hands
//! each peer a session token of its own — so the desktop, the browser and
//! the phone all sign in the same way. Without a provider it refuses to
//! start, unless told it is on a laptop (`HARKEN_DEV_AUTH=1`).
//!
//! A device is a session (`harken/domain/devices.ark`): a connection's
//! volatile `Hello` says what the device is, and the hub authors `on_join`
//! with it, its user and id the verified login's ([`device`]).

pub mod assistant;
pub mod captions;
pub mod catalogue;
pub mod chapters;
pub mod covers;
pub mod domain;
pub mod feeds;
pub mod hosted;
pub mod library;
pub mod speaker;
pub mod sync;
pub mod transcribe;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::Running;

use assistant::ha;
use hosted::Hosted;
use library::Scanner;

/// The phone's URL scheme: a login may always send its code back there.
pub const PHONE_SCHEME: &str = "harken://";

pub use ark_peer::SignIn;

/// The chapterers tried when the server is told none.
pub const DEFAULT_CHAPTERS: &str = "outline,tiling";

/// Everything the server is told, from the environment or a test: any
/// server's (`ark_peer::Serve`, read under `HARKEN_`, which it derefs to)
/// and harken's own beside it.
#[derive(Clone, Debug)]
pub struct Config {
    pub serve: ark_peer::Serve,
    /// The media root: `music/` under it is scanned, all of it is served at
    /// `/media`.
    pub media: Option<PathBuf>,
    /// A catalogue to import (`catalogue.rs`): the URL of its JSON.
    pub catalogue: Option<String>,
    /// What hears the episodes here (`transcribe.rs`): a program run as
    /// `program <audio> <captions.vtt>`.
    pub transcriber: Option<PathBuf>,
    /// Who makes chapters of what was heard in an episode with none
    /// (`chapters.rs`), in the order to try them: `outline,tiling`.
    pub chapters: String,
    /// ffmpeg, which makes a speaker's copy of a file more than it plays
    /// (`speaker.rs`): `ffmpeg` on the path unless told.
    pub ffmpeg: PathBuf,
    /// The house, if there is one.
    pub house: Option<ha::Config>,
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
    /// A dev server on `listen` keeping its data in `data`, and nothing else.
    pub fn dev(listen: &str, data: &Path) -> Config {
        Config {
            serve: ark_peer::Serve::dev(listen, data),
            media: None,
            catalogue: None,
            transcriber: None,
            chapters: DEFAULT_CHAPTERS.into(),
            ffmpeg: "ffmpeg".into(),
            house: None,
        }
    }

    /// Read the environment: what the NixOS module sets. Every
    /// `ark_peer::Serve` reads, under `HARKEN_` (`HARKEN_DATA`,
    /// `HARKEN_OIDC_*`, `HARKEN_KEEPALIVE_MS`, `HARKEN_ROLES`, …), and
    /// harken's own:
    ///
    /// ```text
    /// HARKEN_MEDIA                  the media root
    /// HARKEN_CATALOGUE              a catalogue's JSON URL, imported
    ///                               (with HARKEN_MEDIA)
    /// HARKEN_TRANSCRIBER            a program writing an episode's captions,
    ///                               `program <audio> <out.vtt>` (with
    ///                               HARKEN_MEDIA)
    /// HARKEN_CHAPTERS               the chapterers to try on an episode
    ///                               with none, in order (`outline,tiling`;
    ///                               `none`)
    /// HARKEN_HA_URL, HARKEN_HA_TOKEN_FILE, HARKEN_HA_PLAYERS
    ///                               all three, or none; HARKEN_HA_MEDIA
    /// HARKEN_BRIDGE_TRACE=1         a line per poll per speaker: what the
    ///                               house said and what the bridge did
    /// HARKEN_FFMPEG                 ffmpeg, for a speaker's copy of a file
    ///                               it cannot play (`ffmpeg` on the path)
    /// ```
    pub fn from_env(listen: &str) -> Result<Config> {
        Config::from_vars(listen, |k| std::env::var(k).ok())
    }

    /// [`Config::from_env`] over any lookup; blank is unset.
    pub fn from_vars(listen: &str, var: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let serve = ark_peer::Serve::from_vars("HARKEN", listen, &var)?;
        let env = |k: &str| var(k).filter(|v| !v.trim().is_empty());
        Ok(Config {
            serve,
            media: env("HARKEN_MEDIA").map(PathBuf::from),
            catalogue: env("HARKEN_CATALOGUE"),
            transcriber: env("HARKEN_TRANSCRIBER").map(PathBuf::from),
            chapters: env("HARKEN_CHAPTERS").unwrap_or_else(|| DEFAULT_CHAPTERS.into()),
            ffmpeg: env("HARKEN_FFMPEG")
                .unwrap_or_else(|| "ffmpeg".into())
                .into(),
            house: house(&env)?,
        })
    }
}

/// `docs/plan-auth.md` `library=alice,bob;admin=alice`, as role →
/// accounts: what `services.harken.roles` writes from an attrset.
pub fn roles_of(text: &str) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    ark_peer::settings::roles_of("HARKEN_ROLES", text)
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
                lower: None,
                tick: ha::TICK,
                idle: ha::IDLE,
                trace: match env("HARKEN_BRIDGE_TRACE").is_some_and(|v| v != "0") {
                    true => ha::Trace::Out,
                    false => ha::Trace::Off,
                },
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

/// What the house's sign-in of a listener says it is.
const HOUSE: &str = "the house's speakers";

/// A server that is up, and the things running beside it.
pub struct Server {
    pub running: Running,
    pub auth: Arc<Auth>,
    pub hosted: Hosted,
    pub scanner: Option<Scanner>,
    pub house: Option<ha::Assistant>,
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://{}", self.running.addr)
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

/// Build the server from `config`, listen, and start the scanner and the
/// bridge beside it.
pub async fn start(config: Config) -> Result<Server> {
    let hosted = Hosted::harken();
    // A sign-in is a row of the log, its token signed with a key kept in
    // the data directory (`ark_auth::server`). `docs/plan-auth.md` The
    // roles the configuration grants; the scanner's own `library` is its
    // account's (`library::Scanner::start`): the scanner is the library,
    // and the role is what the library's mutations are guarded by
    // (`is_library`).
    let auth = config.auth::<crate::domain::Harken>(PHONE_SCHEME).await?;
    tracing::info!("data in {}", config.data.display());
    // A person's Favorites, made once, here, at their first sign-in.
    let auth = Arc::new(auth.welcome("playlists.welcome"));
    let public_url = config.public_url();
    let mut server = config.server(
        harken_player::config::hub(&hosted.domain, None),
        auth.clone(),
    );
    if config.keepalive != ark_peer::settings::KEEPALIVE {
        tracing::info!(
            "the sync socket pings every {}ms and gives up after 3 unanswered",
            config.keepalive.as_millis()
        );
    }
    // One root for every kind rather than one per kind, because `file` is
    // on the kind-neutral side of the schema: the scanner writes each
    // track's path relative to this directory, and `/media/` serves that
    // same path back — with range requests, which is what makes seeking
    // work — and with no authentication at all, which is what lets a
    // speaker fetch it. Under it, `/media/.art/` is the pictures the
    // scanner took out of files, from the data directory: the media root
    // may be read-only, and a cover is spelt the way a file is, so every
    // client joins it the same way (`covers`).
    //
    // What is more than a speaker plays is copied at what it does
    // (`speaker.rs`), served at `/media/.speaker/` and handed to the house.
    let mut lower = None;
    if let Some(dir) = &config.media {
        let art = library::art_dir(&config.data);
        tracing::info!(
            "serving {} at /media, and {} at /media/{}, unauthenticated",
            dir.display(),
            art.display(),
            covers::ART
        );
        let speaker = Arc::new(speaker::Lower::new(
            dir,
            &config.data,
            config.ffmpeg.clone(),
        ));
        server.routes = covers::routes(
            dir,
            &art,
            &catalogue::dir(&config.data),
            &feeds::dir(&config.data),
        )
        .merge(speaker::routes(speaker.clone()));
        lower = Some(speaker);
    }
    // Everything the routes above do not match falls through to the page
    // (`config.web`), with the build as its validator: a store path cannot
    // be one, since nix dates every file in it 1970.
    let running = server.listen(&config.listen).await?;
    let hub = running.hub.clone();
    tracing::info!("listening on http://{}", running.addr);

    // Every `media_player` named becomes a device in every listening
    // session somebody is in: a connection of its own in that session,
    // which is how it is a device rather than a client.
    // What plays on a speaker is its listener's: their plays and their
    // place in an episode are written as them, by a sign-in of theirs the
    // house holds. It carries no name, so they are named as their own
    // sign-ins name them.
    let house = config.house.clone().map(|mut house| {
        house.lower = lower.clone();
        tracing::info!("the house: {house}");
        let (auth, workers) = (auth.clone(), hub.clone());
        let sign_in: ha::SignIn = Arc::new(move |user: &str| {
            let account = Account {
                id: user.into(),
                ..Account::default()
            };
            workers.worker(
                &auth,
                &account,
                HOUSE,
                Box::new(ark_peer::Memory::default()),
            )
        });
        ha::start(house, hub.clone(), hosted.clone(), sign_in)
    });

    // The scanner signs in like everything else, as the account `library`.
    let scanner = match &config.media {
        Some(dir) => Some(Scanner::start(
            dir.clone(),
            &hub,
            &auth,
            config.data.clone(),
            config.catalogue.clone(),
            config.transcriber.clone(),
            chapters::chain(&config.chapters),
        )?),
        None => None,
    };

    tracing::info!("one listening session per account, a volatile space on the sync socket");
    match &config.media {
        Some(dir) => tracing::info!(
            "media from {} (music in music/, podcasts/ and sermons/ beside it; feeds followed from the clients)",
            dir.display()
        ),
        // Said out loud: a server with no media looks exactly like one whose
        // directory is misconfigured.
        None => tracing::warn!("no media: set HARKEN_MEDIA to a directory"),
    }
    match &config.web {
        Some(dir) => tracing::info!("browser client on {public_url}/ from {}", dir.display()),
        None => tracing::warn!("no browser client: set HARKEN_WEB to a built one"),
    }

    Ok(Server {
        running,
        auth,
        hosted,
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
                (
                    "HARKEN_PICTURE_HOSTS",
                    "photos.example, photos.example:8443",
                ),
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
        assert_eq!(c.picture_hosts, ["photos.example", "photos.example:8443"]);
        assert_eq!(c.keepalive, ark_peer::settings::KEEPALIVE);
        assert_eq!(c.public_url(), "https://harken.example.com");
    }

    /// The keepalive a test shortens, and nothing a typo could make of it.
    /// Falsified by ignoring `HARKEN_KEEPALIVE_MS`: the interval was 20 s.
    #[test]
    fn the_keepalive_is_the_hubs_unless_told() {
        let dev = ("HARKEN_DEV_AUTH", "1");
        let c = Config::from_vars("x:1", vars(&[dev, ("HARKEN_KEEPALIVE_MS", "250")])).unwrap();
        assert_eq!(c.keepalive, std::time::Duration::from_millis(250));
        for bad in [("HARKEN_KEEPALIVE_MS", "0"), ("HARKEN_KEEPALIVE_MS", "1s")] {
            assert!(
                Config::from_vars("x:1", vars(&[dev, bad])).is_err(),
                "{bad:?}"
            );
        }
    }

    /// `docs/plan-auth.md` `HARKEN_ROLES` is role, `=`, the accounts, `;` to
    /// the next — what `services.harken.roles` writes — and unset is no
    /// role; something that is not that refuses to start rather than
    /// granting nothing quietly. Falsified by reading the accounts as one
    /// (no split on `,`): bob held nothing.
    #[test]
    fn roles_are_read_role_by_role() {
        let dev = ("HARKEN_DEV_AUTH", "1");
        let c = Config::from_vars(
            "x:1",
            vars(&[dev, ("HARKEN_ROLES", "library=alice, bob;admin=alice")]),
        )
        .unwrap();
        let want: std::collections::BTreeMap<String, Vec<String>> = [
            ("admin".to_string(), vec!["alice".to_string()]),
            (
                "library".to_string(),
                vec!["alice".to_string(), "bob".to_string()],
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(c.roles, want);
        assert!(Config::from_vars("x:1", vars(&[dev]))
            .unwrap()
            .roles
            .is_empty());
        assert!(Config::from_vars("x:1", vars(&[dev, ("HARKEN_ROLES", "library")])).is_err());
        assert!(Config::from_vars("x:1", vars(&[dev, ("HARKEN_ROLES", "=alice")])).is_err());
    }

    /// The entries the log keeps are 10,000 unless told, and nothing a typo
    /// could make of them. Falsified by ignoring `HARKEN_RETAIN_ENTRIES`:
    /// the count is 10,000.
    #[test]
    fn retention_is_the_engines_unless_told() {
        let dev = ("HARKEN_DEV_AUTH", "1");
        let c = Config::from_vars("x:1", vars(&[dev])).unwrap();
        assert_eq!(c.retain, 10_000);
        let c = Config::from_vars("x:1", vars(&[dev, ("HARKEN_RETAIN_ENTRIES", " 50 ")])).unwrap();
        assert_eq!(c.retain, 50);
        let bad = ("HARKEN_RETAIN_ENTRIES", "1e4");
        assert!(Config::from_vars("x:1", vars(&[dev, bad])).is_err());
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
        assert!(
            matches!(house.trace, ha::Trace::Off),
            "the bridge's trace is off unless asked for"
        );
        let mut traced = with_media.clone();
        traced.push(("HARKEN_BRIDGE_TRACE", "1"));
        assert!(matches!(
            Config::from_vars("x:1", vars(&traced))
                .unwrap()
                .house
                .unwrap()
                .trace,
            ha::Trace::Out
        ));
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
