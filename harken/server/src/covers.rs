//! Where a record's cover comes from, and where a picture inside a file goes.
//!
//! A cover is spelt the way `media.file` is: a path under the media root,
//! which every client already joins to `<server>/media/` (`harken::
//! listening::url`, the desktop's `media_url`, the phone's `Media.url`). So a
//! cover the scanner finds needs no client to learn anything:
//!
//! - **An image beside the track** is its own path behind a version:
//!   `.cover/<v>/music/Bach/Goldberg/cover.jpg`, which [`COVER`] serves from
//!   the media root as `music/Bach/Goldberg/cover.jpg`, `<v>` ignored. The
//!   version is the file's size and modification time, hashed, so a cover
//!   replaced under the same name is a new value — a new row, and a URL no
//!   client has cached — where the bare path would have been the same
//!   string, and every client would have gone on drawing the picture it
//!   fetched first. Looked for by name — `cover`, `folder`, `front`,
//!   `album`, in that order — and otherwise the one image in the directory
//!   when there is exactly one. A disc directory (`CD1`, `Disc 2`) with
//!   none asks the album's directory above it.
//! - **A picture inside the file** — the front cover where the tags carry
//!   one, otherwise the first — cannot be served from where it is, and the
//!   media root is mounted read-only. So it is written once into the
//!   server's own data, named by the SHA-256 of its bytes, and spelt
//!   `.art/<sha>.<ext>`: [`ART`] is a reserved directory under `/media`
//!   served from there ([`routes`]). Twenty tracks carrying one picture are
//!   one file.
//! - **An artist's picture** is only ever a file somebody put there, never
//!   invented: `artist.*` beside the album or in the directory above it, or
//!   `folder.*` in that directory above when it is named for the artist —
//!   Jellyfin's and Kodi's layouts, `Artist/Album/track`.
//!   Their page's wide picture is `backdrop.*` (or `fanart.*`), looked for
//!   where `artist.*` is: Jellyfin's name for it, and Kodi's.
//!
//! Files directly in `music/` are loose tracks, not a record, so that
//! directory is nobody's album and nobody's artist.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use axum::Router;
use sha2::{Digest, Sha256};
use tower_http::services::ServeDir;

use crate::library::relative;

/// The reserved directory under `/media` that extracted pictures are served
/// at, from the server's data rather than the media root. A dot first, so
/// that no library is likely to have one of its own; if one does, this
/// shadows it.
pub const ART: &str = ".art";

/// The reserved directory under `/media` that a cover beside a track is
/// served at, behind its version: `/media/.cover/<v>/<path>` is
/// `/media/<path>`.
pub const COVER: &str = ".cover";

/// What counts as a picture beside a track.
pub const IMAGES: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// The names a record's cover goes by, in the order they are preferred.
const ALBUM: &[&str] = &["cover", "folder", "front", "album"];

/// The names an artist's wide picture goes by.
const BACKDROP: &[&str] = &["backdrop", "fanart"];

/// `/media`: the media root; [`ART`] under it from `art`; and [`COVER`]
/// under it from the media root again, its version segment dropped. The
/// reserved prefixes are matched before the root, and nothing but [`ART`]
/// is asked of the data directory. `ServeDir` refuses a `..` in each,
/// decoded or not, and guesses the type from the extension — which is what
/// a picture extracted under its hash is named with.
pub fn routes(media: &Path, art: &Path) -> Router {
    Router::new()
        .nest_service(&format!("/media/{ART}"), ServeDir::new(art))
        .nest_service(&format!("/media/{COVER}/:version"), ServeDir::new(media))
        .nest_service("/media", ServeDir::new(media))
}

/// Whether a path is a picture by its extension.
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGES.contains(&e.to_ascii_lowercase().as_str()))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// `CD1`, `cd 2`, `Disc 1`, `disk2`: a directory that is one disc of a
/// record whose directory is the one above.
fn is_disc(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    ["cd", "disc", "disk"].iter().any(|p| {
        name.strip_prefix(p).is_some_and(|rest| {
            let rest = rest.trim_start_matches([' ', '_', '-', '.']);
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        })
    })
}

