//! The admin page's wire, between a server and the explorer it serves
//! compiled to wasm (`docs/plan-guards.md` D4): three requests over HTTP,
//! each body a value in the vectors' JSON dialect (`ark::json`), so the
//! page and the server read one definition of them.
//!
//! ```text
//! GET  <admin>/api/state    → State: the module, the store, the log's lines,
//!                             the connections, the Verify answers, the CRUD
//!                             exposed, whether raw writes are open, who writes
//! POST <admin>/api/raw      {change}       → 200, or the refusal as text
//! POST <admin>/api/author   {fn, args}     → 200, or the refusal as text
//! ```
//!
//! The page is a client of the server process and not of the log: what it
//! is shown is the authority's own state, a snapshot per request, and what
//! it writes the server authors as itself.

use ark::eval::Args;
use ark::json;
use ark::log::Seq;
use ark::protocol::{change_from_value, change_value, entry_from_value, entry_value};
use ark::store::Change;
use ark::value::{TableName, Value};

use crate::data::{Connection, CrudVerbs, Line, Verified};

/// What `GET <admin>/api/state` answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// The server's module, canonical bytes, private blocks and all: the
    /// page is the authority's.
    pub module: Vec<u8>,
    /// Every table's rows, as `Store::store_value` writes them.
    pub store: Value,
    pub head: Seq,
    pub lines: Vec<Line>,
    pub connections: Vec<Connection>,
    pub verifies: Vec<Verified>,
    pub exposed: Vec<(TableName, CrudVerbs)>,
    /// Raw writes are open to this page.
    pub raw: bool,
    /// Who the server writes as.
    pub who: String,
}

fn int(n: i64) -> Value {
    Value::Int(n)
}

fn texts(v: &Value, k: &str) -> Result<String, String> {
    match v.as_struct().get(k) {
        Some(Value::Text(t)) => Ok(t.to_string()),
        other => Err(format!("{k} is not a text: {other:?}")),
    }
}

fn ints(v: &Value, k: &str) -> Result<i64, String> {
    match v.as_struct().get(k) {
        Some(Value::Int(n)) => Ok(*n),
        other => Err(format!("{k} is not an int: {other:?}")),
    }
}

fn list<'v>(v: &'v Value, k: &str) -> Result<&'v [Value], String> {
    match v.as_struct().get(k) {
        Some(Value::List(xs)) => Ok(xs),
        other => Err(format!("{k} is not a list: {other:?}")),
    }
}

fn struct_of(v: &Value) -> Result<(), String> {
    match v {
        Value::Struct(_) => Ok(()),
        other => Err(format!("not a struct: {other:?}")),
    }
}

fn opt_int(v: &Value, k: &str) -> Result<Option<i64>, String> {
    match v.as_struct().get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Int(n)) => Ok(Some(*n)),
        other => Err(format!("{k} is not an int: {other:?}")),
    }
}

impl State {
    pub fn to_value(&self) -> Value {
        let line = |l: &Line| {
            Value::record(vec![
                ("seq", l.seq.map_or(Value::Null, int)),
                ("entry", entry_value(&l.entry)),
                ("fn", Value::text(l.function.clone())),
                (
                    "facts",
                    l.facts
                        .as_ref()
                        .map_or(Value::Null, |f| Value::from(f.iter().map(change_value).collect::<Vec<_>>())),
                ),
                ("standing", Value::text(l.standing.clone())),
            ])
        };
        let conn = |c: &Connection| {
            Value::record(vec![
                ("who", Value::text(c.who.clone())),
                ("cursor", int(c.cursor)),
                ("pending", c.pending.map_or(Value::Null, |n| int(n as i64))),
                ("note", Value::text(c.note.clone())),
            ])
        };
        let verified = |v: &Verified| {
            Value::record(vec![
                ("who", Value::text(v.who.clone())),
                ("seq", int(v.seq)),
                ("answer", v.answer.map_or(Value::Null, Value::Bool)),
            ])
        };
        let exposed = |(t, v): &(TableName, CrudVerbs)| {
            Value::record(vec![
                ("table", Value::text(t.clone())),
                ("insert", Value::Bool(v.insert)),
                ("update", Value::Bool(v.update)),
                ("delete", Value::Bool(v.delete)),
                ("put", Value::Bool(v.put)),
                ("may_author", Value::Bool(v.may_author)),
            ])
        };
        Value::record(vec![
            ("module", Value::Bytes(self.module.clone().into())),
            ("store", self.store.clone()),
            ("head", int(self.head)),
            ("lines", Value::from(self.lines.iter().map(line).collect::<Vec<_>>())),
            ("connections", Value::from(self.connections.iter().map(conn).collect::<Vec<_>>())),
            ("verifies", Value::from(self.verifies.iter().map(verified).collect::<Vec<_>>())),
            ("exposed", Value::from(self.exposed.iter().map(exposed).collect::<Vec<_>>())),
            ("raw", Value::Bool(self.raw)),
            ("who", Value::text(self.who.clone())),
        ])
    }

