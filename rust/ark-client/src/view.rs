//! A query held as a list and kept up to date.
//!
//! Hydrate once, then hand every [`Changes`] to [`View::update`] and splice
//! what it returns into whatever the screen built from the rows. Every
//! query is maintained (`docs/plan-v4.md` §1.5): a query is a plan, and
//! `ark::view` keeps a plan's entries, so a change costs the entries it
//! touches — rebuilt from the store through its indexes — whatever the size
//! of the list, and a batch of changes costs each touched entry once.
//!
//! A query's middleware — its input checks, guards and provides — runs
//! before the hydrate, over the same store, and decides the scope the plan
//! is pulled in (§1.7). The view keeps the tables that middleware reads
//! ([`ark::ir::reads`]); a change to one of them re-runs it before anything
//! is pushed, and a different outcome — another provided value, a refusal
//! appearing or clearing, or another user signed in — re-hydrates (to
//! nothing, on a refusal) and reports [`Update::Reset`].
//!
//! A rebase is changes like any other: the engine undoes this peer's
//! pending intents by their recorded changes, applies what landed and runs
//! them again, and reports every transition (`docs/plan-perf.md` R2), so a
//! view patches through one. `Changes::Rebuilt` — the optimistic store
//! replaced whole, as opening over a snapshot does — is a re-hydrate and
//! [`Update::Reset`].

use std::collections::BTreeSet;

use ark::eval::{self, Args, Ctx, EvalFault};
use ark::hash::Closure;
use ark::peer::Changes;
use ark::store::Change;
use ark::value::{TableName, Value};
use ark::view::{self, Env, Patch};

use crate::peer::Peer;
use crate::Error;

/// What an update did to the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    /// Apply these, in order, to the list as it stood ([`splice`]).
    Patched(Vec<Patch>),
    /// The list was read again: take [`View::rows`] whole.
    Reset,
}

/// What the middleware came to: the checked input and the provided values
/// the plan is pulled with, or the verdict that leaves the list empty.
type Outcome = Result<(Args, Args), ark::store::Refusal>;

