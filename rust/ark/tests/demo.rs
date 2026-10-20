//! The demo of `spec/AUTHORING.md` Appendix B, its mutators written in the
//! IR (§1.4) by the small builder below and installed in the content store
//! through the interpreter. They replaced K1's hand-written `Procedure`s,
//! and the vectors those pinned did not move.
//!
//! `playlist(id: Id<playlist>, name: Text, user_id: Text)` keyed by `id`,
//! unique `(user_id, name)`; `item(playlist_id: Id<playlist> → playlist,
//! track_id: Text, pos: Int)` keyed by `(playlist_id, track_id)`, unique
//! `(playlist_id, pos)`. The types: `Id<playlist> = Bytes where len 16`,
//! `Name = Text where trim, non-empty "a playlist needs a name"`, `TrackId
//! = Text where len 1..`. The mutators: `create_playlist(Name)`, its id the
//! entry's first draw, nothing when the person has a playlist of that name
//! (v4's `on (user_id, name)`); `add_to_playlist(Id<playlist>, TrackId)`,
//! the playlist existing ("playlist_id: no such playlist"), nothing when
//! the track is on it already, at one past the playlist's last `pos`.
//! Beside them, for the rebase's other change kinds, `rename_playlist(id,
//! Name)` (an `Edit`) and `remove_from_playlist(id, Text)` (a `Remove`);
//! and dave's wrong build of `add_to_playlist`, which steps by two.
//!
//! Included by the other tests with `#[path]`; it has no tests of its own.

#![allow(dead_code)]

use std::sync::Arc;

use ark::codec::{bytes_enc, int_enc, put_bytes, put_int, Reader};
use ark::hash::FnHash;
use ark::ir::{CtxField, Decl, Effect, Expr, Function, Node, Op, Refine, Rule, Std};
use ark::module::{Lib, Module, SPEC};
use ark::peer::{Cas, Loader};
use ark::schema::{Schema, Table};
use ark::store::{Dir, Read};
use ark::ty::Ty;

pub fn schema() -> Schema {
    let id = || Ty::Bytes;
    Schema {
        tables: vec![
            Table::new("playlist")
                .col("id", "Id<playlist>", id())
                .col("name", "Text", Ty::Bytes)
                .col("user_id", "Text", Ty::Bytes)
                .index(&["user_id", "name"], true),
            Table::new("item")
                .col("playlist_id", "Id<playlist>", id())
                .col("track_id", "Text", Ty::Bytes)
                .col("pos", "Int", Ty::Int)
                .keyed(2)
                .index(&["playlist_id", "pos"], true)
                .refs("playlist_id", "playlist"),
        ],
    }
}

pub fn shared() -> Arc<Schema> {
    Arc::new(schema())
}

pub fn playlist_row(id: &[u8], name: &str, user: &str) -> Vec<u8> {
    let mut b = bytes_enc(id);
    put_bytes(&mut b, name.as_bytes());
    put_bytes(&mut b, user.as_bytes());
    b
}

pub fn item_row(pid: &[u8], track: &str, pos: i64) -> Vec<u8> {
    let mut b = bytes_enc(pid);
    put_bytes(&mut b, track.as_bytes());
    put_int(&mut b, pos);
    b
}

/// An `item` row read back: `(playlist_id, track_id, pos)`.
pub fn item_of(row: &[u8]) -> (Vec<u8>, String, i64) {
    let mut r = Reader::new(row);
    (r.bytes().unwrap(), r.text().unwrap(), r.int().unwrap())
}

/// The `pos` of a track on a playlist, as a store holds it.
pub fn pos_of(store: &dyn Read, pid: &[u8], track: &str) -> Option<i64> {
    let mut key = bytes_enc(pid);
    put_bytes(&mut key, track.as_bytes());
    store.get("item", &key).map(|r| item_of(&r).2)
}

pub fn create_input(name: &str) -> Vec<u8> {
    bytes_enc(name.as_bytes())
}

pub fn pair_input(id: &[u8], text: &str) -> Vec<u8> {
    let mut b = bytes_enc(id);
    put_bytes(&mut b, text.as_bytes());
    b
}

// * The builder: plain functions, names turned into De Bruijn indices as
//   the expression is built.

/// The names in scope, innermost last.
#[derive(Default)]
pub struct B(Vec<String>);

impl B {
    /// A bound name: its distance from the innermost binder.
    pub fn v(&self, n: &str) -> Expr {
        let i = self.0.iter().rev().position(|m| m == n).unwrap_or_else(|| panic!("{n} is not bound"));
        Expr::Var(i as u32)
    }

