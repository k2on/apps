//! What harken's screens say of what they show, worked out once: where a
//! cover is, how long a track or a record runs, how many of something.

use ark::format::Encode;
use ark_app::Data;
use harken_domain::HarkenView;

/// The rows of `key` as `T`: harken's views hold each row as its bytes (the
/// value format), read here as the screen's type; none while it is not held.
pub fn rows<T: Encode>(data: &Data<'_, HarkenView>, key: &HarkenView) -> Vec<T> {
    data.rows::<Vec<u8>>(key).iter().filter_map(|b| T::from_bytes(b).ok()).collect()
}

/// Where `art` (a path under the server's media root, or a whole URL) is
/// fetched from: `server`'s `/media`, each segment percent-encoded; none
/// for none.
pub fn art_url(server: &str, art: &str) -> String {
    match art.is_empty() {
        true => String::new(),
        false => harken_domain::host::url(server, art),
    }
}

/// Each song's cover: its album's, by the song's id (its hex), from the
/// songs' details (which album each is on) and the albums (each one's
/// cover), both of them views a screen that draws a song's cover holds
/// ([`Covers::VIEWS`]). A song on no album, or an album with no cover, has
/// none, and is drawn as its derived square.
#[derive(Clone, Debug, Default)]
pub struct Covers {
    server: String,
    by: std::collections::BTreeMap<String, String>,
}

impl Covers {
    /// The views a song's cover is read from.
    pub const VIEWS: [HarkenView; 2] = [HarkenView::TrackDetails, HarkenView::Albums];

    /// The covers as `server` serves them, from the songs' details and the
    /// albums.
    pub fn of(server: &str, details: &[harken_domain::TrackDetailsEntry], albums: &[harken_domain::AlbumsEntry]) -> Covers {
        let art: std::collections::BTreeMap<&str, &str> = albums
            .iter()
            .filter(|a| !a.art.is_empty())
            .map(|a| (a.id.as_str(), a.art.as_str()))
            .collect();
        let by = details
            .iter()
            .filter_map(|d| art.get(d.album_id.as_str()).map(|a| (d.media_id.hex(), a.to_string())))
            .collect();
        Covers { server: server.into(), by }
    }

    /// The covers as a screen's views hold them.
    pub fn read(server: &str, data: &Data<'_, HarkenView>) -> Covers {
        let [details, albums] = &Covers::VIEWS;
        Covers::of(server, &rows(data, details), &rows(data, albums))
    }

    /// Where the song `media` (its id's hex)'s cover is fetched from; none
    /// for none.
    pub fn track(&self, media: &str) -> String {
        self.by.get(media).map_or(String::new(), |a| art_url(&self.server, a))
    }
}

/// How long a track runs (`4:21`), nothing for no length.
pub fn length(ms: i64) -> String {
    match ms > 0 {
        true => ark_format::clock(ms),
        false => String::new(),
    }
}

/// How many tracks: "1 track", "12 tracks".
pub fn tracks(n: i64) -> String {
    match n {
        1 => "1 track".into(),
        n => format!("{n} tracks"),
    }
}

/// How many episodes: "1 episode", "8 episodes".
pub fn episodes(n: i64) -> String {
    match n {
        1 => "1 episode".into(),
        n => format!("{n} episodes"),
    }
}

/// A year as a record says it; nothing for "nobody said".
pub fn year(y: i64) -> String {
    match y > 0 {
        true => y.to_string(),
        false => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_is_under_the_servers_media_and_a_whole_url_is_itself() {
        assert_eq!(
            art_url("http://music.local:8787", "Bach/Goldberg/cover.jpg"),
            "http://music.local:8787/media/Bach/Goldberg/cover.jpg"
        );
        assert_eq!(art_url("http://x", "https://e.org/a.png"), "https://e.org/a.png");
        assert_eq!(art_url("http://x", ""), "");
    }

    #[test]
    fn counts_and_lengths_read_as_people_say_them() {
        assert_eq!(
            (tracks(1), tracks(12), episodes(1)),
            ("1 track".into(), "12 tracks".into(), "1 episode".into())
        );
        assert_eq!(
            (length(261_000), length(0), year(0), year(1981)),
            ("4:21".into(), String::new(), String::new(), "1981".into())
        );
    }
}
