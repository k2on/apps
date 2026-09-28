//! harken's sync server, on the `ark` runtime and harken's own domain.
//!
//! The module is `harken_domain::module()` — its bytes are `emit()`, and
//! its procedures run natively — or, with `--module`, an `.ark` file, whose
//! functions are applied natively where this build holds a procedure with
//! the same hash and through the interpreter otherwise. Every scope is
//! hosted as an `ark::peer::Authority` inside one `ark::protocol::Server`. The machine runs
//! on a thread of its own ([`hub`]); axum speaks the protocol to it over a
//! WebSocket at `/sync`, each scope's log is written to disk after every
//! append ([`persist`]), and a media directory is authored into the library
//! by an in-process peer ([`scanner`]).
//!
//! Dev auth only: `ark::protocol::trusting` makes a token a name, and
//! `open_access` lets everyone read every scope. The server says so at
//! startup, loudly, every time.

pub mod hub;
pub mod persist;
pub mod scanner;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use ark::authoring::Procedure;
use ark::canon;
use ark::hash::{closures, Closure, FnHash};
use ark::ir::{module_from_value, Module};
use ark::live::Silent;
use ark::peer::Authority;
use ark::protocol::{open_access, trusting, ClientMsg, Server, ServerMsg};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

use hub::{Hub, HubHandle};

/// The session `ark::protocol::trusting` gives every login. A test that
/// authors as a dev-auth user names it in its `Ctx`.
pub const DEV_SESSION: &str = "dev";

/// The server pings every connection this often…
pub const PING_EVERY: Duration = Duration::from_secs(20);
/// …and closes one that has left this many unanswered.
pub const PINGS_UNANSWERED: u32 = 3;

/// The module, and what the server needs of it: every function's closure
/// by hash, the hash by name, and the procedures it runs natively.
#[derive(Clone, Debug)]
pub struct Domain {
    pub module: Module,
    pub closures: BTreeMap<FnHash, Closure>,
    pub by_name: BTreeMap<String, FnHash>,
    /// harken's own procedures whose hashes this module names: applied
    /// natively, everything else through its closure.
    pub natives: Vec<(FnHash, Procedure)>,
}

impl Domain {
    /// harken's module, as this build authors it: every procedure native.
    pub fn harken() -> Domain {
        let m = harken_domain::module();
        Domain::of(m.build().clone(), m.procedures())
    }

    fn of(module: Module, procedures: Vec<(FnHash, Procedure)>) -> Domain {
        let closures = closures(&module);
        let by_name = closures
            .iter()
            .map(|(h, c)| (c.function.name.clone(), h.clone()))
            .collect();
        let natives = procedures
            .into_iter()
            .filter(|(h, _)| closures.contains_key(h))
            .collect();
        Domain {
            module,
            closures,
            by_name,
            natives,
        }
    }

    /// A module from the canonical CBOR of an `.ark` file, verified.
    pub fn from_bytes(bytes: &[u8]) -> Result<Domain> {
        let v = canon::decode(bytes).context("the module is not canonical CBOR")?;
        let module = module_from_value(&v).context("the module does not decode")?;
        let module = ark::verify::verify(&module)
            .map_err(|es| anyhow::anyhow!("the module does not verify: {es:?}"))?;
        Ok(Domain::of(module, harken_domain::module().procedures()))
    }

    pub fn load(path: &Path) -> Result<Domain> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading the module {}", path.display()))?;
        Domain::from_bytes(&bytes).with_context(|| format!("loading the module {}", path.display()))
    }

    /// The scopes the module declares, in schema order.
    pub fn scopes(&self) -> Vec<String> {
        self.module
            .schema
            .scopes
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }
}

/// Host every scope of the module, each with the log it left on disk.
pub fn open_hub(domain: &Domain, data: &Path) -> Result<Hub> {
    std::fs::create_dir_all(data).with_context(|| format!("creating {}", data.display()))?;
    let schema = &domain.module.schema;
    let mut server = Server::open(trusting(), open_access(), Silent);
    for scope in domain.scopes() {
        let mut a = Authority::new(schema.clone(), &scope, domain.closures.clone());
        a.hold(domain.natives.iter().cloned());
        if let Some(log) = persist::load(data, &scope, schema)? {
            a.store = log
                .state_at(log.head_seq())
                .context("a loaded log has no state at its head")?;
            a.log = log;
        }
        eprintln!("harken-server: hosting {scope} at seq {}", a.log.head_seq());
        server.host(a);
    }
    Ok(Hub::new(server, Some(data.to_path_buf())))
}

