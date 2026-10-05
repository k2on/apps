//! §12 Versions (`docs/plan-db.md` D1): a peer and a server that run
//! different modules. Sans-io, the two state machines and the frames
//! between them.
//!
//! Two modules over the same playlists. `narrow` is the older; `wide` is
//! the same domain grown the way a domain is allowed to grow (§17): a
//! nullable `note` on `playlist`, a new table `tag`, and a mutator for
//! each. `add_to_playlist` is the same function in both, so it has one
//! hash; `create_playlist` writes the new column, so its body — and its
//! hash — moved.
//!
//! - A peer newer than its server authors a function the server never ran:
//!   the server answers `held`, the intent stays pending — not refused —
//!   and lands when the server comes back with the newer module.
//! - A peer older than its server is fed facts carrying a column and a
//!   table it does not have: it knows it is `behind` from the hash the
//!   server says, applies them projected to its own schema, says no
//!   `Verify`, and the rows it holds are its own table's and usable.

use ark::authoring::*;
use ark::eval::{Args, Ctx};
use ark::hash::{closures, module_hash};
use ark::live::Silent;
use ark::peer::{Authority, Replica};
use ark::protocol::{open_access, trusting, unknown_function, Client, ClientMsg, Mode, Server, ServerMsg};
use ark::store::{project_row, MemoryStore, Refusal, Store};
use ark::value::Value;

// The rows' fields are what the vocabulary reads; nothing here reads them.
#[allow(dead_code)]
mod narrow {
    use ark::authoring::*;

    pub struct Lists {
        pub playlist: Table<Playlist>,
        pub item: Table<Item>,
    }
    impl Tables for Lists {
        fn open() -> Self {
            Lists {
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
        pub track_id: Text,
        pub pos: Int,
    }
    impl Row for Item {
        const NAME: &str = "item";
        type Key = (Id<Playlist>, Text);
        fn columns() -> Columns<Self> {
            columns()
                .id(Self::playlist_id)
                .refs::<Playlist>()
                .text(Self::track_id)
                .int(Self::pos)
                .key((Self::playlist_id, Self::track_id))
                .index((Self::playlist_id, Self::pos))
        }
    }
    #[allow(non_upper_case_globals)]
    impl Item {
        pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
        pub const track_id: Col<Self, Text> = col("track_id");
        pub const pos: Col<Self, Int> = col("pos");
    }

    pub struct Create {
        pub name: Text,
    }
    impl Input for Create {
        fn schema() -> Object<Self> {
            object().field("name", text().min(1))
        }
    }

    pub struct Add {
        pub playlist_id: Id<Playlist>,
        pub track_id: Text,
    }
    impl Input for Add {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
        }
    }

    pub fn module() -> Module {
        let r = router::<Lists>("lists");
        Module::new((r.routes((
            r.input::<Create>().mutation("create_playlist", |ctx, db, input| {
                db.playlist.insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
            }),
            r.input::<Add>().mutation("add_to_playlist", |_ctx, db, input| {
                let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
                db.item.insert(Item {
                    playlist_id: input.playlist_id,
                    track_id: input.track_id,
                    pos: last.map_or(0, |row| row.pos).add(1),
                })
            }),
        )),))
    }
}

#[allow(dead_code)]
mod wide {
    use ark::authoring::*;

    pub struct Lists {
        pub playlist: Table<Playlist>,
        pub item: Table<Item>,
        pub tag: Table<Tag>,
    }
    impl Tables for Lists {
        fn open() -> Self {
            Lists {
                playlist: table(),
                item: table(),
                tag: table(),
            }
        }
    }

    pub struct Playlist {
        pub id: Id<Playlist>,
        pub name: Text,
        pub user_id: Text,
        pub note: Opt<Text>,
    }
    impl Row for Playlist {
        const NAME: &str = "playlist";
        type Key = (Id<Playlist>,);
        fn columns() -> Columns<Self> {
            columns()
                .id(Self::id)
                .text(Self::name)
                .text(Self::user_id)
                .text(Self::note)
                .nullable()
                .key((Self::id,))
        }
    }
    #[allow(non_upper_case_globals)]
    impl Playlist {
        pub const id: Col<Self, Id<Self>> = col("id");
        pub const name: Col<Self, Text> = col("name");
        pub const user_id: Col<Self, Text> = col("user_id");
        pub const note: Col<Self, Opt<Text>> = col("note");
    }

