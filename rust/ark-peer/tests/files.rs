//! Files beside the log (docs/plan-files.md): a file made offline goes up
//! on the next connection, in chunks, from where a cut-off one stopped;
//! another device fetches it once it opens it; a connection fed by a scope
//! gets only the files its rows name; one wanted before it arrived is sent
//! when it does; and nothing of the bytes is in the log. A file many
//! windows long goes up and down with neither end holding it whole; one
//! larger than a server takes is refused; a part left long is dropped.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::{self, Read};
use std::sync::Arc;

use ark::file::FileRef;
use ark::prelude::*;
use ark_peer::{dev, Authority, Config, ConnId, Memory, Peer, ServerConfig};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
pub struct Photo {
    #[ark(key)]
    pub id: String,
    pub owner: String,
    pub file: FileRef,
}

#[derive(Schema)]
pub struct Photos {
    pub photo: Photo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Add {
    pub id: String,
    pub file: FileRef,
}

#[mutator("photos.add")]
pub fn add(ctx: &Ctx, db: &mut Db<Photos>, input: Add) -> Result<(), String> {
    let owner = ctx.user().to_string();
    db.photo.insert(Photo {
        id: input.id,
        owner,
        file: input.file,
    })
}

#[scope("photos.mine")]
pub fn mine() -> Scope<Photos> {
    Scope::new().on::<Photo>(|ctx, p| p.owner == ctx.user())
}

fn domain() -> Domain {
    ark::functions!(Domain::new::<Photos>("photos", "1"))
}

fn config() -> Config {
    Config::of(Arc::new(domain()))
}

fn server(scoped: bool) -> ServerConfig {
    let mut c = ServerConfig::of(Arc::new(domain()));
    if scoped {
        c.feed = Arc::new(|_| Some("photos.mine".into()));
    }
    c
}

fn device(who: &str) -> Peer {
    Peer::open(config(), dev(who).unwrap(), Box::new(Memory::default()))
}

/// A device on the directory `dir`.
fn on(dir: &std::path::Path, who: &str) -> Peer {
    Peer::open(config(), dev(who).unwrap(), Box::new(ark_peer::Files::new(dir).unwrap()))
}

/// Whether the device on `dir` marks `f` as made there and not yet sent.
fn unsent(dir: &std::path::Path, f: &FileRef) -> bool {
    dir.join("files").join(format!("{}.unsent", f.hash)).is_file()
}

/// Frames carried both ways until nobody has more; `drop` says which of a
/// device's frames are lost on the way.
fn carry(hub: &mut Authority, ds: &mut [(ConnId, &mut Peer)], drop: &dyn Fn(&Frame) -> bool) {
    for _ in 0..1000 {
        let mut out = vec![];
        for (c, d) in ds.iter_mut() {
            for b in d.take_frames() {
                if !drop(&Frame::dec(&b).unwrap()) {
                    out.extend(hub.frame(*c, b, 1));
                }
            }
        }
        out.extend(hub.release());
        if out.is_empty() {
            return;
        }
        for (to, b) in out {
            if let Some((_, d)) = ds.iter_mut().find(|(c, _)| *c == to) {
                d.receive(b);
            }
        }
    }
    panic!("the frames never stopped");
}

use ark::frames::Frame;

/// A file's bytes when `p` holds it whole, read whole.
fn read(file: Result<Option<Box<dyn ark_peer::files::Bytes>>, String>) -> Result<Option<Vec<u8>>, String> {
    let mut out = vec![];
    match file? {
        Some(mut b) => b.read_to_end(&mut out).map(|_| Some(out)).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

fn none(_: &Frame) -> bool {
    false
}

fn bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 251) as u8).collect()
}

/// Falsified by an upload waiting for a connection that never asks for
/// it, or by a file put in the log: the hub would not hold it, or the log
/// would carry its bytes.
#[test]
fn a_file_made_offline_goes_up_on_the_next_connection_and_down_to_another_device() {
    let mut hub = Authority::serve(server(false), Box::new(Memory::default()), 0).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = on(dir.path(), "alice@phone");
    let mut bob = device("bob@laptop");
    let photo = bytes(2 * ark_peer::files::CHUNK + 1234);

    // Offline: kept, named, pending.
    let f = alice.put_file(&photo[..], "image/jpeg").unwrap();
    assert_eq!(read(alice.file(&f)).unwrap().as_deref(), Some(&photo[..]), "there at once, offline");
    alice
        .mutate(
            "photos.add",
            &Add {
                id: "p1".into(),
                file: f.clone(),
            },
        )
        .unwrap();
    assert!(unsent(dir.path(), &f));

    let (a, b) = (hub.accept(alice.me().clone()), hub.accept(bob.me().clone()));
    alice.connected();
    bob.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert!(!unsent(dir.path(), &f), "sent");
    assert_eq!(read(hub.file(&f)).unwrap().as_deref(), Some(&photo[..]), "the server holds it whole");
    let log = format!("{:?}", hub.log.log().entries());
    assert!(log.len() < photo.len(), "the bytes are not in the log");

    assert_eq!(read(bob.file(&f)), Ok(None), "not here until it is fetched");
    assert!(!bob.pump().shown, "nothing to ring yet");
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert!(bob.pump().shown, "the views rung");
    assert_eq!(read(bob.file(&f)).unwrap().as_deref(), Some(&photo[..]));
}