/// A picture found beside a track: its path under the media root, and what
/// it was when it was found — its size and its modification time — so that
/// a cover replaced under the same name is noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub rel: String,
    pub print: String,
}

impl Image {
    /// How the log spells it: `.cover/<v>/<path>`, `<v>` sixteen hex digits
    /// of the print's hash.
    pub fn value(&self) -> String {
        let v = hex(&Sha256::digest(self.print.as_bytes()));
        format!("{COVER}/{}/{}", &v[..16], self.rel)
    }
}

/// The pictures one directory offers, read once per look however many
/// tracks it holds.
#[derive(Debug, Clone, Default)]
struct Dir {
    /// The record's cover.
    album: Option<Image>,
    /// `artist.*` beside the record or in the directory above it.
    artist: Option<Image>,
    /// `backdrop.*` or `fanart.*`, where `artist.*` is looked for.
    backdrop: Option<Image>,
    /// `folder.*` in the directory above, with that directory's name, which
    /// must be the artist's for it to be theirs.
    above: Option<(String, Image)>,
}

/// The covers of the directories one look reaches, each read once.
pub struct Covers {
    root: PathBuf,
    music: PathBuf,
    art: PathBuf,
    dirs: BTreeMap<PathBuf, Dir>,
    images: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// The pictures written, by this look or before it (the index the
    /// scanner keeps, `library::Art`).
    written: BTreeSet<String>,
}

/// What a track's covers come to: the two values `add_song` carries, and a
/// print of the files they came from, which is part of the track's own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub album: Option<Image>,
    pub artist: Option<Image>,
    pub backdrop: Option<Image>,
}

impl Found {
    /// The pictures' prints, as one string a track's fingerprint holds.
    pub fn print(&self) -> String {
        let p = |i: &Option<Image>| {
            i.as_ref()
                .map_or(String::new(), |i| format!("{}@{}", i.rel, i.print))
        };
        match &self.backdrop {
            // As it was before there were backdrops, so a print kept then
            // has not moved for a track that has none.
            None => format!("{}|{}", p(&self.album), p(&self.artist)),
            Some(_) => format!(
                "{}|{}|{}",
                p(&self.album),
                p(&self.artist),
                p(&self.backdrop)
            ),
        }
    }
}

impl Covers {
    pub fn new(root: &Path, music: &Path, art: &Path, written: BTreeSet<String>) -> Covers {
        Covers {
            root: root.into(),
            music: music.into(),
            art: art.into(),
            dirs: BTreeMap::new(),
            images: BTreeMap::new(),
            written,
        }
    }

    /// The pictures written, by this look or before it.
    pub fn written(&self) -> &BTreeSet<String> {
        &self.written
    }

