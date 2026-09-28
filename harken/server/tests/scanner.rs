//! The scanner against the real module: a directory of files becomes
//! tracks once, and a restart on the same data authors nothing again.
//!
//! `harken/domain/harken.ark` is another crate's output; while it is not
//! there this test says so and passes, so the demo-only tests still run.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ark::canon;
use ark::peer::Replica;
use ark::protocol::{Client, Mode, ServerMsg};
use ark::store::{MemoryStore, Store};
use ark::value::Value;
use futures_util::{SinkExt, StreamExt};
use harken_server::{start, Config, Domain, Running};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

fn module() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../domain/harken.ark");
    p.is_file().then_some(p)
}

async fn healthz(running: &Running) -> String {
    let mut s = TcpStream::connect(running.addr).await.unwrap();
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: harken\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

/// Poll `/healthz` until the library's head is `want`, or give up.
async fn wait_for_head(running: &Running, want: i64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let h = healthz(running).await;
        if h.contains(&format!("scope library head {want}\n")) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the library never reached head {want}:\n{h}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The library, whole, as a fresh peer receives it.
async fn library(running: &Running, domain: &Domain) -> MemoryStore {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/sync", running.addr))
        .await
        .unwrap();
    let schema = domain.module.schema.clone();
    let mut client = Client::open(schema.clone(), Some("carol".into()));
    client.subscribe(
        Mode::Whole,
        Replica::open(
            schema.clone(),
            "library",
            domain.closures.clone(),
            MemoryStore::empty(schema),
            0,
            vec![],
        ),
    );
    client.connected();
    for m in client.take_outgoing() {
        ws.send(Message::Binary(canon::encode(&m.to_value())))
            .await
            .unwrap();
    }
    while let Ok(Some(Ok(Message::Binary(b)))) =
        tokio::time::timeout(Duration::from_millis(300), ws.next()).await
    {
        client.recv(ServerMsg::from_value(&canon::decode(&b).unwrap()).unwrap());
    }
    let _ = ws.close(None).await;
    client.scopes["library"].0.confirmed.clone()
}

#[tokio::test]
async fn a_directory_becomes_tracks_once() {
    let Some(module) = module() else {
        eprintln!("skipping: harken/domain/harken.ark is not built");
        return;
    };
    let domain = Domain::load(&module).unwrap();
    if !domain.by_name.contains_key("add_track") {
        eprintln!("skipping: the module has no add_track");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let media = tempfile::tempdir().unwrap();
    for p in [
        "music/Bach/Goldberg/01 Aria.flac",
        "music/Bach/air.mp3",
        "music/loose.ogg",
        "music/Bach/cover.jpg",
        "podcasts/ep1.mp3",
    ] {
        let full = media.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, b"").unwrap();
    }
    let config = Config {
        module: Some(module),
        data: data.path().to_path_buf(),
        listen: "127.0.0.1:0".into(),
        media: Some(media.path().to_path_buf()),
    };

    let running = start(config.clone()).await.unwrap();
    wait_for_head(&running, 3).await;
    let lib = library(&running, &domain).await;
    let mut tracks: Vec<(String, String, Value)> = lib
        .scan("track")
        .into_iter()
        .map(|r| {
            (
                r["file"].as_text().to_string(),
                r["artist"].as_text().to_string(),
                r["album"].clone(),
            )
        })
        .collect();
    tracks.sort();
    assert_eq!(
        tracks,
        vec![
            (
                "music/Bach/Goldberg/01 Aria.flac".into(),
                "Bach".into(),
                Value::text("Goldberg")
            ),
            ("music/Bach/air.mp3".into(), "Bach".into(), Value::null()),
            (
                "music/loose.ogg".into(),
                "Unknown Artist".into(),
                Value::null()
            ),
        ]
    );
    let title = lib
        .scan("track")
        .into_iter()
        .find(|r| r["file"].as_text() == "music/Bach/air.mp3")
        .unwrap();
    assert_eq!(title["title"], Value::text("air"));
    assert_eq!(title["user_id"], Value::text("library"));
    running.stop().await;

    // The same directory again, on the same data: the log is loaded, the
    // scanner finds everything already there, and the head does not move.
    let running = start(config).await.unwrap();
    wait_for_head(&running, 3).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(healthz(&running).await.contains("scope library head 3\n"));
    assert_eq!(library(&running, &domain).await, lib);
    running.stop().await;
}
