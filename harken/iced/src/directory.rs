//! Finding a show to follow: Apple's podcast directory, searched by what is
//! typed, or a feed's own address pasted in.
//!
//! The directory is asked from here, not through the server: its search
//! answers any origin (`Access-Control-Allow-Origin: *`), as its pictures
//! do, and what is searched for is nobody else's business. What comes back
//! is a list of shows with their feeds — following one is the same
//! `library.subscribe` a pasted address is.
//!
//! A search waits a moment after the last key ([`PAUSE`]) and goes only if
//! nothing has been typed since, so a word typed is one request rather than
//! one a letter.

use iced::Task;
use serde::Deserialize;

/// How long the typing has to stop before a search goes.
pub const PAUSE: u64 = 300;

/// The most shows one search lists.
const LIMIT: usize = 25;

/// A show the directory found: what it is called, who makes it, its feed
/// and its picture's address.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    pub title: String,
    pub author: String,
    pub feed: String,
    pub art: String,
}

/// Whether what was typed is a feed's address rather than words to search
/// for.
pub fn is_feed(typed: &str) -> bool {
    let t = typed.trim();
    t.starts_with("http://") || t.starts_with("https://")
}

/// The directory's search for `words`, podcasts only.
pub fn search_url(words: &str) -> String {
    format!(
        "https://itunes.apple.com/search?media=podcast&entity=podcast&limit={LIMIT}&term={}",
        arkui::url::encoded(words.trim())
    )
}

/// What the directory answered, as shows: each that has a feed, which is
/// every one that can be followed.
pub fn parse(json: &str) -> Result<Vec<Found>, String> {
    #[derive(Deserialize)]
    struct Answer {
        results: Vec<Show>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Show {
        #[serde(default)]
        collection_name: String,
        #[serde(default)]
        artist_name: String,
        #[serde(default)]
        feed_url: String,
        #[serde(default)]
        artwork_url600: String,
        #[serde(default)]
        artwork_url100: String,
    }
    let answer: Answer = serde_json::from_str(json).map_err(|e| format!("the directory said something else: {e}"))?;
    Ok(answer
        .results
        .into_iter()
        .filter(|s| !s.feed_url.is_empty())
        .map(|s| Found {
            title: s.collection_name,
            author: s.artist_name,
            feed: s.feed_url,
            art: match s.artwork_url600.is_empty() {
                true => s.artwork_url100,
                false => s.artwork_url600,
            },
        })
        .collect())
}

/// Search the directory for `words`.
pub fn search(words: String) -> Task<(String, Result<Vec<Found>, String>)> {
    let url = search_url(&words);
    Task::perform(get(url), move |answer| (words.clone(), answer.and_then(|json| parse(&json))))
}

/// A moment, then `()`: the pause a search waits for.
pub fn after(ms: u64) -> Task<()> {
    Task::perform(sleep(ms), |()| ())
}

#[cfg(not(target_arch = "wasm32"))]
fn sleep(ms: u64) -> impl std::future::Future<Output = ()> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        let _ = tx.send(());
    });
    async move {
        let _ = rx.await;
    }
}

/// The body of a GET, on a thread of its own as a picture is fetched
/// (`arkui::images`): the desktop has no runtime to await it on.
#[cfg(not(target_arch = "wasm32"))]
fn get(url: String) -> impl std::future::Future<Output = Result<String, String>> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let answer = ureq::get(&url)
            .call()
            .map_err(|e| format!("could not search: {e}"))
            .and_then(|r| r.into_string().map_err(|e| format!("could not read the answer: {e}")));
        let _ = tx.send(answer);
    });
    async move { rx.await.unwrap_or_else(|_| Err("the search was dropped".into())) }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(inline_js = r#"
export function later(ms, done) { setTimeout(done, ms); }
export function get(url, done) {
  fetch(url, { mode: "cors" })
    .then((r) => r.ok ? r.text().then((t) => done(true, t)) : done(false, "HTTP " + r.status))
    .catch((e) => done(false, String(e)));
}
"#)]
    extern "C" {
        pub fn later(ms: u32, done: &JsValue);
        pub fn get(url: &str, done: &JsValue);
    }
}

#[cfg(target_arch = "wasm32")]
fn sleep(ms: u64) -> impl std::future::Future<Output = ()> {
    use wasm_bindgen::prelude::*;
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    let done = Closure::once_into_js(move || {
        let _ = tx.send(());
    });
    web::later(ms as u32, &done);
    async move {
        let _ = rx.await;
    }
}

#[cfg(target_arch = "wasm32")]
fn get(url: String) -> impl std::future::Future<Output = Result<String, String>> {
    use wasm_bindgen::prelude::*;
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    let done = Closure::once_into_js(move |ok: bool, body: String| {
        let _ = tx.send(match ok {
            true => Ok(body),
            false => Err(format!("could not search: {body}")),
        });
    });
    web::get(&url, &done);
    async move { rx.await.unwrap_or_else(|_| Err("the search was dropped".into())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory's answer, as it spells it: a show with no feed cannot
    /// be followed, so it is not listed. Falsified by listing every result:
    /// the one with no `feedUrl` was there.
    #[test]
    fn a_search_lists_the_shows_with_feeds() {
        let json = r#"{"resultCount":2,"results":[
          {"kind":"podcast","collectionName":"Full Time Nix","artistName":"Shahar \"Dawn\" Or",
           "feedUrl":"https://feeds.transistor.fm/full-time-nix",
           "artworkUrl100":"https://a/100x100bb.jpg","artworkUrl600":"https://a/600x600bb.jpg"},
          {"kind":"podcast","collectionName":"No Feed","artistName":"Somebody"}]}"#;
        assert_eq!(
            parse(json).unwrap(),
            [Found {
                title: "Full Time Nix".into(),
                author: "Shahar \"Dawn\" Or".into(),
                feed: "https://feeds.transistor.fm/full-time-nix".into(),
                art: "https://a/600x600bb.jpg".into(),
            }]
        );
        assert!(parse("<html>").is_err());
    }

    /// An address is followed as it is; words are searched for, escaped.
    #[test]
    fn an_address_is_a_feed_and_words_a_search() {
        assert!(is_feed(" https://feeds.transistor.fm/full-time-nix"));
        assert!(!is_feed("full time nix"));
        assert!(search_url("gospel & life").ends_with("&term=gospel%20%26%20life"));
    }
}