    /// The pictures a directory holds, sorted, read once.
    fn images(&mut self, dir: &Path) -> Vec<PathBuf> {
        if let Some(i) = self.images.get(dir) {
            return i.clone();
        }
        let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| is_image(p))
            .collect();
        out.sort();
        self.images.insert(dir.into(), out.clone());
        out
    }

    fn image(&self, path: &Path) -> Option<Image> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Image {
            rel: relative(&self.root, path)?,
            print: format!("{}:{}", meta.len(), mtime(&meta)),
        })
    }

    /// The first picture in `dir` going by one of `names`, in their order.
    fn named(&mut self, dir: &Path, names: &[&str]) -> Option<Image> {
        let images = self.images(dir);
        names
            .iter()
            .find_map(|n| images.iter().find(|p| stem(p) == *n))
            .and_then(|p| self.image(p))
    }

    /// A record's cover in `dir`: by name, then the only picture there that
    /// is not the artist's.
    fn cover_in(&mut self, dir: &Path) -> Option<Image> {
        if let Some(i) = self.named(dir, ALBUM) {
            return Some(i);
        }
        let images = self.images(dir);
        let mut rest = images.iter().filter(|p| stem(p) != "artist");
        match (rest.next(), rest.next()) {
            (Some(only), None) => self.image(only),
            _ => None,
        }
    }

    fn dir(&mut self, dir: &Path) -> Dir {
        if let Some(d) = self.dirs.get(dir) {
            return d.clone();
        }
        let mut found = Dir::default();
        // Loose files in `music/` are nobody's record, and a path outside it
        // is nobody's at all.
        if dir.starts_with(&self.music) && dir != self.music {
            let record = match is_disc(dir) && dir.parent() != Some(self.music.as_path()) {
                true => dir.parent().unwrap_or(dir).to_path_buf(),
                false => dir.to_path_buf(),
            };
            found.album = self
                .cover_in(dir)
                .or_else(|| match record.as_path() != dir {
                    true => self.cover_in(&record),
                    false => None,
                });
            found.artist = self.named(&record, &["artist"]);
            found.backdrop = self.named(&record, BACKDROP);
            if let Some(above) = record
                .parent()
                .filter(|a| a.starts_with(&self.music) && *a != self.music)
            {
                let above = above.to_path_buf();
                if found.artist.is_none() {
                    found.artist = self.named(&above, &["artist"]);
                }
                if found.backdrop.is_none() {
                    found.backdrop = self.named(&above, BACKDROP);
                }
                let name = above
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                found.above = self.named(&above, &["folder"]).map(|i| (name, i));
            }
        }
        self.dirs.insert(dir.into(), found.clone());
        found
    }

    /// The covers a track in `dir` by `artist` would carry from the files
    /// around it.
    pub fn beside(&mut self, dir: &Path, artist: &str) -> Found {
        let d = self.dir(dir);
        let named =
            |n: &str| !artist.trim().is_empty() && n.trim().eq_ignore_ascii_case(artist.trim());
        Found {
            album: d.album,
            artist: d
                .artist
                .or_else(|| d.above.filter(|(n, _)| named(n)).map(|(_, i)| i)),
            backdrop: d.backdrop,
        }
    }

    /// Write a picture found inside a file into the data directory, once,
    /// and say how the log spells it: `.art/<sha256>.<ext>`. `None` for
    /// bytes that are no picture a browser draws.
    pub fn extract(&mut self, bytes: &[u8]) -> Option<String> {
        let ext = sniff(bytes)?;
        let hash = hex(&Sha256::digest(bytes));
        let name = format!("{hash}.{ext}");
        let value = format!("{ART}/{name}");
        if self.written.contains(&name) {
            return Some(value);
        }
        let path = self.art.join(&name);
        if !path.exists() {
            // Beside and renamed in, so a fetch never reads half a picture,
            // and two scanners writing one picture each write all of it.
            let tmp = self
                .art
                .join(format!(".{name}.{}.partial", std::process::id()));
            let written = std::fs::create_dir_all(&self.art)
                .and_then(|()| std::fs::write(&tmp, bytes))
                .and_then(|()| std::fs::rename(&tmp, &path));
            if let Err(e) = written {
                let _ = std::fs::remove_file(&tmp);
                eprintln!(
                    "harken-server: library: cannot keep a cover in {}: {e}",
                    self.art.display()
                );
                return None;
            }
        }
        self.written.insert(name);
        Some(value)
    }
}

/// The picture a file's tags carry: the front cover where one is marked,
/// otherwise the first, from whichever tag holds one.
pub fn embedded(tagged: &lofty::file::TaggedFile) -> Option<Vec<u8>> {
    use lofty::file::TaggedFileExt;
    use lofty::picture::PictureType;
    let pictures: Vec<&lofty::picture::Picture> =
        tagged.tags().iter().flat_map(|t| t.pictures()).collect();
    pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or(pictures.first())
        .map(|p| p.data().to_vec())
}