/// A maintained query.
pub struct View {
    name: String,
    args: Args,
    /// Who the middleware and the plan ran as: another user signed in is
    /// another outcome.
    ctx: Ctx,
    /// The tables the middleware reads (§1.7).
    guards: BTreeSet<TableName>,
    outcome: Outcome,
    /// The plan's entries; `None` while the middleware refuses.
    held: Option<view::View>,
    rows: Vec<Value>,
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View")
            .field("name", &self.name)
            .field("refused", &self.outcome.is_err())
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl View {
    /// Run the middleware, hydrate, and keep what the middleware read. A
    /// refusal at open is the caller's answer, as a query's would be.
    pub(crate) fn open(peer: &Peer, name: &str, args: Args) -> Result<View, Error> {
        let c = closure(peer, name)?;
        let mut guards = ark::ir::reads(&c.function);
        for u in &c.function.uses {
            if let Some(mw) = c.helpers.iter().find(|h| h.name == *u) {
                guards.extend(ark::ir::reads(mw));
            }
        }
        let outcome = middleware(peer, name, &c, &args)?;
        if let Err(r) = &outcome {
            return Err(Error::Refused(r.clone()));
        }
        let mut v = View {
            name: name.into(),
            args,
            ctx: peer.ctx().clone(),
            guards,
            outcome,
            held: None,
            rows: vec![],
        };
        v.hydrate(peer, &c)?;
        Ok(v)
    }

    /// The list, as it stands.
    pub fn rows(&self) -> &[Value] {
        &self.rows
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn args(&self) -> &Args {
        &self.args
    }

    /// Whether the view holds the plan's entries — every query's does,
    /// except while its middleware refuses and the list is empty.
    pub fn incremental(&self) -> bool {
        self.held.is_some()
    }

    /// The engine's view underneath, while there is one: its entries and
    /// indexes, to look at.
    pub fn entries(&self) -> Option<&view::View> {
        self.held.as_ref()
    }

    /// Run the middleware again and hydrate whole.
    pub fn rehydrate(&mut self, peer: &Peer) -> Result<(), Error> {
        let c = closure(peer, &self.name)?;
        self.ctx = peer.ctx().clone();
        self.outcome = middleware(peer, &self.name, &c, &self.args)?;
        self.hydrate(peer, &c)
    }

    // The plan pulled in the scope the outcome gives, or nothing on a
    // refusal.
    fn hydrate(&mut self, peer: &Peer, c: &Closure) -> Result<(), Error> {
        let Ok((args, provided)) = &self.outcome else {
            self.held = None;
            self.rows = vec![];
            return Ok(());
        };
        let plan = c
            .function
            .plan
            .as_ref()
            .ok_or_else(|| Error::Bug(format!("{}: a query with no plan", self.name)))?;
        let env = Env {
            helpers: c.helpers.clone(),
            ctx: self.ctx.clone(),
            args: args.clone(),
            provided: provided.clone(),
        };
        let v = view::hydrate(peer.schema(), plan, env, peer.store()).map_err(|e| fault(&self.name, e))?;
        self.rows = v.rows();
        self.held = Some(v);
        Ok(())
    }

    /// Bring the list up to date with what moved. `changes` is what
    /// [`Peer::take_changes`] returned, handed to every view in turn.
    pub fn update(&mut self, peer: &Peer, changes: &Changes) -> Result<Update, Error> {
        let chs = match changes {
            Changes::Rebuilt => {
                self.rehydrate(peer)?;
                return Ok(Update::Reset);
            }
            Changes::Applied(chs) => chs,
        };
        // §1.7 The middleware first: whoever is signed in, and whatever
        // the tables it reads now say.
        if *peer.ctx() != self.ctx || chs.iter().any(|c| self.guards.contains(c.table())) {
            let c = closure(peer, &self.name)?;
            let outcome = middleware(peer, &self.name, &c, &self.args)?;
            if *peer.ctx() != self.ctx || outcome != self.outcome {
                self.ctx = peer.ctx().clone();
                self.outcome = outcome;
                self.hydrate(peer, &c)?;
                return Ok(Update::Reset);
            }
        }
        let Some(v) = &mut self.held else {
            return Ok(Update::Unchanged);
        };
        if chs.is_empty() {
            return Ok(Update::Unchanged);
        }
        let patches = match push(peer, chs, v) {
            Ok(ps) => ps,
            // A fault leaves the engine's view as it was, and stale: read it
            // again, and say so.
            Err(_) => {
                self.rehydrate(peer)?;
                return Ok(Update::Reset);
            }
        };
        if patches.is_empty() {
            return Ok(Update::Unchanged);
        }
        splice(&mut self.rows, &patches);
        Ok(Update::Patched(patches))
    }
}

fn push(peer: &Peer, chs: &[Change], v: &mut view::View) -> Result<Vec<Patch>, EvalFault> {
    view::push_all(peer.schema(), peer.store(), chs, v)
}

fn closure(peer: &Peer, name: &str) -> Result<Closure, Error> {
    let (fh, _) = peer.domain().query(name)?;
    Ok(peer.domain().closures()[fh].clone())
}

// The middleware over the peer's store, as the peer's user: a verdict is an
// outcome, a bug is an error.
fn middleware(peer: &Peer, name: &str, c: &Closure, args: &Args) -> Result<Outcome, Error> {
    match eval::middleware(peer.schema(), c, peer.ctx(), args, peer.store()) {
        Ok(o) => Ok(Ok(o)),
        Err(EvalFault::Verdict(r)) => Ok(Err(r)),
        Err(EvalFault::Bug(b)) => Err(Error::Bug(format!("{name}: {b:?}"))),
    }
}

fn fault(name: &str, e: EvalFault) -> Error {
    match e {
        EvalFault::Verdict(r) => Error::Refused(r),
        EvalFault::Bug(b) => Error::Bug(format!("{name}: {b:?}")),
    }
}

/// Apply patches in order, in place: the definition a list is held to
/// (`ark::view::splice`, without the copy).
pub fn splice<T: Clone>(xs: &mut Vec<T>, ps: &[Patch])
where
    Value: Into<T>,
{
    for p in ps {
        match p {
            Patch::Insert { at, node } => {
                let at = (*at).min(xs.len());
                xs.insert(at, node.clone().into());
            }
            Patch::Remove { at } => {
                if *at < xs.len() {
                    xs.remove(*at);
                }
            }
            Patch::Update { at, node } => {
                if *at < xs.len() {
                    xs[*at] = node.clone().into();
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../ark/tests/support/churn.rs"]
mod churn;

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use crate::{args, demo, Options};
    use ark::authoring::*;
    use ark::store::Store as _;

    // A shelf: playlists and their items, with `owned` providing the
    // playlist a procedure is about — the shape of harken's playlists
    // router, small enough to read (§1.7).
    pub struct Shelf {
        pub playlist: Table<Playlist>,
        pub item: Table<Item>,
    }
    impl Tables for Shelf {
        fn open() -> Self {
            Shelf {
                playlist: table(),
                item: table(),
            }
        }
    }

    pub struct Playlist {
        pub id: Id<Playlist>,
        pub name: Text,
        pub user_id: Text,
    }
    impl Row for Playlist {
        const NAME: &str = "playlist";
        type Key = (Id<Playlist>,);
        fn columns() -> Columns<Self> {
            columns().id(Self::id).text(Self::name).text(Self::user_id).key((Self::id,))
        }
    }
    #[allow(non_upper_case_globals)]
    impl Playlist {
        pub const id: Col<Self, Id<Self>> = col("id");
        pub const name: Col<Self, Text> = col("name");
        pub const user_id: Col<Self, Text> = col("user_id");
    }

    pub struct Item {
        pub playlist_id: Id<Playlist>,
        pub track: Text,
        pub pos: Int,
    }
    impl Row for Item {
        const NAME: &str = "item";
        type Key = (Id<Playlist>, Text);
        fn columns() -> Columns<Self> {
            columns()
                .id(Self::playlist_id)
                .refs::<Playlist>()
                .text(Self::track)
                .int(Self::pos)
                .key((Self::playlist_id, Self::track))
        }
    }
    #[allow(non_upper_case_globals)]
    impl Item {
        pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
        pub const track: Col<Self, Text> = col("track");
        pub const pos: Col<Self, Int> = col("pos");
    }

    pub struct Shown {
        pub list: Text,
        pub track: Text,
    }
    impl Record for Shown {
        fn fields() -> Fields<Self> {
            fields().field("list", text()).field("track", text())
        }
    }

    pub struct Named {
        pub name: Text,
    }
    impl Input for Named {
        fn schema() -> Object<Self> {
            object().field("name", text())
        }
    }
    pub struct Owned {
        pub playlist_id: Id<Playlist>,
    }
    impl Input for Owned {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>())
        }
    }
    pub struct Rename {
        pub playlist_id: Id<Playlist>,
        pub name: Text,
    }
    impl Input for Rename {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>()).field("name", text())
        }
    }
    pub struct Put {
        pub playlist_id: Id<Playlist>,
        pub track: Text,
    }
    impl Input for Put {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>()).field("track", text())
        }
    }

