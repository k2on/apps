//! `devices-server [ADDR]`, and everything else from the environment.
//!
//! ```text
//! DEVICES_DEV_AUTH=1 devices-server            # 127.0.0.1:8790, state in the temp dir
//! DEVICES_OIDC_ISSUER=https://id.example.com DEVICES_OIDC_CLIENT_ID=devices \
//!   DEVICES_OIDC_CLIENT_SECRET_FILE=/run/credentials/devices.service/oidc-secret \
//!   DEVICES_PUBLIC_URL=https://devices.example.com DEVICES_DATA=/var/lib/devices \
//!   DEVICES_POLICIES=/nix/store/…-policies.json \
//!   devices-server 127.0.0.1:8790              # what the NixOS module runs
//! ```

use anyhow::Result;
use devices_server::{start, Config, LISTEN};

const USAGE: &str = "usage: devices-server [ADDR]   (default 127.0.0.1:8790)

Everything else is the environment:
  DEVICES_DEV_AUTH=1, or DEVICES_OIDC_ISSUER + DEVICES_OIDC_CLIENT_ID + DEVICES_OIDC_CLIENT_SECRET_FILE
  DEVICES_DATA  DEVICES_PUBLIC_URL  DEVICES_REDIRECTS  DEVICES_ROLES
  DEVICES_POLICIES   the household's policies and who each applies to (JSON)";

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
    let server = start(Config::from_env(&listen)?).await?;
    wait().await?;
    eprintln!("devices-server: stopping");
    server.stop().await;
    Ok(())
}

/// Until Ctrl-C or `SIGTERM`.
#[cfg(unix)]
async fn wait() -> Result<()> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    tokio::select! {
        r = tokio::signal::ctrl_c() => Ok(r?),
        _ = term.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn wait() -> Result<()> {
    Ok(tokio::signal::ctrl_c().await?)
}
