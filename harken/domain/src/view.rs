//! What a screen holds of a library list: [`Item`], one row of what
//! `library`, `album`, `artist`, `recording` and `playlist` answer.
//!
//! The lists themselves are maintained by the engine: a client opens an
//! `ark_client::View` on the query by name, hydrates once and splices the
//! patches each change reports (`docs/plan-v4.md` §1.5). Every query is a
//! plan and every plan is maintained, so nothing here reads a query's plan
//! back out or maintains one beside it.

use ark::value::{Id, Value};

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
    /// An entry, as a query answers it.
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
