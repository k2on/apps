//! One library, one store, and every mutation held to the interpreter:
//! each is applied natively and through `apply_closure` over copies of the
//! store, compared, then applied. So every test in this directory is an
//! agreement test too. A query has no native half to agree with (a query
//! is its plan, and `ark::view::pull` is what it means), so it is asked
//! once.

#![allow(dead_code)]

use std::collections::BTreeMap;

use ark::authoring::Procedure;
use ark::eval::{Args, Ctx, EvalFault};
use ark::ir::Auto;
use ark::store::{MemoryStore, Refusal};
use ark::value::{Id, Value};

pub fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub struct Lib {
    pub procs: BTreeMap<String, Procedure>,
    /// Every procedure a test has run, by name.
    pub called: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// What the last mutation changed, as the store reports it.
    pub last: Vec<ark::store::Change>,
    pub store: MemoryStore,
    /// The next fresh id's last byte pair, and the clock.
    next: u16,
    now: i64,
}

impl Default for Lib {
    fn default() -> Self {
        Lib::new()
    }
}

impl Lib {
    pub fn new() -> Lib {
        let m = harken_domain::module();
        Lib {
            procs: m.procedures().into_iter().map(|(_, p)| (p.name().to_string(), p)).collect(),
            called: Default::default(),
            last: vec![],
            store: MemoryStore::empty(m.build().schema.clone()),
            next: 0,
            now: 1_000,
        }
    }

    /// Fresh autos for a procedure, as a peer draws them at the origin.
    pub fn autos(&mut self, name: &str) -> Args {
        let f = self.procs[name].function().clone();
        f.autos
            .iter()
            .map(|(n, a)| {
                let v = match a {
                    Auto::NewId(_) => {
                        self.next += 1;
                        let mut b: Id = [0; 16];
                        b[14..].copy_from_slice(&self.next.to_be_bytes());
                        Value::Id(b)
                    }
                    Auto::Now => {
                        self.now += 1;
                        Value::int(self.now)
                    }
                };
                (n.clone(), v)
            })
            .collect()
    }

    /// Author as `user`: the number of changes, or the refusal's text.
    pub fn mutate(&mut self, user: &str, name: &str, a: Args) -> Result<usize, String> {
        let autos = self.autos(name);
        self.mutate_with(user, name, autos, a)
    }

    pub fn mutate_with(&mut self, user: &str, name: &str, autos: Args, a: Args) -> Result<usize, String> {
        let p = self.procs.get(name).unwrap_or_else(|| panic!("no procedure {name}")).clone();
        let ctx = Ctx::new(user, "s");
        let out = p.agrees(&ctx, &autos, &a, &self.store).unwrap_or_else(|e| panic!("{e}"));
        let applied = p.apply(&ctx, &autos, &a, &mut self.store);
        assert_eq!(applied, out, "{name}: applying again gave another answer");
        self.called.borrow_mut().insert(name.into());
        self.last = match &out {
            Ok(Ok(chs)) => chs.clone(),
            _ => vec![],
        };
        match out {
            Ok(Ok(chs)) => Ok(chs.len()),
            Ok(Err(Refusal::Refused(t))) => Err(t),
            Ok(Err(other)) => Err(other.to_string()),
            Err(bug) => panic!("{name}: bug {bug:?}"),
        }
    }

    /// Ask as `user`: the value, or the refusal's text.
    pub fn query(&self, user: &str, name: &str, a: Args) -> Result<Value, String> {
        let p = self.procs.get(name).unwrap_or_else(|| panic!("no procedure {name}"));
        self.called.borrow_mut().insert(name.into());
        match p.query(&Ctx::new(user, "s"), &a, &self.store) {
            Ok(v) => Ok(v),
            Err(EvalFault::Verdict(Refusal::Refused(t))) => Err(t),
            Err(other) => panic!("{name}: {other:?}"),
        }
    }

    /// A query that must answer: its list.
    pub fn list(&self, name: &str, a: Args) -> Vec<Value> {
        self.query("alice", name, a).unwrap_or_else(|e| panic!("{name}: {e}")).as_list()
    }

    /// Make a playlist as `user` and answer its id.
    pub fn playlist(&mut self, user: &str, name: &str) -> Id {
        self.mutate(user, "create_playlist", args([("name", Value::text(name))]))
            .unwrap_or_else(|e| panic!("{e}"));
        // The newest of theirs: a name they already had is numbered.
        self.query(user, "playlists", args([]))
            .unwrap()
            .as_list()
            .into_iter()
            .max_by_key(|p| p.field("pos").as_int())
            .expect("the playlist just made")
            .field("id")
            .as_id()
    }

    /// The library read against a playlist.
    pub fn library(&self, playlist: Id) -> Vec<Value> {
        self.list("library", args([("playlist_id", Value::Id(playlist))]))
    }

    /// Add a song as the scanner would, unwrapped: a refusal here is a bug.
    pub fn add(&mut self, s: Song) {
        self.mutate("alice", "add_song", s.args()).unwrap_or_else(|e| panic!("add_song: {e}"));
    }
}

/// `add_song`'s fifteen arguments, with the defaults an entry that says
/// nothing about them carries.
#[derive(Clone, Debug, Default)]
pub struct Song {
    pub title: &'static str,
    pub artist: &'static str,
    pub album: &'static str,
    pub duration_ms: i64,
    pub file: String,
    pub track: i64,
    pub part: &'static str,
    pub catalogue: &'static str,
    pub performer: &'static str,
    pub bpm: i64,
    pub album_art: &'static str,
    pub artist_art: &'static str,
    pub disc: i64,
    pub work_title: &'static str,
    pub movement_no: i64,
}

impl Song {
    pub fn new(title: &'static str, artist: &'static str, album: &'static str) -> Song {
        Song {
            title,
            artist,
            album,
            ..Song::default()
        }
    }

    pub fn args(&self) -> Args {
        args([
            ("title", Value::text(self.title)),
            ("artist", Value::text(self.artist)),
            ("album", Value::text(self.album)),
            ("duration_ms", Value::int(self.duration_ms)),
            ("file", Value::text(self.file.clone())),
            ("track", Value::int(self.track)),
            ("part", Value::text(self.part)),
            ("catalogue", Value::text(self.catalogue)),
            ("performer", Value::text(self.performer)),
            ("bpm", Value::int(self.bpm)),
            ("album_art", Value::text(self.album_art)),
            ("artist_art", Value::text(self.artist_art)),
            ("disc", Value::int(self.disc)),
            ("work_title", Value::text(self.work_title)),
            ("movement_no", Value::int(self.movement_no)),
        ])
    }
}

/// The classical chain's arguments, as the tests below read the music.
#[allow(clippy::too_many_arguments)]
pub fn track(
    title: &'static str,
    composer: &'static str,
    album: &'static str,
    catalogue: &'static str,
    performer: &'static str,
    file: &str,
    track_no: i64,
    work_title: &'static str,
    movement_no: i64,
) -> Song {
    Song {
        catalogue,
        performer,
        file: file.into(),
        track: track_no,
        work_title,
        movement_no,
        ..Song::new(title, composer, album)
    }
}

/// A field of each row, as text.
pub fn texts(rows: &[Value], field: &str) -> Vec<String> {
    rows.iter().map(|r| r.field(field).as_text().to_string()).collect()
}

/// A field of each row, as an int.
pub fn ints(rows: &[Value], field: &str) -> Vec<i64> {
    rows.iter().map(|r| r.field(field).as_int()).collect()
}
