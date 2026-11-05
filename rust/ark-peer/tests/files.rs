//! Files beside the log (docs/plan-files.md): a file made offline goes up
//! on the next connection, in chunks, from where a cut-off one stopped;
//! another device fetches it once it opens it; a connection fed by a scope
//! gets only the files its rows name; one wanted before it arrived is sent
//! when it does; and nothing of the bytes is in the log.

use std::sync::Arc;

use ark::file::FileRef;
use ark::prelude::*;
use ark_peer::{dev, Config, ConnId, Memory, Peer};

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
    ark::register!(Domain::new::<Photos>("photos", "1"); add, mine)
}

fn config(scoped: bool) -> Config {
    let mut c = Config::of(Arc::new(domain()));
    if scoped {
        c.feed = Arc::new(|_| Some("photos.mine".into()));
    }
    c
}

fn device(scoped: bool, who: &str) -> Peer {
    Peer::open(config(scoped), dev(who).unwrap(), Box::new(Memory::default()))
}

/// A device on the directory `dir`.
fn on(dir: &std::path::Path, who: &str) -> Peer {
    Peer::open(config(false), dev(who).unwrap(), Box::new(ark_peer::Files::new(dir).unwrap()))
}

/// Whether the device on `dir` marks `f` as made there and not yet sent.
fn unsent(dir: &std::path::Path, f: &FileRef) -> bool {
    dir.join("files").join(format!("{}.unsent", f.hash)).is_file()
}

/// Frames carried both ways until nobody has more; `drop` says which of a
/// device's frames are lost on the way.
fn carry(hub: &mut Peer, ds: &mut [(ConnId, &mut Peer)], drop: &dyn Fn(&Frame) -> bool) {
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
    let mut hub = Peer::serve(config(false), Box::new(Memory::default()), 0);
    let dir = tempfile::tempdir().unwrap();
    let mut alice = on(dir.path(), "alice@phone");
    let mut bob = device(false, "bob@laptop");
    let photo = bytes(2 * ark_peer::files::CHUNK + 1234);

    // Offline: kept, named, pending.
    let f = alice.put_file(&photo, "image/jpeg").unwrap();
    assert_eq!(alice.file(&f).unwrap().as_deref(), Some(&photo[..]), "there at once, offline");
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
    assert_eq!(hub.file(&f).unwrap().as_deref(), Some(&photo[..]), "the server holds it whole");
    let log = format!("{:?}", hub.log.log().entries());
    assert!(log.len() < photo.len(), "the bytes are not in the log");

    assert_eq!(bob.file(&f), Ok(None), "not here until it is fetched");
    assert!(!bob.pump().shown, "nothing to ring yet");
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert!(bob.pump().shown, "the views rung");
    assert_eq!(bob.file(&f).unwrap().as_deref(), Some(&photo[..]));
}

/// Falsified by an upload that starts again from nothing after its
/// connection is cut, or one that is taken as done though cut short.
#[test]
fn an_upload_cut_off_goes_on_from_where_it_stopped() {
    let mut hub = Peer::serve(config(false), Box::new(Memory::default()), 0);
    let dir = tempfile::tempdir().unwrap();
    let mut alice = on(dir.path(), "alice@phone");
    let photo = bytes(3 * ark_peer::files::CHUNK);
    let f = alice.put_file(&photo, "image/jpeg").unwrap();

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
    assert_eq!(hub.file(&f), Ok(None), "not whole");
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
    assert_eq!(hub.file(&f).unwrap().as_deref(), Some(&photo[..]));
    assert!(!unsent(dir.path(), &f));
}

/// Falsified by a server that gives any file to anyone who names it.
#[test]
fn a_scoped_connection_gets_only_the_files_its_rows_name() {
    let mut hub = Peer::serve(config(true), Box::new(Memory::default()), 0);
    let mut alice = device(true, "alice@phone");
    let mut bob = device(true, "bob@laptop");
    let f = alice.put_file(b"alice's receipt", "image/jpeg").unwrap();
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
    assert!(hub.file(&f).unwrap().is_some());

    assert_eq!(bob.file(&f), Ok(None));
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert_eq!(bob.file(&f), Err("no row this connection reads names it".into()), "not bob's");

    // Alice's other device reads her rows, so it gets her file.
    let mut tablet = device(true, "alice@tablet");
    let t = hub.accept(tablet.me().clone());
    tablet.connected();
    carry(&mut hub, &mut [(t, &mut tablet)], &none);
    assert_eq!(tablet.file(&f), Ok(None));
    carry(&mut hub, &mut [(t, &mut tablet)], &none);
    assert_eq!(tablet.file(&f).unwrap().as_deref(), Some(&b"alice's receipt"[..]));
}

/// Falsified by a server that forgets who wanted a file before it arrived:
/// the other device would wait for ever.
#[test]
fn a_file_wanted_before_it_arrived_is_sent_when_it_does() {
    let mut hub = Peer::serve(config(false), Box::new(Memory::default()), 0);
    let mut alice = device(false, "alice@phone");
    let mut bob = device(false, "bob@laptop");
    let (a, b) = (hub.accept(alice.me().clone()), hub.accept(bob.me().clone()));
    alice.connected();
    bob.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);

    let f = alice.put_file(b"art", "image/png").unwrap();
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
    assert_eq!(bob.file(&f), Ok(None));
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &upload);
    assert_eq!(bob.file(&f), Err("not here yet".into()));

    alice.disconnected();
    hub.leave(a, 2);
    let a = hub.accept(alice.me().clone());
    alice.connected();
    carry(&mut hub, &mut [(a, &mut alice), (b, &mut bob)], &none);
    assert_eq!(bob.file(&f).unwrap().as_deref(), Some(&b"art"[..]), "sent when it arrived");
}

/// Falsified by a server that takes bytes under any name.
#[test]
fn bytes_that_are_not_their_name_are_not_kept() {
    let mut hub = Peer::serve(config(false), Box::new(Memory::default()), 0);
    let alice = device(false, "alice@phone");
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
    assert_eq!(hub.file(&f), Ok(None));
}

/// Falsified by a device that forgets, when it opens again, what it made
/// and has not sent; or by a store that takes a path for a name.
#[test]
fn a_device_s_unsent_files_outlast_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let open = || {
        Peer::open(
            config(false),
            dev("alice@phone").unwrap(),
            Box::new(ark_peer::Files::new(dir.path()).unwrap()),
        )
    };
    let mut alice = open();
    let f = alice.put_file(b"a playlist's art", "image/png").unwrap();
    drop(alice);

    let mut alice = open();
    assert!(unsent(dir.path(), &f));
    assert_eq!(alice.file(&f).unwrap().as_deref(), Some(&b"a playlist's art"[..]));
    assert!(dir.path().join("files").join(&f.hash).is_file(), "kept in the directory");

    let mut hub = Peer::serve(config(false), Box::new(Memory::default()), 0);
    let a = hub.accept(alice.me().clone());
    alice.connected();
    carry(&mut hub, &mut [(a, &mut alice)], &none);
    assert!(!unsent(dir.path(), &f), "the mark is off on the disk");

    // A name that is a path is no file's: nothing is written beside `files/`.
    let mut hub = Peer::serve(config(false), Box::new(ark_peer::Files::new(dir.path().join("x")).unwrap()), 0);
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
