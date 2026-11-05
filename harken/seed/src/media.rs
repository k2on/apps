//! The demo's catalogue as a media root a harken server scans: every album
//! as a folder under `music/<performer>/<album>/`, a FLAC of silence for
//! each track as long as the catalogue says, tagged as the catalogue says
//! (title, composer, performer, album, track, year), and the album's cover
//! beside it: fetched from the catalogue's address, or, when that cannot be
//! had, a gradient of its own. The real server's scanner reads these as it
//! reads anyone's library (`harken/server/src/library.rs`), so a phone
//! shows what a server with this library would show.
//!
//! The FLAC is written here rather than by an encoder: silence is one
//! constant subframe a block, which is a few bytes of framing, and the tags
//! are a Vorbis comment block. Nothing outside the crate is run.

use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use harken_domain::catalogue::{Album, Catalogue};
use sha2::{Digest, Sha256};

/// The sample rate, in Hz, and the samples a frame holds: mono, 16 bits, the
/// least a FLAC of silence needs to say how long it is.
const RATE: u64 = 8_000;
const BLOCK: u64 = 4_096;

/// The side of a cover made when the catalogue's cannot be fetched.
const SIDE: u32 = 600;

/// What [`write`] made of one album.
pub struct Written {
    pub performer: String,
    pub album: String,
    pub tracks: usize,
    /// Whether the cover is the catalogue's, rather than a gradient.
    pub fetched: bool,
}

/// Every album of `catalogue` under `root`, which is made if it is not
/// there. Each album is reported to `done` as it is finished.
///
/// `covers`, when given, is a folder of the catalogue's covers fetched
/// already (the flake's `harken-covers`, kept in the repository as
/// harken/seed/covers), each named
/// [`key`] of its address and the kind it is: an album's is taken from
/// there, and fetched only when it is not.
pub fn write(catalogue: &Catalogue, root: &Path, covers: Option<&Path>, mut done: impl FnMut(&Written)) -> io::Result<()> {
    // Named as Wikimedia's policy asks a program to be, with where it is
    // from: a bare name is throttled, and its covers were all gradients.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .try_proxy_from_env(true)
        .user_agent(concat!("harken-seed/", env!("CARGO_PKG_VERSION"), " (https://github.com/k2on/apps)"))
        .build();
    for a in &catalogue.albums {
        let performer = a.tracks.iter().map(|t| t.performer.as_str()).find(|p| !p.is_empty()).unwrap_or("Various");
        let at = root.join("music").join(safe(performer)).join(safe(&a.name));
        std::fs::create_dir_all(&at)?;
        let fetched = cover(&agent, covers, a, &at)?;
        for t in &a.tracks {
            let seconds = (t.ms / 1000).clamp(1, 3600) as u64;
            let artist = if t.performer.is_empty() { &t.composer } else { &t.performer };
            let tags = [
                ("TITLE", t.title.clone()),
                ("ARTIST", artist.clone()),
                ("ALBUMARTIST", t.performer.clone()),
                ("ALBUM", a.name.clone()),
                ("COMPOSER", t.composer.clone()),
                ("TRACKNUMBER", if t.track == 0 { String::new() } else { t.track.to_string() }),
                ("DATE", if a.year == 0 { String::new() } else { a.year.to_string() }),
            ];
            let name = format!("{:02} {}.flac", t.track, safe(&t.title));
            std::fs::write(at.join(name), flac(seconds * RATE, &tags))?;
        }
        done(&Written {
            performer: performer.to_string(),
            album: a.name.clone(),
            tracks: a.tracks.len(),
            fetched,
        });
    }
    Ok(())
}

/// A path segment: no slashes, nothing hidden.
fn safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) { '-' } else { c }).collect();
    let s: String = s.trim().trim_start_matches('.').chars().take(120).collect();
    if s.is_empty() {
        "untitled".into()
    } else {
        s
    }
}

/// The album's cover in `at`: the catalogue's, named for what its bytes are
/// (`cover.jpg`, `cover.png`, `cover.webp`), or a gradient of its own as
/// `cover.png`. Whether it was the catalogue's.
fn cover(agent: &ureq::Agent, covers: Option<&Path>, a: &Album, at: &Path) -> io::Result<bool> {
    if let Some((bytes, ext)) = covers.and_then(|d| pinned(d, &a.art)).or_else(|| fetch(agent, &a.art)) {
        std::fs::write(at.join(format!("cover.{ext}")), bytes)?;
        return Ok(true);
    }
    std::fs::write(at.join("cover.png"), gradient(&a.name))?;
    Ok(false)
}

