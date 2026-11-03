//! `money-server [ADDR]`, and everything else from the environment.
//!
//! ```text
//! MONEY_DEV_AUTH=1 money-server              # 127.0.0.1:8788, state in the temp dir
//! MONEY_OIDC_ISSUER=https://auth.example.com MONEY_OIDC_CLIENT_ID=money \
//!   MONEY_OIDC_CLIENT_SECRET_FILE=/run/credentials/money.service/oidc-secret \
//!   MONEY_PUBLIC_URL=https://money.example.com MONEY_WEB=/path/to/money-web-server \
//!   MONEY_DATA=/var/lib/money \
//!   MONEY_PLAID_CLIENT_ID_FILE=… MONEY_PLAID_SECRET_FILE=… \
//!   money-server 127.0.0.1:8788              # what the NixOS module runs
//! ```
//!
//! `SIGUSR1` asks the importer to run now: what the module's timer sends.
//! [`money_server::Config::from_env`] lists every variable.

use anyhow::Result;
use money_server::{start, Config, LISTEN};

const USAGE: &str = "usage: money-server [ADDR]   (default 127.0.0.1:8788)

Everything else is the environment:
  MONEY_DEV_AUTH=1, or MONEY_OIDC_ISSUER + MONEY_OIDC_CLIENT_ID + MONEY_OIDC_CLIENT_SECRET_FILE
  MONEY_DATA  MONEY_PUBLIC_URL  MONEY_REDIRECTS  MONEY_PICTURE_HOSTS  MONEY_WEB  MONEY_ROLES
  MONEY_PLAID_CLIENT_ID_FILE + MONEY_PLAID_SECRET_FILE  MONEY_PLAID_ENV  MONEY_PLAID_URL
  MONEY_PLAID_HISTORY

The banks are connected from the app and kept in $MONEY_DATA/banks.json.

kill -USR1 asks the bank importer to run now.";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let listen = match args.next() {
        Some(a) if a == "--help" || a == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        Some(a) => a,
        None => LISTEN.into(),
    };
    if let Some(extra) = args.next() {
        anyhow::bail!("unexpected argument {extra}\n{USAGE}");
    }
    let config = Config::from_env(&listen)?;
    let events = config.data.join("events");
    match ark_peer::observe::Files::new(&events) {
        Ok(f) => drop(ark_peer::observe::install(ark_peer::observe::Sink::Files(std::sync::Mutex::new(f)))),
        Err(e) => eprintln!("money-server: no events kept: {}: {e}", events.display()),
    }
    let server = start(config).await?;
    run(&server).await?;
    eprintln!("money-server: stopping");
    server.stop().await;
    Ok(())
}

/// Until Ctrl-C or `SIGTERM`; each `SIGUSR1` a nudge to the importer.
#[cfg(unix)]
async fn run(server: &money_server::Server) -> Result<()> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut usr1 = signal(SignalKind::user_defined1())?;
    loop {
        tokio::select! {
            r = tokio::signal::ctrl_c() => return Ok(r?),
            _ = term.recv() => return Ok(()),
            _ = usr1.recv() => match &server.importer {
                Some(i) => i.nudge(),
                None => eprintln!("money-server: asked to import, and there is no importer"),
            },
        }
    }
}

#[cfg(not(unix))]
async fn run(_: &money_server::Server) -> Result<()> {
    Ok(tokio::signal::ctrl_c().await?)
}
