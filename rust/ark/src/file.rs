//! Files (docs/plan-files.md): a row names a file by the sha256 of its
//! bytes, and the bytes go beside the log, never in it — not an entry, a
//! fact, a snapshot or a recording. [`FileRef`] is an ordinary record
//! column, so every layer that carries a record carries it; what reads the
//! bytes is the peer's (`ark_peer::files`), and a mutator only ever sees
//! the reference.

use serde::{Deserialize, Serialize};

use crate::row::Type;
use crate::store::Store;
use serde_json::Value as Json;

/// The name a file's record has in a schema: what [`named`] looks for.
pub const RECORD: &str = "FileRef";

/// A file a row names: the hex sha256 of its bytes, how many there are,
/// and their media type (`image/jpeg`).
#[derive(crate::prelude::Describe, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FileRef {
    pub hash: String,
    pub size: i64,
    pub media: String,
}

impl FileRef {
    /// The reference to `bytes`.
    pub fn of(bytes: &[u8], media: &str) -> FileRef {
        FileRef {
            hash: hash(bytes),
            size: bytes.len() as i64,
            media: media.into(),
        }
    }
}

/// A file's name: the hex sha256 of its bytes.
pub fn hash(bytes: &[u8]) -> String {
    crate::hash::hex(&crate::hash::sha256(&[bytes]))
}

/// Whether `name` is a file's name: sixty-four hex digits, lower case.
pub fn is_hash(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether a value of `t` can hold a file.
pub fn mentions(t: &Type) -> bool {
    match t {
        Type::Record(n, _) if n == RECORD => true,
        Type::Option(t) | Type::List(t) => mentions(t),
        Type::Tuple(ts) => ts.iter().any(mentions),
        Type::Record(_, fs) => fs.iter().any(|(_, t)| mentions(t)),
        Type::Sum(_, vs) => vs.iter().any(|(_, fs)| fs.iter().any(|(_, t)| mentions(t))),
        _ => false,
    }
}

/// The files a value of `t`, as JSON, names.
pub fn named(t: &Type, v: &Json, out: &mut Vec<String>) {
    let fields = |fs: &[(String, Type)], v: &Json, out: &mut Vec<String>| match v {
        Json::Object(m) => fs.iter().for_each(|(f, t)| m.get(f).iter().for_each(|v| named(t, v, out))),
        Json::Array(xs) if fs.len() == xs.len() => fs.iter().zip(xs).for_each(|((_, t), v)| named(t, v, out)),
        v if fs.len() == 1 => named(&fs[0].1, v, out),
        _ => {}
    };
    match (t, v) {
        (Type::Record(n, _), Json::Object(m)) if n == RECORD => {
            if let Some(Json::String(h)) = m.get("hash") {
                out.push(h.clone());
            }
        }
        (Type::Option(_), Json::Null) => {}
        (Type::Option(t), v) => named(t, v, out),
        (Type::List(t), Json::Array(xs)) => xs.iter().for_each(|v| named(t, v, out)),
        (Type::Tuple(ts), Json::Array(xs)) => ts.iter().zip(xs).for_each(|(t, v)| named(t, v, out)),
        (Type::Record(_, fs), v) => fields(fs, v, out),
        (Type::Sum(_, vs), Json::Object(m)) if m.len() == 1 => {
            let (name, p) = m.iter().next().unwrap();
            if let Some((_, fs)) = vs.iter().find(|(x, _)| x == name) {
                fields(fs, p, out);
            }
        }
        _ => {}
    }
}

/// Whether a row of `store` names the file `hash`: who may read the store
/// may read the file.
pub fn held(store: &Store, hash: &str) -> bool {
    let schema = store.schema_arc().clone();
    schema.tables.iter().filter(|t| mentions(&t.row_type())).any(|t| {
        let ty = t.row_type();
        store.rows(&t.name).any(|r| {
            let mut out = vec![];
            if let Ok(v) = crate::dynamic::row(t, r) {
                named(&ty, &v, &mut out);
            }
            out.iter().any(|h| h == hash)
        })
    })
}

/// A file's name, read from what holds its bytes.
pub fn hash_reader(mut r: impl std::io::Read) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let (mut h, mut buf) = (Sha256::new(), vec![0; 1 << 16]);
    loop {
        match r.read(&mut buf)? {
            0 => return Ok(crate::hash::hex(&h.finalize())),
            n => h.update(&buf[..n]),
        }
    }
}
