//! A data directory an older build wrote opens in this one (spec/README.md
//! § The data directory): `tests/data/server/` is a server's — its log
//! (`log/kv`, `log/log`), its local instance beside it (`log/local.*`),
//! the key its tokens are signed with (`session.key`) and a device's
//! replica it ran in process (`replica/`, as harken's scanner is) — written
//! once by `ARK_WRITE_DATA=1 cargo test -p ark-peer --test old_data` and
//! never again by a test. A change to what is on disk either reads this
//! directory as it is, or comes with a migration that does, or writes a
//! new fixture beside it with a note of what moved; this one stays.
//!
//! Written again once, when entries took their cause (`Entry::cause`,
//! docs/plan-observe.md): that change started every log afresh, by
//! decision, rather than migrating one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ark::prelude::*;
use ark::schema::Space;
use ark_auth::server::{Auth, Mode};
use ark_auth::{Role, SignIn};
use ark_peer::link::{bell, wait};
use ark_peer::{Config, Files, Peer, Server};
use arkdb::demo::{CreatePlaylist, Playlist};

/// The fixture's app: the demo's playlists, and sign-in's two tables.
#[derive(Schema)]
pub struct App {
    pub playlist: Playlist,
    pub sign_in: SignIn,
    pub role: Role,
}

ark_auth::install!(App);

#[mutator("app.create_playlist")]
fn create_playlist(ctx: &Ctx, db: &mut Db<App>, input: CreatePlaylist) -> Result<(), Refusal> {
    db.playlist.insert(Playlist {
        id: ctx.new_id(),
        name: input.name,
        user_id: ctx.user().into(),
    })
}

fn config() -> Config {
    Config::of(Arc::new(ark::register!(
        Domain::new::<App>("app", "1").with(ark_auth::register::<App>); create_playlist)))
}

/// The token the writer's sign-in was given, and what the fixture holds:
/// the hub's head and state hash, and `N` playlists, `p0`… — enough that
/// the device's local instance folds into its `local.kv` too.
const TOKEN: &str = "c3d0755a76b5a48571c12ae1b690ebed.e0d2394e21051a98976bd9d4d1f10569192f49f49b56c0d235ed92ed3273d0a1";
const HEAD: i64 = 72;
const STATE: &str = "9781dcf488c5df53e9f7a28cfdf17fa345565dcd57c04ae8b40c27f8ef3d67d9";
const N: usize = 70;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/server")
}

fn auth(data: &Path) -> Arc<Auth> {
    Arc::new(
        Auth::open::<App>(Mode::Dev, "http://localhost", data)
            .unwrap()
            .with_ttl(100 * 365 * 86_400_000),
    )
}

/// The server over `data`: its hub, and its sign-in.
fn server(data: &Path) -> (ark_peer::Handle, Arc<Auth>) {
    let auth = auth(data);
    // A server keeping one entry folds its log into `kv` as it goes.
    let mut s = Server::new(Config { keep: Some(1), ..config() }).auth(auth.clone());
    s.data = Some(data.to_path_buf());
    (s.build().unwrap().0, auth)
}

fn names() -> Vec<String> {
    (0..N).map(|n| format!("p{n}")).collect()
}

fn held(store: &ark::store::Store) -> Vec<String> {
    let mut ps = ark::db::read::<App, _>(store, |db| db.playlist.scan());
    assert!(ps.iter().all(|p| p.user_id == "alice"), "every playlist alice's");
    ps.sort_by_key(|p| p.name[1..].parse::<usize>().unwrap());
    ps.into_iter().map(|p| p.name).collect()
}

/// The fixture, written: alice signs in, her device (a replica in the
/// data directory, linked in process) creates `N` playlists, and they
/// are confirmed; the hub then stops. Prints what the test is to expect.
fn write() {
    let data = fixture();
    let _ = std::fs::remove_dir_all(&data);
    let (hub, auth) = server(&data);
    let login = auth.issue(&ark_auth::server::dev_account("alice"), "laptop").unwrap();
    let who = auth.authenticate()(&login.token).unwrap();
    let mut device = Peer::open(config(), who, Box::new(Files::new(data.join("replica")).unwrap()), false);
    let (wake, rx) = bell();
    device.set_waker(wake);
    device.set_token(Some(login.token.clone()));
    device.connect_with("", hub.dialer(auth.authenticate()));
    for name in names() {
        let input = CreatePlaylist { name }.to_bytes();
        device.author(Space::Durable, "app.create_playlist", input, 1).unwrap();
    }
    for _ in 0..400 {
        device.pump();
        if device.linked && device.log.pending.is_empty() && device.log.log().head().0 > N as i64 {
            break;
        }
        wait(&device, &rx, std::time::Duration::from_millis(5));
    }
    assert!(device.log.pending.is_empty(), "confirmed");
    // One entry after the last fold, in `log/log`: bob's sign-in.
    auth.issue(&ark_auth::server::dev_account("bob"), "phone").unwrap();
    let (head, state) = hub.with(|p| (p.log.log().head().0, p.log.store.state_hash())).unwrap();
    hub.stop();
    println!("TOKEN {}\nHEAD {head}\nSTATE {}", login.token, ark::hash::hex(&state.0));
}

/// The fixture opens: the key reads back and verifies the token it signed,
/// the session row judges it alice's, the hub's log and the device's
/// replica hold the chain and the rows they held, and the replica its
/// origin and nothing pending. Falsified by a byte changed in `log/kv`
/// (`p42` as `q42`): the hub's state hash differed; in `log/log` (its one
/// record's length): the record was cut as a torn tail, the head 71; in
/// `session.key`: the old token was not judged.
#[test]
fn an_old_data_directory_opens() {
    if std::env::var_os("ARK_WRITE_DATA").is_some() {
        return write();
    }
    let data = tempfile::tempdir().unwrap();
    copy(&fixture(), data.path());
    let (hub, auth) = server(data.path());
    let who = auth.authenticate()(TOKEN).expect("the old token, judged");
    assert_eq!(who.actor, "alice");
    let (head, state, store) = hub
        .with(|p| (p.log.log().head().0, p.log.store.state_hash(), p.log.store.clone()))
        .unwrap();
    assert_eq!((head, ark::hash::hex(&state.0)), (HEAD, STATE.to_string()), "the hub's chain");
    assert_eq!(held(&store), names());
    hub.stop();
    let device = Peer::open(config(), who, Box::new(Files::new(data.path().join("replica")).unwrap()), false);
    assert_eq!(device.log.log().head().0, HEAD - 1, "the replica's chain: all but bob's sign-in");
    assert_eq!(held(&device.log.store), names(), "and its rows");
    assert!(device.log.pending.is_empty(), "nothing pending");
    let replica = device.local.read::<ark_peer::local::Local, _>(ark_peer::local::replica).unwrap();
    let log = device.log.log();
    let origins: Vec<_> = log
        .entries()
        .iter()
        .filter(|e| e.entry.actor == "alice")
        .map(|e| e.entry.origin)
        .collect();
    assert_eq!(origins, vec![replica.origin; N], "its origin");
}

fn copy(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let at = to.join(e.file_name());
        match e.file_type().unwrap().is_dir() {
            true => {
                std::fs::create_dir_all(&at).unwrap();
                copy(&e.path(), &at);
            }
            false => drop(std::fs::copy(e.path(), at).unwrap()),
        }
    }
}