    pub struct Item {
        pub playlist_id: Id<Playlist>,
        pub track_id: Text,
        pub pos: Int,
    }
    impl Row for Item {
        const NAME: &str = "item";
        type Key = (Id<Playlist>, Text);
        fn columns() -> Columns<Self> {
            columns()
                .id(Self::playlist_id)
                .refs::<Playlist>()
                .text(Self::track_id)
                .int(Self::pos)
                .key((Self::playlist_id, Self::track_id))
                .index((Self::playlist_id, Self::pos))
        }
    }
    #[allow(non_upper_case_globals)]
    impl Item {
        pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
        pub const track_id: Col<Self, Text> = col("track_id");
        pub const pos: Col<Self, Int> = col("pos");
    }

    pub struct Tag {
        pub playlist_id: Id<Playlist>,
        pub tag: Text,
    }
    impl Row for Tag {
        const NAME: &str = "tag";
        type Key = (Id<Playlist>, Text);
        fn columns() -> Columns<Self> {
            columns()
                .id(Self::playlist_id)
                .refs::<Playlist>()
                .text(Self::tag)
                .key((Self::playlist_id, Self::tag))
        }
    }
    #[allow(non_upper_case_globals)]
    impl Tag {
        pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
        pub const tag: Col<Self, Text> = col("tag");
    }

    pub struct Create {
        pub name: Text,
    }
    impl Input for Create {
        fn schema() -> Object<Self> {
            object().field("name", text().min(1))
        }
    }

    pub struct Add {
        pub playlist_id: Id<Playlist>,
        pub track_id: Text,
    }
    impl Input for Add {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
        }
    }

    pub struct Note {
        pub playlist_id: Id<Playlist>,
        pub note: Text,
    }
    impl Input for Note {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>().exists()).field("note", text())
        }
    }

    pub struct Label {
        pub playlist_id: Id<Playlist>,
        pub tag: Text,
    }
    impl Input for Label {
        fn schema() -> Object<Self> {
            object().field("playlist_id", id::<Playlist>().exists()).field("tag", text().min(1))
        }
    }

    pub fn module() -> Module {
        let r = router::<Lists>("lists");
        Module::new((r.routes((
            r.input::<Create>().mutation("create_playlist", |ctx, db, input| {
                db.playlist.insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                    note: none(),
                })
            }),
            r.input::<Add>().mutation("add_to_playlist", |_ctx, db, input| {
                let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
                db.item.insert(Item {
                    playlist_id: input.playlist_id,
                    track_id: input.track_id,
                    pos: last.map_or(0, |row| row.pos).add(1),
                })
            }),
            r.input::<Note>().mutation("set_note", |_ctx, db, input| {
                db.playlist.update((input.playlist_id,), |p| Playlist {
                    id: p.id,
                    name: p.name,
                    user_id: p.user_id,
                    note: some(input.note),
                })
            }),
            r.input::<Label>().mutation("tag_playlist", |_ctx, db, input| {
                db.tag.insert(Tag {
                    playlist_id: input.playlist_id,
                    tag: input.tag,
                })
            }),
        )),))
    }
}

fn key(k: u8, n: u8) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = k;
    b[15] = n;
    b
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// A module as a server and a peer hold it: schema, closures, hash.
struct Held {
    module: ark::ir::Module,
    hash: Vec<u8>,
}

fn held(m: &Module) -> Held {
    let built = m.build().clone();
    Held {
        hash: module_hash(&built),
        module: built,
    }
}

impl Held {
    fn authority(&self) -> Authority {
        let mut a = Authority::new(self.module.schema.clone(), closures(&self.module));
        a.ran(self.hash.clone(), closures(&self.module));
        a
    }
    fn server(&self, a: Authority) -> Server<Silent> {
        Server::open(trusting(), open_access(), Silent, a).with_module(self.hash.clone())
    }
    fn client(&self, mode: Mode) -> Client {
        let sch = self.module.schema.clone();
        let r = Replica::open(sch.clone(), closures(&self.module), MemoryStore::empty(sch), 0, vec![]);
        let mut c = Client::open(r, mode, Some("alice".into()));
        c.module = Some(self.hash.clone());
        c
    }
    fn fh(&self, name: &str) -> Vec<u8> {
        closures(&self.module)
            .into_iter()
            .find(|(_, c)| c.function.name == name)
            .map(|(h, _)| h)
            .unwrap_or_else(|| panic!("no {name}"))
    }
}