/// Falsified by an upload that starts again from nothing after its
/// connection is cut, or one that is taken as done though cut short.
#[test]
fn an_upload_cut_off_goes_on_from_where_it_stopped() {
    let mut hub = Authority::serve(server(false), Box::new(Memory::default()), 0).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = on(dir.path(), "alice@phone");
    let photo = bytes(3 * ark_peer::files::CHUNK);
    let f = alice.put_file(&photo[..], "image/jpeg").unwrap();

    let a = hub.accept(alice.me().clone());
    alice.connected();
    // The connection drops after the first chunk arrived.
    let mut chunks = 0;
    let cut = std::cell::Cell::new(0);
    carry(&mut hub, &mut [(a, &mut alice)], &|fr| {
        let is = matches!(fr, Frame::Chunk { .. });
        if is {
            cut.set(cut.get() + 1);
        }
        is && cut.get() > 1
    });
    chunks += cut.get();
    assert!(chunks >= 2);
    assert_eq!(read(hub.file(&f)), Ok(None), "not whole");
    alice.disconnected();
    hub.leave(a, 2);

    let a = hub.accept(alice.me().clone());
    alice.connected();
    let resumed = std::cell::RefCell::new(vec![]);
    carry(&mut hub, &mut [(a, &mut alice)], &|fr| {
        if let Frame::Chunk { at, .. } = fr {
            resumed.borrow_mut().push(*at);
        }
        false
    });
    let at = resumed.borrow().clone();
    assert_eq!(at.first(), Some(&(ark_peer::files::CHUNK as u64)), "from the second chunk: {at:?}");
    assert_eq!(read(hub.file(&f)).unwrap().as_deref(), Some(&photo[..]));
    assert!(!unsent(dir.path(), &f));
}

/// Falsified by a download that starts again from nothing after its
/// connection is cut.
#[test]
fn a_download_cut_off_goes_on_from_where_it_stopped() {
    let mut hub = Authority::serve(server(false), Box::new(Memory::default()), 0).unwrap();
    let mut alice = device("alice@phone");
    let dir = tempfile::tempdir().unwrap();
    let mut bob = on(dir.path(), "bob@laptop");
    let photo = bytes(3 * ark_peer::files::CHUNK + 5);
    let f = alice.put_file(&photo[..], "image/jpeg").unwrap();
    alice
        .mutate(
            "photos.add",
            &Add {
                id: "p1".into(),
                file: f.clone(),
            },
        )
        .unwrap();
    let (a, b) = (hub.accept(alice.me().clone()), hub.accept(bob.me().clone()));
    alice.connected();
    bob.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);

    // Only the first ask reaches the server before the connection drops.
    assert_eq!(read(bob.file(&f)), Ok(None));
    let asked = Cell::new(0);
    carry(&mut hub, &mut [(b, &mut bob)], &|fr| {
        asked.set(asked.get() + matches!(fr, Frame::Want { .. }) as usize);
        asked.get() > 1
    });
    assert!(asked.get() > 2, "the rest asked at once: {}", asked.get());
    bob.disconnected();
    hub.leave(b, 2);

    let b = hub.accept(bob.me().clone());
    bob.connected();
    let resumed = std::cell::RefCell::new(vec![]);
    carry(&mut hub, &mut [(b, &mut bob)], &|fr| {
        if let Frame::Want { at, .. } = fr {
            resumed.borrow_mut().push(*at);
        }
        false
    });
    assert_eq!(resumed.borrow().first(), Some(&(ark_peer::files::CHUNK as u64)), "from the second chunk");
    assert_eq!(read(bob.file(&f)).unwrap().as_deref(), Some(&photo[..]));
}

