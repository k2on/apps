//! `harken-server [--module PATH] [--data DIR] [--listen ADDR] [--media DIR]`,
//! each also an environment variable: `HARKEN_MODULE`, `HARKEN_DATA`,
//! `HARKEN_LISTEN` (default `127.0.0.1:8787`), `HARKEN_MEDIA`.

use std::path::PathBuf;

use anyhow::{bail, Result};
use harken_server::{start, Config};

const USAGE: &str =
    "usage: harken-server [--module PATH] [--data DIR] [--listen ADDR] [--media DIR]

  --module PATH   an .ark module to host instead of harken's own  (HARKEN_MODULE; optional)
  --data DIR      where each scope's log is kept    (HARKEN_DATA; default ./harken-data)
  --listen ADDR   host:port to serve on             (HARKEN_LISTEN; default 127.0.0.1:8787)
  --media DIR     media root: scanned, served at /media  (HARKEN_MEDIA; optional)";

fn parse(args: impl Iterator<Item = String>) -> Result<Option<Config>> {
    let mut module = std::env::var_os("HARKEN_MODULE").map(PathBuf::from);
    let mut data = std::env::var_os("HARKEN_DATA").map(PathBuf::from);
    let mut listen = std::env::var("HARKEN_LISTEN").ok();
    let mut media = std::env::var_os("HARKEN_MEDIA").map(PathBuf::from);
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            return Ok(None);
        }
        let Some(value) = args.next() else {
            bail!("{flag} needs a value\n{USAGE}");
        };
        match flag.as_str() {
            "--module" => module = Some(value.into()),
            "--data" => data = Some(value.into()),
            "--listen" => listen = Some(value),
            "--media" => media = Some(value.into()),
            other => bail!("unknown flag {other}\n{USAGE}"),
        }
    }
    Ok(Some(Config {
        module,
        data: data.unwrap_or_else(|| PathBuf::from("harken-data")),
        listen: listen.unwrap_or_else(|| "127.0.0.1:8787".to_string()),
        media,
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    let Some(config) = parse(std::env::args().skip(1))? else {
        println!("{USAGE}");
        return Ok(());
    };
    eprintln!("harken-server: data in {}", config.data.display());
    let running = start(config).await?;
    tokio::signal::ctrl_c().await?;
    eprintln!("harken-server: stopping");
    running.stop().await;
    Ok(())
}
