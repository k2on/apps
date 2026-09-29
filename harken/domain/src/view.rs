//! What a client maintains: the library list, read against one playlist.
//!
//! `library(playlist_id)` answers every media row in library order, each
//! with where it sits on that playlist ([`crate::library::LibraryEntry`]).
//! Until the engine maintains every plan (`docs/plan-v4.md` §1.5), a client
//! holds an `ark::view::View` over [`library_plan`] — the query's own plan
//! without its projection, the playlist's entries hung beneath each media
//! row and pinned to the playlist — hydrates it once, pushes every change
//! through `ark::view::push`, and turns each node into the entry the query
//! answers with [`entry_of`]. [`patch`] is that splice, for a list of
//! entries a screen holds. The plan is read out of the emitted `library`
//! query rather than written a second time, so the query and the view
//! cannot come to disagree; the tests hold the two equal after every step.
//!
//! A rebase (`Changes::Rebuilt`) is no sequence of patches: re-hydrate and
//! take the list whole.

use std::collections::BTreeMap;

use ark::ir::{CmpOp, Expr, Pred};
use ark::value::{Id, Value};
use ark::view::{self, Patch, ViewPlan};

use crate::module;

/// The field of a node holding the playlist's entries for that media row.
pub const ITEMS: &str = "playlist_item";

/// The plan a client maintains for `library(playlist_id)`: the query's
/// plan, its projection left to [`entry_of`], its entries' filter pinned to
/// the playlist.
///
/// # Panics
///
/// If the emitted `library` query is no longer the media with the
/// playlist's entries beneath — a change to the query that this file must
/// follow.
pub fn library_plan(playlist_id: Id) -> ViewPlan {
    let m = module();
    let f = m.build().lookup_function("library").expect("the library query");
    let mut plan = f.plan.clone().expect("library is a plan");
    assert!(
        plan.table() == "media" && plan.related.len() == 1,
        "library reads media with the entries beneath: {plan:?}"
    );
    plan.project = None;
    plan.related[0].name = ITEMS.into();
    plan.related[0].plan.filter = Some(Pred::Cmp("playlist_id".into(), CmpOp::Eq, Expr::Lit(Value::Id(playlist_id))));
    view::eval_plan(&plan, &mut |e: &Expr| match e {
        Expr::Lit(v) => Ok(v.clone()),
        other => Err(format!("{other:?}")),
    })
    .expect("the library's plan, pinned to the playlist")
}

/// A node of [`library_plan`] as the entry `library` answers with: the media
/// row's columns, and `playlist_pos` for where it sits on the playlist.
pub fn entry_of(node: &Value) -> Value {
    let Value::Struct(m) = node else {
        return node.clone();
    };
    let mut out: BTreeMap<String, Value> = m
        .iter()
        .filter(|(k, _)| k.as_str() != ITEMS)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let pos = match m.get(ITEMS) {
        Some(Value::List(xs)) => xs.first().map(|x| x.field("pos")).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    out.insert("playlist_pos".into(), pos);
    Value::Struct(out)
}

/// Splice what a view reported into a list of entries, in order: an insert
/// or an update carries a node, turned into an entry here.
pub fn patch(entries: &mut Vec<Value>, patches: &[Patch]) {
    let as_entries: Vec<Patch> = patches
        .iter()
        .map(|p| match p {
            Patch::Insert { at, node } => Patch::Insert {
                at: *at,
                node: entry_of(node),
            },
            Patch::Update { at, node } => Patch::Update {
                at: *at,
                node: entry_of(node),
            },
            Patch::Remove { at } => Patch::Remove { at: *at },
        })
        .collect();
    *entries = view::splice(&as_entries, entries);
}

/// A library entry as plain Rust, for a screen: what `library`, `album`,
/// `artist`, `recording` and `playlist` answer, one row each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: Id,
    /// `"song"` today; the name of the table carrying the rest.
    pub kind: String,
    pub title: String,
    /// Whoever made it: a song's artist, a sermon's speaker.
    pub creator: String,
    pub duration_ms: i64,
    /// A path under the media root, or a whole URL.
    pub file: String,
    pub pos: i64,
    pub added_ms: i64,
    pub user_id: String,
    /// Where it sits on the playlist it was read against, if it is on it.
    pub playlist_pos: Option<i64>,
}

impl Item {
    /// An entry, as a query answers it or [`entry_of`] makes it.
    pub fn from_value(v: &Value) -> Item {
        let int = |k: &str| match v.field(k) {
            Value::Int(n) => n,
            _ => 0,
        };
        let text = |k: &str| match v.field(k) {
            Value::Text(t) => t,
            _ => String::new(),
        };
        Item {
            id: match v.field("id") {
                Value::Id(i) => i,
                _ => [0; 16],
            },
            kind: text("kind"),
            title: text("title"),
            creator: text("creator"),
            duration_ms: int("duration_ms"),
            file: text("file"),
            pos: int("pos"),
            added_ms: int("added_ms"),
            user_id: text("user_id"),
            playlist_pos: match v.field("playlist_pos") {
                Value::Int(n) => Some(n),
                _ => None,
            },
        }
    }

    /// On the playlist it was read against.
    pub fn on_playlist(&self) -> bool {
        self.playlist_pos.is_some()
    }
}