/// Falsified by a server that gives any file to anyone who names it.
#[test]
fn a_scoped_connection_gets_only_the_files_its_rows_name() {
    let mut hub = Authority::serve(server(true), Box::new(Memory::default()), 0).unwrap();
    let mut alice = device("alice@phone");
    let mut bob = device("bob@laptop");
    let f = alice.put_file(&b"alice's receipt"[..], "image/jpeg").unwrap();
    alice
        .mutate(
            "photos.add",
            &Add {
                id: "p1".into(),
                file: f.clone(),
            },
        )
        .unwrap();
    let (a, b) = (hub.accept(alice.me().clone()), hub.accept(bob.me().clone()));
    alice.connected();
    bob.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert!(read(hub.file(&f)).unwrap().is_some());

    assert_eq!(read(bob.file(&f)), Ok(None));
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert_eq!(read(bob.file(&f)), Err("no row this connection reads names it".into()), "not bob's");

    // Alice's other device reads her rows, so it gets her file.
    let mut tablet = device("alice@tablet");
    let t = hub.accept(tablet.me().clone());
    tablet.connected();
    carry(&mut hub, &mut [(t, &mut tablet)], &none);
    assert_eq!(read(tablet.file(&f)), Ok(None));
    carry(&mut hub, &mut [(t, &mut tablet)], &none);
    assert_eq!(read(tablet.file(&f)).unwrap().as_deref(), Some(&b"alice's receipt"[..]));
}

/// Falsified by a server that forgets who wanted a file before it arrived:
/// the other device would wait for ever.
#[test]
fn a_file_wanted_before_it_arrived_is_sent_when_it_does() {
    let mut hub = Authority::serve(server(false), Box::new(Memory::default()), 0).unwrap();
    let mut alice = device("alice@phone");
    let mut bob = device("bob@laptop");
    let (a, b) = (hub.accept(alice.me().clone()), hub.accept(bob.me().clone()));
    alice.connected();
    bob.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);

    let f = alice.put_file(&b"art"[..], "image/png").unwrap();
    alice
        .mutate(
            "photos.add",
            &Add {
                id: "p1".into(),
                file: f.clone(),
            },
        )
        .unwrap();
    // The row goes; the upload is held up.
    let upload = |fr: &Frame| matches!(fr, Frame::Offer { .. } | Frame::Chunk { .. });
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &upload);
    assert_eq!(read(bob.file(&f)), Ok(None));
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &upload);
    assert_eq!(read(bob.file(&f)), Err("not here yet".into()));

    alice.disconnected();
    hub.leave(a, 2);
    let a = hub.accept(alice.me().clone());
    alice.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert_eq!(read(bob.file(&f)).unwrap().as_deref(), Some(&b"art"[..]), "sent when it arrived");
}

/// Falsified by a server that takes bytes under any name.
#[test]
fn bytes_that_are_not_their_name_are_not_kept() {
    let mut hub = Authority::serve(server(false), Box::new(Memory::default()), 0).unwrap();
    let alice = device("alice@phone");
    let a = hub.accept(alice.me().clone());
    let hash = ark::file::hash(b"the real one");
    let _ = hub.frame(a, Frame::Offer { hash: hash.clone(), size: 4 }.enc(), 1);
    let out = hub.frame(
        a,
        Frame::Chunk {
            hash: hash.clone(),
            at: 0,
            size: 4,
            bytes: b"fake".to_vec(),
        }
        .enc(),
        1,
    );
    let said: Vec<Frame> = out.iter().map(|(_, b)| Frame::dec(b).unwrap()).collect();
    assert!(matches!(&said[..], [Frame::Absent { .. }]), "{said:?}");
    let f = FileRef {
        hash,
        size: 4,
        media: String::new(),
    };
    assert_eq!(read(hub.file(&f)), Ok(None));
}

/// Falsified by a device that forgets, when it opens again, what it made
/// and has not sent; or by a store that takes a path for a name.
#[test]
fn a_device_s_unsent_files_outlast_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let open = || Peer::open(config(), dev("alice@phone").unwrap(), Box::new(ark_peer::Files::new(dir.path()).unwrap()));
    let mut alice = open();
    let f = alice.put_file(&b"a playlist's art"[..], "image/png").unwrap();
    drop(alice);

    let mut alice = open();
    assert!(unsent(dir.path(), &f));
    assert_eq!(read(alice.file(&f)).unwrap().as_deref(), Some(&b"a playlist's art"[..]));
    assert!(dir.path().join("files").join(&f.hash).is_file(), "kept in the directory");

    let mut hub = Authority::serve(server(false), Box::new(Memory::default()), 0).unwrap();
    let a = hub.accept(alice.me().clone());
    alice.connected();
    carry(&mut hub, &mut [(a, &mut alice)], &none);
    assert!(!unsent(dir.path(), &f), "the mark is off on the disk");

    // A name that is a path is no file's: nothing is written beside `files/`.
    let mut hub = Authority::serve(server(false), Box::new(ark_peer::Files::new(dir.path().join("x")).unwrap()), 0).unwrap();
    let c = hub.accept(alice.me().clone());
    let bytes = b"no".to_vec();
    let chunk = ark::frames::Frame::Chunk {
        hash: "../escape".into(),
        at: 0,
        size: 2,
        bytes,
    };
    hub.frame(c, chunk.enc(), 0);
    assert!(!dir.path().join("x").join("escape.part").exists(), "a path is no file's name");
}