    fn shelf() -> Router<Shelf> {
        let r = router::<Shelf>("shelf");
        let owned = r.provide("owned", |ctx, db, input: &Owned| {
            db.playlist
                .get((input.playlist_id,))
                .filter(|row| row.user_id.eq(ctx.user))
                .or_refuse("not your playlist")
        });
        r.routes((
            r.input::<Named>().mutation("create", |ctx, db, input| {
                db.playlist.insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
            }),
            owned.input::<Rename>().mutation("rename", |_ctx, db, input, playlist| {
                db.playlist.update((playlist.id,), |row| Playlist {
                    id: row.id,
                    name: input.name,
                    user_id: row.user_id,
                })
            }),
            owned.input::<Put>().mutation("put", |_ctx, db, input, playlist| {
                let last = db.item.filter(Item::playlist_id.eq(playlist.id)).order_by(Item::pos.desc()).first();
                db.item.insert(Item {
                    playlist_id: playlist.id,
                    track: input.track,
                    pos: last.map_or(0, |row| row.pos).add(1),
                })
            }),
            owned.input::<Owned>().mutation("drop", |_ctx, db, _input, playlist| {
                let items = db.item.filter(Item::playlist_id.eq(playlist.id)).all();
                for_each(items, |row| db.item.delete((row.playlist_id, row.track)));
                db.playlist.delete((playlist.id,))
            }),
            r.query("playlists", |ctx, db, _input: ()| {
                db.playlist.filter(Playlist::user_id.eq(ctx.user)).order_by(Playlist::name.asc())
            }),
            // Under `owned`: the items of the playlist it provided, each
            // with that playlist's name — so a rename is another outcome.
            owned.input::<Owned>().query("playlist", |_ctx, db, _input, playlist| {
                db.item
                    .filter(Item::playlist_id.eq(playlist.id))
                    .order_by(Item::pos.asc())
                    .map(|item, ()| Shown {
                        list: playlist.name,
                        track: item.track,
                    })
            }),
        ))
    }

