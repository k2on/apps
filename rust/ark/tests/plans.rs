//! §1.10, §1.16 Views over typed plans: after every settle a view's list
//! is what a fresh run of its plan gives, and its patches splice the list
//! before into it — over the `views/` vectors' schema, its queries as
//! plans, and seeded random churn (inserts, edits and deletes in every
//! table, references refused as the store refuses them).

use std::sync::Arc;

use ark::db::{write_in, Durable};
use ark::peer::Ctx;
use ark::plan::{splice, Make, View};
use ark::prelude::*;
use ark::store::Store;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(unique(user_id, name))]
pub struct Playlist {
    #[ark(key)]
    pub id: Id<Playlist>,
    pub name: String,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(unique(playlist_id, pos))]
pub struct Item {
    #[ark(key, refs(Playlist))]
    pub playlist_id: Id<Playlist>,
    #[ark(key)]
    pub track_id: String,
    pub pos: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
pub struct Person {
    #[ark(key)]
    pub name: String,
    pub born: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(composer))]
pub struct Work {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Person))]
    pub composer: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(work_id))]
pub struct Movement {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Work))]
    pub work_id: String,
    pub no: i64,
    pub part: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(creator), index(movement_id))]
pub struct Song {
    #[ark(key)]
    pub id: String,
    pub title: String,
    pub creator: String,
    #[ark(refs(Movement))]
    pub movement_id: Option<String>,
    pub pos: i64,
}

#[derive(Schema)]
pub struct V {
    pub playlist: Playlist,
    pub item: Item,
    pub person: Person,
    pub work: Work,
    pub movement: Movement,
    pub song: Song,
}

fn ctx() -> Ctx {
    Ctx {
        actor: "alice".into(),
        session: "dev".into(),
        ..Default::default()
    }
}

fn make<T>(f: impl for<'t> Fn(&Db<'t, V>) -> Plan<V, T> + Send + Sync + 'static) -> Make<V, T> {
    Arc::new(move |_: &Ctx, db: &Db<V>| Ok(f(db)))
}

fn one(n: u8) -> Id<Playlist> {
    let mut b = [0; 16];
    b[15] = n;
    Id::new(b)
}

fn movements(db: &Db<V>, w: &Work) -> i64 {
    db.movement.by(Movement::WORK_ID).from(&w.id).count() as i64
}

/// A view held to a fresh run after every push.
trait Check {
    fn push(&mut self, changes: &[ark::store::Change], store: &Store) -> Result<(), String>;
}

impl<T: Clone + PartialEq + Send + std::fmt::Debug + 'static> Check for (View<V, T>, Make<V, T>) {
    fn push(&mut self, changes: &[ark::store::Change], store: &Store) -> Result<(), String> {
        let before = self.0.rows();
        let patches = self.0.push(changes, store).map_err(|e| e.0)?;
        let fresh = ark::plan::answer(&self.1, &ctx(), store).map_err(|e| e.0)?;
        let rows = self.0.rows();
        if rows != fresh {
            return Err(format!("the view is not a fresh run:\n{rows:?}\n{fresh:?}"));
        }
        if splice(&patches, &before) != fresh {
            return Err(format!("the patches do not splice: {patches:?}"));
        }
        Ok(())
    }
}

