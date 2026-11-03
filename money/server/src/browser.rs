//! A browser the server is told of (`MONEY_BROWSER`), driven over the
//! Chrome DevTools Protocol: a real, headed browser that people sign in to
//! themselves, from its own page (`MONEY_BROWSER_PAGE`), and that keeps
//! those sign-ins. The server opens a tab of its own in it, asks what it
//! needs of the page there, and closes it; the browser's sign-ins are the
//! page's, used by the page, and nothing of them comes to the server.
//!
//! Only what this needs of the protocol: a tab opened at an address
//! (`/json/new`) and closed (`/json/close`), and script run in it and its
//! answer awaited (`Runtime.evaluate`), over the tab's WebSocket.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value as Json};
use tungstenite::{Message, WebSocket};

/// The browser, by its DevTools address (`http://host:port`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Browser {
    pub devtools: String,
}

/// A tab the server opened, closed when dropped.
pub struct Tab {
    devtools: String,
    id: String,
    socket: WebSocket<TcpStream>,
    next: u64,
    /// Left open when the handle goes: a tab for a person.
    keep: bool,
}

impl Browser {
    /// A new tab at `url`: opened blank and sent there (`Page.navigate`), so
    /// an address with a fragment (costco.com's pages are all `#/…`) is
    /// whole — one put in `/json/new?…` loses it, as any URL's fragment is
    /// never sent.
    pub fn open(&self, url: &str) -> Result<Tab> {
        let base = self.devtools.trim_end_matches('/');
        let made: Json = agent()
            .put(&format!("{base}/json/new?about:blank"))
            .call()
            .map_err(|e| anyhow!("the browser at {base} did not open a tab: {e}"))?
            .into_json()
            .context("reading the browser's new tab")?;
        let id = made["id"].as_str().ok_or_else(|| anyhow!("the browser's new tab has no id"))?.to_string();
        let ws = made["webSocketDebuggerUrl"]
            .as_str()
            .ok_or_else(|| anyhow!("the browser's new tab has no DevTools address"))?;
        let socket = connect(ws)?;
        let mut tab = Tab {
            devtools: base.to_string(),
            id,
            socket,
            next: 0,
            keep: false,
        };
        tab.send("Page.navigate", json!({ "url": url }))?;
        Ok(tab)
    }

    /// A tab opened at `url` and left for a person: the sign-in page,
    /// shown on the browser's screen.
    pub fn show(&self, url: &str) -> Result<()> {
        let mut tab = self.open(url)?;
        // Not closed with the handle: it is the person's now.
        tab.keep = true;
        Ok(())
    }
}

impl Tab {
    /// `script` run in the page, its promise awaited, its value as JSON.
    pub fn eval(&mut self, script: &str) -> Result<Json> {
        let result = self.send(
            "Runtime.evaluate",
            json!({ "expression": script, "awaitPromise": true, "returnByValue": true }),
        )?;
        if let Some(x) = result.get("exceptionDetails") {
            let why = x["exception"]["description"].as_str().or(x["text"].as_str()).unwrap_or("an error");
            bail!("the page's script failed: {}", why.lines().next().unwrap_or(why));
        }
        Ok(result["result"]["value"].clone())
    }

    /// One of the protocol's methods, and its result.
    fn send(&mut self, method: &str, params: Json) -> Result<Json> {
        self.next += 1;
        let id = self.next;
        let ask = json!({ "id": id, "method": method, "params": params });
        self.socket.send(Message::Text(ask.to_string())).context("asking the browser's tab")?;
        loop {
            let said = match self.socket.read().context("reading the browser's tab")? {
                Message::Text(t) => t,
                Message::Close(_) => bail!("the browser closed the tab"),
                _ => continue,
            };
            let said: Json = serde_json::from_str(&said).context("the browser said something that is not JSON")?;
            // Events, and answers to anything else, pass by.
            if said["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(e) = said.get("error") {
                bail!("the browser refused {method}: {}", e["message"].as_str().unwrap_or("no reason"));
            }
            return Ok(said["result"].clone());
        }
    }

