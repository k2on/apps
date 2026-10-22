//! The fuzzer: random scripts over the demo domain (`crate::demo`) on the
//! simulator, and every check a session must pass.
//!
//! A [`Gen`] draws a script's intents over the demo: playlists created
//! under names that trim to one another or to nothing, tracks added to a
//! playlist a client holds or to one nobody does, renames, removals; and a
//! view of a playlist's items. A session ([`session`]) is a fleet of two
//! to four clients — one fed by the scope `demo.top_two`, some holding the
//! admin role, sometimes the live room — and a script of authoring,
//! partitions and heals, the server restarted, a client reopened, a
//! sign-in, going alone and joining, waits and deliveries; sometimes the
//! hub keeps only a few entries above its horizon. After every op every
//! view kept is its patches and a fresh evaluation; once settled,
//! [`check`]: every whole client converged, the partial one exactly its
//! union, the log replayed from its facts and each entry run again to
//! them, every entry's roles its login's, every frame read back as sent,
//! and every client's trace replayed to the same.

use std::sync::Arc;

use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::{Ctx, Feed, Who};
use ark::prelude::Id;
use ark::schema::Space;
use ark::store::Store;
use ark_peer::{Config, Live, Module, Peer};

use crate::demo::{AddToPlaylist, CreatePlaylist, Items, Playlist, RemoveFromPlaylist, RenamePlaylist};
use crate::sim::{lcg, Net, Op};

pub struct Rng(pub u64);

impl Rng {
    pub fn below(&mut self, n: u64) -> u64 {
        self.0 = lcg(self.0);
        (self.0 >> 11) % n.max(1)
    }
    pub fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
}

/// The demo, and what a script can do with it.
pub struct Gen {
    pub module: Module,
    pub scope: String,
}

const NAMES: [&str; 5] = ["Road", "Sea", " Road ", "", " "];
const TRACKS: [&str; 6] = ["t1", "t2", "t3", "t4", "t5", ""];

impl Gen {
    pub fn new() -> Gen {
        Gen {
            module: Module::of(&crate::demo::demo()),
            scope: "demo.top_two".into(),
        }
    }

    /// A playlist client `c` holds, or, one time in five or when it holds
    /// none, one nobody does.
    fn playlist(r: &mut Rng, c: &Peer) -> Id<Playlist> {
        let held: Vec<Playlist> = ark::plan::rows(&c.log.store);
        match held.len() {
            n if n > 0 && !r.one_in(5) => held[r.below(n as u64) as usize].id,
            _ => Id::new([r.below(256) as u8; 16]),
        }
    }

    /// A random intent of client `i`'s.
    pub fn mutate(&self, r: &mut Rng, net: &Net, i: i64) -> Op {
        let c = &net.clients[&i];
        let pick = |r: &mut Rng, xs: &[&str]| xs[r.below(xs.len() as u64) as usize].to_string();
        let (f, input) = match r.below(8) {
            0 | 1 => ("demo.create_playlist", CreatePlaylist { name: pick(r, &NAMES) }.to_bytes()),
            2..=5 => {
                let playlist_id = Gen::playlist(r, c);
                (
                    "demo.add_to_playlist",
                    AddToPlaylist {
                        playlist_id,
                        track_id: pick(r, &TRACKS),
                    }
                    .to_bytes(),
                )
            }
            6 => {
                let playlist_id = Gen::playlist(r, c);
                (
                    "demo.rename_playlist",
                    RenamePlaylist {
                        playlist_id,
                        name: pick(r, &NAMES),
                    }
                    .to_bytes(),
                )
            }
            _ => {
                let playlist_id = Gen::playlist(r, c);
                (
                    "demo.remove_from_playlist",
                    RemoveFromPlaylist {
                        playlist_id,
                        track_id: pick(r, &TRACKS),
                    }
                    .to_bytes(),
                )
            }
        };
        Op::Mutate(i, f.into(), input)
    }

    /// A random view of client `i`'s and its input: a playlist's items.
    pub fn query(&self, r: &mut Rng, net: &Net, i: i64) -> (String, Vec<u8>) {
        let playlist_id = Gen::playlist(r, &net.clients[&i]);
        ("demo.items".into(), Items { playlist_id }.to_bytes())
    }
}

impl Default for Gen {
    fn default() -> Gen {
        Gen::new()
    }
}

/// What a session reached, for a report.
#[derive(Debug, Default)]
pub struct Found {
    pub entries: usize,
    pub refused: usize,
    pub ops: usize,
}

fn login(i: i64, n: u64, admin: bool) -> Who {
    let roles = if admin { vec!["admin".to_string()] } else { vec![] };
    Who {
        actor: format!("u{}", i % 2),
        session: if i == 0 { format!("partial-{n}") } else { format!("s{i}-{n}") },
        roles,
    }
}

fn ctx_of(w: &Who) -> Ctx {
    Ctx {
        draws: Default::default(),
        id: EntryId::default(),
        actor: w.actor.clone(),
        session: w.session.clone(),
        roles: w.roles.clone(),
        now: 0,
    }
}