fn views() -> Vec<(&'static str, Box<dyn Check>)> {
    fn kept<T: Clone + PartialEq + Send + std::fmt::Debug + 'static>(name: &'static str, m: Make<V, T>) -> (&'static str, Box<dyn Check>) {
        let v = View::new(
            m.clone(),
            ctx(),
            &Store::new(Arc::new(<V as Tables>::schema()), ark::schema::Space::Durable),
        )
        .unwrap();
        (name, Box::new((v, m)))
    }
    vec![
        kept(
            "playlist-with-items",
            make(|_| {
                scan::<V, Playlist>().sort(|p| (p.name.clone(), p.id)).map_with(|db, p| {
                    let items = db.item.by(Item::PLAYLIST_ID_POS).from(p.id).rev().take(3);
                    (p.id, p.name.clone(), p.user_id.clone(), items)
                })
            }),
        ),
        kept(
            "top-two-by-pos",
            make(|_| {
                scan::<V, Item>()
                    .filter(|i| i.playlist_id == one(1))
                    .sort(|i| (i.pos, i.playlist_id, i.track_id.clone()))
                    .take(2)
            }),
        ),
        kept("items-of-one-from-its-key", make(|_| scan_from::<V, Item, _, _>(one(1)))),
        kept(
            "group-source",
            make(|_| scan::<V, Song>().group(|s| s.creator.clone(), |_, name, xs| (name.clone(), xs.len() as i64))),
        ),
        kept(
            "related-on-a-non-key-column",
            make(|_| scan::<V, Person>().map_with(|db, p| (db.song.by(Song::CREATOR).from(&p.name).count() as i64, p.name.clone()))),
        ),
        kept(
            "lookup-chain",
            make(|_| {
                scan::<V, Song>().sort(|s| (s.pos, s.id.clone())).map_with(|db, s| {
                    let m = s.movement_id.as_ref().and_then(|m| db.movement.get(m));
                    match m {
                        None => (String::new(), s.title.clone(), String::new()),
                        Some(m) => {
                            let title = db.work.get(&m.work_id).map(|w| w.title).unwrap_or_default();
                            (m.part, s.title.clone(), title)
                        }
                    }
                })
            }),
        ),
        kept(
            "playlist-join-item",
            make(|_| scan::<V, Playlist>().join(scan::<V, Item>(), |p| p.id, |i| i.playlist_id)),
        ),
        kept(
            "kept-max",
            make(|_| {
                scan::<V, Person>().map_with(|db, p| {
                    let max = db.song.by(Song::CREATOR).from(&p.name).all().iter().map(|s| s.pos).max();
                    (max.unwrap_or(-1), p.name.clone())
                })
            }),
        ),
        kept(
            "expression-order-under-a-limit",
            make(|_| {
                scan::<V, Work>()
                    .sort(|w| (w.title.clone(), w.id.clone()))
                    .map_with(|db, w| (movements(db, w), w.title.clone(), w.id.clone()))
                    .sort(|x| (Desc(x.0), x.2.clone()))
                    .take(2)
                    .map(|x| (x.0, x.1.clone()))
            }),
        ),
        kept(
            "having-admits-on-a-child",
            make(|_| {
                scan::<V, Work>()
                    .sort(|w| (w.title.clone(), w.id.clone()))
                    .map_with(|db, w| (movements(db, w), w.title.clone()))
                    .filter(|x| x.0 > 0)
            }),
        ),
    ]
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) % n.max(1)
    }
}

/// One random write, as the store judges it: its changes, or none when
/// refused.
fn churn(r: &mut Rng, store: &mut Store) -> Vec<ark::store::Change> {
    let names = ["Bach", "Gould", "Handel", "Pinnock"];
    let pick = |r: &mut Rng, xs: &[&str]| xs[r.below(xs.len() as u64) as usize].to_string();
    let n = r.below(1000) as i64;
    let k = r.below(11);
    let pid = one(r.below(3) as u8 + 1);
    let res = write_in::<V, Durable, ()>(store, |db| match k {
        0 => db.playlist.put(Playlist {
            id: pid,
            name: format!("p{}", n % 4),
            user_id: pick(&mut Rng(n as u64), &["alice", "bob"]),
        }),
        1 | 2 => db.item.put(Item {
            playlist_id: pid,
            track_id: format!("t{}", n % 9),
            pos: n % 7,
        }),
        3 => db.item.delete((pid, format!("t{}", n % 9))),
        4 => db.playlist.delete(pid),
        5 => db.person.put(Person {
            name: names[(n % 4) as usize].into(),
            born: 1600 + n % 400,
        }),
        6 => db.work.put(Work {
            id: format!("w{}", n % 5),
            composer: names[(n % 3) as usize].into(),
            title: format!("title {}", n % 6),
        }),
        7 => db.movement.put(Movement {
            id: format!("w{}#{}", n % 5, n % 3),
            work_id: format!("w{}", n % 5),
            no: n % 3,
            part: format!("part {}", n % 4),
        }),
        8 | 9 => db.song.put(Song {
            id: format!("s{}", n % 12),
            title: format!("song {}", n % 8),
            creator: names[(n % 4) as usize].into(),
            movement_id: (n % 3 != 0).then(|| format!("w{}#{}", n % 5, n % 3)),
            pos: n % 20,
        }),
        _ => match n % 4 {
            0 => db.song.delete(format!("s{}", n % 12)),
            1 => db.movement.delete(format!("w{}#{}", n % 5, n % 3)),
            2 => db.work.delete(format!("w{}", n % 5)),
            _ => db.person.delete(names[(n % 4) as usize].to_string()),
        },
    });
    res.map(|(_, cs)| cs).unwrap_or_default()
}