// The most bytes live at once on this thread ------------------------------------------

struct Peak;

thread_local!(static LIVE: Cell<(isize, isize)> = const { Cell::new((0, 0)) });

fn tally(bytes: isize) {
    let _ = LIVE.try_with(|c| {
        let (live, peak) = c.get();
        c.set((live + bytes, peak.max(live + bytes)));
    });
}

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        tally(l.size() as isize);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        tally(-(l.size() as isize));
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        tally(n as isize - l.size() as isize);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Peak = Peak;

/// What `f` returns, and the most bytes it held live at once on this thread.
fn peak<T>(f: impl FnOnce() -> T) -> (T, isize) {
    let (live, _) = LIVE.with(Cell::get);
    LIVE.with(|c| c.set((live, live)));
    let t = f();
    (t, LIVE.with(Cell::get).1 - live)
}

/// `n` bytes of [`bytes`]' pattern, made as they are read.
struct Made(u64, u64);

impl Read for Made {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min((self.1 - self.0) as usize);
        for (i, b) in buf[..n].iter_mut().enumerate() {
            *b = ((self.0 as usize + i) * 7 % 251) as u8;
        }
        self.0 += n as u64;
        Ok(n)
    }
}

/// Falsified by a put, a transfer or a read that holds a file whole, or by
/// a store in memory: 256 MiB, 32 windows, goes up from a directory to a
/// server's and down to another, never more than a few windows of it live.
#[test]
fn a_file_many_windows_long_goes_up_and_down_never_held_whole() {
    let size = 256u64 << 20;
    let dir = tempfile::tempdir().unwrap();
    let disk = Box::new(ark_peer::Files::new(dir.path().join("server")).unwrap());
    let mut hub = Authority::serve(server(false), disk, 0).unwrap();
    let mut alice = on(&dir.path().join("alice"), "alice@phone");
    let mut bob = on(&dir.path().join("bob"), "bob@laptop");
    let (a, b) = (hub.accept(alice.me().clone()), hub.accept(bob.me().clone()));
    alice.connected();
    bob.connected();
    let ((f, said), held) = peak(|| {
        let f = alice.put_file(Made(0, size), "video/mp4").unwrap();
        alice
            .mutate(
                "photos.add",
                &Add {
                    id: "v".into(),
                    file: f.clone(),
                },
            )
            .unwrap();
        carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
        assert!(read(bob.file(&f)).unwrap().is_none());
        carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
        let mut whole = bob.file(&f).unwrap().expect("bob holds it whole");
        let said = ark::file::hash_reader(&mut whole).unwrap();
        (f, said)
    });
    assert_eq!((said, f.size as u64), (f.hash.clone(), size));
    let windows = 3 * ark_peer::files::WINDOW as isize * ark_peer::files::CHUNK as isize;
    assert!(held < windows, "{} MiB held at once of {} MiB", held >> 20, size >> 20);
}

