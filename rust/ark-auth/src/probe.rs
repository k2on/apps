//! Finding an Ark app's server from what somebody typed, any app's: the
//! address read ([`Address::parse`]), its `/healthz` asked ([`find`]) and
//! what it answered decided ([`decide`]). What [`crate::onboarding`]'s
//! `Effect::FindServer` asks of a shell, written once: the phones' app
//! host (ark-ffi's `ArkHost`) carries it out with [`get`]; a test hands
//! [`find`] its own answers.
//!
//! One field takes `host`, `host:port` or a whole URL. A URL with a scheme
//! is taken at its word. Anything else is tried over `https` first, and
//! over `http` as well only where the address is one a certificate is
//! unlikely to cover and nobody outside can reach ([`is_local`]): loopback,
//! the private ranges, link-local, the carrier-grade range a tailnet uses,
//! and a name with no dot or one under `.local`, `.lan`, `.home.arpa` or
//! `.internal`.

use crate::onboarding::Found;

/// What the server field came to: the URLs to try, in order, or why there
/// are none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    /// Base URLs with no trailing slash, `https` first.
    Candidates {
        urls: Vec<String>,
        host: String,
    },
    Invalid(String),
}

impl Address {
    /// What `typed` names.
    pub fn parse(typed: &str) -> Address {
        let text = typed.trim();
        if text.is_empty() {
            return Address::Invalid("Type the address of your server.".into());
        }
        if text.chars().any(char::is_whitespace) {
            return Address::Invalid("An address has no spaces in it.".into());
        }
        let (scheme, rest) = match text.find("://") {
            Some(i) => {
                let s = &text[..i];
                if !["http", "https"].contains(&s.to_lowercase().as_str()) {
                    return Address::Invalid(format!("A server is reached over http or https, not {s}."));
                }
                (s.to_lowercase(), &text[i + 3..])
            }
            None => (String::new(), text),
        };
        let authority: String = rest.chars().take_while(|c| !matches!(c, '/' | '?' | '#')).collect();
        let mut path: String = rest[authority.len()..].chars().take_while(|c| !matches!(c, '?' | '#')).collect();
        while path.ends_with('/') {
            path.pop();
        }
        let bad = || Address::Invalid(format!("That isn't an address a server can have: {text}"));
        if authority.is_empty() || authority.contains('@') {
            return bad();
        }
        let Some(host) = host_of(&authority) else { return bad() };
        let skip = if authority.starts_with('[') { host.len() + 2 } else { host.len() };
        let port = &authority[skip..];
        if !port.is_empty() {
            let n = port.strip_prefix(':').and_then(|p| p.parse::<u32>().ok());
            if !matches!(n, Some(1..=65535)) {
                return Address::Invalid("The port has to be a number from 1 to 65535.".into());
            }
        }
        let base = authority.to_lowercase() + &path;
        let urls = if !scheme.is_empty() {
            vec![format!("{scheme}://{base}")]
        } else if is_local(&host) {
            vec![format!("https://{base}"), format!("http://{base}")]
        } else {
            vec![format!("https://{base}")]
        };
        Address::Candidates {
            urls,
            host: host.to_lowercase(),
        }
    }
}

/// The host of `host`, `host:port`, `[v6]` or `[v6]:port`, or none when it
/// has characters no host has.
fn host_of(authority: &str) -> Option<String> {
    if let Some(inner) = authority.strip_prefix('[') {
        let end = inner.find(']')?;
        let v6 = &inner[..end];
        let ok = !v6.is_empty() && v6.chars().all(|c| c.is_alphanumeric() || c == ':' || c == '.');
        return ok.then(|| v6.to_string());
    }
    let h: String = authority.chars().take_while(|&c| c != ':').collect();
    let ok = !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') && !h.starts_with('.') && !h.starts_with('-');
    ok.then_some(h)
}

/// Whether `host` is one nobody outside this network reaches, where a
/// server is as likely to speak plain http as https.
pub fn is_local(host: &str) -> bool {
    let h = host.to_lowercase();
    let h = h.trim_end_matches('.');
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    if !h.contains('.') && !h.contains(':') {
        return true;
    }
    if [".local", ".lan", ".home.arpa", ".internal"].iter().any(|s| h.ends_with(s)) {
        return true;
    }
    if h.contains(':') {
        return h == "::1" || h.starts_with("fc") || h.starts_with("fd") || h.starts_with("fe80:");
    }
    let parts: Vec<Option<u32>> = h.split('.').map(|p| p.parse().ok()).collect();
    if parts.len() != 4 || parts.iter().any(|p| !matches!(p, Some(0..=255))) {
        return false;
    }
    let (a, b) = (parts[0].unwrap_or(0), parts[1].unwrap_or(0));
    a == 127
        || a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
}

