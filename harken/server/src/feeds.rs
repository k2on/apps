//! Podcasts and sermons read off the feeds the library follows (a
//! `subscription` row each, added from a client): a source of the library
//! (`sync.rs`). Each feed is read — its RSS, and its pictures downloaded
//! into the data directory — and said as a plan: what was read
//! (`library.feed_read`), the show (`library.add_show`) and its newest
//! episodes (`library.add_episode`, as many as the subscription keeps),
//! each episode's file still to come and then downloaded and served at
//! `/media/.feeds/`. A feed that cannot be read is said so, on its
//! subscription.
//!
//! The episodes are the server's, not the feed's: a client plays the copy
//! here, so an episode the host takes down or moves still plays, a speaker
//! fetches it from the house, and a phone offline has what it downloaded
//! from this server. Nothing is deleted: an episode gone from the feed
//! stays, until the feed is unfollowed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ark::format::Encode;

use crate::domain::keys::show_key;
use crate::domain::{
    AddEpisode, AddShow, ChapterAt, FeedRead, PersonOn, SetChapters, SetPeople, Subscription,
};
use crate::sync::{self, Call, Plan, Source, Want};

/// Where under `/media` the downloads are served.
pub const DIR: &str = ".feeds";

/// How often every feed is read again.
pub const EVERY: Duration = Duration::from_secs(60 * 60);

const ITUNES: &str = "http://www.itunes.com/dtds/podcast-1.0.dtd";

/// Podcasting 2.0's namespace: `<podcast:chapters url type>`, a JSON file
/// of chapters beside the audio; `<podcast:person role href img>`,
/// somebody on it. (Its `<podcast:transcript>` is not read: timed to the
/// host's file, it is not to this one; `transcribe.rs` hears this one.)
const PODCAST: &str = "https://podcastindex.org/namespace/1.0";

/// Podlove Simple Chapters: `<psc:chapter start title href image>` in the
/// item itself.
const PSC: &str = "http://podlove.org/simple-chapters";

pub fn dir(data: &Path) -> PathBuf {
    data.join("feeds")
}

/// The feeds the library follows, as the scanner last read them off its
/// replica, kept in `dir`.
pub struct Follow {
    pub subscriptions: Arc<Mutex<Vec<Subscription>>>,
    pub dir: PathBuf,
}

impl Source for Follow {
    fn name(&self) -> &str {
        "feeds"
    }

    /// Every feed followed, a plan each, gated on it still being followed.
    fn read(&mut self) -> (Vec<Plan>, usize) {
        let subscriptions = self
            .subscriptions
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        let agent = sync::agent();
        let mut failed = 0;
        let plans = subscriptions
            .iter()
            .map(|s| match plan(&agent, s, &self.dir, &mut failed) {
                Ok(plan) => plan,
                Err(error) => {
                    eprintln!("harken-server: feeds: {error}");
                    let read = FeedRead {
                        url: s.url.clone(),
                        error,
                        ..FeedRead::default()
                    };
                    Plan {
                        what: s.url.clone(),
                        calls: vec![call("library.feed_read", &s.url, &read)],
                        gate: true,
                        files: vec![],
                    }
                }
            })
            .collect();
        (plans, failed)
    }

    fn every(&self) -> Option<Duration> {
        Some(EVERY)
    }
}

fn call(name: &'static str, what: &str, input: &impl Encode) -> Call {
    Call {
        name,
        what: what.to_string(),
        input: input.to_bytes(),
        if_there: false,
    }
}