/// Every view of every shape, after every batch of three seeds' churn, is
/// a fresh run, and its patches splice. Falsified by `take` keeping its
/// window without refilling it from its input (`expression-order-under-a-
/// limit` held one where a fresh run had two), and by a `map_with` element
/// whose read moved not being run again (`playlist-with-items` kept items
/// it no longer had).
#[test]
fn every_view_is_a_fresh_run_and_its_patches_splice() {
    for seed in [1, 7, 42] {
        let mut store = Store::new(Arc::new(<V as Tables>::schema()), ark::schema::Space::Durable);
        let mut vs = views();
        let mut r = Rng(seed);
        for step in 0..400 {
            let mut changes = vec![];
            for _ in 0..1 + r.below(3) {
                changes.extend(churn(&mut r, &mut store));
            }
            for (name, v) in &mut vs {
                v.push(&changes, &store)
                    .unwrap_or_else(|e| panic!("seed {seed}, step {step}, {name}: {e}"));
            }
        }
    }
}

/// A view whose building reads the store is built again when what it read
/// moves: one over a playlist's items while the playlist is alice's, and
/// none once it is not.
#[test]
fn a_view_built_from_a_read_is_built_again_when_it_moves() {
    let mut store = Store::new(Arc::new(<V as Tables>::schema()), ark::schema::Space::Durable);
    let m: Make<V, Item> = Arc::new(|ctx: &Ctx, db: &Db<V>| {
        let p = db
            .playlist
            .get(one(1))
            .filter(|p| p.user_id == ctx.user())
            .ok_or_else(|| Refusal::new("not yours"))?;
        Ok(scan::<V, Item>().filter(move |i| i.playlist_id == p.id))
    });
    let put = |store: &mut Store, user: &str| {
        write_in::<V, Durable, ()>(store, |db| {
            db.playlist.put(Playlist {
                id: one(1),
                name: "Mix".into(),
                user_id: user.into(),
            })
        })
        .unwrap()
        .1
    };
    put(&mut store, "alice");
    let mut v = View::new(m, ctx(), &store).unwrap();
    let cs = write_in::<V, Durable, ()>(&mut store, |db| {
        db.item.insert(Item {
            playlist_id: one(1),
            track_id: "t".into(),
            pos: 1,
        })
    })
    .unwrap()
    .1;
    assert_eq!(v.push(&cs, &store).unwrap().len(), 1);
    let cs = put(&mut store, "bob");
    assert!(v.push(&cs, &store).is_err(), "built again, and refused");
}

/// A run that refuses leaves nothing behind: what it wrote before the
/// refusal is undone.
#[test]
fn a_refused_run_writes_nothing() {
    let mut store = Store::new(Arc::new(<V as Tables>::schema()), ark::schema::Space::Durable);
    let before = store.state_hash();
    let r = write_in::<V, Durable, ()>(&mut store, |db| {
        db.person.insert(Person {
            name: "Bach".into(),
            born: 1685,
        })?;
        db.person.insert(Person {
            name: "Bach".into(),
            born: 1,
        })
    });
    assert!(r.is_err());
    assert_eq!(store.state_hash(), before);
}

/// A scan from a key's leading columns is the whole table's scan filtered
/// by them, after every batch of churn, and no change to another key moves
/// it. Falsified by `scan_from` taking every key's changes: the second
/// playlist's items came into the first's view.
#[test]
fn a_scan_from_a_key_is_the_scan_filtered() {
    let mut store = Store::new(Arc::new(<V as Tables>::schema()), ark::schema::Space::Durable);
    let from = make(|_| scan_from::<V, Item, _, _>(one(1)));
    let filtered = make(|_| scan::<V, Item>().filter(|i| i.playlist_id == one(1)));
    let mut view = View::new(from.clone(), ctx(), &store).unwrap();
    let mut r = Rng(5);
    for step in 0..400 {
        let changes = churn(&mut r, &mut store);
        let patches = view.push(&changes, &store).unwrap();
        let want = ark::plan::answer(&filtered, &ctx(), &store).unwrap();
        assert_eq!(view.rows(), want, "step {step}");
        let first = |c: &ark::store::Change| {
            let row = match c {
                ark::store::Change::Add { row, .. } | ark::store::Change::Remove { row, .. } | ark::store::Change::Edit { new: row, .. } => row,
            };
            c.table() == "item" && <Item as ark::row::Value>::decode(row).is_ok_and(|i| i.playlist_id == one(1))
        };
        if !changes.iter().any(first) {
            assert!(patches.is_empty(), "step {step}: moved by another key's change");
        }
    }
}