/// What a request came back with, or why it did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Answered {
        status: u16,
        body: String,
    },
    /// No answer: the sentence for why ("nothing answered there").
    Unreachable(String),
}

/// Whether `host` serves this app, from what its `/healthz` answered:
/// `ok`, its connections, the log's head and horizon, and `domain <schema
/// hash> <version> current`. `domain` is this app's own line; `Ok(newer)`
/// when the server runs another schema of it.
pub fn decide(host: &str, status: u16, body: &str, domain: &str) -> Result<bool, String> {
    if status == 404 {
        return Err(format!("{host} answered, but it isn't an Ark server: it has no health page."));
    }
    if status != 200 {
        return Err(format!(
            "{host} answered {status}. It may be starting up or broken; try again in a moment."
        ));
    }
    let lines: Vec<&str> = body.split('\n').map(str::trim).collect();
    if lines.first() != Some(&"ok") {
        return Err(format!("{host} answered, but not the way an Ark server does."));
    }
    let Some(line) = lines.iter().find(|l| l.starts_with("domain ")) else {
        return Err(format!("{host} is an Ark server with no app on it yet."));
    };
    let runs = line["domain ".len()..].trim_end_matches(" current");
    let ours = domain.split(' ').next().unwrap_or(domain);
    let theirs = runs.split(' ').next().unwrap_or(runs);
    Ok(theirs != ours)
}

/// A URL as a person would write it: the host and port.
pub fn display(url: &str) -> &str {
    url.find("://").map_or(url, |i| &url[i + 3..])
}

/// Ask `/healthz` of each URL `typed` could mean, in order, with `get`
/// making the request: the first answer decides — an `https` server that
/// answers and is the wrong app is not tried again in the clear.
pub fn find(typed: &str, domain: &str, get: impl Fn(&str) -> Answer) -> Found {
    let urls = match Address::parse(typed) {
        Address::Invalid(why) => return Found::Failed { why },
        Address::Candidates { urls, .. } => urls,
    };
    let mut last = "no answer".to_string();
    for url in &urls {
        match get(&format!("{url}/healthz")) {
            Answer::Unreachable(why) => last = why,
            Answer::Answered { status, body } => {
                return match decide(display(url), status, &body, domain) {
                    Ok(newer) => Found::Server { url: url.clone(), newer },
                    Err(why) => Found::Failed { why },
                }
            }
        }
    }
    let over = if urls.len() > 1 { " over https or http" } else { "" };
    Found::Failed {
        why: format!("Couldn't reach {}{over}: {last}.", display(&urls[0])),
    }
}

/// A GET asking for JSON, ten seconds to connect and ten to answer: what a
/// shell hands [`find`]. Blocking: call it off the UI thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(url: &str) -> Answer {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(10))
        .build();
    match agent.get(url).set("Accept", "application/json").call() {
        Ok(r) => Answer::Answered {
            status: r.status(),
            body: r.into_string().unwrap_or_default(),
        },
        Err(ureq::Error::Status(status, r)) => Answer::Answered {
            status,
            body: r.into_string().unwrap_or_default(),
        },
        Err(ureq::Error::Transport(t)) => Answer::Unreachable(reason(&t)),
    }
}

/// Why a request failed, in words.
#[cfg(not(target_arch = "wasm32"))]
fn reason(t: &ureq::Transport) -> String {
    use ureq::ErrorKind::*;
    match t.kind() {
        Dns => "there is no such name".into(),
        ConnectionFailed => "nothing answered there".into(),
        Io if t.to_string().contains("timed out") => "it took too long to answer".into(),
        _ if t.to_string().to_lowercase().contains("certificate") => "its certificate isn't one this device trusts".into(),
        _ => t.to_string(),
    }
}

/// The code in `<scheme>://auth?code=…`, the sign-in coming back; none for
/// any other link.
pub fn code_of(url: &str, scheme: &str) -> Option<String> {
    let rest = url.strip_prefix(&format!("{scheme}://auth"))?;
    let query = rest.trim_start_matches('/').strip_prefix('?')?;
    crate::query_value(query, "code").filter(|c| !c.is_empty())
}