/// Falsified by a server that takes a file of any size, or keeps the part
/// of one it refused.
#[test]
fn a_file_larger_than_the_server_takes_is_refused() {
    let mut c = server(false);
    c.max_file = 3 * ark_peer::files::CHUNK as u64;
    let mut hub = Authority::serve(c, Box::new(Memory::default()), 0).unwrap();
    let mut alice = device("alice@phone");
    let a = hub.accept(alice.me().clone());
    alice.connected();
    let ok = alice.put_file(&bytes(3 * ark_peer::files::CHUNK)[..], "image/png").unwrap();
    let big = alice.put_file(&bytes(3 * ark_peer::files::CHUNK + 1)[..], "video/mp4").unwrap();
    carry(&mut hub, &mut [(a, &mut alice)], &none);
    assert!(read(hub.file(&ok)).unwrap().is_some(), "as large as it may be");
    assert_eq!(read(hub.file(&big)), Ok(None));

    // Asked for a window ahead; refused when it says it is larger.
    let said = |out: Vec<(ConnId, Vec<u8>)>| out.iter().map(|(_, b)| Frame::dec(b).unwrap()).collect::<Vec<_>>();
    let offer = |hash: &str, size| Frame::Offer { hash: hash.into(), size }.enc();
    let other = "c".repeat(64);
    let asks = said(hub.frame(a, offer(&other, 3 * ark_peer::files::CHUNK as u64), 1));
    assert_eq!(asks.len(), 3, "every chunk asked at once: {asks:?}");
    let refused = said(hub.frame(a, offer(&big.hash, big.size as u64), 1));
    assert!(
        matches!(&refused[..], [Frame::Absent { why, .. }] if why.contains("larger than")),
        "{refused:?}"
    );
    let chunk = Frame::Chunk {
        hash: other.clone(),
        at: 0,
        size: big.size as u64,
        bytes: bytes(10),
    };
    assert!(matches!(&said(hub.frame(a, chunk.enc(), 1))[..], [Frame::Absent { .. }]));
    assert_eq!(read(hub.file(&FileRef { hash: other, ..big })), Ok(None), "nothing of it kept");
}

/// Falsified by a part of a file kept for ever: an upload given up, a
/// download let go, a put cut off.
#[test]
fn a_part_left_long_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let files = dir.path().join("files");
    std::fs::create_dir_all(&files).unwrap();
    let (old, new) = (
        files.join(format!("{}.part", "a".repeat(64))),
        files.join(format!("{}.part", "b".repeat(64))),
    );
    std::fs::write(&old, b"given up").unwrap();
    std::fs::write(&new, b"going").unwrap();
    let long_ago = std::time::SystemTime::now() - ark_peer::files::STALE - std::time::Duration::from_secs(60);
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(long_ago).unwrap();
    let _alice = on(dir.path(), "alice@phone");
    assert!(!old.exists(), "left long, dropped");
    assert!(new.exists(), "still going, kept");
}

/// A device dialling `hub` in process from the directory `dir`, and its bell.
fn dialled(dir: &std::path::Path, who: &str, hub: &ark_peer::Handle<Authority>) -> (Peer, std::sync::mpsc::Receiver<()>) {
    let mut c = on(dir, who);
    let (wake, rx) = ark_peer::link::bell();
    c.set_waker(wake);
    c.set_token(Some(who.into()));
    c.connect_with("", hub.dialer(Arc::new(dev)));
    (c, rx)
}

/// Pumps `c` until `f` holds.
fn pumped(c: &mut (Peer, std::sync::mpsc::Receiver<()>), f: impl Fn(&Peer) -> bool) {
    while !f(&c.0) {
        c.0.pump();
        ark_peer::link::wait(&c.0, &c.1, std::time::Duration::from_millis(5));
    }
}

/// How fast a large file goes up to a server in process and down to
/// another device: `cargo test --release -p ark-peer --test files throughput -- --ignored --nocapture`.
#[test]
#[ignore]
fn throughput() {
    let mb = 256;
    let dir = tempfile::tempdir().unwrap();
    let mut s = ark_peer::Server::new(server(false));
    s.data = Some(dir.path().join("server"));
    let hub = s.build().unwrap().0;
    let mut alice = dialled(&dir.path().join("alice"), "alice@phone", &hub);
    pumped(&mut alice, |p| p.linked);
    let photo = bytes(mb << 20);
    let t = std::time::Instant::now();
    let f = alice.0.put_file(&photo[..], "video/mp4").unwrap();
    let put = t.elapsed();
    let add = Add {
        id: "v".into(),
        file: f.clone(),
    };
    alice.0.mutate("photos.add", &add).unwrap();
    let t = std::time::Instant::now();
    let a = dir.path().join("alice");
    pumped(&mut alice, |_| !unsent(&a, &f));
    let up = t.elapsed();
    let mut bob = dialled(&dir.path().join("bob"), "bob@laptop", &hub);
    pumped(&mut bob, |p| p.linked && p.log.store.rows("photo").count() == 1);
    let t = std::time::Instant::now();
    let whole = dir.path().join("bob").join("files").join(&f.hash);
    let _ = bob.0.file(&f);
    pumped(&mut bob, |_| whole.is_file());
    let down = t.elapsed();
    let rate = |d: std::time::Duration| mb as f64 / d.as_secs_f64();
    eprintln!(
        "files: {mb} MiB put {:.0} MiB/s, up {:.0} MiB/s, down {:.0} MiB/s",
        rate(put),
        rate(up),
        rate(down)
    );
}