#[derive(Clone, Debug)]
pub struct Config {
    /// An `.ark` file to host instead of harken's own module.
    pub module: Option<PathBuf>,
    pub data: PathBuf,
    /// `host:port`; port 0 takes an ephemeral one, reported in [`Running::addr`].
    pub listen: String,
    pub media: Option<PathBuf>,
}

/// A server that is up: where, and how to stop it.
pub struct Running {
    pub addr: SocketAddr,
    pub hub: HubHandle,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<()>>,
}

impl Running {
    /// Stop accepting, let open connections finish, and give up on them
    /// after a few seconds.
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
        }
    }
}

/// Load the module, host its scopes, bind, and serve. The scanner runs
/// beside the listener when a media directory is set.
pub async fn start(config: Config) -> Result<Running> {
    let domain = match &config.module {
        Some(p) => Domain::load(p)?,
        None => Domain::harken(),
    };
    eprintln!(
        "harken-server: module {} ({} functions, {} native, scopes {})",
        config
            .module
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "harken (built in)".into()),
        domain.module.functions.len(),
        domain.natives.len(),
        domain.scopes().join(", ")
    );
    eprintln!("harken-server: *** DEV AUTH: anyone is whoever they say. A token is a name, nothing is checked, every scope is open. ***");
    let hub = {
        let (domain, data) = (domain.clone(), config.data.clone());
        HubHandle::spawn(move || open_hub(&domain, &data))?
    };

    let mut router = Router::new()
        .route("/healthz", get(healthz))
        .route("/sync", get(sync));
    if let Some(media) = &config.media {
        eprintln!(
            "harken-server: serving {} at /media, unauthenticated",
            media.display()
        );
        router = router.nest_service("/media", ServeDir::new(media));
    }
    let router = router.with_state(hub.clone());

    let listener = TcpListener::bind(&config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    let addr = listener.local_addr()?;
    eprintln!("harken-server: listening on http://{addr} (sync at ws://{addr}/sync)");
    let (shutdown, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .context("serving")
    });

    if let Some(media) = config.media.clone() {
        let hub = hub.clone();
        tokio::spawn(async move {
            if let Err(e) = scanner::scan(&hub, &domain, &media).await {
                eprintln!("harken-server: scanner: {e:#}");
            }
        });
    }

    Ok(Running {
        addr,
        hub,
        shutdown: Some(shutdown),
        task,
    })
}

async fn healthz(State(hub): State<HubHandle>) -> Response {
    match hub.health().await {
        Ok(h) => {
            let mut text = format!("ok\nconnections {}\n", h.connections);
            for (scope, head) in h.heads {
                text.push_str(&format!("scope {scope} head {head}\n"));
            }
            text.into_response()
        }
        Err(e) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            format!("{e:#}\n"),
        )
            .into_response(),
    }
}

async fn sync(ws: WebSocketUpgrade, State(hub): State<HubHandle>) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = connection(socket, hub).await {
            eprintln!("harken-server: connection closed: {e:#}");
        }
    })
}

/// One socket: frames in go to the hub, frames the hub queues for this
/// connection go out, and the server pings — a client never has to.
async fn connection(mut socket: WebSocket, hub: HubHandle) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    let conn = hub.connect(tx).await?;
    let mut ticker = tokio::time::interval(PING_EVERY);
    ticker.tick().await;
    let mut unanswered: u32 = 0;
    let outcome = loop {
        tokio::select! {
            frame = socket.recv() => match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    unanswered = 0;
                    let msg = canon::decode(&bytes)
                        .context("a frame that is not canonical CBOR")
                        .and_then(|v| ClientMsg::from_value(&v).context("a frame that is not a client message"));
                    match msg {
                        Ok(m) => hub.recv(conn, m)?,
                        Err(e) => break Err(e),
                    }
                }
                Some(Ok(Message::Pong(_))) => unanswered = 0,
                Some(Ok(Message::Ping(_))) => {}
                Some(Ok(Message::Text(_))) => break Err(anyhow::anyhow!("a text frame; the protocol is binary")),
                Some(Ok(Message::Close(_))) | None => break Ok(()),
                Some(Err(e)) => break Err(e.into()),
            },
            Some(msg) = rx.recv() => {
                if let Err(e) = socket.send(Message::Binary(canon::encode(&msg.to_value()))).await {
                    break Err(e.into());
                }
            }
            _ = ticker.tick() => {
                if unanswered >= PINGS_UNANSWERED {
                    break Err(anyhow::anyhow!("{PINGS_UNANSWERED} pings unanswered"));
                }
                unanswered += 1;
                if let Err(e) = socket.send(Message::Ping(vec![])).await {
                    break Err(e.into());
                }
            }
        }
    };
    hub.disconnect(conn)?;
    outcome
}
