//! harken's server in this process on a real port, and peers that pump
//! against it: an `ark_client::Client` and its link over the server's
//! WebSocket, as a desktop or a phone is.
#![allow(dead_code)]

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ark::frames::Frame;
use ark::ir::Node;
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::schema::Space;
use ark_auth::{Account, Login};
use ark_client::link::{Backoff, Event as LinkEvent, Link, Socket};
use ark_client::{Client, Event, Memory};
use ark_server::handle::now;
use harken_domain::listening::read::{Kind, Listening};
use harken_domain::listening::Device;
use harken_server::hosted::Hosted;
use harken_server::{start, Config, Server};

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// harken's module, assembled once.
pub fn hosted() -> &'static Hosted {
    static H: OnceLock<Hosted> = OnceLock::new();
    H.get_or_init(Hosted::harken)
}

/// Reconnect fast: a test has no thirty seconds to spare.
pub fn quick() -> Backoff {
    Backoff {
        first: Duration::from_millis(20),
        most: Duration::from_millis(200),
    }
}

/// How often a device relays its scrubber.
pub const RELAY_MS: i64 = 250;

/// A dev-auth server on an ephemeral port, told whatever `more` adds.
pub fn serve(
    rt: &tokio::runtime::Runtime,
    data: &std::path::Path,
    more: impl FnOnce(&mut Config),
) -> Server {
    let mut config = Config::dev("127.0.0.1:0", data);
    more(&mut config);
    rt.block_on(start(config)).expect("the server starts")
}

/// A login on `server` for `user`, issued as the sign-in routes would.
pub fn login(server: &Server, user: &str) -> Login {
    server
        .auth
        .issue(&Account {
            id: user.into(),
            ..Account::default()
        })
        .unwrap()
}

/// A client of harken, the device it says it is (none for no live state).
pub struct Peer {
    pub client: Client,
    pub link: Option<Link>,
    /// The server and token it last dialled, to dial again.
    pub dialled: Option<(String, String)>,
    /// What the server refused, as it said so, in order.
    pub refused: Vec<(Space, String)>,
    /// Whether the hub has sent the live machine a page or a snapshot
    /// since the connection opened: its view is the hub's, not what it
    /// held before.
    pub heard: bool,
}

/// A client as `who`, in memory, linked to nothing.
pub fn open(who: Who, device: Option<(Kind, &str, bool)>) -> Peer {
    let live = device.map(|(kind, name, can_play)| ark_client::Live {
        module: hosted().module.clone(),
        device: Some(
            Device {
                can_play,
                id: String::new(),
                kind: kind.name().into(),
                name: name.into(),
                user: String::new(),
            }
            .to_bytes(),
        ),
        ephemeral: [hosted().func("listening.scrub")].into(),
        every_ms: RELAY_MS,
    });
    let config = ark_client::Config {
        log: hosted().module.clone(),
        live,
    };
    Peer {
        client: Client::open(config, who, Box::new(Memory::default()), false),
        link: None,
        dialled: None,
        refused: vec![],
        heard: false,
    }
}

/// Nobody: a client used before anybody signed in.
pub fn nobody() -> Who {
    Who {
        actor: String::new(),
        session: String::new(),
        roles: vec![],
    }
}

/// `user`, signed in on this server as a device of their own: a fresh
/// login, dialling the socket.
pub fn signed_in(server: &Server, user: &str) -> Peer {
    let l = login(server, user);
    let mut p = open(ark_client::auth::who(&l), None);
    p.connect(&server.url(), &l.token);
    p
}

/// [`signed_in`], as a device of the listening session.
pub fn device(server: &Server, user: &str, kind: Kind, name: &str, can_play: bool) -> Peer {
    let l = login(server, user);
    let mut p = open(ark_client::auth::who(&l), Some((kind, name, can_play)));
    p.connect(&server.url(), &l.token);
    p
}

impl Peer {
    /// Dial `server`'s socket with `token`, and keep dialling.
    pub fn connect(&mut self, server: &str, token: &str) {
        let socket = Socket(ark_auth::socket_url(server));
        self.link = Some(Link::start(Box::new(socket), token.into(), quick()));
        self.dialled = Some((server.into(), token.into()));
    }

    /// Back on the network, as it last was.
    pub fn reconnect(&mut self) {
        let (server, token) = self.dialled.clone().expect("dialled before");
        self.connect(&server, &token);
    }

    /// Off the network.
    pub fn disconnect(&mut self) {
        self.link = None;
        self.client.disconnected();
    }

    /// Another login: what is pending is authored again under it, and the
    /// socket dialled with its token.
    pub fn sign_in(&mut self, server: &str, l: &Login) {
        self.disconnect();
        self.client.sign_in(ark_client::auth::who(l));
        self.connect(server, &l.token);
    }