/// Frames both ways until neither side has anything to say; what the
/// server sent this client, in order.
fn exchange(c: &mut Client, conn: ark::live::ConnId, sv: &mut Server<Silent>) -> Vec<ServerMsg> {
    let mut heard = vec![];
    loop {
        let up = c.take_outgoing();
        for f in up.iter().cloned() {
            sv.recv(conn, f);
        }
        let down: Vec<ServerMsg> = sv.take_outgoing().into_iter().filter(|(to, _)| *to == conn).map(|(_, f)| f).collect();
        if up.is_empty() && down.is_empty() {
            return heard;
        }
        for f in down {
            heard.push(f.clone());
            c.recv(f);
        }
        c.settle();
    }
}

/// A peer newer than its server: `create_playlist` at the wide hash, which
/// the narrow server never ran, is answered `held` — not `reject` — and
/// stays pending, counted in `held`, with no verdict told and the view
/// still holding it. The server restarted over the same log with the wide
/// module (its modules: both) takes it on the next connection, and it is
/// confirmed. And a server from before holding, which said `reject` with
/// the unknown function's sentence, is read the same way.
///
/// Falsified once: with `ServerMsg::Held` read as a `reject` in
/// `Client::recv`, the intent left pending at the first answer, the
/// rejection was told, and nothing landed after the upgrade.
#[test]
fn an_unknown_function_is_held_and_lands_after_the_upgrade() {
    let (n, w) = (held(&narrow::module()), held(&wide::module()));
    let alice = Ctx::new("alice", "dev");
    let old = n.authority();
    let mut sv = n.server(old);
    let mut c = w.client(Mode::Whole);
    c.connected();
    exchange(&mut c, 1, &mut sv);
    let p = Value::Id(key(1, 1));
    c.mutate(
        key(9, 1),
        &alice,
        &w.fh("create_playlist"),
        &args([("id", p.clone())]),
        &args([("name", Value::text("Road"))]),
    )
    .expect("authored");
    let heard = exchange(&mut c, 1, &mut sv);
    assert!(
        matches!(heard.as_slice(), [ServerMsg::Held { id, .. }] if *id == key(9, 1)),
        "the narrow server holds the wide hash: {heard:?}"
    );
    assert_eq!((c.replica.pending.len(), c.held()), (1, 1), "pending, and held");
    assert!(c.replica.rejections.is_empty(), "no verdict was told");
    assert_eq!(c.replica.view.scan("playlist").len(), 1, "the view still has it");
    assert_eq!(c.replica.cursor, 0, "nothing landed");

    // The server, upgraded in place: the wide module, both modules run,
    // and the log as it was — empty, since nothing was taken.
    assert_eq!(sv.authority.log.head_seq(), 0);
    let mut up = w.authority();
    up.ran(n.hash.clone(), closures(&n.module));
    let mut sv = w.server(up);
    c.disconnected();
    c.connected();
    let heard = exchange(&mut c, 2, &mut sv);
    assert!(
        heard.iter().any(|f| matches!(f, ServerMsg::Ack { .. })),
        "acknowledged after the upgrade: {heard:?}"
    );
    assert_eq!((c.replica.pending.len(), c.held(), c.replica.cursor), (0, 0, 1), "confirmed");
    assert!(c.replica.rejections.is_empty());

    // A server from before holding said `reject` with the sentence.
    let mut c = w.client(Mode::Whole);
    c.mutate(
        key(9, 2),
        &alice,
        &w.fh("create_playlist"),
        &args([("id", Value::Id(key(1, 2)))]),
        &args([("name", Value::text("Old"))]),
    )
    .expect("authored");
    c.recv(ServerMsg::Reject {
        id: key(9, 2),
        reason: unknown_function(&w.fh("create_playlist")),
    });
    c.settle();
    assert_eq!((c.replica.pending.len(), c.held()), (1, 1), "an old server's unknown function is held");
    c.recv(ServerMsg::Reject {
        id: key(9, 2),
        reason: "a playlist needs a name".into(),
    });
    c.settle();
    assert_eq!(
        (c.replica.pending.len(), c.replica.rejections.len()),
        (0, 1),
        "any other reject is a verdict"
    );
}