    /// `script` run until it gives something other than null, or `wait`
    /// passes.
    pub fn until(&mut self, script: &str, wait: Duration) -> Result<Json> {
        let deadline = Instant::now() + wait;
        loop {
            // A page between two documents has none to run in; it is asked again.
            if let Ok(v) = self.eval(script) {
                if !v.is_null() {
                    return Ok(v);
                }
            }
            if Instant::now() > deadline {
                bail!("the page did not get there in {} seconds", wait.as_secs());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        let _ = self.socket.close(None);
        if !self.keep {
            let _ = agent().put(&format!("{}/json/close/{}", self.devtools, self.id)).call();
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build()
}

/// The tab's WebSocket, plain (the browser is on the host's own network),
/// with a read timeout so a tab that never answers ends.
fn connect(ws: &str) -> Result<WebSocket<TcpStream>> {
    let rest = ws
        .strip_prefix("ws://")
        .ok_or_else(|| anyhow!("the browser's tab is not at a ws:// address: {ws}"))?;
    let host = rest.split('/').next().unwrap_or(rest);
    let stream = TcpStream::connect(host).with_context(|| format!("connecting to the browser at {host}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    let (socket, _) = tungstenite::client(ws, stream).map_err(|e| anyhow!("opening the browser's tab: {e}"))?;
    Ok(socket)
}

#[cfg(test)]
pub(crate) mod fake {
    //! A browser for tests: `/json/new` and `/json/close` over HTTP, and
    //! the tab's WebSocket answering each `Runtime.evaluate` with what
    //! `answer` makes of its script.

    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// What the fake browser was asked: each tab opened (its address), each
    /// script run, and each tab closed.
    #[derive(Default, Debug)]
    pub struct Seen {
        pub opened: Vec<String>,
        pub scripts: Vec<String>,
        pub closed: usize,
    }

    pub fn serve(answer: impl Fn(&str) -> Json + Send + 'static) -> (Browser, Arc<Mutex<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let s = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = [0; 4096];
                let n = stream.peek(&mut head).unwrap_or(0);
                let head = String::from_utf8_lossy(&head[..n]).to_string();
                let line = head.lines().next().unwrap_or("").to_string();
                if head.to_ascii_lowercase().contains("upgrade: websocket") {
                    let mut ws = tungstenite::accept(stream).unwrap();
                    while let Ok(Message::Text(t)) = ws.read() {
                        let ask: Json = serde_json::from_str(&t).unwrap();
                        // An event first, as a real tab sends them.
                        ws.send(Message::Text(json!({"method": "Runtime.consoleAPICalled"}).to_string())).unwrap();
                        let result = if ask["method"] == "Page.navigate" {
                            let url = ask["params"]["url"].as_str().unwrap_or("").to_string();
                            s.lock().unwrap().opened.push(url);
                            json!({"frameId": "F1"})
                        } else {
                            let script = ask["params"]["expression"].as_str().unwrap_or("").to_string();
                            s.lock().unwrap().scripts.push(script.clone());
                            json!({"result": {"value": answer(&script)}})
                        };
                        let said = json!({"id": ask["id"], "result": result});
                        ws.send(Message::Text(said.to_string())).unwrap();
                    }
                    continue;
                }
                let mut buf = vec![0; n];
                let _ = stream.read(&mut buf);
                let body = if let Some(url) = line.strip_prefix("PUT /json/new?") {
                    assert!(url.starts_with("about:blank "), "a tab is opened blank, then sent: {url}");
                    json!({"id": "T1", "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/T1")}).to_string()
                } else {
                    s.lock().unwrap().closed += 1;
                    "Target is closing".to_string()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (
            Browser {
                devtools: format!("http://127.0.0.1:{port}"),
            },
            seen,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_runs_script_and_is_closed() {
        let (browser, seen) = fake::serve(|script| json!(format!("ran {}", script.len())));
        {
            let mut tab = browser.open("https://example.com/a").unwrap();
            assert_eq!(tab.eval("1 + 1").unwrap(), json!("ran 5"));
            assert_eq!(tab.eval("2").unwrap(), json!("ran 1"), "answers matched to their asks");
        }
        std::thread::sleep(Duration::from_millis(200));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.opened, ["https://example.com/a"]);
        assert_eq!(seen.scripts, ["1 + 1", "2"]);
        assert_eq!(seen.closed, 1, "closed with its handle");
    }
}
