//! Small things every half needs: the login and socket URLs, and a query
//! value's encoding (`percent-encoding`'s, over the unreserved characters).

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// Everything but the unreserved characters is encoded in a query value.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// The URL a client sends a person to, to sign in.
///
/// `server` is the server's base URL — `https://harken.example.com` or
/// `http://127.0.0.1:8787` — and `redirect` where the login code should come
/// back to. `user` is honoured only by a server in dev mode, and is how a
/// laptop runs two peers with two names and no browser.
pub fn login_url(server: &str, redirect: &str, user: Option<&str>) -> String {
    let user = user.map_or(String::new(), |u| format!("&user={}", percent_encode(u)));
    format!("{}/auth/login?redirect={}{user}", server.trim_end_matches('/'), percent_encode(redirect))
}

/// The socket beside the login: `/sync`, with the scheme to match.
///
/// A client is told one address and derives the other, so a server moved
/// behind TLS changes one setting and not two.
pub fn socket_url(server: &str) -> String {
    let server = server.trim_end_matches('/');
    let socket = match (server.strip_prefix("https://"), server.strip_prefix("http://")) {
        (Some(rest), _) => format!("wss://{rest}"),
        (_, Some(rest)) => format!("ws://{rest}"),
        _ if server.starts_with("ws://") || server.starts_with("wss://") => server.to_string(),
        _ => format!("ws://{server}"),
    };
    format!("{socket}/sync")
}

/// A query value, encoded: everything but the unreserved characters.
pub fn percent_encode(s: &str) -> String {
    utf8_percent_encode(s, QUERY).to_string()
}

/// The other way, for a query value the server or a client reads back; `+`
/// is a space, as a form writes one.
pub fn percent_decode(s: &str) -> String {
    percent_decode_str(&s.replace('+', " ")).decode_utf8_lossy().into_owned()
}

/// The value of one key in a query string, decoded.
pub fn query_value(query: &str, key: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
        .map(percent_decode)
}

/// `url` with `code` added to its query, whichever punctuation that takes.
pub fn with_code(url: &str, code: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}code={}", percent_encode(code))
}
