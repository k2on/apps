//! A page's disk: its blobs in the browser's IndexedDB, held in memory as
//! well ([`Browser`]).
//!
//! A [`Disk`] is asked synchronously, and IndexedDB answers only later; so
//! every blob is read into memory once, before anything opens ([`open`]),
//! a read is of memory, and a write is made in memory and sent on to the
//! database behind it, in order — a page's transactions on one store run
//! in the order they were made. A blob appended to is kept as its parts,
//! one record each (`[name, n]`), so an append writes its own bytes and no
//! more; a put replaces them all with one.
//!
//! It used to be the page's `localStorage`, as hex: a few megabytes at most,
//! a blob read, decoded, encoded and written whole for every append — a
//! fill's every part — and refused once the library outgrew it, and tried
//! again at every pump. A blob still there is taken from it the first time
//! it is asked for, and taken out.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;

use js_sys::{Array, Promise, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbKeyRange, IdbObjectStore, IdbRequest, IdbTransactionMode};

use crate::Disk;

const DATABASE: &str = "ark";
const STORE: &str = "blobs";

thread_local! {
    /// Every blob, by its whole name, and the number its next part takes.
    static HELD: RefCell<BTreeMap<String, (Vec<u8>, u32)>> = const { RefCell::new(BTreeMap::new()) };
    /// The database, once [`open`]ed; or why it would not.
    static DB: RefCell<Option<Result<IdbDatabase, String>>> = const { RefCell::new(None) };
}

/// The blobs under one prefix of the page's database.
pub struct Browser(pub String);

/// The page's database opened and every blob in it read: what a [`Browser`]
/// needs before it is asked anything.
pub async fn open() -> Result<(), String> {
    let opened = read().await;
    let said = opened.as_ref().map(|_| ()).map_err(|e| e.clone());
    if let Ok((_, blobs)) = &opened {
        HELD.with(|h| *h.borrow_mut() = blobs.clone());
    }
    DB.with(|d| *d.borrow_mut() = Some(opened.map(|(db, _)| db)));
    said
}

async fn read() -> Result<(IdbDatabase, BTreeMap<String, (Vec<u8>, u32)>), String> {
    let factory = web_sys::window().and_then(|w| w.indexed_db().ok().flatten()).ok_or("no IndexedDB")?;
    let req = factory.open_with_u32(DATABASE, 1).map_err(js)?;
    let upgrade = Closure::once_into_js({
        let req = req.clone();
        move || {
            if let Some(db) = req.result().ok().and_then(|r| r.dyn_into::<IdbDatabase>().ok()) {
                let _ = db.create_object_store(STORE);
            }
        }
    });
    req.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
    let db: IdbDatabase = done(&req).await?.dyn_into().map_err(js)?;
    let store = db.transaction_with_str(STORE).and_then(|t| t.object_store(STORE)).map_err(js)?;
    let keys = store.get_all_keys().map_err(js)?;
    let values = store.get_all().map_err(js)?;
    let (keys, values): (Array, Array) = (done(&keys).await?.into(), done(&values).await?.into());
    // In key order: a blob's parts one after another, in the order they
    // were written.
    let mut blobs: BTreeMap<String, (Vec<u8>, u32)> = BTreeMap::new();
    for (k, v) in keys.iter().zip(values.iter()) {
        let k: Array = k.into();
        let (Some(name), Some(n)) = (k.get(0).as_string(), k.get(1).as_f64()) else {
            continue;
        };
        let held = blobs.entry(name).or_default();
        held.0.extend(Uint8Array::new(&v).to_vec());
        held.1 = n as u32 + 1;
    }
    Ok((db, blobs))
}

