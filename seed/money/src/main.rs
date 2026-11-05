//! `money-seed DATA`: the demo's household (this crate's library) written
//! into a money server's data directory before it first starts, as the log
//! it boots on, so a phone signed in as `alex` under dev auth opens on a
//! household three months deep: what the app screenshots run
//! (.github/screens/apps.md). Refuses a directory that already has
//! anything in it, as `harken-seed` does (`seed/README.md`).
//!
//! ```text
//! money-seed /tmp/money && MONEY_DEV_AUTH=1 MONEY_DATA=/tmp/money money-server 0.0.0.0:8788
//! ```

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(data), None) = (args.next(), args.next()) else {
        bail!("usage: money-seed DATA   (a money server's data directory, empty or not there yet)");
    };
    let data = PathBuf::from(data);
    if std::fs::read_dir(&data).is_ok_and(|mut d| d.next().is_some()) {
        bail!("{} is not empty; money-seed writes only a new data directory", data.display());
    }
    std::fs::create_dir_all(&data).with_context(|| format!("making {}", data.display()))?;
    let log = data.join("log");
    let mut disk = ark_peer::Files::new(&log).with_context(|| format!("opening {}", log.display()))?;
    let now = ark_peer::now_ms();
    money_seed::log(&mut disk, &money_domain::dates::day_of_ms(now), now).with_context(|| format!("writing {}", log.display()))?;
    eprintln!("money-seed: the demo's household in {}, as {}", log.display(), money_seed::ME);
    Ok(())
}
