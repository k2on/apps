//! What an empty demo library starts with: the classical catalogue
//! (github.com/k2on/classical), as `catalogue.json` beside this file holds
//! it — the copy `nix run .#update-catalogue` writes from the flake's
//! `classical` input, and `checks.catalogue` holds to it.
//!
//! Only the demo has this: the real client is built with no features and
//! never compiles the file. The library there is whatever its server's
//! scanner found and its catalogue imported.
//!
//! Everything goes in through the domain's own mutations, the ones
//! `harken_domain::catalogue` says a catalogue is — the demo runs the same
//! functions as every other peer, by the one interpreter, and seeding it
//! any other way would be showing something the engine did not do. Their
//! changes are applied to one store directly rather than one entry at a time
//! through a peer (see [`seeded`]), which is the difference between a demo
//! that opens and one that spends its first ten seconds cloning its own
//! library.
//!
//! Most recordings are public domain on Wikimedia Commons, by way of the mp3
//! transcode Commons generates for every audio file: a browser plays mp3
//! everywhere, and Vorbis in an `.ogg` does not play in Safari at all. The
//! rest are mp3 or FLAC on the Internet Archive. The URL goes in `file`,
//! which is what that column has always been for. A recording that is not
//! public domain carries a `licence` (CC BY, CC BY-SA, or the copyright it
//! is under), and the credit is drawn beside the performer.

use std::sync::OnceLock;

use crate::domain::catalogue::Catalogue;
use crate::domain::shows::Shelf;
use crate::domain::{AddToPlaylist, CreatePlaylist, Library, LibraryEntry};
use ark::format::Encode;
use ark::log::{EntryId, Record, Snapshot};
use ark::peer::Ctx;
use ark::store::Store;
use ark_peer::{Disk, Memory};

use crate::client::{domain, Replica};
use crate::peer::key;
use crate::rows::Id;

/// The podcasts and sermons the demo has beside its catalogue: Gospel in
/// Life's sermons and Full Time Nix, their newest episodes as their feeds
/// said them — and, of Full Time Nix's, who is on each, and one older
/// episode with the start of its transcript — written down in `shows.json`
/// beside this file. The demo plays them from their hosts, as it plays the
/// catalogue's recordings; a server downloads its feeds' (`harken-server`'s
/// `feeds.rs`).
pub fn shelf() -> &'static Shelf {
    static SHELF: OnceLock<Shelf> = OnceLock::new();
    SHELF.get_or_init(|| Shelf::parse(include_str!("shows.json")).expect("shows.json is the shows"))
}

/// The catalogue the demo is, read once.
pub fn catalogue() -> &'static Catalogue {
    static CATALOGUE: OnceLock<Catalogue> = OnceLock::new();
    CATALOGUE.get_or_init(|| Catalogue::parse(include_str!("catalogue.json")).expect("catalogue.json is the catalogue"))
}

/// The demo's library, made by running each mutation once over one store.
///
/// A peer alone sequences each intent through its own authority and writes
/// its chain after every one: seeding four hundred tracks that way is a log
/// of four hundred entries every load. Here each mutation runs once, as the
/// machine runs it, over one store, with the entry a peer would have authored
/// (numbered from one, so the demo is the same library every load) — the
/// same changes the log would have recorded, and no log.
struct Seeder {
    store: Store,
    ctx: Ctx,
    local: i64,
}

impl Seeder {
    fn new() -> Seeder {
        Seeder {
            store: Store::new(domain().schema.clone(), ark::schema::Space::Durable),
            // The demo authors its own library, as a server's scanner does,
            // so it holds the role the library's guard asks
            // (`docs/plan-guards.md` D1).
            ctx: Ctx {
                id: EntryId::of(LOCAL, 0),
                actor: DEMO.into(),
                session: LOCAL.into(),
                roles: vec![crate::domain::LIBRARY.into()],
                now: 1_700_000_000_000,
                ..Default::default()
            },
            local: 0,
        }
    }

    /// One mutation, by its full name. A seed mutation can only be refused
    /// by a mistake — an argument that moved, a key computed two different
    /// ways — and every one of those presents on the page as something
    /// silently absent, so it is an assertion rather than a `let _`.
    fn apply(&mut self, what: &str, name: &str, input: &[u8]) {
        self.local += 1;
        self.ctx.id = EntryId::of(LOCAL, self.local);
        self.ctx.now += 1;
        let f = domain()
            .get(name)
            .and_then(|f| f.proc.clone())
            .unwrap_or_else(|| panic!("{name} writes nothing"));
        f.apply(&mut self.store, &self.ctx, input)
            .unwrap_or_else(|r| panic!("the demo could not author {what}: {r}"));
    }

    /// One mutation by the name the domain binds it to.
    fn mutate(&mut self, what: &str, name: &str, input: &impl Encode) {
        let b = domain().binding(name).unwrap_or_else(|| panic!("the demo authors {name}"));
        self.apply(what, b, &input.to_bytes());
    }

    fn query<T: Encode>(&self, name: &str, input: &impl Encode) -> T {
        let b = domain().binding(name).unwrap_or_else(|| panic!("the demo reads {name}"));
        let bytes = domain()
            .ask(&self.store, &self.ctx, b, &input.to_bytes())
            .unwrap_or_else(|r| panic!("the demo reads {name}: {r}"));
        T::from_bytes(&bytes).unwrap_or_else(|r| panic!("{name}: {r}"))
    }