/// A peer older than its server: the wide server's log has a playlist with
/// a `note`, an item, and a `tag`. The narrow peer — fed by facts, and by
/// intent with facts asked for what it cannot run — learns from the
/// server's first answer that it is `behind`; every playlist row it holds
/// is laid out as its own table's (no `note`), the tag is not there, the
/// rows read, nothing diverged, no `Verify` is said; and it authors on
/// top of them — `add_to_playlist`, one hash in both modules — and is
/// confirmed. A peer whose module is the server's is not behind.
///
/// Falsified once: with `behind` never set (`Client::heard_module`
/// storing `false`), the rows were the server's — four columns, a row of
/// no table — and the `Verify` went out.
#[test]
fn a_narrower_schema_applies_facts_projected() {
    let (n, w) = (held(&narrow::module()), held(&wide::module()));
    let alice = Ctx::new("alice", "dev");
    let mut sv = w.server(w.authority());
    let mut author = w.client(Mode::Whole);
    author.connected();
    exchange(&mut author, 1, &mut sv);
    let p = Value::Id(key(1, 1));
    let say = |c: &mut Client, i: u8, f: &str, autos: Args, a: Args| {
        c.mutate(key(9, i), &alice, &w.fh(f), &autos, &a).unwrap_or_else(|e| panic!("{f}: {e:?}"));
    };
    say(
        &mut author,
        1,
        "create_playlist",
        args([("id", p.clone())]),
        args([("name", Value::text("Road"))]),
    );
    say(
        &mut author,
        2,
        "set_note",
        Args::new(),
        args([("playlist_id", p.clone()), ("note", Value::text("for the drive"))]),
    );
    say(
        &mut author,
        3,
        "tag_playlist",
        Args::new(),
        args([("playlist_id", p.clone()), ("tag", Value::text("summer"))]),
    );
    say(
        &mut author,
        4,
        "add_to_playlist",
        Args::new(),
        args([("playlist_id", p.clone()), ("track_id", Value::text("t1"))]),
    );
    exchange(&mut author, 1, &mut sv);
    assert_eq!(author.replica.cursor, 4);
    assert!(!author.behind(), "a peer of the server's own module is not behind");

    for mode in [Mode::ByFacts, Mode::Whole] {
        let mut old = n.client(mode);
        old.connected();
        let head = sv.authority.log.head_seq();
        exchange(&mut old, 2, &mut sv);
        assert!(old.behind(), "{mode:?}: the server's module is not this peer's");
        assert_eq!(old.replica.cursor, head, "{mode:?}");
        assert!(old.replica.diverged.is_empty(), "{mode:?}: nothing diverged");
        let tbl = n.module.schema.lookup_table("playlist").unwrap();
        let rows = old.replica.confirmed.scan("playlist");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_of(tbl), "{mode:?}: the row is this peer's table's: {:?}", rows[0]);
        assert_eq!(rows[0].get("name"), Some(&Value::text("Road")));
        assert_eq!(rows[0].get("note"), None, "{mode:?}: a column this schema lacks is dropped");
        assert_eq!(old.replica.confirmed.scan("item").len(), sv.authority.store.scan("item").len());
        old.verify_all();
        assert!(
            !old.take_outgoing().iter().any(|m| matches!(m, ClientMsg::Verify { .. })),
            "{mode:?}: no Verify while behind"
        );
        old.mutate(
            key(9, 10 + mode as u8),
            &alice,
            &n.fh("add_to_playlist"),
            &Args::new(),
            &args([("playlist_id", p.clone()), ("track_id", Value::text(format!("t{mode:?}")))]),
        )
        .expect("authored over a projected row");
        exchange(&mut old, 2, &mut sv);
        assert_eq!(old.replica.pending.len(), 0, "{mode:?}: confirmed");
        assert!(old.replica.rejections.is_empty(), "{mode:?}: {:?}", old.replica.rejections);
        sv.disconnect(2);
    }
}