/// One feed read: what it is, its show and its newest episodes as the
/// subscription keeps them, their pictures downloaded into `dir`.
pub fn plan(
    agent: &ureq::Agent,
    subscription: &Subscription,
    dir: &Path,
    failed: &mut usize,
) -> Result<Plan, String> {
    let url = &subscription.url;
    let xml = sync::text(agent, url)?;
    let channel = parse(&xml).map_err(|e| format!("{url}: {e}"))?;
    if channel.title.is_empty() {
        return Err(format!("{url}: the feed has no title"));
    }
    let kind = &subscription.kind;
    let picture =
        |u: &str, failed: &mut usize| sync::picture(agent, u, DIR, dir, failed).unwrap_or_default();
    let read = FeedRead {
        url: url.clone(),
        title: channel.title.clone(),
        items: channel.items.len() as i64,
        error: String::new(),
    };
    let show = AddShow {
        kind: kind.clone(),
        title: channel.title.clone(),
        author: channel.author.clone(),
        description: channel.description.clone(),
        art: picture(&channel.art, failed),
        feed: url.clone(),
        link: channel.link.clone(),
    };
    let mut calls = vec![
        call("library.feed_read", url, &read),
        call("library.add_show", &channel.title, &show),
    ];
    let id = show_key(kind, &channel.title);
    let take = match subscription.keep {
        n if n <= 0 => channel.items.len(),
        n => n as usize,
    };
    let mut files = vec![];
    for item in channel.items.iter().take(take) {
        let want = Want::of(&item.url, DIR, dir);
        let art = match item.art == channel.art {
            true => String::new(),
            false => picture(&item.art, failed),
        };
        let episode = AddEpisode {
            show: id.clone(),
            title: if item.title.is_empty() {
                item.guid.clone()
            } else {
                item.title.clone()
            },
            duration_ms: item.duration_ms,
            file: want.file.clone(),
            guid: item.guid.clone(),
            published_ms: item.published_ms,
            description: item.description.clone(),
            source: item.url.clone(),
            art,
            season: item.season,
            number: item.number,
        };
        calls.push(call("library.add_episode", &episode.title, &episode));
        let chapters = match item.chapters.is_empty() {
            true => json_chapters(agent, &item.chapters_url, dir, failed),
            false => item.chapters.clone(),
        };
        if !chapters.is_empty() {
            let set = SetChapters {
                file: want.file.clone(),
                chapters,
            };
            calls.push(call("library.set_chapters", &episode.title, &set));
        }
        let people = match item.people.is_empty() {
            true => &channel.people,
            false => &item.people,
        };
        if !people.is_empty() {
            let set = SetPeople {
                file: want.file.clone(),
                people: people
                    .iter()
                    .map(|p| PersonOn {
                        art: picture(&p.art, failed),
                        ..p.clone()
                    })
                    .collect(),
            };
            calls.push(call("library.set_people", &episode.title, &set));
        }
        files.push(want);
    }
    Ok(Plan {
        what: channel.title.clone(),
        calls,
        gate: true,
        files,
    })
}

/// A feed as RSS says it, before anything is downloaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Channel {
    pub title: String,
    pub author: String,
    pub description: String,
    pub art: String,
    pub link: String,
    /// Who is on every episode, unless an episode names its own.
    pub people: Vec<PersonOn>,
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
    /// Its chapters as the item says them (Podlove's), and where its JSON
    /// chapters are (Podcasting 2.0's), when it says.
    pub chapters: Vec<ChapterAt>,
    pub chapters_url: String,
    /// Who is on it, their pictures where the feed has them (not yet
    /// downloaded).
    pub people: Vec<PersonOn>,
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
                chapters: psc(i),
                chapters_url: i
                    .children()
                    .find(|c| c.has_tag_name((PODCAST, "chapters")))
                    .and_then(|c| c.attribute("url"))
                    .map(|u| u.trim().to_string())
                    .unwrap_or_default(),
                people: people(i),
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
        people: people(channel),
        items,
    })
}

