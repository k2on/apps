//! An address: what a URL says of where an app is, on every shell — the
//! browser's address bar, the desktop's, a phone's deep link, a server's
//! request. Its **path** names the content (`/account/<id>/checking/`), and
//! its **query** the UI's state over it (`?inspector=1&search=rent`), as
//! Next.js has them; an app's `Route` enum ([`crate::Routable`]) is the
//! path's meaning, and the query is the shell's.
//!
//! **Spelled one way** (ui/docs/app.md, "Addresses"): every path ends in a
//! slash (`/budget/`, the root `/`); each segment is percent-encoded
//! ([`crate::url::encode`]), a kind is lowercase kebab (`expanding-card`),
//! a row is its 32 lowercase hex digits, a month is `YYYY-MM`; query keys
//! are lowercase kebab, written in the order set, and a key with no value
//! is not written. An app served under a base (`/apps/docs/`, `/money/`)
//! writes its addresses under it, and reads them back without it.

use crate::url;

/// Where an app is, as a URL says it: the path's segments, decoded, and the
/// query's keys and values, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Address {
    pub path: Vec<String>,
    pub query: Vec<(String, String)>,
}

/// A query's value: `+` is a space, as a form sends it, and what does not
/// decode is kept as written.
fn value(s: &str) -> String {
    url::decode(&s.replace('+', " "))
}

impl Address {
    /// The address at `path`, its UI at its defaults.
    pub fn at<S: Into<String>>(path: impl IntoIterator<Item = S>) -> Address {
        Address {
            path: path.into_iter().map(Into::into).collect(),
            query: vec![],
        }
    }

    /// The path's `i`th segment.
    pub fn seg(&self, i: usize) -> Option<&str> {
        self.path.get(i).map(String::as_str)
    }

    /// The query's `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The query's `key` set to `value`, or taken out when `value` is empty.
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Address {
        let value = value.into();
        self.query.retain(|(k, _)| k != key);
        if !value.is_empty() {
            self.query.push((key.into(), value));
        }
        self
    }

    /// The path alone: the page a static site writes for it.
    pub fn bare(&self) -> Address {
        Address::at(self.path.clone())
    }

    /// `/budget/2026-11/`: the path, each segment percent-encoded, a slash
    /// after each.
    pub fn path_url(&self) -> String {
        let mut out = String::from("/");
        for s in &self.path {
            url::encode(s, &mut out);
            out.push('/');
        }
        out
    }

    /// `?inspector=1&search=rent`, or nothing.
    pub fn query_url(&self) -> String {
        let mut out = String::new();
        for (i, (k, v)) in self.query.iter().enumerate() {
            out.push(if i == 0 { '?' } else { '&' });
            url::encode(k, &mut out);
            out.push('=');
            url::encode(v, &mut out);
        }
        out
    }

    /// The whole address: the path, then the query.
    pub fn url(&self) -> String {
        format!("{}{}", self.path_url(), self.query_url())
    }

    /// The whole address under `base` (`/apps/docs/`).
    pub fn url_under(&self, base: &str) -> String {
        format!("{}{}", base.trim_end_matches('/'), self.url())
    }

    /// What `link` says, read back, under `base` (`/` for none): a URL's
    /// path and query (`/money/budget/?inspector=1`), a phone's link
    /// (`money://budget/`, its scheme dropped), a desktop's fragment
    /// (`#/budget/`), with or without the slashes at either end and an
    /// `index.html` at its end. Never fails: what does not decode is kept
    /// as written.
    pub fn of_url(link: &str, base: &str) -> Address {
        let link = match link.split_once("://") {
            // A scheme's: the rest is the path, its host the first segment.
            Some((scheme, rest))
                if !scheme.is_empty() && scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') =>
            {
                rest
            }
            _ => link,
        };
        let link = link.trim_start_matches('#');
        let link = link.split('#').next().unwrap_or("");
        let (path, query) = link.split_once('?').unwrap_or((link, ""));
        let base = base.trim_matches('/');
        let path = path.trim_start_matches('/');
        let path = match base.is_empty() {
            true => path,
            false => match path.strip_prefix(base) {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
                _ => path,
            },
        };
        let path = path.trim_end_matches("index.html");
        Address {
            path: path.split('/').filter(|s| !s.is_empty()).map(url::decode).collect(),
            query: Address::query_of(query),
        }
    }

    /// A query read (`?search=rent&inspector=1`, the `?` or not).
    pub fn query_of(q: &str) -> Vec<(String, String)> {
        q.trim_start_matches('?')
            .split('&')
            .filter(|kv| !kv.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (value(k), value(v))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every address survives its URL, under a base and not, and an
    /// awkward name in a segment or a value. Falsified by not encoding a
    /// segment: a `/` in a name is two segments.
    #[test]
    fn an_address_survives_its_url() {
        for a in [
            Address::default(),
            Address::at(["budget"]),
            Address::at(["budget", "2026-11", "category", "0a1b"]).with("inspector", "1"),
            Address::at(["transactions", "a b/c"]).with("search", "Trader Joe's & more"),
            Address::default().with("month", "2026-10"),
        ] {
            assert_eq!(Address::of_url(&a.url(), "/"), a, "{}", a.url());
            assert_eq!(Address::of_url(&a.url_under("/money/"), "/money/"), a);
            assert_eq!(Address::of_url(&format!("/money{}index.html", a.path_url()), "/money/"), a.bare());
        }
        assert_eq!(Address::at(["budget"]).with("inspector", "1").url(), "/budget/?inspector=1");
        assert_eq!(Address::default().url(), "/");
        assert_eq!(Address::at(["a b/c"]).path_url(), "/a%20b%2Fc/");
    }

    /// A phone's link, a desktop's fragment and a hand-typed path with no
    /// slashes read as the same address; a base is only taken off whole.
    #[test]
    fn every_spelling_reads_as_one_address() {
        let budget = Address::at(["budget"]).with("inspector", "1");
        for s in [
            "money://budget/?inspector=1",
            "#/budget/?inspector=1",
            "budget?inspector=1",
            "/budget?inspector=1",
        ] {
            assert_eq!(Address::of_url(s, "/"), budget, "{s}");
        }
        assert_eq!(
            Address::of_url("/moneybox/", "/money/"),
            Address::at(["moneybox"]),
            "a base is a whole segment"
        );
        assert_eq!(Address::of_url("/money/", "/money/"), Address::default());
        assert_eq!(Address::of_url("/q/?search=a+b", "/").get("search"), Some("a b"));
        assert_eq!(Address::default().with("search", "").query, vec![], "an empty value is not written");
    }
}