/// What a cover's address is called in a folder of them: the first sixteen
/// hex digits of its SHA-256, as the flake's `harken-covers` names it.
pub fn key(url: &str) -> String {
    Sha256::digest(url.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The cover at `url` from a folder of them fetched already, if it is there.
fn pinned(dir: &Path, url: &str) -> Option<(Vec<u8>, &'static str)> {
    if url.is_empty() {
        return None;
    }
    let key = key(url);
    ["jpg", "png", "webp"]
        .into_iter()
        .find_map(|ext| std::fs::read(dir.join(format!("{key}.{ext}"))).ok().map(|b| (b, ext)))
}

/// A picture at `url`, and the extension its bytes are, when it can be had
/// and is a picture a browser draws.
fn fetch(agent: &ureq::Agent, url: &str) -> Option<(Vec<u8>, &'static str)> {
    if url.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    let response = agent.get(url).call().ok()?;
    response.into_reader().take(32 << 20).read_to_end(&mut bytes).ok()?;
    let ext = match bytes.as_slice() {
        [0xFF, 0xD8, 0xFF, ..] => "jpg",
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => "png",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        _ => return None,
    };
    Some((bytes, ext))
}

/// harken's derived squares against white, as `art.gold` in arkui/theme.toml.
const GOLD: [([u8; 3], [u8; 3]); 6] = [
    ([0xE8, 0xD1, 0x9A], [0xC9, 0xA8, 0x5F]),
    ([0xEF, 0xDC, 0xAE], [0xD2, 0xB3, 0x70]),
    ([0xE2, 0xD2, 0xAC], [0xBF, 0xA6, 0x71]),
    ([0xF0, 0xDF, 0xA8], [0xCB, 0xAA, 0x5C]),
    ([0xE6, 0xD0, 0xA0], [0xC3, 0xA0, 0x5A]),
    ([0xF3, 0xE6, 0xBE], [0xD6, 0xB8, 0x77]),
];

/// A PNG from one corner to the other between two of harken's colours, picked by
/// `seed`'s hash, so an album without its cover still has one of its own,
/// the same every time.
fn gradient(seed: &str) -> Vec<u8> {
    // One of harken's own squares (arkui/theme.toml, `art.gold`, light),
    // picked by the hash: a cover that could not be had reads as the app's,
    // not as whatever two colours the hash happened to name.
    let h = Sha256::digest(seed.as_bytes());
    let (from, to) = GOLD[h[0] as usize % GOLD.len()];
    let span = 2 * (SIDE - 1);
    let mut pixels = Vec::with_capacity((SIDE * SIDE * 3) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let t = x + y;
            for c in 0..3 {
                let (a, b) = (from[c] as u32, to[c] as u32);
                pixels.push(((a * (span - t) + b * t) / span) as u8);
            }
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, SIDE, SIDE);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut w = encoder.write_header().expect("a vector takes it");
    w.write_image_data(&pixels).expect("a vector takes it");
    w.finish().expect("a vector takes it");
    out
}

/// A FLAC of `samples` samples of silence at [`RATE`], mono, 16 bits, with
/// `tags` (the empty ones left out) as its Vorbis comment.
pub fn flac(samples: u64, tags: &[(&str, String)]) -> Vec<u8> {
    let mut out = b"fLaC".to_vec();

    // STREAMINFO: the block sizes (the last may be shorter), the frame sizes
    // unknown, the rate, one channel, 16 bits, how many samples, and no MD5.
    let mut info = Vec::with_capacity(34);
    info.extend_from_slice(&(BLOCK as u16).to_be_bytes());
    info.extend_from_slice(&(BLOCK as u16).to_be_bytes());
    info.extend_from_slice(&[0; 6]);
    // Channels and bits are each one less than they are: 0 is mono, 15 is
    // 16 bits.
    let packed: u64 = (RATE << 44) | (15 << 36) | (samples & ((1 << 36) - 1));
    info.extend_from_slice(&packed.to_be_bytes());
    info.extend_from_slice(&[0; 16]);
    block(&mut out, 0, false, &info);

    // VORBIS_COMMENT: little-endian lengths, `KEY=value` each.
    let vendor = b"harken-seed";
    let kept: Vec<String> = tags.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}={v}")).collect();
    let mut comment = Vec::new();
    comment.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    comment.extend_from_slice(vendor);
    comment.extend_from_slice(&(kept.len() as u32).to_le_bytes());
    for c in &kept {
        comment.extend_from_slice(&(c.len() as u32).to_le_bytes());
        comment.extend_from_slice(c.as_bytes());
    }
    block(&mut out, 4, true, &comment);

    // The frames: each a header, one constant subframe of zero, and the
    // frame's CRC.
    let (mut n, mut left) = (0u32, samples);
    while left > 0 {
        let size = left.min(BLOCK);
        let start = out.len();
        out.extend_from_slice(&[0xFF, 0xF8]);
        // Block size: 4096 by its code, anything else spelled after the
        // frame number. Sample rate 8 kHz by its code.
        let sized = size == BLOCK;
        out.push((if sized { 0b1100 } else { 0b0111 }) << 4 | 0b0100);
        // Mono, 16 bits.
        out.push(0b0000_1000);
        utf8(&mut out, n);
        if !sized {
            out.extend_from_slice(&((size - 1) as u16).to_be_bytes());
        }
        let c = crc8(&out[start..]);
        out.push(c);
        // CONSTANT subframe, no wasted bits, the value zero.
        out.extend_from_slice(&[0x00, 0x00, 0x00]);
        let c = crc16(&out[start..]);
        out.extend_from_slice(&c.to_be_bytes());
        left -= size;
        n += 1;
    }
    out
}

/// One metadata block: whether it is the last, its type, its length in 24
/// bits, and its body.
fn block(out: &mut Vec<u8>, kind: u8, last: bool, body: &[u8]) {
    out.push(if last { 0x80 } else { 0 } | kind);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    out.extend_from_slice(body);
}

/// A frame number as FLAC spells it: UTF-8's encoding of the number, which
/// [`flac`]'s longest file (an hour, 7,032 frames) keeps to three bytes.
fn utf8(out: &mut Vec<u8>, n: u32) {
    match n {
        0..=0x7F => out.push(n as u8),
        0x80..=0x7FF => out.extend_from_slice(&[0xC0 | (n >> 6) as u8, 0x80 | (n & 0x3F) as u8]),
        _ => out.extend_from_slice(&[0xE0 | (n >> 12) as u8, 0x80 | ((n >> 6) & 0x3F) as u8, 0x80 | (n & 0x3F) as u8]),
    }
}

/// The frame header's CRC: polynomial x^8 + x^2 + x + 1, from zero.
fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |mut c, b| {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 { (c << 1) ^ 0x07 } else { c << 1 };
        }
        c
    })
}