/// The `<podcast:person>`s right under a channel or an item, in order.
/// Their role is as the feed says it, `Host` when it says none (the
/// namespace's default).
fn people(node: roxmltree::Node) -> Vec<PersonOn> {
    node.children()
        .filter(|c| c.has_tag_name((PODCAST, "person")))
        .filter_map(|c| {
            let name = c.text().unwrap_or("").trim().to_string();
            let attr = |a: &str| c.attribute(a).unwrap_or("").trim().to_string();
            let role = match attr("role") {
                r if r.is_empty() => "Host".to_string(),
                r => capitalised(&r),
            };
            (!name.is_empty()).then(|| PersonOn {
                name,
                role,
                link: attr("href"),
                art: attr("img"),
            })
        })
        .collect()
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// An item's Podlove Simple Chapters, in order.
fn psc(item: roxmltree::Node) -> Vec<ChapterAt> {
    let Some(list) = item.children().find(|c| c.has_tag_name((PSC, "chapters"))) else {
        return vec![];
    };
    let mut out: Vec<ChapterAt> = list
        .children()
        .filter(|c| c.has_tag_name((PSC, "chapter")))
        .filter_map(|c| {
            let start = c.attribute("start")?;
            Some(ChapterAt {
                start_ms: duration(start),
                title: c.attribute("title").unwrap_or("").trim().to_string(),
                url: c.attribute("href").unwrap_or("").trim().to_string(),
                image: c.attribute("image").unwrap_or("").trim().to_string(),
            })
        })
        .collect();
    out.sort_by_key(|c| c.start_ms);
    out
}

/// Podcasting 2.0's JSON chapters at `url`, kept in `dir` as a picture is
/// — a file once had is not asked for again — and read: none when there
/// is no URL or it cannot be had (counted in `failed`, so it is tried at
/// the next read).
fn json_chapters(agent: &ureq::Agent, url: &str, dir: &Path, failed: &mut usize) -> Vec<ChapterAt> {
    if url.is_empty() {
        return vec![];
    }
    let want = Want::of(url, DIR, dir);
    if !want.here() {
        if let Err(e) = sync::download(agent, url, &want.path, |_, _| {}) {
            *failed += 1;
            eprintln!("harken-server: feeds: chapters: {e}");
            return vec![];
        }
    }
    std::fs::read_to_string(&want.path)
        .map(|j| chapters_of(&j))
        .unwrap_or_default()
}

/// A JSON chapters file (`{"chapters": [{"startTime", "title", "url",
/// "img", "toc"}]}`), in order: those marked out of the table of contents
/// (`"toc": false`) left out.
pub fn chapters_of(json: &str) -> Vec<ChapterAt> {
    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        chapters: Vec<One>,
    }
    #[derive(serde::Deserialize)]
    struct One {
        #[serde(rename = "startTime", default)]
        start: f64,
        #[serde(default)]
        title: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        img: String,
        #[serde(default = "shown")]
        toc: bool,
    }
    fn shown() -> bool {
        true
    }
    let Ok(file) = serde_json::from_str::<File>(json) else {
        return vec![];
    };
    let mut out: Vec<ChapterAt> = file
        .chapters
        .into_iter()
        .filter(|c| c.toc && c.start >= 0.0)
        .map(|c| ChapterAt {
            start_ms: (c.start * 1000.0) as i64,
            title: c.title.trim().to_string(),
            url: c.url.trim().to_string(),
            image: c.img.trim().to_string(),
        })
        .collect();
    out.sort_by_key(|c| c.start_ms);
    out
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

    /// An item's chapters: Podlove's in the item, in order, and where its
    /// JSON chapters are; the JSON read with those out of the table of
    /// contents left out. Falsified by reading `start` as whole seconds:
    /// the half second was lost.
    #[test]
    fn an_items_chapters_are_read() {
        let rss = r#"<rss xmlns:psc="http://podlove.org/simple-chapters" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>T</title>
            <item><title>A</title><enclosure url="https://m/a.mp3" type="audio/mpeg"/>
              <psc:chapters version="1.2"><psc:chapter start="00:01:02.500" title="News"/><psc:chapter start="0" title="Intro" href="https://x"/></psc:chapters></item>
            <item><title>B</title><enclosure url="https://m/b.mp3" type="audio/mpeg"/>
              <podcast:chapters url="https://m/b.json" type="application/json+chapters"/></item>
        </channel></rss>"#;
        let c = parse(rss).unwrap();
        let a = c.items.iter().find(|i| i.title == "A").unwrap();
        let read: Vec<(i64, &str, &str)> = a
            .chapters
            .iter()
            .map(|c| (c.start_ms, c.title.as_str(), c.url.as_str()))
            .collect();
        assert_eq!(read, [(0, "Intro", "https://x"), (62_500, "News", "")]);
        let b = c.items.iter().find(|i| i.title == "B").unwrap();
        assert_eq!(
            (b.chapters.len(), b.chapters_url.as_str()),
            (0, "https://m/b.json")
        );
        let json = r#"{"version":"1.2.0","chapters":[{"startTime":95.5,"title":"Two","img":"https://i"},{"startTime":0,"title":"One"},{"startTime":40,"title":"Hidden","toc":false}]}"#;
        let read: Vec<(i64, String, String)> = chapters_of(json)
            .into_iter()
            .map(|c| (c.start_ms, c.title, c.image))
            .collect();
        assert_eq!(
            read,
            [
                (0, "One".into(), "".into()),
                (95_500, "Two".into(), "https://i".into())
            ]
        );
        assert!(chapters_of("not json").is_empty());
    }

    /// An item's people are its own, or else its channel's. Full Time
    /// Nix's, cut down.
    #[test]
    fn an_items_people_are_read() {
        let rss = r#"<rss xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>T</title>
            <podcast:person href="https://h">Shahar "Dawn" Or</podcast:person>
            <item><title>A</title><enclosure url="https://m/a.mp3" type="audio/mpeg"/>
              <podcast:person role="host" href="https://h" img="https://i/h.jpg">Shahar "Dawn" Or</podcast:person>
              <podcast:person role="Guest" href="https://galowicz.de">Jacek Galowicz</podcast:person>
              </item>
            <item><title>B</title><enclosure url="https://m/b.mp3" type="audio/mpeg"/>
              </item>
        </channel></rss>"#;
        let c = parse(rss).unwrap();
        let who = |people: &[PersonOn]| -> Vec<(String, String, String)> {
            people
                .iter()
                .map(|p| (p.name.clone(), p.role.clone(), p.art.clone()))
                .collect()
        };
        assert_eq!(
            who(&c.people),
            [("Shahar \"Dawn\" Or".into(), "Host".into(), "".into())]
        );
        let a = c.items.iter().find(|i| i.title == "A").unwrap();
        assert_eq!(
            who(&a.people),
            [
                (
                    "Shahar \"Dawn\" Or".into(),
                    "Host".into(),
                    "https://i/h.jpg".into()
                ),
                ("Jacek Galowicz".into(), "Guest".into(), "".into())
            ]
        );
        let b = c.items.iter().find(|i| i.title == "B").unwrap();
        assert!(b.people.is_empty());
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
}
