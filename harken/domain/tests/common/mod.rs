//! One library, one store, every mutation run over it as the machine runs
//! it, every query asked of it: v4's `tests/common`, over the domain in
//! Rust (§1.16).

#![allow(dead_code)]

use std::sync::OnceLock;

use ark::domain::{Domain, Func, Sandbox};
use ark::format::Encode;
use ark::peer::Ctx;
use harken_domain::Id;
use harken_domain::Playlist;
use harken_domain::LIBRARY;
use harken_domain::{browse, library, playlists, CreatePlaylist};
use harken_domain::{AddSong, LibraryEntry};

/// The module, assembled once.
pub fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(harken_domain::domain)
}

/// The library's store: a [`Sandbox`] authored in as session `s`.
pub struct Lib(pub Sandbox);

impl std::ops::Deref for Lib {
    type Target = Sandbox;
    fn deref(&self) -> &Sandbox {
        &self.0
    }
}

impl std::ops::DerefMut for Lib {
    fn deref_mut(&mut self) -> &mut Sandbox {
        &mut self.0
    }
}

impl Default for Lib {
    fn default() -> Self {
        Lib::new()
    }
}

impl Lib {
    pub fn new() -> Lib {
        let ctx = Ctx {
            session: "s".into(),
            now: 1_000,
            ..Default::default()
        };
        Lib(Sandbox::new(harken_domain::shared(), ctx))
    }

    /// A fresh entry's context, as a peer authors one: its id from the
    /// session and the next local number, the clock moved on.
    pub fn ctx(&mut self, user: &str, roles: &[&str]) -> Ctx {
        self.as_(user, roles).entry()
    }

    /// Author as `user` holding the library's role, as the scanner does:
    /// the number of changes, or the refusal's text.
    pub fn mutate<I: Encode>(&mut self, user: &str, f: Func<I>, input: &I) -> Result<usize, String> {
        self.as_(user, &[LIBRARY]).run(f, input)
    }

    /// Author the entry `ctx`.
    pub fn mutate_as<I: Encode>(&mut self, ctx: &Ctx, f: Func<I>, input: &I) -> Result<usize, String> {
        self.run_at(ctx, f.name, &input.to_bytes())
    }

    /// Ask as `user`: the answer, or the refusal's text.
    pub fn query<I: Encode, T: Encode>(&self, user: &str, Func { name, .. }: Func<I, T>, input: &I) -> Result<Vec<T>, String> {
        let ctx = Ctx {
            actor: user.into(),
            roles: vec![],
            ..self.ctx.clone()
        };
        let b = self.domain.ask(&self.store, &ctx, name, &input.to_bytes())?;
        Ok(Vec::from_bytes(&b).unwrap_or_else(|e| panic!("{name}: {e}")))
    }

    /// A query that must answer, as alice.
    pub fn list<I: Encode, T: Encode>(&self, f: Func<I, T>, input: &I) -> Vec<T> {
        self.query("alice", f, input).unwrap_or_else(|e| panic!("{}: {e}", f.name))
    }

    /// The key of the album of that name the albums page lists.
    pub fn album_id(&self, name: &str) -> String {
        let albums = self.list(browse::ALBUMS, &());
        albums
            .into_iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no album {name}"))
            .id
    }

    /// Make a playlist as `user` and answer its id: the newest of theirs.
    pub fn playlist(&mut self, user: &str, name: &str) -> Id<Playlist> {
        self.mutate(user, playlists::CREATE_PLAYLIST, &CreatePlaylist { name: name.into() })
            .unwrap_or_else(|e| panic!("{e}"));
        let mine: Vec<Playlist> = self.query(user, playlists::PLAYLISTS, &()).unwrap();
        mine.into_iter().max_by_key(|p| p.pos).expect("the playlist just made").id
    }

    /// The library read against a playlist.
    pub fn library(&self, playlist: &Id<Playlist>) -> Vec<LibraryEntry> {
        self.list(browse::LIBRARY, &harken_domain::Library { playlist_id: *playlist })
    }

    /// Add a song as the scanner would: a refusal here is a bug.
    pub fn add(&mut self, s: AddSong) {
        self.mutate("alice", library::ADD_SONG, &s).unwrap_or_else(|e| panic!("add_song: {e}"));
    }

    /// A table's rows, decoded.
    pub fn rows<T: Encode>(&self, table: &str) -> Vec<T> {
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