    /// What the link said, to the client, and what it says back; the
    /// server's refusals kept.
    pub fn pump(&mut self) {
        if let Some(link) = &self.link {
            let ev = link.events();
            for e in &ev {
                match e {
                    LinkEvent::Open => self.heard = false,
                    LinkEvent::Frame(b) => {
                        if let Ok(
                            Frame::Page {
                                space: Space::Volatile,
                                ..
                            }
                            | Frame::Snapshot {
                                space: Space::Volatile,
                                ..
                            },
                        ) = Frame::dec(b)
                        {
                            self.heard = true;
                        }
                    }
                    LinkEvent::Closed(_) => {}
                }
            }
            self.client.pump(link, ev, now());
        } else {
            self.client.tick(now());
        }
        for e in self.client.take_events() {
            if let Event::Refused { space, why, .. } = e {
                self.refused.push((space, why));
            }
        }
    }

    /// Whether the link has said `Closed` since the last look, pumping.
    pub fn closed(&mut self) -> bool {
        let Some(link) = &self.link else { return true };
        let ev = link.events();
        let closed = ev.iter().any(|e| matches!(e, LinkEvent::Closed(_)));
        self.client.pump(link, ev, now());
        closed
    }

    pub fn linked(&self) -> bool {
        self.client.linked
    }

    /// Who this client authors as; its session is its device's id.
    pub fn me(&self) -> String {
        self.client.log.me.session.clone()
    }

    pub fn user(&self) -> String {
        self.client.log.me.actor.clone()
    }

    pub fn pending(&self) -> usize {
        self.client.log.pending.len()
    }

    /// How far into the log the client has confirmed.
    pub fn cursor(&self) -> i64 {
        self.client.log.log().head().0
    }

    /// An intent on the log: its id.
    pub fn mutate(&mut self, name: &str, input: &impl Node) -> EntryId {
        self.client
            .mutate(hosted().func(name), input.to_bytes(), now());
        self.pump();
        self.client.log.pending.last().expect("authored").entry.id
    }

    /// A transition of the session; a refusal of its preview, said.
    pub fn call(&mut self, transition: &str, input: Vec<u8>) -> Result<(), String> {
        self.client.call(
            hosted().func(&format!("listening.{transition}")),
            input,
            now(),
        );
        let mut out = Ok(());
        for e in self.client.take_events() {
            if let Event::Refused { space, why, .. } = e {
                self.refused.push((space, why.clone()));
                out = Err(why);
            }
        }
        self.pump();
        out
    }

    /// A query over what the client holds.
    pub fn query<T: Node>(&self, name: &str, input: &impl Node) -> Result<T, String> {
        let me = &self.client.log.me;
        let ctx = Ctx {
            id: EntryId::of("q", 0),
            actor: me.actor.clone(),
            session: me.session.clone(),
            roles: me.roles.clone(),
            now: now(),
        };
        let b = hosted()
            .ask(&self.client.log.store, &ctx, name, &input.to_bytes())
            .map_err(|r| r.0)?;
        Ok(T::from_bytes(&b).unwrap_or_else(|e| panic!("{name}: {e}")))
    }

    /// A table's rows, as the client holds them.
    pub fn rows<T: Node>(&self, table: &str) -> Vec<T> {
        self.client
            .log
            .store
            .rows(table)
            .map(|r| T::from_bytes(r).unwrap())
            .collect()
    }

    /// The account's listening session as this client holds it, with the
    /// house's speakers named as the bridge names them.
    pub fn listening(&self, speakers: &[&str]) -> Option<Listening> {
        let live = self.client.live.as_ref()?;
        let user = self.user();
        let s = Listening::of(&live.store, &user);
        let es: Vec<String> = speakers.iter().map(|s| s.to_string()).collect();
        Some(harken_server::assistant::ha::as_entities(s, &user, &es))
    }

    /// Whether the live machine is linked, has heard the hub since, and
    /// has nothing of its own outstanding.
    pub fn live_settled(&self) -> bool {
        self.linked()
            && self.heard
            && self
                .client
                .live
                .as_ref()
                .is_some_and(|l| l.pending.is_empty())
    }
}

/// A table's rows as the hub's log holds them.
pub fn hub_rows<T: Node + Send + 'static>(server: &Server, table: &'static str) -> Vec<T> {
    server
        .running
        .hub
        .with(move |h| {
            h.log
                .store
                .rows(table)
                .map(|r| T::from_bytes(r).unwrap())
                .collect()
        })
        .unwrap()
}

/// Pump every peer until `done` says so, or `secs` pass.
pub fn pump_until(
    peers: &mut [&mut Peer],
    secs: u64,
    mut done: impl FnMut(&mut [&mut Peer]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        for p in peers.iter_mut() {
            p.pump();
        }
        if done(peers) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out; {:#?}",
            peers
                .iter()
                .map(|p| (p.me(), p.linked(), p.cursor(), p.pending(), &p.refused))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Wait for `f` to hold, or `secs` pass; what it last said either way.
pub fn eventually<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(t) = f() {
            return Some(t);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A playable WAV of `ms` milliseconds: 44.1kHz, mono, 16-bit silence —
/// silence because nothing listens to it, and a real header because lofty
/// reads a duration out of it the way it would from a real track.
pub fn wav(path: &std::path::Path, ms: u32) {
    let rate = 44_100u32;
    let samples = rate * ms / 1000;
    let data = samples * 2;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    out.extend(std::iter::repeat_n(0u8, data as usize));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Written beside and renamed in, so a watch sees a whole file appear
    // rather than one being written.
    let tmp = path.with_extension("partial");
    std::fs::write(&tmp, out).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}