    /// `body` with `names` bound, as the binder that binds them builds it.
    pub fn with(&mut self, names: &[&str], body: impl FnOnce(&mut B) -> Expr) -> Expr {
        self.0.extend(names.iter().map(|n| n.to_string()));
        let e = body(self);
        self.0.truncate(self.0.len() - names.len());
        e
    }

    pub fn let_(&mut self, n: &str, e: Expr, body: impl FnOnce(&mut B) -> Expr) -> Expr {
        Expr::Let(Box::new(e), Box::new(self.with(&[n], body)))
    }

    pub fn lam(&mut self, names: &[&str], body: impl FnOnce(&mut B) -> Expr) -> Expr {
        Expr::Lambda(names.len() as u32, Box::new(self.with(names, body)))
    }
}

pub fn named(n: &str) -> Ty {
    Ty::Named(n.into())
}

pub fn text_ty() -> Ty {
    named("Text")
}

pub fn func(types: Vec<(String, Decl)>, params: &[(&str, Ty)], ret: Ty, effect: Effect, body: impl FnOnce(&mut B) -> Expr) -> Function {
    let names: Vec<&str> = params.iter().map(|p| p.0).collect();
    let body = B::default().with(&names, body);
    Function {
        types,
        params: params.iter().map(|p| p.1.clone()).collect(),
        ret,
        effect,
        body,
    }
}

pub fn int(n: i64) -> Expr {
    Expr::Lit(Ty::Int, int_enc(n).to_vec())
}

pub fn text(s: &str) -> Expr {
    Expr::Lit(text_ty(), bytes_enc(s.as_bytes()))
}

pub fn op(o: Op, a: Expr, b: Expr) -> Expr {
    Expr::Op(o, Box::new(a), Box::new(b))
}

pub fn field(e: Expr, i: u32) -> Expr {
    Expr::Field(Box::new(e), i)
}

pub fn std(f: Std, args: Vec<Expr>) -> Expr {
    Expr::Std(f, args)
}

pub fn if_(c: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If(Box::new(c), Box::new(a), Box::new(b))
}

pub fn nothing() -> Expr {
    Expr::Seq(vec![])
}

pub fn refuse(why: &str) -> Expr {
    Expr::Refuse(Box::new(text(why)))
}

pub fn ctx(f: CtxField) -> Expr {
    Expr::Ctx(f)
}

pub fn scan(t: &str) -> Expr {
    Expr::Scan(t.into())
}

pub fn lookup(t: &str, key: Vec<Expr>) -> Expr {
    Expr::Lookup(t.into(), Box::new(Expr::Tuple(key)))
}

pub fn filter(l: Expr, lam: Expr) -> Expr {
    Expr::Filter(Box::new(l), Box::new(lam))
}

pub fn insert(t: &str, row: Vec<Expr>) -> Expr {
    Expr::Insert(t.into(), Box::new(Expr::Tuple(row)))
}

pub fn decl(ty: Ty, rules: &[(Rule, Option<&str>)]) -> Decl {
    Decl {
        ty,
        refine: rules
            .iter()
            .map(|(rule, why)| Refine {
                rule: rule.clone(),
                why: why.map(Into::into),
            })
            .collect(),
    }
}

// * The demo

pub fn id_decl() -> (String, Decl) {
    ("Id<playlist>".into(), decl(Ty::Bytes, &[(Rule::Len(16), None)]))
}

pub fn name_decl() -> (String, Decl) {
    let rules = [(Rule::Trim, None), (Rule::NonEmpty, Some("a playlist needs a name"))];
    ("Name".into(), decl(text_ty(), &rules))
}

pub fn track_decl() -> (String, Decl) {
    ("TrackId".into(), decl(text_ty(), &[(Rule::LenRange(1, i64::MAX), None)]))
}

/// Every declaration the demo makes, sorted: the module's type section.
pub fn types() -> Vec<(String, Decl)> {
    vec![id_decl(), name_decl(), track_decl()]
}

fn mutator(types: Vec<(String, Decl)>, params: &[(&str, Ty)], body: impl FnOnce(&mut B) -> Expr) -> Function {
    func(types, params, Ty::unit(), Effect::Write, body)
}

pub fn create_playlist() -> Function {
    mutator(vec![id_decl(), name_decl()], &[("name", named("Name"))], |b| {
        let mine = b.lam(&["p"], |b| {
            let user = op(Op::Eq, field(b.v("p"), 2), ctx(CtxField::User));
            op(Op::And, user, op(Op::Eq, field(b.v("p"), 1), b.v("name")))
        });
        let row = vec![ctx(CtxField::NewId("playlist".into())), b.v("name"), ctx(CtxField::User)];
        if_(
            op(Op::Eq, std(Std::Len, vec![filter(scan("playlist"), mine)]), int(0)),
            insert("playlist", row),
            nothing(),
        )
    })
}

