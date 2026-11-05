//! `harken-seed DATA`: the demo's catalogue (this crate's library) written
//! as a media root at `DATA/media`, a folder of tagged FLAC files and their
//! covers ([`harken_seed::media`]), for a harken server to boot on with
//! `DATA` its data directory: its own scanner reads the files, so a phone
//! signed in under dev auth opens on the demo's albums. What the app
//! screenshots run (.github/screens/apps.md). Refuses a directory that
//! already has anything in it, as `money-seed` does (`seed/README.md`).
//!
//! ```text
//! harken-seed /tmp/harken && HARKEN_DEV_AUTH=1 HARKEN_DATA=/tmp/harken HARKEN_MEDIA=/tmp/harken/media harken-server 0.0.0.0:8787
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(data), None) = (args.next(), args.next()) else {
        eprintln!("usage: harken-seed DATA   (a harken server's data directory, empty or not there yet; the media root is DATA/media)");
        return ExitCode::FAILURE;
    };
    let data = PathBuf::from(data);
    if std::fs::read_dir(&data).is_ok_and(|mut d| d.next().is_some()) {
        eprintln!(
            "harken-seed: {} is not empty; harken-seed writes only a new data directory",
            data.display()
        );
        return ExitCode::FAILURE;
    }
    let media = data.join("media");
    let (mut albums, mut fetched) = (0, 0);
    let wrote = harken_seed::media::write(harken_seed::catalogue(), &media, |w| {
        albums += 1;
        fetched += usize::from(w.fetched);
        let cover = if w.fetched { "its cover" } else { "a cover of its own" };
        eprintln!("{} / {}: {} tracks, {cover}", w.performer, w.album, w.tracks);
    });
    if let Err(e) = wrote {
        eprintln!("harken-seed: writing {}: {e}", media.display());
        return ExitCode::FAILURE;
    }
    eprintln!(
        "harken-seed: the demo's {albums} albums in {} ({fetched} covers fetched)",
        media.display()
    );
    ExitCode::SUCCESS
}
