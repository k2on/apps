//! The sync socket: binary frames of canonical CBOR, both ways, and the
//! keepalive, which is the server's.
//!
//! A socket that says nothing is one something between the two ends will
//! close — nginx gives an idle proxied connection sixty seconds — and sync
//! is quiet whenever nobody is mutating. So the server pings every
//! [`Keepalive::every`], a browser answers without being asked (it cannot
//! ping from JavaScript), and a connection that leaves
//! [`Keepalive::missed`] pings unanswered is closed: open as far as the OS
//! is concerned, with nobody at the other end, is exactly when a room goes
//! on believing a device is listening.

use std::time::Duration;

use anyhow::{Context, Result};
use ark::canon;
use ark::protocol::{ClientMsg, ServerMsg};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use tokio::sync::mpsc;

use crate::hub::HubHandle;

/// How often the server pings, and how many may go unanswered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keepalive {
    pub every: Duration,
    pub missed: u32,
}

impl Default for Keepalive {
    fn default() -> Keepalive {
        Keepalive {
            every: Duration::from_secs(20),
            missed: 3,
        }
    }
}

#[derive(Clone)]
pub(crate) struct SyncState {
    pub hub: HubHandle,
    pub keepalive: Keepalive,
}

pub(crate) async fn sync(ws: WebSocketUpgrade, State(st): State<SyncState>) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = connection(socket, st.hub, st.keepalive).await {
            eprintln!("ark-server: connection closed: {e:#}");
        }
    })
}

/// One socket: frames in go to the hub, frames the hub queues for this
/// connection go out, and the server pings. A frame that is not the
/// protocol closes this socket and nothing else; a denial is the last frame
/// a socket gets.
async fn connection(mut socket: WebSocket, hub: HubHandle, keepalive: Keepalive) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    let conn = hub.fresh();
    hub.attach_socket(conn, tx)?;
    let mut ticker = tokio::time::interval(keepalive.every);
    ticker.tick().await;
    let mut unanswered: u32 = 0;
    let outcome = loop {
        tokio::select! {
            frame = socket.recv() => match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    unanswered = 0;
                    let msg = canon::decode(&bytes)
                        .context("a frame that is not canonical CBOR")
                        .and_then(|v| ClientMsg::from_value(&v).context("a frame that is not a client message"));
                    match msg {
                        Ok(m) => hub.recv(conn, m)?,
                        Err(e) => break Err(e),
                    }
                }
                Some(Ok(Message::Text(_))) => break Err(anyhow::anyhow!("a text frame; the protocol is binary")),
                Some(Ok(Message::Close(_))) | None => break Ok(()),
                // A pong, a ping: the peer is there.
                Some(Ok(_)) => unanswered = 0,
                Some(Err(e)) => break Err(e.into()),
            },
            Some(msg) = rx.recv() => {
                let last = matches!(msg, ServerMsg::Denied { .. });
                if let Err(e) = socket.send(Message::Binary(canon::encode(&msg.to_value()))).await {
                    break Err(e.into());
                }
                if last {
                    let _ = socket.send(Message::Close(None)).await;
                    break Ok(());
                }
            }
            _ = ticker.tick() => {
                if unanswered >= keepalive.missed {
                    break Err(anyhow::anyhow!("{} pings unanswered", keepalive.missed));
                }
                unanswered += 1;
                if let Err(e) = socket.send(Message::Ping(vec![])).await {
                    break Err(e.into());
                }
            }
        }
    };
    hub.detach(conn)?;
    outcome
}
