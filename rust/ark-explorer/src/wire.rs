//! The admin page's wire, between a server and the explorer it serves
//! compiled to wasm (`docs/plan-guards.md` D4): four requests over HTTP,
//! each body a value of §1.1's codec — derived, so the page and the server
//! read one definition of them — written as hex, so it travels as text.
//!
//! ```text
//! GET  <admin>/api/hello    → Hello: whether the page is open, where to sign in
//! GET  <admin>/api/state    → State: the domain's catalog, the
//!                             store's rows, the log's lines, the connections,
//!                             the CRUD exposed, whether raw writes are open, who
//!                             writes
//! POST <admin>/api/raw      Change              → the sequence, or the refusal as text
//! POST <admin>/api/author   (binding, input)    → the sequence, or the refusal as text
//! ```
//!
//! The page is a client of the server process and not of the log: what it
//! is shown is the authority's own state, a snapshot per request, and what
//! it writes the server authors as itself.

use ark::gen::Node;
use ark::log::Seq;
use ark::store::Change;

use crate::data::{Catalog, Connection, CrudVerbs, Line};

/// What `GET <admin>/api/hello` answers.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Hello {
    pub open: bool,
    pub server: Option<String>,
}

/// What `GET <admin>/api/state` answers.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct State {
    /// The server's domain: its schema and its functions described.
    pub catalog: Catalog,
    /// Every durable table's rows, as a snapshot carries them.
    pub rows: Vec<(String, Vec<Vec<u8>>)>,
    pub head: Seq,
    pub lines: Vec<Line>,
    pub connections: Vec<Connection>,
    pub exposed: Vec<(String, CrudVerbs)>,
    /// Raw writes are open to this page.
    pub raw: bool,
    /// Who the server writes as.
    pub who: String,
}

/// A value as a body: its bytes, in hex.
pub fn body<T: Node>(v: &T) -> String {
    ark::hash::hex(&v.to_bytes())
}

/// A body read back.
pub fn read<T: Node>(text: &str) -> Result<T, String> {
    let b = ark::hash::unhex(text.trim()).ok_or("the body is not hex")?;
    T::from_bytes(&b).map_err(|e| e.0)
}

impl State {
    pub fn to_body(&self) -> String {
        body(self)
    }

    pub fn from_body(text: &str) -> Result<State, String> {
        read(text)
    }
}

/// `POST <admin>/api/raw`'s body: the change.
pub fn raw_body(change: &Change) -> String {
    body(change)
}

/// [`raw_body`], read back.
pub fn raw_from(text: &str) -> Result<Change, String> {
    read(text)
}

/// `POST <admin>/api/author`'s body: the binding and its input's bytes.
pub fn author_body(function: &str, input: &[u8]) -> String {
    body(&(function.to_string(), input.to_vec()))
}

/// [`author_body`], read back.
pub fn author_from(text: &str) -> Result<(String, Vec<u8>), String> {
    read(text)
}
