//! Podcasts and sermons read off their feeds (`HARKEN_PODCASTS`,
//! `HARKEN_SERMONS`: RSS URLs — a podcast host's, a church's, an
//! Audiobookshelf library's): each feed's newest episodes and its pictures
//! downloaded into the data directory and served at `/media/.feeds/`, and
//! the show and its episodes authored by the scanner (`library.rs`) as
//! `library.add_show` and `library.add_episode`, each only if it would
//! change something.
//!
//! The episodes are the server's, not the feed's: a client plays the copy
//! here, so an episode the host takes down or moves still plays, a speaker
//! fetches it from the house, and a phone offline has what it downloaded
//! from this server. A file is named by its URL's hash, as a catalogue's
//! is (`catalogue::name_of`), so a refresh downloads only what is new.
//! Nothing is deleted: an episode gone from the feed stays.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::catalogue::{agent, download, name_of};
use crate::domain::keys::show_key;
use crate::domain::{AddEpisode, AddShow};

/// Where under `/media` the downloads are served.
pub const DIR: &str = ".feeds";

/// How many of a feed's newest episodes are downloaded, unless told
/// otherwise (`HARKEN_FEED_EPISODES`, 0 for all).
pub const KEEP: usize = 20;

/// How often every feed is read again.
pub const EVERY: Duration = Duration::from_secs(60 * 60);

const ITUNES: &str = "http://www.itunes.com/dtds/podcast-1.0.dtd";

pub fn dir(data: &Path) -> PathBuf {
    data.join("feeds")
}

/// A feed the server follows: what kind of show it is, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    /// `podcast` or `sermon`.
    pub kind: String,
    pub url: String,
}

/// The feeds a server follows, and how much of each it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feeds {
    pub feeds: Vec<Feed>,
    /// The newest this many episodes of each; 0 for every one.
    pub keep: usize,
}

impl Default for Feeds {
    fn default() -> Feeds {
        Feeds {
            feeds: vec![],
            keep: KEEP,
        }
    }
}

impl Feeds {
    /// `HARKEN_PODCASTS` and `HARKEN_SERMONS`, URLs apart by white space or
    /// commas, and `HARKEN_FEED_EPISODES`.
    pub fn of(
        podcasts: Option<&str>,
        sermons: Option<&str>,
        keep: Option<&str>,
    ) -> Result<Feeds, String> {
        let mut feeds = vec![];
        for (kind, urls) in [("podcast", podcasts), ("sermon", sermons)] {
            for url in urls
                .unwrap_or("")
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|u| !u.is_empty())
            {
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(format!("{url}: a feed is an http or https URL"));
                }
                feeds.push(Feed {
                    kind: kind.into(),
                    url: url.into(),
                });
            }
        }
        let keep = match keep.map(str::trim) {
            None | Some("") => KEEP,
            Some(n) => n
                .parse()
                .map_err(|_| format!("HARKEN_FEED_EPISODES: {n} is not a number"))?,
        };
        Ok(Feeds { feeds, keep })
    }

    pub fn is_empty(&self) -> bool {
        self.feeds.is_empty()
    }
}

/// A feed as RSS says it, before anything is downloaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Channel {
    pub title: String,
    pub author: String,
    pub description: String,
    pub art: String,
    pub link: String,
    pub items: Vec<Item>,
}

/// One episode as RSS says it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub guid: String,
    pub published_ms: i64,
    pub duration_ms: i64,
    /// The audio: the enclosure's URL.
    pub url: String,
    pub description: String,
    pub art: String,
    pub season: i64,
    pub number: i64,
}

/// A feed read, and what of it is here: the show, and the episodes whose
/// files were downloaded, as the scanner authors them.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub show: AddShow,
    pub episodes: Vec<AddEpisode>,
    /// How many could not be had: the next refresh tries them.
    pub failed: usize,
}

