//! One library, one store, every mutation run by the interpreter (the one
//! execution path, §1.12) over it and its changes applied, every query
//! asked of it: v4's `tests/common`, without the native half v4 held the
//! interpreter to, which v5 does not have.

#![allow(dead_code)]

use std::sync::OnceLock;

use ark5::author::{Domain, Id, Node};
use ark5::log::EntryId;
use ark5::peer::Ctx;
use ark5::store::{Change, Store};
use harken_domain5::library::{AddSong, LibraryEntry};
use harken_domain5::playlists::CreatePlaylist;
use harken_domain5::schema::{Playlist, LIBRARY};

/// The module, assembled once.
pub fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(harken_domain5::domain)
}

/// A function's binding by its last name: `create_playlist` is
/// `playlists.create_playlist`.
pub fn full(name: &str) -> String {
    let m = &domain().module;
    let hit: Vec<&String> = m.bindings.iter().map(|b| &b.0).filter(|b| b.rsplit('.').next() == Some(name)).collect();
    match hit[..] {
        [b] => b.clone(),
        _ => panic!("{name}: {hit:?}"),
    }
}

pub struct Lib {
    pub store: Store,
    /// What the last mutation changed.
    pub last: Vec<Change>,
    /// The session's next entry, and the clock.
    local: i64,
    now: i64,
}

impl Default for Lib {
    fn default() -> Self {
        Lib::new()
    }
}

impl Lib {
    pub fn new() -> Lib {
        Lib {
            store: Store::new(domain().schema.clone()),
            last: vec![],
            local: 0,
            now: 1_000,
        }
    }

    /// A fresh entry's context, as a peer authors one: its id from the
    /// session and the next local number, the clock moved on.
    pub fn ctx(&mut self, user: &str, roles: &[&str]) -> Ctx {
        self.local += 1;
        self.now += 1;
        Ctx {
            id: EntryId::of("s", self.local),
            actor: user.into(),
            session: "s".into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            now: self.now,
        }
    }

    /// Author as `user` holding the library's role, as the scanner does:
    /// the number of changes, or the refusal's text.
    pub fn mutate(&mut self, user: &str, name: &str, input: &impl Node) -> Result<usize, String> {
        let ctx = self.ctx(user, &[LIBRARY]);
        self.mutate_as(&ctx, name, input)
    }

    /// Author the entry `ctx`.
    pub fn mutate_as(&mut self, ctx: &Ctx, name: &str, input: &impl Node) -> Result<usize, String> {
        self.mutate_bytes(ctx, name, &input.to_bytes())
    }

    /// Author the entry `ctx`, its input as bytes.
    pub fn mutate_bytes(&mut self, ctx: &Ctx, name: &str, input: &[u8]) -> Result<usize, String> {
        let changes = domain().run(&self.store, ctx, &full(name), input).map_err(|r| r.0)?;
        self.store.apply_all(&changes).unwrap_or_else(|r| panic!("{name}: the store refused {r}"));
        self.last = changes;
        Ok(self.last.len())
    }

    /// Ask as `user`: the answer, or the refusal's text.
    pub fn query<T: Node>(&self, user: &str, name: &str, input: &impl Node) -> Result<T, String> {
        let ctx = Ctx {
            id: EntryId::of("q", 0),
            actor: user.into(),
            session: "q".into(),
            roles: vec![],
            now: self.now,
        };
        let b = domain().ask(&self.store, &ctx, &full(name), &input.to_bytes()).map_err(|r| r.0)?;
        Ok(T::from_bytes(&b).unwrap_or_else(|e| panic!("{name}: {e}")))
    }

    /// A query that must answer, as alice.
    pub fn list<T: Node>(&self, name: &str, input: &impl Node) -> Vec<T> {
        self.query("alice", name, input).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// Make a playlist as `user` and answer its id: the newest of theirs.
    pub fn playlist(&mut self, user: &str, name: &str) -> Id<Playlist> {
        self.mutate(user, "create_playlist", &CreatePlaylist { name: name.into() })
            .unwrap_or_else(|e| panic!("{e}"));
        let mine: Vec<Playlist> = self.query(user, "playlists", &()).unwrap();
        mine.into_iter().max_by_key(|p| p.pos).expect("the playlist just made").id
    }

    /// The library read against a playlist.
    pub fn library(&self, playlist: &Id<Playlist>) -> Vec<LibraryEntry> {
        self.list(
            "library",
            &harken_domain5::library::Library {
                playlist_id: playlist.clone(),
            },
        )
    }

    /// Add a song as the scanner would: a refusal here is a bug.
    pub fn add(&mut self, s: AddSong) {
        self.mutate("alice", "add_song", &s).unwrap_or_else(|e| panic!("add_song: {e}"));
    }

    /// A table's rows, decoded.
    pub fn rows<T: Node>(&self, table: &str) -> Vec<T> {
        self.store.rows(table).map(|r| T::from_bytes(r).unwrap()).collect()
    }
}

/// A song with a title, an artist and an album, and nothing else said.
pub fn song(title: &str, artist: &str, album: &str) -> AddSong {
    AddSong {
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        ..AddSong::default()
    }
}

/// The classical chain's arguments, as the tests read the music.
#[allow(clippy::too_many_arguments)]
pub fn track(
    title: &str,
    composer: &str,
    album: &str,
    catalogue: &str,
    performer: &str,
    file: &str,
    track_no: i64,
    work_title: &str,
    movement_no: i64,
) -> AddSong {
    AddSong {
        catalogue: catalogue.into(),
        performer: performer.into(),
        file: file.into(),
        track: track_no,
        work_title: work_title.into(),
        movement_no,
        ..song(title, composer, album)
    }
}

/// `s` at `file`.
pub fn with(file: &str, s: AddSong) -> AddSong {
    AddSong { file: file.into(), ..s }
}

/// One field of each row.
pub fn each<T, U>(rows: &[T], f: impl Fn(&T) -> U) -> Vec<U> {
    rows.iter().map(f).collect()
}
