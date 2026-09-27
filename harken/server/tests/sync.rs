//! Two peers over real sockets against the demo module, and a restart on
//! the same data directory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ark::canon;
use ark::eval::{Args, Ctx};
use ark::peer::Replica;
use ark::protocol::{Client, Mode, ServerMsg};
use ark::store::MemoryStore;
use ark::value::Value;
use futures_util::{SinkExt, StreamExt};
use harken_server::{start, Config, Domain, Running, DEV_SESSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const SCOPE: &str = "playlists";
/// How long a peer waits for another frame before deciding the server has
/// nothing more to say.
const IDLE: Duration = Duration::from_millis(300);

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo.ark")
}

async fn serve(data: &Path) -> Running {
    start(Config {
        module: demo(),
        data: data.to_path_buf(),
        listen: "127.0.0.1:0".into(),
        media: None,
    })
    .await
    .expect("the server starts")
}

/// A test peer: an `ark::protocol::Client` over one tokio-tungstenite
/// socket.
struct Peer {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    client: Client,
    ctx: Ctx,
}

impl Peer {
    async fn connect(running: &Running, domain: &Domain, name: &str) -> Peer {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/sync", running.addr))
            .await
            .expect("the socket opens");
        let schema = domain.module.schema.clone();
        let mut client = Client::open(schema.clone(), Some(name.into()));
        client.subscribe(
            Mode::Whole,
            Replica::open(
                schema.clone(),
                SCOPE,
                domain.closures.clone(),
                MemoryStore::empty(schema),
                0,
                vec![],
            ),
        );
        client.connected();
        Peer {
            ws,
            client,
            ctx: Ctx::new(name, DEV_SESSION),
        }
    }

    /// Send what is queued and take what comes back, until a round trip
    /// moves nothing.
    async fn pump(&mut self) {
        loop {
            let out = self.client.take_outgoing();
            let sent = !out.is_empty();
            for m in out {
                self.ws
                    .send(Message::Binary(canon::encode(&m.to_value())))
                    .await
                    .expect("send");
            }
            let mut got = false;
            loop {
                match tokio::time::timeout(IDLE, self.ws.next()).await {
                    Ok(Some(Ok(Message::Binary(b)))) => {
                        let v = canon::decode(&b).expect("canonical");
                        self.client
                            .recv(ServerMsg::from_value(&v).expect("a server frame"));
                        got = true;
                    }
                    Ok(Some(Ok(_))) => {}
                    Ok(Some(Err(e))) => panic!("socket: {e}"),
                    Ok(None) => panic!("the server closed the socket"),
                    Err(_) => break,
                }
            }
            if !sent && !got {
                return;
            }
        }
    }

    fn replica(&self) -> &Replica {
        &self.client.scopes[SCOPE].0
    }

    fn mutate(&mut self, domain: &Domain, id: u8, name: &str, autos: Args, args: Args) {
        let fh = &domain.by_name[name];
        let id = [id; 16];
        self.client
            .mutate(SCOPE, id, &self.ctx, fh, &autos, &args)
            .unwrap_or_else(|e| panic!("{name} refused: {e}"));
    }

    async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
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

#[tokio::test]
async fn two_peers_agree_and_a_restart_serves_the_same_log() {
    let domain = Domain::load(&demo()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let running = serve(data.path()).await;

    // (1) Two peers, one authoring, both reaching one hash the authority agrees with.
    let mut a = Peer::connect(&running, &domain, "alice").await;
    let mut b = Peer::connect(&running, &domain, "bob").await;
    a.pump().await;
    b.pump().await;

    let playlist = [1u8; 16];
    a.mutate(
        &domain,
        11,
        "create_playlist",
        args([("id", Value::Id(playlist))]),
        args([("name", Value::text("  Road trip "))]),
    );
    a.mutate(
        &domain,
        12,
        "add_to_playlist",
        args([("added_ms", Value::int(1_700_000_000_000))]),
        args([
            ("playlist_id", Value::Id(playlist)),
            ("media_id", Value::bytes(vec![9, 9, 9])),
        ]),
    );
    assert_eq!(a.replica().pending.len(), 2);
    a.pump().await;
    b.pump().await;

    assert!(
        a.replica().pending.is_empty(),
        "everything was acknowledged"
    );
    assert!(
        a.replica().rejections.is_empty(),
        "{:?}",
        a.replica().rejections
    );
    assert_eq!(a.replica().cursor, 2);
    assert_eq!(b.replica().cursor, 2);
    let claim = a.replica().verify_at();
    assert_eq!(b.replica().verify_at(), claim);
    let rows = ark::store::Store::scan(&b.replica().confirmed, "playlist");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::text("Road trip"));

    a.client.verify_all();
    b.client.verify_all();
    a.pump().await;
    b.pump().await;
    assert_eq!(a.client.agreed, vec![(SCOPE.to_string(), 2, true)]);
    assert_eq!(b.client.agreed, vec![(SCOPE.to_string(), 2, true)]);

    let health = healthz(&running).await;
    assert!(health.contains("connections 2"), "{health}");
    assert!(health.contains("scope playlists head 2"), "{health}");

    // (2) Stop, start again on the same data, and a fresh peer catches up to the same hash.
    a.close().await;
    b.close().await;
    running.stop().await;
    assert!(data.path().join("playlists.ark-log").is_file());

    let running = serve(data.path()).await;
    let mut c = Peer::connect(&running, &domain, "carol").await;
    c.pump().await;
    assert_eq!(c.replica().cursor, 2);
    assert_eq!(c.replica().verify_at(), claim);
    c.client.verify_all();
    c.pump().await;
    assert_eq!(c.client.agreed, vec![(SCOPE.to_string(), 2, true)]);
    assert!(healthz(&running).await.contains("scope playlists head 2"));

    // …and what is added after the restart lands on top of what was kept.
    c.mutate(
        &domain,
        13,
        "create_playlist",
        args([("id", Value::Id([2; 16]))]),
        args([("name", Value::text("Focus"))]),
    );
    c.pump().await;
    assert_eq!(c.replica().cursor, 3);
    assert!(c.replica().rejections.is_empty());
    c.close().await;
    running.stop().await;
}

#[tokio::test]
async fn a_frame_that_is_not_the_protocol_closes_the_socket_and_nothing_else() {
    let domain = Domain::load(&demo()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let running = serve(data.path()).await;
    let mut good = Peer::connect(&running, &domain, "alice").await;
    good.pump().await;

    let (mut bad, _) = tokio_tungstenite::connect_async(format!("ws://{}/sync", running.addr))
        .await
        .unwrap();
    bad.send(Message::Text("hello".into())).await.unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match bad.next().await {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "the server closes a socket that does not speak the protocol"
    );

    good.mutate(
        &domain,
        14,
        "create_playlist",
        args([("id", Value::Id([3; 16]))]),
        args([("name", Value::text("Still here"))]),
    );
    good.pump().await;
    assert_eq!(good.replica().cursor, 1);
    assert!(healthz(&running).await.contains("connections 1"));
    good.close().await;
    running.stop().await;
}