/// The frame's CRC: polynomial x^16 + x^15 + x^2 + 1, from zero.
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |mut c, b| {
        c ^= (*b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x8005 } else { c << 1 };
        }
        c
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::{Accessor, ItemKey};

    /// Falsified by a frame or a CRC lofty will not read past, or a tag
    /// under a name it does not map: the scanner reads files with lofty, and
    /// a file it cannot read is a track the server never has.
    #[test]
    fn lofty_reads_the_flac_as_the_scanner_would() {
        let dir = std::env::temp_dir().join(format!("harken-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("01 Aria.flac");
        let tags = [
            ("TITLE", "Aria".to_string()),
            ("ARTIST", "Glenn Gould".to_string()),
            ("ALBUMARTIST", "Glenn Gould".to_string()),
            ("ALBUM", "Goldberg Variations".to_string()),
            ("COMPOSER", "Johann Sebastian Bach".to_string()),
            ("TRACKNUMBER", "1".to_string()),
            ("DATE", "1981".to_string()),
        ];
        // Not a whole number of blocks, so the last frame is a short one.
        std::fs::write(&path, flac(187 * RATE, &tags)).unwrap();
        let file = lofty::probe::Probe::open(&path).unwrap().read().unwrap();
        assert_eq!(file.properties().duration().as_secs(), 187);
        let tag = file.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Aria"));
        assert_eq!(tag.album().as_deref(), Some("Goldberg Variations"));
        assert_eq!(tag.get_string(ItemKey::Composer), Some("Johann Sebastian Bach"));
        assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("Glenn Gould"));
        assert_eq!(tag.track(), Some(1));
        assert_eq!(tag.date().map(|d| d.year), Some(1981));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Falsified by a CRC computed over the wrong bytes: the reference
    /// values are the standard check values of FLAC's two CRCs.
    #[test]
    fn the_crcs_are_flacs() {
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16(b"123456789"), 0xFEE8);
    }

    #[test]
    fn a_segment_is_safe() {
        assert_eq!(safe("AC/DC: Live?"), "AC-DC- Live-");
        assert_eq!(safe("  .hidden "), "hidden");
        assert_eq!(safe(""), "untitled");
    }
}