    pub fn from_value(v: &Value) -> Result<State, String> {
        struct_of(v)?;
        let module = match v.as_struct().get("module") {
            Some(Value::Bytes(b)) => b.to_vec(),
            other => return Err(format!("module is not bytes: {other:?}")),
        };
        let flag = |x: &Value, k: &str| match x.as_struct().get(k) {
            Some(Value::Bool(b)) => Ok(*b),
            other => Err(format!("{k} is not a bool: {other:?}")),
        };
        let lines = list(v, "lines")?
            .iter()
            .map(|l| {
                struct_of(l)?;
                let facts = match l.as_struct().get("facts") {
                    None | Some(Value::Null) => None,
                    Some(Value::List(cs)) => Some(
                        cs.iter()
                            .map(|c| change_from_value(c).map_err(|e| e.to_string()))
                            .collect::<Result<Vec<Change>, String>>()?,
                    ),
                    other => return Err(format!("facts is not a list: {other:?}")),
                };
                Ok(Line {
                    seq: opt_int(l, "seq")?,
                    entry: entry_from_value(&l.field("entry")).map_err(|e| e.to_string())?,
                    function: texts(l, "fn")?,
                    facts,
                    standing: texts(l, "standing")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let connections = list(v, "connections")?
            .iter()
            .map(|c| {
                struct_of(c)?;
                Ok(Connection {
                    who: texts(c, "who")?,
                    cursor: ints(c, "cursor")?,
                    pending: opt_int(c, "pending")?.map(|n| n as usize),
                    note: texts(c, "note")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let verifies = list(v, "verifies")?
            .iter()
            .map(|x| {
                struct_of(x)?;
                Ok(Verified {
                    who: texts(x, "who")?,
                    seq: ints(x, "seq")?,
                    answer: match x.as_struct().get("answer") {
                        None | Some(Value::Null) => None,
                        Some(Value::Bool(b)) => Some(*b),
                        other => return Err(format!("answer is not a bool: {other:?}")),
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let exposed = list(v, "exposed")?
            .iter()
            .map(|x| {
                struct_of(x)?;
                Ok((
                    texts(x, "table")?,
                    CrudVerbs {
                        insert: flag(x, "insert")?,
                        update: flag(x, "update")?,
                        delete: flag(x, "delete")?,
                        put: flag(x, "put")?,
                        may_author: flag(x, "may_author")?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(State {
            module,
            store: v.field("store"),
            head: ints(v, "head")?,
            lines,
            connections,
            verifies,
            exposed,
            raw: flag(v, "raw")?,
            who: texts(v, "who")?,
        })
    }

    /// As the body of the answer.
    pub fn to_json(&self) -> String {
        json::json(&self.to_value())
    }

    pub fn from_json(text: &str) -> Result<State, String> {
        State::from_value(&json::decode(text).map_err(|e| e.to_string())?)
    }
}

/// `POST <admin>/api/raw`'s body: `{change}`, the change as the protocol's
/// fact form.
pub fn raw_body(change: &Change) -> String {
    json::json(&Value::record(vec![("change", change_value(change))]))
}

/// [`raw_body`], read back.
pub fn raw_from(text: &str) -> Result<Change, String> {
    let v = json::decode(text).map_err(|e| e.to_string())?;
    struct_of(&v)?;
    change_from_value(&v.field("change")).map_err(|e| e.to_string())
}

/// `POST <admin>/api/author`'s body: `{fn, args}`.
pub fn author_body(function: &str, args: &Args) -> String {
    json::json(&Value::record(vec![("fn", Value::text(function)), ("args", Value::from(args.clone()))]))
}

/// [`author_body`], read back.
pub fn author_from(text: &str) -> Result<(String, Args), String> {
    let v = json::decode(text).map_err(|e| e.to_string())?;
    struct_of(&v)?;
    let args = match v.as_struct().get("args") {
        Some(Value::Struct(m)) => (**m).clone(),
        other => return Err(format!("args is not a struct: {other:?}")),
    };
    Ok((texts(&v, "fn")?, args))
}