    fn peer(opts: Options) -> Peer {
        Peer::open_memory(crate::Domain::new(&Module::new((shelf(),))), opts).unwrap()
    }

    fn tracks(rows: &[Value]) -> Vec<String> {
        rows.iter().map(|r| r.field("track").as_text().to_string()).collect()
    }

    fn made(p: &mut Peer, name: &str) -> ark::value::Id {
        p.mutate("create", args([("name", Value::text(name))])).unwrap();
        p.query("playlists", &args([]))
            .unwrap()
            .as_list()
            .into_iter()
            .find(|r| r.field("name") == Value::text(name))
            .unwrap()
            .field("id")
            .as_id()
    }

    /// §1.7 A playlist renamed under an open page is another outcome of
    /// `owned` — the provided row moved — so the view re-hydrates and says
    /// `Reset`, with the new name on every row; deleted, the middleware
    /// refuses and the list is empty, still `Reset`. A change to the items
    /// alone is patches. Falsified by leaving the middleware's tables out
    /// of `guards` (the rename is pushed over the items, which it does not
    /// touch: `Unchanged`, the old name kept).
    #[test]
    fn the_middleware_moving_resets_the_view() {
        let mut p = peer(Options::alone("alice"));
        let list = made(&mut p, "Road trip");
        for t in ["a", "b"] {
            p.mutate("put", args([("playlist_id", Value::Id(list)), ("track", Value::text(t))]))
                .unwrap();
        }
        p.take_changes();
        let mut v = p.view("playlist", args([("playlist_id", Value::Id(list))])).unwrap();
        assert_eq!(tracks(v.rows()), ["a", "b"]);

        p.mutate("put", args([("playlist_id", Value::Id(list)), ("track", Value::text("c"))]))
            .unwrap();
        let ch = p.take_changes();
        assert!(matches!(v.update(&p, &ch).unwrap(), Update::Patched(ps) if ps.len() == 1));
        assert_eq!(tracks(v.rows()), ["a", "b", "c"]);

        p.mutate("rename", args([("playlist_id", Value::Id(list)), ("name", Value::text("Home"))]))
            .unwrap();
        let ch = p.take_changes();
        assert_eq!(v.update(&p, &ch).unwrap(), Update::Reset);
        assert!(v.rows().iter().all(|r| r.field("list") == Value::text("Home")), "{:?}", v.rows());
        assert_eq!(
            v.rows(),
            p.query("playlist", &args([("playlist_id", Value::Id(list))])).unwrap().as_list()
        );

        // Another playlist's rename reads the same table, runs the middleware
        // again, and comes to the same outcome: nothing to reset.
        let other = made(&mut p, "Other");
        p.take_changes();
        p.mutate("rename", args([("playlist_id", Value::Id(other)), ("name", Value::text("Else"))]))
            .unwrap();
        let ch = p.take_changes();
        assert_eq!(v.update(&p, &ch).unwrap(), Update::Unchanged);

        p.mutate("drop", args([("playlist_id", Value::Id(list))])).unwrap();
        let ch = p.take_changes();
        assert_eq!(v.update(&p, &ch).unwrap(), Update::Reset);
        assert!(v.rows().is_empty() && !v.incremental(), "a refusal is an empty list");
        assert!(p.query("playlist", &args([("playlist_id", Value::Id(list))])).is_err());
    }