/// What a picture is, from its first bytes rather than from what the tag
/// says it is: a tag's MIME type is somebody's claim, and the extension is
/// what `/media` will tell a browser.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("png"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file's modification time in nanoseconds, or 0 where the platform says
/// nothing.
pub fn mtime(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_is_a_disc_and_a_record_is_not() {
        for d in ["CD1", "cd 2", "Disc 1", "disk-3", "DISC_10"] {
            assert!(is_disc(Path::new(d)), "{d}");
        }
        for d in ["CD", "Discography", "Disc One", "Abbey Road", "cd1x"] {
            assert!(!is_disc(Path::new(d)), "{d}");
        }
    }

    #[test]
    fn a_picture_is_what_its_bytes_say() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(
            sniff(b"RIFF\0\0\0\0WAVEfmt "),
            None,
            "a WAV is not a picture"
        );
        assert_eq!(sniff(b"<svg"), None);
        assert_eq!(sniff(b""), None);
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, p.to_string_lossy().as_bytes()).unwrap();
    }

    #[test]
    fn a_cover_is_found_by_name_then_by_being_the_only_one() {
        let root = tempfile::tempdir().unwrap();
        let (root, music) = (root.path(), root.path().join("music"));
        let art = root.join("art");
        let album = music.join("Bach/Goldberg");
        touch(&album.join("back.jpg"));
        touch(&album.join("Folder.JPG"));
        touch(&album.join("front.png"));
        touch(&album.join("artist.jpg"));
        touch(&music.join("Bach/folder.jpg"));
        touch(&music.join("loose.jpg"));
        let only = music.join("Satie/Gymnopedies");
        touch(&only.join("scan 1.webp"));
        touch(&only.join("artist.png"));
        let cd = music.join("Bach/Partitas/CD1");
        touch(&music.join("Bach/Partitas/cover.jpeg"));
        std::fs::create_dir_all(&cd).unwrap();
        let two = music.join("Two");
        touch(&two.join("a.jpg"));
        touch(&two.join("b.jpg"));

        let mut c = Covers::new(root, &music, &art, Default::default());
        let rel = |i: Option<Image>| i.map(|i| i.rel);
        let bach = c.beside(&album, "Johann Sebastian Bach");
        assert_eq!(
            rel(bach.album),
            Some("music/Bach/Goldberg/Folder.JPG".into()),
            "folder before front, any case"
        );
        assert_eq!(
            rel(bach.artist),
            Some("music/Bach/Goldberg/artist.jpg".into())
        );
        let satie = c.beside(&only, "Erik Satie");
        assert_eq!(
            rel(satie.album),
            Some("music/Satie/Gymnopedies/scan 1.webp".into()),
            "the only picture not the artist's"
        );
        assert_eq!(
            rel(satie.artist),
            Some("music/Satie/Gymnopedies/artist.png".into())
        );
        let partitas = c.beside(&cd, "Bach");
        assert_eq!(
            rel(partitas.album),
            Some("music/Bach/Partitas/cover.jpeg".into()),
            "a disc asks its record"
        );
        assert_eq!(
            rel(partitas.artist),
            Some("music/Bach/folder.jpg".into()),
            "the folder named for the artist"
        );
        assert_eq!(
            rel(c.beside(&cd, "Glenn Gould").artist),
            None,
            "and nobody else's"
        );
        assert_eq!(
            c.beside(&two, "x"),
            Found::default(),
            "two pictures and no name is no cover"
        );
        assert_eq!(
            c.beside(&music, "x"),
            Found::default(),
            "loose files are no record"
        );
    }

    #[test]
    fn a_picture_inside_a_file_is_written_once_by_its_hash() {
        let root = tempfile::tempdir().unwrap();
        let art = root.path().join("data/art");
        let mut c = Covers::new(
            root.path(),
            &root.path().join("music"),
            &art,
            Default::default(),
        );
        let png = b"\x89PNG\r\n\x1a\none picture".to_vec();
        let v = c.extract(&png).unwrap();
        assert_eq!(v, format!(".art/{}.png", hex(&Sha256::digest(&png))));
        assert_eq!(
            std::fs::read(art.join(v.strip_prefix(".art/").unwrap())).unwrap(),
            png
        );
        assert_eq!(c.extract(&png).unwrap(), v);
        let mut again = Covers::new(
            root.path(),
            &root.path().join("music"),
            &art,
            Default::default(),
        );
        assert_eq!(
            again.extract(&png).unwrap(),
            v,
            "another look finds it written"
        );
        assert_eq!(
            std::fs::read_dir(&art).unwrap().count(),
            1,
            "one file, and no partial left"
        );
        assert_eq!(c.extract(b"not a picture"), None);
    }
}
