//! A download follows a podcast's chain of trackers to the file.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

/// A host where `/hop/n` redirects to `/hop/n-1`, and `/hop/0` is the file.
fn chain() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let n: u32 = line
                .split_whitespace()
                .nth(1)
                .and_then(|p| p.strip_prefix("/hop/"))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let _ = match n {
                0 => write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nmp3!"),
                n => write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: /hop/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    n - 1
                ),
            };
        }
    });
    base
}

/// Seven trackers before the file, as Full Time Nix's episodes have, are
/// followed to the end. Falsified by ureq's default of five: "Too Many
/// Redirects".
#[test]
fn a_download_follows_a_chain_of_trackers() {
    let base = chain();
    let agent = harken_server::sync::agent();
    let body = agent
        .get(&format!("{base}/hop/7"))
        .call()
        .expect("followed to the file")
        .into_string()
        .unwrap();
    assert_eq!(body, "mp3!");
}
