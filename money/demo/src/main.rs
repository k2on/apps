//! `money-seed DATA [DAY]`: the demo's household (the core's `demo`)
//! written into a money server's data directory before it first starts, as
//! the log it boots on, so a phone signed in as `alex` under dev auth opens
//! on a household three months deep: what the app screenshots run
//! (.github/screens/apps.md). Seeded up to DAY (`2026-11-12`) when it is
//! given, today otherwise: a screenshot run gives every phone's server the
//! run's one day, so each shows the same month and days. Refuses a
//! directory that already has anything in it, as `harken-seed` does
//! (`docs/seed.md`).
//!
//! ```text
//! money-seed /tmp/money && MONEY_DEV_AUTH=1 MONEY_DATA=/tmp/money money-server 0.0.0.0:8788
//! ```

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Why the household was not written.
enum Failed {
    Usage,
    NotEmpty(PathBuf),
    Io {
        doing: &'static str,
        path: PathBuf,
        error: std::io::Error,
    },
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Failed::Usage => f.write_str(
                "usage: money-seed DATA [DAY]   (a money server's data directory, empty or not there yet; \
                 the day the household runs up to, as 2026-11-12, today when none)",
            ),
            Failed::NotEmpty(p) => write!(f, "{} is not empty; money-seed writes only a new data directory", p.display()),
            Failed::Io { doing, path, error } => write!(f, "{doing} {}: {error}", path.display()),
        }
    }
}

fn io<'a>(doing: &'static str, path: &'a Path) -> impl FnOnce(std::io::Error) -> Failed + 'a {
    move |error| Failed::Io {
        doing,
        path: path.into(),
        error,
    }
}

fn seed() -> Result<PathBuf, Failed> {
    let mut args = std::env::args().skip(1);
    let (Some(data), day, None) = (args.next(), args.next(), args.next()) else {
        return Err(Failed::Usage);
    };
    let now = ark_peer::now_ms();
    // A day given is the household's today, its clock no later than that
    // day's noon (UTC) nor than now: written at once, what it holds is the
    // same household the phones' demo opens on that day.
    let (today, now) = match day.filter(|d| !d.trim().is_empty()) {
        None => (money_core::dates::day_of_ms(now), now),
        Some(d) => {
            let Some((y, m, dd)) = money_core::dates::parse_day(d.trim()) else {
                return Err(Failed::Usage);
            };
            let noon = money_core::dates::days_from_civil(y, m, dd) * 86_400_000 + 43_200_000;
            (d.trim().to_string(), noon.min(now))
        }
    };
    let data = PathBuf::from(data);
    if std::fs::read_dir(&data).is_ok_and(|mut d| d.next().is_some()) {
        return Err(Failed::NotEmpty(data));
    }
    std::fs::create_dir_all(&data).map_err(io("making", &data))?;
    let log = data.join("log");
    let mut disk = ark_peer::Files::new(&log).map_err(io("opening", &log))?;
    money_seed::log(&mut disk, &today, now).map_err(io("writing", &log))?;
    Ok(log)
}

fn main() -> ExitCode {
    match seed() {
        Ok(log) => {
            eprintln!("money-seed: the demo's household in {}, as {}", log.display(), money_seed::ME);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("money-seed: {e}");
            ExitCode::FAILURE
        }
    }
}