/// The launch a link of `scheme` is (`money://?screen=budget&month=…`): its
/// query as arguments (`-screen budget -month …`), as a launch's are; none
/// for any other link.
pub fn launch_args(url: &str, scheme: &str) -> Option<Vec<String>> {
    let query = url.strip_prefix(&format!("{scheme}://?"))?;
    let args: Vec<String> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .flat_map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            [format!("-{}", crate::percent_decode(k)), crate::percent_decode(v)]
        })
        .collect();
    (!args.is_empty()).then_some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(typed: &str) -> Vec<String> {
        match Address::parse(typed) {
            Address::Candidates { urls, .. } => urls,
            Address::Invalid(why) => panic!("{typed}: {why}"),
        }
    }

    /// The phones' rules, letter for letter (ArkUICore's `Address`): a
    /// scheme is taken at its word; a public name is https alone; a local
    /// one is tried in the clear after. Falsified by trying a public name
    /// over http.
    #[test]
    fn an_address_is_read_as_the_phones_read_it() {
        assert_eq!(urls("money.example.com"), ["https://money.example.com"]);
        assert_eq!(urls("http://Money.Example.com/x/"), ["http://money.example.com/x"]);
        assert_eq!(urls("box:8788"), ["https://box:8788", "http://box:8788"]);
        assert_eq!(urls("192.168.1.4"), ["https://192.168.1.4", "http://192.168.1.4"]);
        assert_eq!(urls("[::1]:80"), ["https://[::1]:80", "http://[::1]:80"]);
        assert!(matches!(Address::parse(""), Address::Invalid(_)));
        assert!(matches!(Address::parse("a b"), Address::Invalid(_)));
        assert!(matches!(Address::parse("ftp://x"), Address::Invalid(_)));
        assert!(matches!(Address::parse("x:99999"), Address::Invalid(_)));
        assert!(matches!(Address::parse("me@x"), Address::Invalid(_)));
        assert!(!is_local("8.8.8.8") && is_local("100.64.0.1") && is_local("nas.lan"));
    }

    /// The first answer decides: a wrong app over https is not tried in the
    /// clear; nothing answering is said with both schemes. Falsified by
    /// going on past an answer.
    #[test]
    fn the_first_answer_decides() {
        let domain = "abc 3";
        let ok = |_: &str| Answer::Answered {
            status: 200,
            body: "ok\nconnections 1\ndomain abc 3 current\n".into(),
        };
        assert_eq!(
            find("box", domain, ok),
            Found::Server {
                url: "https://box".into(),
                newer: false
            }
        );
        let newer = |_: &str| Answer::Answered {
            status: 200,
            body: "ok\ndomain def 4 current".into(),
        };
        assert_eq!(
            find("box", domain, newer),
            Found::Server {
                url: "https://box".into(),
                newer: true
            }
        );
        let asked = std::cell::RefCell::new(vec![]);
        let wrong = |u: &str| {
            asked.borrow_mut().push(u.to_string());
            Answer::Answered {
                status: 404,
                body: String::new(),
            }
        };
        assert!(matches!(find("box", domain, wrong), Found::Failed { .. }));
        assert_eq!(*asked.borrow(), ["https://box/healthz"]);
        let none = |_: &str| Answer::Unreachable("nothing answered there".into());
        assert_eq!(
            find("box", domain, none),
            Found::Failed {
                why: "Couldn't reach box over https or http: nothing answered there.".into()
            }
        );
    }

    /// A sign-in's code, and a launch's arguments, read from a link of the
    /// app's scheme only. Falsified by reading another scheme's.
    #[test]
    fn links_are_read() {
        assert_eq!(code_of("money://auth?code=abc", "money").as_deref(), Some("abc"));
        assert_eq!(code_of("money://auth?code=", "money"), None);
        assert_eq!(code_of("harken://auth?code=abc", "money"), None);
        assert_eq!(code_of("money://budget", "money"), None);
        assert_eq!(
            launch_args("money://?screen=budget&month=2026-10", "money").unwrap(),
            ["-screen", "budget", "-month", "2026-10"]
        );
        assert_eq!(launch_args("money:///budget", "money"), None);
    }
}