/// One session from its seed: the net it left, or the first check it failed.
pub fn session(seed: u64, traces: bool) -> Result<(Net, Found), String> {
    let mut r = Rng(seed);
    let g = Gen::new();
    let room = crate::demo::room();
    let live = r.one_in(2);
    let mut hub = Config::of(Arc::new(crate::demo::demo()));
    let scope = g.scope.clone();
    let scoped = scope.clone();
    hub.feed = Arc::new(move |w: &Who| {
        if w.session.starts_with("partial") {
            Feed::Scoped(scoped.clone())
        } else {
            Feed::Whole
        }
    });
    hub.keep = r.one_in(3).then(|| 2 + r.below(4) as usize);
    if live {
        hub.live = Some(Live {
            on_join: Some("room.on_join".into()),
            on_leave: Some("room.on_leave".into()),
            arrive: Arc::new(crate::demo::arrival),
            ..Live::new(Module::of(&room))
        });
    }
    let n = 2 + r.below(3) as i64;
    let partial = r.one_in(2);
    let config = |i: i64| Config {
        live: live.then(|| crate::demo::live(&format!("d{i}"), 50)),
        ..Config::of(Arc::new(crate::demo::demo()))
    };
    let logins: Vec<(Who, Config)> = (0..n).map(|i| (login(i + !partial as i64, 0, r.one_in(3)), config(i))).collect();
    let mut net = Net::of(hub, logins, seed);
    if let Some(c) = net.clients.get_mut(&0).filter(|_| partial) {
        c.log.feed = Feed::Scoped(scope.clone());
    }
    let watch = |net: &mut Net, r: &mut Rng, i: i64| -> Result<(), String> {
        let (f, input) = g.query(r, net, i);
        let ctx = ctx_of(&net.clients[&i].log.me);
        net.watch(i, Space::Durable, &f, input, ctx).map(|_| ())
    };
    for i in 0..n {
        watch(&mut net, &mut r, i)?;
    }
    let mut found = Found::default();
    for k in 0..30 + r.below(90) {
        let i = r.below(n as u64) as i64;
        let whole = !(partial && i == 0);
        let op = match r.below(40) {
            0..=11 => g.mutate(&mut r, &net, i),
            12..=13 if live => Op::Call(i, "room.point".into(), crate::demo::Point { x: r.below(100) as i64 }.to_bytes()),
            14 if live => Op::Call(i, "room.rename".into(), crate::demo::Rename { name: "x".into() }.to_bytes()),
            15 => Op::Partition(i),
            16 => Op::Heal(i),
            17 if r.one_in(3) => Op::Restart,
            18 if r.one_in(2) => Op::Reopen(i),
            19 if whole && r.one_in(3) => Op::SignIn(i, login(i + !partial as i64, k + 1, r.one_in(3))),
            20 if whole && r.one_in(2) => Op::Alone(i),
            21 => Op::Join(i),
            22 => Op::Wait(r.below(80) as i64),
            _ => Op::Step,
        };
        net.run(&op);
        found.ops += 1;
        if let Some(c) = net.clients.get_mut(&0).filter(|_| partial) {
            c.log.feed = Feed::Scoped(scope.clone());
        }
        if matches!(op, Op::Reopen(_)) {
            watch(&mut net, &mut r, i)?;
        }
        net.views_hold().map_err(|e| format!("after op {k} ({op:?}): {e}"))?;
    }
    net.settle()?;
    check(&net, &g, partial, traces)?;
    found.entries = net.hub.log.log().head().0 as usize;
    found.refused = net.refused.len();
    Ok((net, found))
}

/// Everything a settled session must be.
pub fn check(net: &Net, g: &Gen, partial: bool, traces: bool) -> Result<(), String> {
    net.converged()?;
    if let Some(b) = net.misread.first() {
        return Err(format!("a frame did not read back as sent: {}", ark::hash::hex(b)));
    }
    net.views_hold()?;
    let hub = &net.hub.log;
    let schema = hub.store.schema_arc().clone();
    let entries = hub.log().entries();
    let mut s = hub.store.clone();
    for c in entries.iter().rev().flat_map(|l| l.facts.iter().rev()) {
        s.apply(&c.inverse()).map_err(|r| format!("the log's facts do not undo: {r}"))?;
    }
    if s.state_hash() != hub.log().base().state {
        return Err("the store at the horizon is not the state the log stands on".into());
    }
    for l in &entries {
        let p = g.module.functions.get(&l.entry.func).ok_or("an entry of no function")?;
        let ran = p
            .apply(&mut s, &Ctx::of(&l.entry), &l.entry.input)
            .map_err(|e| format!("entry {} refuses when run again: {e}", l.seq))?;
        if ran != l.facts {
            return Err(format!("entry {} runs again to other facts", l.seq));
        }
        if s.state_hash() != l.state {
            return Err(format!("entry {}'s state is not its facts'", l.seq));
        }
        let w = net.logins.get(&l.entry.session).ok_or(format!("entry {} of no login", l.seq))?;
        if (&w.actor, &w.roles) != (&l.entry.actor, &l.entry.roles) {
            return Err(format!("entry {} is stamped other than its login", l.seq));
        }
    }
    if s != hub.store {
        return Err("the log replayed from its facts is not the store".into());
    }
    if let Some(c) = net.clients.get(&0).filter(|_| partial) {
        let holds = g.module.functions.get(&g.scope).expect("the scope");
        let ctx = ctx_of(&c.log.me);
        let mut rows = vec![];
        for (t, (name, rs)) in hub.store.snapshot_rows().into_iter().enumerate() {
            let held = rs
                .into_iter()
                .filter(|row| holds.holds(&ctx, &[&[t as u8][..], row].concat()) == Ok(true));
            rows.push((name, held.collect()));
        }
        let union = Store::from_rows(schema, &rows).map_err(|r| r.0)?;
        if c.log.store != union || !c.log.pending.is_empty() {
            return Err("the partial client is not exactly its union".into());
        }
    }
    if traces {
        for (i, c) in &net.clients {
            let t = crate::trace::record(&format!("client {i}"), c);
            crate::trace::replay(&t).map_err(|e| format!("client {i}'s trace: {e}"))?;
        }
    }
    Ok(())
}