    /// §1.7 Another user signed in is another scope for `playlists`
    /// (`ctx.user` in its filter), even when the view is told the store did
    /// not move: `Reset`, and what that user makes next is pushed in their
    /// scope. Falsified by not comparing the context in `update` (the first
    /// update is `Unchanged`).
    #[test]
    fn another_user_resets_the_view() {
        let mut p = peer(Options::dev("alice"));
        let mut v = p.view("playlists", args([])).unwrap();
        assert!(v.rows().is_empty());
        p.sign_in("bob", "s2", Some("bob".into())).unwrap();
        // Told nothing moved, the view still notices whose it is now…
        assert_eq!(v.update(&p, &Changes::Applied(vec![])).unwrap(), Update::Reset);
        // …and signing in with nothing pending moves nothing (R2): the view
        // already follows its user.
        let ch = p.take_changes();
        assert_eq!(ch, Changes::Applied(vec![]));
        assert_eq!(v.update(&p, &ch).unwrap(), Update::Unchanged, "{ch:?}");
        made(&mut p, "His");
        let ch = p.take_changes();
        assert!(matches!(v.update(&p, &ch).unwrap(), Update::Patched(_)));
        assert_eq!(v.rows(), p.query("playlists", &args([])).unwrap().as_list());
        assert_eq!(v.rows().len(), 1);
    }

    /// The demo's `items`, patch for patch: a track added at the end is one
    /// `Insert` at the end; two added in one settle are two inserts in
    /// order; a store replaced whole is `Reset`. Falsified by settling the rebuilt keys
    /// in reverse (the batch's inserts come out as `at: 2` then `at: 2`,
    /// which splices them the wrong way round).
    #[test]
    fn the_demos_items_patch_by_patch() {
        let mut p = Peer::open_memory(demo::domain(), Options::alone("me")).unwrap();
        p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        let list = p.store().scan("playlist")[0]["id"].clone();
        let put = |p: &mut Peer, t: &str| {
            p.mutate("add_to_playlist", args([("playlist_id", list.clone()), ("track_id", Value::text(t))]))
                .unwrap();
        };
        put(&mut p, "a");
        p.take_changes();
        let mut v = p.view("items", args([("playlist_id", list.clone())])).unwrap();
        let row = |p: &Peer, t: &str| Value::Struct(p.store().get("item", &[list.clone(), Value::text(t)]).unwrap());

        put(&mut p, "b");
        let ch = p.take_changes();
        assert_eq!(
            v.update(&p, &ch).unwrap(),
            Update::Patched(vec![Patch::Insert { at: 1, node: row(&p, "b") }])
        );

        put(&mut p, "c");
        put(&mut p, "d");
        let ch = p.take_changes();
        assert_eq!(
            v.update(&p, &ch).unwrap(),
            Update::Patched(vec![
                Patch::Insert { at: 2, node: row(&p, "c") },
                Patch::Insert { at: 3, node: row(&p, "d") },
            ])
        );
        assert_eq!(v.update(&p, &Changes::Applied(vec![])).unwrap(), Update::Unchanged);
        assert_eq!(v.update(&p, &Changes::Rebuilt).unwrap(), Update::Reset);
        assert_eq!(v.rows(), p.query("items", &args([("playlist_id", list)])).unwrap().as_list());
    }