/// `add_to_playlist`, stepping `pos` by `step`: 1 is the demo's, 2 dave's.
/// `names` spells its bound variables, which its bytes do not depend on.
pub fn add_to_playlist_as(step: i64, names: [&str; 4]) -> Function {
    let [pid, track, last, r] = names;
    let params = [(pid, named("Id<playlist>")), (track, named("TrackId"))];
    mutator(vec![id_decl(), track_decl()], &params, |b| {
        let mine = b.lam(&[r], |b| op(Op::Eq, field(b.v(r), 0), b.v(pid)));
        let by_pos = b.lam(&[r], |b| field(b.v(r), 2));
        let sorted = Expr::Sort(Box::new(filter(scan("item"), mine)), Box::new(by_pos), vec![Dir::Desc]);
        let first = std(Std::First, vec![Expr::Take(Box::new(sorted), Box::new(int(1)))]);
        let add = b.let_(last, first, |b| {
            let some = b.with(&[r], |b| field(b.v(r), 2));
            let pos = op(Op::Add, Expr::Match(Box::new(b.v(last)), vec![int(0), some]), int(step));
            insert("item", vec![b.v(pid), b.v(track), pos])
        });
        let on = std(Std::IsSome, vec![lookup("item", vec![b.v(pid), b.v(track)])]);
        let exists = std(Std::IsSome, vec![lookup("playlist", vec![b.v(pid)])]);
        if_(exists, if_(on, nothing(), add), refuse("playlist_id: no such playlist"))
    })
}

pub fn add_to_playlist() -> Function {
    add_to_playlist_as(1, ["playlist_id", "track_id", "last", "row"])
}

pub fn rename_playlist() -> Function {
    let params = [("id", named("Id<playlist>")), ("name", named("Name"))];
    mutator(vec![id_decl(), name_decl()], &params, |b| {
        let new = b.lam(&["p"], |b| Expr::Tuple(vec![field(b.v("p"), 0), b.v("name"), field(b.v("p"), 2)]));
        let update = Expr::Update("playlist".into(), Box::new(Expr::Tuple(vec![b.v("id")])), Box::new(new));
        if_(
            std(Std::IsSome, vec![lookup("playlist", vec![b.v("id")])]),
            update,
            refuse("no such playlist"),
        )
    })
}

pub fn remove_from_playlist() -> Function {
    let params = [("playlist_id", named("Id<playlist>")), ("track_id", text_ty())];
    mutator(vec![id_decl()], &params, |b| {
        Expr::Delete("item".into(), Box::new(Expr::Tuple(vec![b.v("playlist_id"), b.v("track_id")])))
    })
}

pub const CREATE: &str = "demo.create_playlist";
pub const ADD: &str = "demo.add_to_playlist";
pub const RENAME: &str = "demo.rename_playlist";
pub const REMOVE: &str = "demo.remove_from_playlist";

pub fn function(name: &str) -> Function {
    match name {
        CREATE => create_playlist(),
        ADD => add_to_playlist(),
        RENAME => rename_playlist(),
        REMOVE => remove_from_playlist(),
        _ => panic!("no demo function {name}"),
    }
}

/// A function's bytes, which are what a body sent in a frame is.
pub fn body(name: &str) -> Vec<u8> {
    function(name).to_bytes()
}

/// A demo function's hash: `sha256(enc(function))`.
pub fn h(name: &str) -> FnHash {
    function(name).hash()
}

/// The demo as a module: its schema, types and bindings, every function
/// checked into `lib`.
pub fn module(lib: &Lib) -> Module {
    let bindings = [ADD, CREATE, REMOVE, RENAME].map(|n| (n.to_string(), lib.install(&schema(), function(n)).unwrap()));
    Module {
        spec: SPEC,
        schema: schema(),
        types: types(),
        bindings: bindings.to_vec(),
    }
}

/// What a content store loads a demo body with.
pub fn loader() -> Loader {
    Lib::default().loader(shared())
}

/// Every demo function, as a peer that authors all of them holds them:
/// each body loaded through the interpreter.
pub fn cas() -> Cas {
    let mut c = Cas::default().with_loader(loader());
    for n in [CREATE, ADD, RENAME, REMOVE] {
        assert_eq!(c.add_body(body(n)), h(n));
        assert!(c.get(&h(n)).is_some(), "{n} loads");
    }
    c
}

/// Dave's build: `add_to_playlist`'s hash, running a body stepping by two.
pub fn daves_cas() -> Cas {
    let mut c = cas();
    let lib = Lib::default();
    let daves = lib.install(&schema(), add_to_playlist_as(2, ["p", "t", "l", "r"])).unwrap();
    c.insert(h(ADD), lib.procedure(&daves).unwrap());
    c
}