/// Read a feed's RSS. Its items newest first, those with no audio left out.
pub fn parse(xml: &str) -> Result<Channel, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("not RSS: {e}"))?;
    let channel = doc
        .descendants()
        .find(|n| n.has_tag_name("channel"))
        .ok_or("not RSS: no <channel>")?;
    let text = |n: roxmltree::Node, name: &str, ns: Option<&str>| {
        n.children()
            .find(|c| c.tag_name().name() == name && c.tag_name().namespace() == ns)
            // Every text under it: a description may carry its HTML as
            // elements rather than escaped.
            .map(|c| {
                c.descendants()
                    .filter(|d| d.is_text())
                    .filter_map(|d| d.text())
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .unwrap_or_default()
    };
    let itunes_image = |n: roxmltree::Node| {
        n.children()
            .find(|c| c.has_tag_name((ITUNES, "image")))
            .and_then(|c| c.attribute("href"))
            .map(|h| h.trim().to_string())
            .unwrap_or_default()
    };
    let or = |a: String, b: String| if a.is_empty() { b } else { a };
    let rss_image = channel
        .children()
        .find(|c| c.has_tag_name("image"))
        .map(|i| text(i, "url", None))
        .unwrap_or_default();
    let mut items: Vec<Item> = channel
        .children()
        .filter(|c| c.has_tag_name("item"))
        .filter_map(|i| {
            let enclosure = i.children().find(|c| c.has_tag_name("enclosure"))?;
            let url = enclosure.attribute("url")?.trim().to_string();
            let audio = enclosure
                .attribute("type")
                .is_none_or(|t| t.starts_with("audio") || t.is_empty());
            (audio && !url.is_empty()).then(|| Item {
                title: text(i, "title", None),
                guid: or(text(i, "guid", None), url.clone()),
                published_ms: rfc2822(&text(i, "pubDate", None)).unwrap_or(0),
                duration_ms: duration(&text(i, "duration", Some(ITUNES))),
                description: plain(&or(
                    text(i, "summary", Some(ITUNES)),
                    text(i, "description", None),
                )),
                art: itunes_image(i),
                season: text(i, "season", Some(ITUNES)).parse().unwrap_or(0),
                number: text(i, "episode", Some(ITUNES)).parse().unwrap_or(0),
                url,
            })
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.published_ms));
    Ok(Channel {
        title: text(channel, "title", None),
        author: or(
            text(channel, "author", Some(ITUNES)),
            text(channel, "managingEditor", None),
        ),
        description: plain(&or(
            text(channel, "summary", Some(ITUNES)),
            text(channel, "description", None),
        )),
        art: or(itunes_image(channel), rss_image),
        link: text(channel, "link", None),
        items,
    })
}

/// The feed at `feed.url`, its newest `keep` episodes (0 for all) and its
/// pictures downloaded into `dir`. Blocking, and as long as the downloads
/// take; after each episode downloaded, what is here so far is given to
/// `so_far`, so a show fills as it goes.
pub fn fetch(
    feed: &Feed,
    keep: usize,
    dir: &Path,
    mut so_far: impl FnMut(Fetched),
) -> Result<Fetched, String> {
    let agent = agent();
    let url = &feed.url;
    let xml = agent
        .get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .into_string()
        .map_err(|e| format!("{url}: {e}"))?;
    let channel = parse(&xml).map_err(|e| format!("{url}: {e}"))?;
    if channel.title.is_empty() {
        return Err(format!("{url}: the feed has no title"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut failed = 0;
    // A file here, by its URL: its path under the media root, or empty
    // when it could not be had.
    let here = |u: &str, failed: &mut usize| -> Option<String> {
        if u.is_empty() {
            return None;
        }
        let name = name_of(u);
        let path = dir.join(&name);
        if !path.exists() {
            if let Err(e) = download(&agent, u, &path) {
                *failed += 1;
                eprintln!("harken-server: feeds: {u}: {e}");
                return None;
            }
        }
        Some(format!("{DIR}/{name}"))
    };
    let show = AddShow {
        kind: feed.kind.clone(),
        title: channel.title.clone(),
        author: channel.author.clone(),
        description: channel.description.clone(),
        art: here(&channel.art, &mut failed).unwrap_or_default(),
        feed: url.clone(),
        link: channel.link.clone(),
    };
    let id = show_key(&feed.kind, &channel.title);
    let take = if keep == 0 { channel.items.len() } else { keep };
    let mut episodes = vec![];
    for item in channel.items.iter().take(take) {
        let had = dir.join(name_of(&item.url)).exists();
        let Some(file) = here(&item.url, &mut failed) else {
            continue;
        };
        let art = match item.art == channel.art {
            true => String::new(),
            false => here(&item.art, &mut failed).unwrap_or_default(),
        };
        episodes.push(AddEpisode {
            show: id.clone(),
            title: if item.title.is_empty() {
                item.guid.clone()
            } else {
                item.title.clone()
            },
            duration_ms: item.duration_ms,
            file,
            guid: item.guid.clone(),
            published_ms: item.published_ms,
            description: item.description.clone(),
            source: item.url.clone(),
            art,
            season: item.season,
            number: item.number,
        });
        if !had {
            eprintln!(
                "harken-server: feeds: {}: {} downloaded",
                channel.title,
                episodes.last().map_or("", |e| e.title.as_str())
            );
            so_far(Fetched {
                show: show.clone(),
                episodes: episodes.clone(),
                failed,
            });
        }
    }
    Ok(Fetched {
        show,
        episodes,
        failed,
    })
}

/// `itunes:duration`: seconds, `MM:SS` or `HH:MM:SS`, as milliseconds; 0
/// for anything else.
pub fn duration(s: &str) -> i64 {
    let mut total = 0f64;
    for part in s.trim().split(':') {
        let Ok(n) = part.trim().parse::<f64>() else {
            return 0;
        };
        total = total * 60.0 + n;
    }
    (total * 1000.0) as i64
}

/// An RFC 2822 date (`Sun, 25 Oct 2026 16:01:48 +0000`), as RSS writes
/// `pubDate`, in milliseconds since 1970.
pub fn rfc2822(s: &str) -> Option<i64> {
    let s = s.split_once(',').map_or(s, |(_, rest)| rest).trim();
    let mut parts = s.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let month = parts.next()?.get(..3)?.to_ascii_lowercase();
    let month = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|m| *m == month)? as i64
        + 1;
    let mut year: i64 = parts.next()?.parse().ok()?;
    if year < 100 {
        year += if year < 50 { 2000 } else { 1900 };
    }
    let time = parts.next().unwrap_or("00:00:00");
    let mut hms = time.split(':').map(|p| p.parse::<i64>().ok());
    let (h, m, sec) = (
        hms.next()??,
        hms.next()??,
        hms.next().flatten().unwrap_or(0),
    );
    let offset_min = match parts.next().unwrap_or("+0000") {
        z if z.len() == 5 && (z.starts_with('+') || z.starts_with('-')) => {
            let n: i64 = z[1..].parse().ok()?;
            let m = (n / 100) * 60 + n % 100;
            if z.starts_with('-') {
                -m
            } else {
                m
            }
        }
        "EST" | "CDT" => -5 * 60,
        "EDT" => -4 * 60,
        "MST" | "PDT" => -7 * 60,
        "CST" | "MDT" => -6 * 60,
        "PST" => -8 * 60,
        _ => 0,
    };
    let days = days_from_civil(year, month, day);
    Some(((days * 86_400 + h * 3600 + m * 60 + sec) - offset_min * 60) * 1000)
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A feed's description as plain text: its HTML tags taken out, its
/// entities read, its white space one space, and no longer than a page
/// wants.
pub fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "\u{2019}")
        .replace("&lsquo;", "\u{2018}")
        .replace("&rdquo;", "\u{201d}")
        .replace("&ldquo;", "\u{201c}");
    let words: Vec<&str> = out.split_whitespace().collect();
    let mut said = words.join(" ");
    if said.chars().count() > 2000 {
        said = said.chars().take(1999).collect::<String>() + "\u{2026}";
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Full Time Nix</title>
    <link>https://fulltimenix.com</link>
    <description><![CDATA[<p>Real chats with brilliant Nixers</p>]]></description>
    <itunes:author>Shahar "Dawn" Or</itunes:author>
    <image><url>https://img/rss.jpg</url><title>Full Time Nix</title></image>
    <itunes:image href="https://img/show.jpg"/>
    <item>
      <title>Nix Freaks 50</title>
      <guid isPermaLink="false">ep-50</guid>
      <pubDate>Sun, 11 Oct 2026 16:01:48 +0000</pubDate>
      <itunes:duration>01:02:03</itunes:duration>
      <enclosure url="https://media/50.mp3" type="audio/mpeg" length="1"/>
      <itunes:episode>50</itunes:episode>
      <itunes:image href="https://img/show.jpg"/>
    </item>
    <item>
      <title>Nix Freaks 51</title>
      <pubDate>Sun, 18 Oct 2026 16:01:48 -0100</pubDate>
      <itunes:duration>125</itunes:duration>
      <description>Wild &amp; <b>spontaneous</b></description>
      <enclosure url="https://media/51.mp3" type="audio/mpeg" length="1"/>
    </item>
    <item>
      <title>A video</title>
      <enclosure url="https://media/v.mp4" type="video/mp4" length="1"/>
    </item>
  </channel>
</rss>"#;

    /// A feed reads as its show and its audio episodes, newest first.
    /// Falsified by reading `<image><title>` as the show's title.
    #[test]
    fn a_feed_is_a_show_and_its_episodes() {
        let c = parse(RSS).unwrap();
        assert_eq!(c.title, "Full Time Nix");
        assert_eq!(c.author, "Shahar \"Dawn\" Or");
        assert_eq!(c.description, "Real chats with brilliant Nixers");
        assert_eq!(c.art, "https://img/show.jpg");
        let titles: Vec<&str> = c.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Nix Freaks 51", "Nix Freaks 50"],
            "the video is left out"
        );
        assert_eq!(c.items[0].duration_ms, 125_000);
        assert_eq!(
            c.items[0].guid, "https://media/51.mp3",
            "no guid: the audio's URL"
        );
        assert_eq!(c.items[0].description, "Wild & spontaneous");
        assert_eq!(c.items[1].duration_ms, 3_723_000);
        assert_eq!((c.items[1].guid.as_str(), c.items[1].number), ("ep-50", 50));
    }

    /// Falsified by adding the offset rather than taking it away.
    #[test]
    fn a_pub_date_is_milliseconds_since_1970() {
        assert_eq!(rfc2822("Thu, 01 Jan 1970 00:00:00 +0000"), Some(0));
        assert_eq!(
            rfc2822("Sun, 25 Oct 2026 16:01:48 +0000"),
            Some(1_792_944_108_000)
        );
        assert_eq!(
            rfc2822("Sun, 25 Oct 2026 17:01:48 +0100"),
            Some(1_792_944_108_000)
        );
        assert_eq!(rfc2822("25 Oct 2026 11:01:48 EST"), Some(1_792_944_108_000));
        assert_eq!(rfc2822("yesterday"), None);
    }

    #[test]
    fn the_feeds_a_server_follows_are_its_environment() {
        let f = Feeds::of(
            Some("https://a/feed.xml, https://b/rss"),
            Some("https://c/sermons.xml"),
            None,
        )
        .unwrap();
        let kinds: Vec<(&str, &str)> = f
            .feeds
            .iter()
            .map(|f| (f.kind.as_str(), f.url.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("podcast", "https://a/feed.xml"),
                ("podcast", "https://b/rss"),
                ("sermon", "https://c/sermons.xml")
            ]
        );
        assert_eq!(f.keep, KEEP);
        assert_eq!(Feeds::of(None, None, Some("0")).unwrap().keep, 0);
        assert!(Feeds::of(Some("feed.xml"), None, None).is_err());
    }
}