    /// `docs/plan-perf.md` R2: a rebase is patches. Bob goes offline and
    /// adds to a shared playlist while alice's additions land at the
    /// server; he comes back, hers land under his pending ones, and his own
    /// are then confirmed. His view of `items` is told every transition the
    /// rebase made — his tracks going, hers arriving, his coming back after
    /// them — so it is `Patched`, never `Reset`, and after every settle it
    /// is the query, a fresh hydrate, and its own splice. Four rounds, bob
    /// with one to four pending. Falsified by reporting a rebase as the
    /// replay it was (`Rebuilt`): the first reconnect is a `Reset`.
    #[test]
    fn a_rebase_is_patches_to_the_demos_items() {
        use ark::live::{ConnId, Silent};
        use ark::peer::Authority;
        use ark::protocol::{open_access, trusting, Server};

        fn settle(s: &mut Server<Silent>, peers: &mut [(ConnId, &mut Peer)]) {
            loop {
                let mut moved = false;
                for (c, p) in peers.iter_mut() {
                    for m in p.take_outgoing() {
                        moved = true;
                        s.recv(*c, m);
                    }
                }
                for (c, m) in s.take_outgoing() {
                    if let Some((_, p)) = peers.iter_mut().find(|(pc, _)| *pc == c) {
                        moved = true;
                        p.recv(m);
                    }
                }
                if !moved {
                    return;
                }
            }
        }

        let d = demo::domain();
        let mut a = Authority::new(d.module().schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let mut s = Server::open(trusting(), open_access(), Silent, a);
        let dev = |u: &str| Options::dev(u).with_autos(crate::Autos::seeded(u.as_bytes()[0] as u64));
        let mut alice = Peer::open_memory(demo::domain(), dev("alice")).unwrap();
        let mut bob = Peer::open_memory(demo::domain(), dev("bob")).unwrap();
        alice.connected();
        bob.connected();
        alice.mutate("create_playlist", args([("name", Value::text("Shared"))])).unwrap();
        settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
        let list = bob.store().scan("playlist")[0]["id"].clone();
        let put = |p: &mut Peer, t: String| {
            p.mutate("add_to_playlist", args([("playlist_id", list.clone()), ("track_id", Value::text(t))]))
                .unwrap();
        };
        let mut v = bob.view("items", args([("playlist_id", list.clone())])).unwrap();
        let _ = bob.take_changes();
        let mut shown = v.rows().to_vec();
        let mut patched = 0;
        let mut told = |bob: &mut Peer, v: &mut View, what: &str| {
            let ch = bob.take_changes();
            assert!(matches!(ch, Changes::Applied(_)), "{what}: a rebase is changes, not {ch:?}");
            match v.update(bob, &ch).unwrap() {
                Update::Reset => panic!("{what}: the view was reset"),
                Update::Patched(ps) => {
                    splice(&mut shown, &ps);
                    patched += 1;
                }
                Update::Unchanged => {}
            }
            assert_eq!(
                v.rows(),
                bob.query("items", &args([("playlist_id", list.clone())])).unwrap().as_list(),
                "{what}: the query"
            );
            assert!(
                ark::view::contract(bob.schema(), bob.store(), v.entries().unwrap()),
                "{what}: a fresh hydrate"
            );
            assert_eq!(shown, v.rows(), "{what}: its own splice");
        };
        for round in 0..4 {
            bob.disconnected();
            s.disconnect(2);
            for k in 0..=round {
                put(&mut bob, format!("b{round}.{k}"));
            }
            told(&mut bob, &mut v, &format!("round {round}: bob's, offline"));
            put(&mut alice, format!("a{round}.0"));
            put(&mut alice, format!("a{round}.1"));
            settle(&mut s, &mut [(1, &mut alice)]);
            bob.connected();
            settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
            assert_eq!(bob.pending_len(), 0, "round {round}");
            told(&mut bob, &mut v, &format!("round {round}: bob back, rebased and confirmed"));
            // Alice's two of this round sit before bob's, as the log has them.
            let tracks: Vec<String> = v.rows().iter().map(|r| r.field("track_id").as_text().to_string()).collect();
            let at = tracks.iter().position(|t| *t == format!("a{round}.0")).unwrap();
            assert_eq!(tracks[at + 2], format!("b{round}.0"), "round {round}: {tracks:?}");
        }
        assert_eq!(patched, 8, "every step was patches");
        assert!(bob.take_rejections().is_empty());
    }

    /// §1.5 The demo's one query under the contract: seeded churn over
    /// `item` and `playlist`, the view a fresh hydrate after every batch.
    /// Falsified by reconciling a source key from the change's own row
    /// rather than the store (a row added and removed in one batch stays).
    #[test]
    fn the_demos_items_hold_the_contract() {
        let mut p = Peer::open_memory(demo::domain(), Options::alone("me")).unwrap();
        p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        p.mutate("create_playlist", args([("name", Value::text("Yours"))])).unwrap();
        let lists: Vec<Value> = p.store().scan("playlist").iter().map(|r| r["id"].clone()).collect();
        for (i, t) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            p.mutate(
                "add_to_playlist",
                args([("playlist_id", lists[i % 2].clone()), ("track_id", Value::text(*t))]),
            )
            .unwrap();
        }
        p.pump();
        let v = p.view("items", args([("playlist_id", lists[0].clone())])).unwrap();
        let held = v.entries().unwrap();
        let mut sum = super::churn::Tally::default();
        for seed in 0..16 {
            let t = super::churn::drive("items", p.schema(), &held.plan, &held.env, p.store().clone(), seed, 60);
            sum.inserts += t.inserts;
            sum.removes += t.removes;
            sum.updates += t.updates;
            sum.batches += t.batches;
        }
        assert!(sum.inserts > 0 && sum.removes > 0 && sum.updates > 0 && sum.batches > 0, "{sum:?}");
    }
}