/// A request's result, once it has one.
async fn done(req: &IdbRequest) -> Result<JsValue, String> {
    let promise = Promise::new(&mut |resolve, reject| {
        let ok = Closure::once_into_js({
            let req = req.clone();
            move || _ = resolve.call1(&JsValue::NULL, &req.result().unwrap_or(JsValue::UNDEFINED))
        });
        let failed = Closure::once_into_js({
            let req = req.clone();
            move || {
                let why = req.error().ok().flatten().map_or(JsValue::UNDEFINED, JsValue::from);
                _ = reject.call1(&JsValue::NULL, &why);
            }
        });
        req.set_onsuccess(Some(ok.unchecked_ref()));
        req.set_onerror(Some(failed.unchecked_ref()));
    });
    JsFuture::from(promise).await.map_err(js)
}

fn js(e: impl Into<JsValue>) -> String {
    let e = e.into();
    e.as_string()
        .or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| e.message().into()))
        .unwrap_or_else(|| format!("{e:?}"))
}

/// A part's key: its blob's name and its number.
fn key(name: &str, n: f64) -> JsValue {
    Array::of2(&JsValue::from_str(name), &JsValue::from_f64(n)).into()
}

/// `f` on the store, in a transaction of its own, made now and run after
/// every one made before it; a write that fails is said, and the blob in
/// memory stands.
fn write(f: impl FnOnce(&IdbObjectStore) -> Result<(), JsValue>) -> io::Result<()> {
    DB.with(|d| {
        let d = d.borrow();
        let db = match d.as_ref() {
            Some(Ok(db)) => db,
            Some(Err(e)) => return Err(io::Error::other(e.clone())),
            None => return Err(io::Error::other("the page's database was not opened")),
        };
        let tx = db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite)
            .map_err(|e| io::Error::other(js(e)))?;
        let failed = Closure::once_into_js(|| tracing::warn!(target: "ark", "a write to the page's database failed"));
        tx.set_onerror(Some(failed.unchecked_ref()));
        tx.object_store(STORE).and_then(|s| f(&s)).map_err(|e| io::Error::other(js(e)))
    })
}

/// `bytes` as the whole of blob `name`, in the database.
fn replace(name: &str, bytes: &[u8]) -> io::Result<()> {
    write(|s| {
        let all = IdbKeyRange::bound(&key(name, 0.0), &key(name, f64::INFINITY))?;
        s.delete(&all)?;
        s.put_with_key(&Uint8Array::from(bytes), &key(name, 0.0)).map(drop)
    })
}

impl Browser {
    fn name(&self, name: &str) -> String {
        format!("{}:{name}", self.0)
    }

    /// The blob as the page's `localStorage` kept it, moved here.
    fn from_local(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let name = self.name(name);
        let Some(s) = ark_auth::web::storage() else { return Ok(None) };
        let Some(bytes) = s.get_item(&name).ok().flatten().and_then(|h| ark::hash::unhex(&h)) else {
            return Ok(None);
        };
        replace(&name, &bytes)?;
        HELD.with(|h| h.borrow_mut().insert(name.clone(), (bytes.clone(), 1)));
        let _ = s.remove_item(&name);
        Ok(Some(bytes))
    }
}

impl Disk for Browser {
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match DB.with(|d| d.borrow().as_ref().map(|r| r.as_ref().err().cloned())) {
            None => return Err(io::Error::other("the page's database was not opened")),
            Some(Some(e)) => return Err(io::Error::other(e)),
            Some(None) => {}
        }
        match HELD.with(|h| h.borrow().get(&self.name(name)).map(|(b, _)| b.clone())) {
            Some(b) => Ok(Some(b)),
            None => self.from_local(name),
        }
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let name = self.name(name);
        replace(&name, bytes)?;
        HELD.with(|h| h.borrow_mut().insert(name, (bytes.to_vec(), 1)));
        Ok(())
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let name = self.name(name);
        let n = HELD.with(|h| h.borrow().get(&name).map_or(0, |(_, n)| *n));
        write(|s| s.put_with_key(&Uint8Array::from(bytes), &key(&name, n as f64)).map(drop))?;
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            let held = h.entry(name).or_default();
            held.0.extend_from_slice(bytes);
            held.1 = n + 1;
        });
        Ok(())
    }
}