/// Projection narrows and widens by what may be absent: a column the table
/// lacks is dropped, a nullable one the row lacks is `Null`, and one the
/// table requires and the row lacks is still `MalformedRow`.
///
/// Falsified once: with a missing column read as `Null` whatever its
/// nullability, the last assertion failed.
#[test]
fn projection_never_invents_a_required_value() {
    let (n, w) = (held(&narrow::module()), held(&wide::module()));
    let (nt, wt) = (
        n.module.schema.lookup_table("playlist").unwrap(),
        w.module.schema.lookup_table("playlist").unwrap(),
    );
    let row = |pairs: Vec<(&str, Value)>| ark::store::Row::from_struct(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let full = row(vec![
        ("id", Value::Id(key(1, 1))),
        ("name", Value::text("Road")),
        ("user_id", Value::text("alice")),
        ("note", Value::text("n")),
    ]);
    let narrowed = project_row(nt, &full).expect("narrowed");
    assert!(narrowed.is_of(nt) && narrowed.get("note").is_none());
    let widened = project_row(wt, &narrowed).expect("widened");
    assert!(widened.is_of(wt) && widened.get("note") == Some(&Value::Null));
    let short = row(vec![("id", Value::Id(key(1, 1))), ("name", Value::text("Road"))]);
    assert!(matches!(project_row(nt, &short), Err(Refusal::MalformedRow(t, _)) if t == "playlist"));
}

/// Facts below the head are widened as the head is (`docs/plan-db.md` D1):
/// a log sequenced under `narrow` — playlists without a `note` — is the
/// wide server's log after its upgrade. Its state at the head holds the
/// rows with `note` `Null`, as a wide peer fed those facts does; and a
/// `Verify` from that peer *below* the head, over the older facts, agrees,
/// where taking a raw row's leaf off a widened row's digest did not.
///
/// Falsified once: with `Log::hash_at` taking the facts above the sequence
/// off the head as they came, the server answered `ok: false`.
#[test]
fn a_verify_below_the_head_over_older_facts_agrees() {
    let (n, w) = (held(&narrow::module()), held(&wide::module()));
    let alice = Ctx::new("alice", "dev");
    let mut old = n.authority();
    let mut author = n.client(Mode::Whole).replica;
    let p = Value::Id(key(1, 1));
    let mut sequence = |r: &mut Replica, i: u8, f: &str, autos: Args, a: Args| {
        let e = r.mutate(key(9, i), &alice, &n.fh(f), &autos, &a).unwrap();
        assert!(matches!(old.sequence_entry(&e), ark::peer::Sequenced::Appended(..)));
    };
    sequence(
        &mut author,
        1,
        "create_playlist",
        args([("id", p.clone())]),
        args([("name", Value::text("Road"))]),
    );
    sequence(
        &mut author,
        2,
        "add_to_playlist",
        Args::new(),
        args([("playlist_id", p.clone()), ("track_id", Value::text("t1"))]),
    );
    sequence(
        &mut author,
        3,
        "create_playlist",
        args([("id", Value::Id(key(1, 2)))]),
        args([("name", Value::text("Later"))]),
    );

    // The server, upgraded: the same entries and facts, the wide schema.
    let mut log = ark::log::Log::empty(w.module.schema.clone());
    for (_, (e, f)) in old.log.entries.clone() {
        log.append(e, f);
    }
    let head = log.state_at(log.head_seq()).unwrap();
    let row = head.get("playlist", std::slice::from_ref(&p)).unwrap();
    assert_eq!(row.get("note"), Some(&Value::Null), "the head holds the column, Null");
    let mut up = w.authority();
    up.ran(n.hash.clone(), closures(&n.module));
    up.log = log;
    up.store = head;
    let mut sv = w.server(up);

    // A wide peer fed the first two entries by their facts: one below the head.
    let mut c = w.client(Mode::ByFacts);
    for (seq, (e, f)) in old.log.entries.range(..=2) {
        c.replica.receive_with(*seq, e.clone(), f.clone());
    }
    c.replica.settle();
    assert_eq!(c.replica.cursor, 2);
    c.connected();
    let _ = c.take_outgoing();
    let (seq, hash) = c.replica.verify_at();
    sv.recv(
        1,
        ClientMsg::Hello {
            sub: ark::protocol::Subscription {
                since: 2,
                mode: Mode::ByFacts,
                log_id: None,
                partial: false,
            },
            token: Some("alice".into()),
            spec: ark::ir::SPEC_VERSION,
        },
    );
    let _ = sv.take_outgoing();
    sv.recv(1, ClientMsg::Verify { seq, hash, log_id: None });
    let agreed: Vec<bool> = sv
        .take_outgoing()
        .into_iter()
        .filter_map(|(_, m)| match m {
            ServerMsg::Agree { ok, .. } => Some(ok),
            _ => None,
        })
        .collect();
    assert_eq!(agreed, [true], "a Verify below the head over pre-upgrade facts agrees");
}
