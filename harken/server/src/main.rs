//! `harken-server [ADDR]`, and everything else from the environment.
//!
//! ```text
//! HARKEN_DEV_AUTH=1 harken-server              # 127.0.0.1:8787, state in the temp dir
//! HARKEN_DEV_AUTH=1 harken-server 0.0.0.0:8787 # reachable from a phone on the same network
//! HARKEN_OIDC_ISSUER=https://auth.example.com HARKEN_OIDC_CLIENT_ID=harken \
//!   HARKEN_OIDC_CLIENT_SECRET_FILE=/run/credentials/harken.service/oidc-secret \
//!   HARKEN_PUBLIC_URL=https://harken.example.com HARKEN_WEB=/path/to/harken-web \
//!   HARKEN_DATA=/var/lib/harken HARKEN_MEDIA=/srv/media \
//!   harken-server 127.0.0.1:8787               # what the NixOS module runs
//! ```
//!
//! [`harken_server::Config::from_env`] lists every variable.

use anyhow::Result;
use harken_server::{start, Config};

const USAGE: &str = "usage: harken-server [ADDR]   (default 127.0.0.1:8787)

Everything else is the environment:
  HARKEN_DEV_AUTH=1, or HARKEN_OIDC_ISSUER + HARKEN_OIDC_CLIENT_ID + HARKEN_OIDC_CLIENT_SECRET_FILE
  HARKEN_DATA  HARKEN_PUBLIC_URL  HARKEN_REDIRECTS  HARKEN_MEDIA  HARKEN_WEB  HARKEN_WEB_MODULE
  HARKEN_MODULE  HARKEN_HA_URL + HARKEN_HA_TOKEN_FILE + HARKEN_HA_PLAYERS  HARKEN_HA_MEDIA
  HARKEN_KEEPALIVE_MS  HARKEN_KEEPALIVE_MISSED";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let listen = match args.next() {
        Some(a) if a == "--help" || a == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        Some(a) => a,
        None => "127.0.0.1:8787".into(),
    };
    if let Some(extra) = args.next() {
        anyhow::bail!("unexpected argument {extra}\n{USAGE}");
    }
    let server = start(Config::from_env(&listen)?).await?;
    stopped().await?;
    eprintln!("harken-server: stopping");
    server.stop().await;
    Ok(())
}

/// Ctrl-C on a laptop, or `SIGTERM` from systemd: either is a request to
/// stop, answered by taking the scanner and the house off the hub first.
#[cfg(unix)]
async fn stopped() -> Result<()> {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        r = tokio::signal::ctrl_c() => r?,
        _ = term.recv() => {}
    }
    Ok(())
}

#[cfg(not(unix))]
async fn stopped() -> Result<()> {
    Ok(tokio::signal::ctrl_c().await?)
}