    /// Which playlist is called `name`.
    fn playlist(&self, name: &str) -> Option<Id> {
        self.query::<Vec<crate::domain::Playlist>>("playlists", &())
            .into_iter()
            .find(|p| p.name == name)
            .map(|p| p.id.0)
    }

    /// The library's ids by title.
    fn ids(&self, titles: &[&str]) -> Vec<Id> {
        let items: Vec<LibraryEntry> = self.query("library", &Library { playlist_id: key([0; 16]) });
        titles
            .iter()
            .filter_map(|t| items.iter().find(|i| i.title == *t))
            .map(|i| i.id.0)
            .collect()
    }
}

/// Who the demo authors as, and under which login: nobody real.
pub const DEMO: &str = "demo";
const LOCAL: &str = "local";

/// The demo's replica: every seeded row, as the confirmed state of a peer that
/// is its own authority.
pub fn seeded() -> Replica {
    open(seeded_disk(1))
}

/// The demo's replica opened from its records: its own authority, as the
/// library's identity.
pub fn open(disk: Memory) -> Replica {
    let mut peer = crate::client::open(Box::new(disk), None, crate::listening::device());
    // The demo's identity is the library's, as its seed was
    // (`docs/plan-guards.md` D1).
    peer.set_roles([crate::domain::LIBRARY]);
    peer.alone();
    peer
}

/// The demo's library `copies` times over, for measuring how a list's cost
/// grows with the library: every copy after the first has its own composers,
/// albums, works, recordings and files (the names carry ` (k)`), so every
/// list is `copies` times as long, and the playlists are the first copy's.
/// `1` is the demo.
#[cfg(test)]
pub fn seeded_times(copies: usize) -> Replica {
    open(seeded_disk(copies))
}

/// The catalogue as copy `k` of it: every name and file with ` (k)` after
/// it, so that nothing in one copy is a row of another. Copy 0 is itself.
fn copy_of(c: &Catalogue, k: usize) -> Catalogue {
    let mut c = c.clone();
    if k == 0 {
        return c;
    }
    let n = |s: &str| match s.is_empty() {
        true => String::new(),
        false => format!("{s} ({k})"),
    };
    for p in &mut c.people {
        (p.name, p.sort_name) = (n(&p.name), n(&p.sort_name));
    }
    for w in &mut c.works {
        w.composer = n(&w.composer);
    }
    for a in &mut c.albums {
        a.name = n(&a.name);
        for t in &mut a.tracks {
            (t.title, t.composer, t.file) = (n(&t.title), n(&t.composer), n(&t.file));
        }
    }
    c
}

/// The records of [`seeded_times`]: a snapshot of the seeded store, and
/// who it is.
pub fn seeded_disk(copies: usize) -> Memory {
    let mut seed = Seeder::new();
    // People, then tracks, then what a track could not carry, copy by copy:
    // the order `Catalogue::calls` gives. The demo has every file it names:
    // they are URLs, played from where they are. What it describes that
    // only a library of its own would have (`Call::if_there`) is not here.
    for k in 0..copies {
        for call in copy_of(catalogue(), k).calls(|url| Some(url.to_string())) {
            if call.if_there {
                continue;
            }
            seed.apply(&call.what, call.name, &call.input);
        }
    }

    // Then the shows, each and its episodes.
    for (show, episodes) in shelf().calls() {
        seed.mutate(&show.title, "add_show", &show);
        for e in episodes {
            seed.mutate(&e.title, "add_episode", &e);
        }
    }
    // …and what their feeds say beside them: who is on each, and what is
    // said in it.
    let (people, transcripts) = shelf().besides();
    for p in people {
        seed.mutate(&p.file, "set_people", &p);
    }
    for t in transcripts {
        seed.mutate(&t.file, "set_transcript", &t);
    }

    // Three playlists, so the sidebar shows what a playlist *is* here: an
    // ordered list somebody made, of which "Favorites" is one and not a
    // special case. Favorites first, so it is the one the library is read
    // against.
    const SETS: &[(&str, &[&str])] = &[
        ("Favorites", &["Clair de lune", "Für Elise", "Alla Hornpipe"]),
        (
            "Piano",
            &[
                "Für Elise",
                "Clair de lune",
                "Ballade No. 1 in G minor, Op. 23",
                "Impromptu in G-flat major, D. 899",
                "Aria",
            ],
        ),
        ("Water Music", &["Alla Hornpipe", "Bourrée", "Sarabande", "Gigue"]),
    ];
    for (name, titles) in SETS {
        seed.mutate(name, "create_playlist", &CreatePlaylist { name: name.to_string() });
        let list = seed.playlist(name).expect("the playlist just made");
        for id in seed.ids(titles) {
            let a = AddToPlaylist {
                playlist_id: key(list),
                media_id: key(id),
            };
            seed.mutate(name, "add_to_playlist", &a);
        }
    }

    // The store, as a peer alone keeps it: a snapshot at the start of an
    // empty chain, which its own authority stands on.
    let snapshot = Snapshot {
        seq: 0,
        hash: Default::default(),
        state: seed.store.state_hash(),
        rows: seed.store.snapshot_rows(),
    };
    let mut disk = Memory::default();
    disk.record(&Record::Rows(snapshot.kv_rows()).enc()).expect("memory takes it");
    ark_peer::local::preset(&mut disk, |r| (r.actor, r.session) = (Some(DEMO.into()), LOCAL.into())).expect("memory takes it");
    disk
}
