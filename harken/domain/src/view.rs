//! What a client maintains: the library list, read against one playlist.
//!
//! `library(playlist_id)` answers every media row in library order, each
//! with where it sits on that playlist ([`crate::library::LibraryEntry`]).
//! Re-running it on every change costs the library; a client instead holds
//! an `ark::view::View` over [`library_plan`] — the media rows in `pos`
//! order with that playlist's entries beneath each (`playlist_item`, the
//! relationship `playlist_item.media_id` declares) — hydrates it once,
//! pushes every change through `ark::view::push`, and turns each node into
//! the same entry the query answers with [`entry_of`]. [`patch`] is that
//! splice, for a list of entries a screen holds. The plan is read out of
//! the emitted `library` query rather than written a second time, so the
//! query and the view cannot come to disagree; the tests hold the two
//! equal after every step.
//!
//! A rebase (`Changes::Rebuilt`) is no sequence of patches: re-hydrate and
//! take the list whole.

use std::collections::BTreeMap;

use ark::ir::{Expr, Stmt};
use ark::schema::Relation;
use ark::value::{Id, Value};
use ark::view::{self, Patch, ViewPlan};

use crate::module;

/// The field of a node holding the playlist's entries for that media row.
pub const ITEMS: &str = "playlist_item";

/// The plan a client maintains for `library(playlist_id)`: the query's own
/// reads of the media and of the playlist's entries, the entries hung
/// beneath each media row. `playlist_id` is the playlist as `playlists`
/// lists it; the query itself also resolves another id of the same
/// playlist (a same-name playlist made on another device), which a plan
/// fixed to one id cannot.
///
/// # Panics
///
/// If the emitted `library` query no longer reads `playlist_item` and then
/// `media` — a change to the query that this file must follow.
pub fn library_plan(playlist_id: Id) -> ViewPlan {
    let m = module();
    let f = m.build().lookup_function("library").expect("the library query");
    let read = |table: &str| -> ark::ir::Plan {
        f.body
            .iter()
            .find_map(|s| match s {
                Stmt::Let(_, Expr::Select(p)) if p.table == table => Some((**p).clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("library reads {table}: {:?}", f.body))
    };
    let mut items = read("playlist_item");
    items.filter = Some(ark::ir::Pred::Cmp(
        "playlist_id".into(),
        ark::ir::CmpOp::Eq,
        Expr::Lit(Value::Id(playlist_id)),
    ));
    let mut lit = |e: &Expr| -> Result<Value, String> {
        match e {
            Expr::Lit(v) => Ok(v.clone()),
            other => Err(format!("{other:?}")),
        }
    };
    let items = view::eval_plan(&items, &mut lit).expect("the entries' plan, pinned to the playlist");
    let mut media = view::eval_plan(&read("media"), &mut lit).expect("the media plan reads nothing");
    media.related.push((
        ITEMS.into(),
        Relation {
            parent: "media".into(),
            child: "playlist_item".into(),
            column: "media_id".into(),
        },
        items,
    ));
    media
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
