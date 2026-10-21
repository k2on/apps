//! §1.16 Rows without a Rust type: a JSON value under the schema.
//!
//! A domain's rows are Rust types and its views typed plans. The explorer
//! and the vectors have neither: they hold a row as a JSON object of its
//! columns, read under the types the schema gives them, and a view as a
//! plan written as JSON — the same operators ([`crate::plan`]) over the
//! same store, so a view's patches are the typed engine's. One plan form
//! serves a runner in any language.
//!
//! **Values** are the store's own JSON (`crate::json`): a row is the
//! object of its columns, written back in the schema's column order
//! ([`row_bytes`]), its key and a related read's prefix the schema's types'
//! key bytes.
//!
//! **Order.** Sort, group and join keys are a value's [`ordered`] bytes:
//! null, then booleans, numbers, strings, arrays and objects, each kind in
//! its own order (a string by its bytes, an array element by element) —
//! the order of the typed values a Rust view sorts, for the types a key is
//! made of. A sum orders by its variants' names here.
//!
//! **Plans.** An object names its operator by its first key:
//!
//! | form | is |
//! |---|---|
//! | `{"scan": "item"}` | every row of a table, by key |
//! | `{"filter": P, "as": "i", "where": E}` | the elements `E` holds of |
//! | `{"map": P, "as": "i", "to": E}` | each element as `E` makes it |
//! | `{"sort": P, "as": "i", "by": [E, {"desc": E}]}` | by the keys, stable |
//! | `{"take": P, "n": 2}` | the first `n` |
//! | `{"group": P, "as": "i", "by": E, "key": "k", "members": "xs", "to": E}` | `[key, E]` per group, by key |
//! | `{"join": [P, Q], "as": ["p", "i"], "on": [E, E]}` | `[p, i]` per pair whose keys are equal |
//!
//! At the top of a view each is an incremental operator; inside an
//! expression it is evaluated whole, its reads recorded as a closure's.
//!
//! **Expressions.** A number, string, boolean or null is itself; an array
//! a tuple; `{"var": "i.pos"}` a variable and a path of fields and
//! indices; `{"arg": "playlist_id"}` the input's field; `{"lit": v}`,
//! `{"record": {..}}`, `{"==": [a, b]}` (and `!=`, `<`, `<=`, `>`, `>=`),
//! `{"and": [..]}`, `{"or": [..]}`, `{"not": e}`, `{"+": [a, b]}`, `{"-":
//! [a, b]}`, `{"len": e}`, `{"sum": e}`, `{"first": e}`, `{"??": [e, d]}`
//! (`d` where `e` is null), `{"lower": e}`, `{"contains": [s, t]}`, `{"if":
//! [c, a, b]}`, `{"let": {"x": e}, "in": e}`, `{"get": "work", "key": e}`
//! (the row or null) and `{"related": "song", "on": "creator", "to": e}`
//! (the rows whose column is `e`, through an index that leads with it).

use std::sync::Arc;

use serde_json::{Map, Value as Json};

use crate::db::{Db, Tables};
use crate::json::{typed_json, typed_key};
use crate::plan::{scan_raw, Make, Plan};
use crate::row::Key;
use crate::schema::{Schema, Table};
use crate::store::Store;
use crate::Refusal;

/// A schema known only at run time: the store's.
pub struct Dynamic;

impl Tables for Dynamic {
    type Db<'t> = ();
    fn handles<'t>(_: &'t std::cell::RefCell<crate::db::Txn<'t>>) {}
    fn schema() -> Schema {
        Schema::default()
    }
}

fn no(why: impl Into<String>) -> Refusal {
    Refusal(why.into())
}

// * Rows

/// A table's row as JSON: an object of its columns.
pub fn row(table: &Table, bytes: &[u8]) -> Result<Json, Refusal> {
    serde_json::from_slice(bytes).map_err(|e| no(format!("a row of {}: {e}", table.name)))
}

/// A row's bytes from its JSON object, a column missing read as null: the
/// row's one encoding, as its Rust type's would be.
pub fn row_bytes(table: &Table, v: &Json) -> Result<Vec<u8>, Refusal> {
    typed_json(&table.row_type(), v)
}

/// The bytes of a key, given as its one column's value or, keyed by more,
/// an array of them.
pub fn key_bytes(table: &Table, key: &Json) -> Result<Vec<u8>, Refusal> {
    let cols = &table.columns[..table.key];
    let parts: Vec<&Json> = match (cols.len(), key) {
        (1, k) => vec![k],
        (_, Json::Array(ks)) if ks.len() == cols.len() => ks.iter().collect(),
        _ => return Err(no(format!("a key of {} is {} columns", table.name, cols.len()))),
    };
    let mut out = vec![];
    for (c, k) in cols.iter().zip(parts) {
        typed_key(&c.ty, k, &mut out)?;
    }
    Ok(out)
}

fn table<'s>(s: &'s Schema, name: &str) -> Result<&'s Table, Refusal> {
    s.table(name).map(|(_, t)| t).ok_or_else(|| no(format!("no table {name}")))
}

// * Order

/// `v`'s bytes in the order the module's head says: prefix-free, so keys
/// concatenate.
pub fn ordered(v: &Json, out: &mut Vec<u8>) {
    let text = |s: &str, out: &mut Vec<u8>| {
        for &b in s.as_bytes() {
            out.push(b);
            if b == 0 {
                out.push(0xFF);
            }
        }
        out.extend([0, 0]);
    };
    match v {
        Json::Null => out.push(1),
        Json::Bool(b) => out.push(2 + *b as u8),
        Json::Number(n) => {
            out.push(4);
            out.extend_from_slice(&((n.as_i64().unwrap_or(i64::MAX) as u64) ^ (1 << 63)).to_be_bytes());
        }
        Json::String(s) => {
            out.push(5);
            text(s, out);
        }
        Json::Array(xs) => {
            out.push(6);
            xs.iter().for_each(|x| ordered(x, out));
            out.push(0);
        }
        Json::Object(m) => {
            out.push(7);
            for (k, x) in m {
                text(k, out);
                ordered(x, out);
            }
            out.push(0);
        }
    }
}

/// A key already in its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ordered(pub Vec<u8>, pub Json);

impl Key for Ordered {
    fn put_key(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0)
    }
}

fn order_of(v: Json) -> Ordered {
    let mut b = vec![];
    ordered(&v, &mut b);
    Ordered(b, v)
}

// * Expressions

/// A closure's one parameter and its body.
#[derive(Clone, Debug)]
pub struct Lam {
    var: String,
    body: Box<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A plan or an expression, parsed.
#[derive(Clone, Debug)]
pub enum Expr {
    Lit(Json),
    Var(String, Vec<Json>),
    Arg(String),
    Tuple(Vec<Expr>),
    Record(Vec<(String, Expr)>),
    Cmp(Cmp, Box<Expr>, Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Len(Box<Expr>),
    Sum(Box<Expr>),
    First(Box<Expr>),
    Else(Box<Expr>, Box<Expr>),
    Lower(Box<Expr>),
    Contains(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Let(Vec<(String, Expr)>, Box<Expr>),
    Get(String, Box<Expr>),
    Related(String, String, Box<Expr>),
    Scan(String),
    Filter(Box<Expr>, Lam),
    Map(Box<Expr>, Lam),
    Sort(Box<Expr>, String, Vec<(Expr, bool)>),
    Take(Box<Expr>, usize),
    Group {
        input: Box<Expr>,
        by: Lam,
        key: String,
        members: String,
        to: Box<Expr>,
    },
    Join(Box<Expr>, Box<Expr>, Lam, Lam),
}

fn bx(e: Expr) -> Box<Expr> {
    Box::new(e)
}

fn name(m: &Map<String, Json>, k: &str) -> Result<String, Refusal> {
    m.get(k)
        .and_then(Json::as_str)
        .map(String::from)
        .ok_or_else(|| no(format!("`{k}` is a name")))
}

fn sub(m: &Map<String, Json>, k: &str) -> Result<Expr, Refusal> {
    parse(m.get(k).ok_or_else(|| no(format!("`{k}` is missing")))?)
}

fn two(v: &Json) -> Result<(Expr, Expr), Refusal> {
    match v.as_array().map(|a| &a[..]) {
        Some([a, b]) => Ok((parse(a)?, parse(b)?)),
        _ => Err(no(format!("{v}: two expressions"))),
    }
}

fn lam(m: &Map<String, Json>, body: &str) -> Result<Lam, Refusal> {
    Ok(Lam {
        var: name(m, "as")?,
        body: bx(sub(m, body)?),
    })
}

/// A plan or an expression from its JSON, or why it is not one.
pub fn parse(v: &Json) -> Result<Expr, Refusal> {
    let m = match v {
        Json::Array(xs) => return Ok(Expr::Tuple(xs.iter().map(parse).collect::<Result<_, _>>()?)),
        Json::Object(m) => m,
        v => return Ok(Expr::Lit(v.clone())),
    };
    const PARAMS: [&str; 9] = ["as", "where", "to", "by", "n", "key", "members", "on", "in"];
    let mut ops = m.iter().filter(|(k, _)| !PARAMS.contains(&k.as_str()));
    let (op, a) = match (ops.next(), ops.next()) {
        (Some(o), None) => o,
        _ => return Err(no(format!("{v}: one operator"))),
    };
    let one = || parse(a).map(bx);
    let cmp = |c| two(a).map(|(x, y)| Expr::Cmp(c, bx(x), bx(y)));
    Ok(match op.as_str() {
        "lit" => Expr::Lit(a.clone()),
        "var" => {
            let path = a.as_str().ok_or_else(|| no("a var is a name"))?;
            let mut parts = path.split('.');
            let v = parts.next().unwrap().to_string();
            let path = parts
                .map(|p| p.parse::<u64>().map(Json::from).unwrap_or_else(|_| Json::from(p)))
                .collect();
            Expr::Var(v, path)
        }
        "arg" => Expr::Arg(name(m, "arg")?),
        "record" => Expr::Record(
            a.as_object()
                .ok_or_else(|| no("a record is an object"))?
                .iter()
                .map(|(k, e)| Ok((k.clone(), parse(e)?)))
                .collect::<Result<_, Refusal>>()?,
        ),
        "==" => cmp(Cmp::Eq)?,
        "!=" => cmp(Cmp::Ne)?,
        "<" => cmp(Cmp::Lt)?,
        "<=" => cmp(Cmp::Le)?,
        ">" => cmp(Cmp::Gt)?,
        ">=" => cmp(Cmp::Ge)?,
        "and" | "or" => {
            let es = a
                .as_array()
                .ok_or_else(|| no("a list of expressions"))?
                .iter()
                .map(parse)
                .collect::<Result<_, _>>()?;
            if op == "and" {
                Expr::And(es)
            } else {
                Expr::Or(es)
            }
        }
        "not" => Expr::Not(one()?),
        "+" => two(a).map(|(x, y)| Expr::Add(bx(x), bx(y)))?,
        "-" => two(a).map(|(x, y)| Expr::Sub(bx(x), bx(y)))?,
        "len" => Expr::Len(one()?),
        "sum" => Expr::Sum(one()?),
        "first" => Expr::First(one()?),
        "??" => two(a).map(|(x, y)| Expr::Else(bx(x), bx(y)))?,
        "lower" => Expr::Lower(one()?),
        "contains" => two(a).map(|(x, y)| Expr::Contains(bx(x), bx(y)))?,
        "if" => match a.as_array().map(|a| &a[..]) {
            Some([c, x, y]) => Expr::If(bx(parse(c)?), bx(parse(x)?), bx(parse(y)?)),
            _ => return Err(no("`if` is [condition, then, else]")),
        },
        "let" => Expr::Let(
            a.as_object()
                .ok_or_else(|| no("`let` binds an object"))?
                .iter()
                .map(|(k, e)| Ok((k.clone(), parse(e)?)))
                .collect::<Result<_, Refusal>>()?,
            bx(sub(m, "in")?),
        ),
        "get" => Expr::Get(name(m, "get")?, bx(sub(m, "key")?)),
        "related" => Expr::Related(name(m, "related")?, name(m, "on")?, bx(sub(m, "to")?)),
        "scan" => Expr::Scan(name(m, "scan")?),
        "filter" => Expr::Filter(one()?, lam(m, "where")?),
        "map" => Expr::Map(one()?, lam(m, "to")?),
        "sort" => {
            let keys = m.get("by").and_then(Json::as_array).ok_or_else(|| no("`by` is a list of keys"))?;
            let keys = keys
                .iter()
                .map(|k| match k.get("desc") {
                    Some(d) if k.as_object().is_some_and(|o| o.len() == 1) => Ok((parse(d)?, true)),
                    _ => Ok((parse(k)?, false)),
                })
                .collect::<Result<_, Refusal>>()?;
            Expr::Sort(one()?, name(m, "as")?, keys)
        }
        "take" => Expr::Take(one()?, m.get("n").and_then(Json::as_u64).ok_or_else(|| no("`n` is a count"))? as usize),
        "group" => Expr::Group {
            input: one()?,
            by: lam(m, "by")?,
            key: name(m, "key")?,
            members: name(m, "members")?,
            to: bx(sub(m, "to")?),
        },
        "join" => {
            let (l, r) = two(a)?;
            let vars = m
                .get("as")
                .and_then(Json::as_array)
                .filter(|a| a.len() == 2)
                .ok_or_else(|| no("`as` names both sides"))?;
            let (lk, rk) = two(m.get("on").ok_or_else(|| no("`on` is the two keys"))?)?;
            let var = |i: usize| vars[i].as_str().map(String::from).ok_or_else(|| no("a side's name"));
            Expr::Join(bx(l), bx(r), Lam { var: var(0)?, body: bx(lk) }, Lam { var: var(1)?, body: bx(rk) })
        }
        o => return Err(no(format!("`{o}` is no operator"))),
    })
}

/// What an expression is evaluated in: the input, the variables bound,
/// and the store when it may read.
struct Env<'a, 'd, 't> {
    arg: &'a Json,
    vars: Vec<(String, Json)>,
    db: Option<&'d Db<'t, Dynamic>>,
}

impl<'d, 't> Env<'_, 'd, 't> {
    fn bind<T>(&mut self, var: &str, v: Json, f: impl FnOnce(&mut Self) -> T) -> T {
        self.vars.push((var.into(), v));
        let t = f(self);
        self.vars.pop();
        t
    }

    fn db(&self) -> Result<&'d Db<'t, Dynamic>, Refusal> {
        self.db.ok_or_else(|| no("a key is not read from the store"))
    }

    fn schema(&self) -> Result<Arc<Schema>, Refusal> {
        Ok(self.db()?.schema())
    }
}

fn at(v: &Json, p: &Json) -> Json {
    match (v, p) {
        (Json::Object(m), Json::String(k)) => m.get(k).cloned().unwrap_or(Json::Null),
        (Json::Array(xs), Json::Number(i)) => i.as_u64().and_then(|i| xs.get(i as usize)).cloned().unwrap_or(Json::Null),
        _ => Json::Null,
    }
}

fn int(v: &Json) -> Result<i64, Refusal> {
    v.as_i64().ok_or_else(|| no(format!("{v} is not an Int")))
}

fn list(v: Json) -> Result<Vec<Json>, Refusal> {
    match v {
        Json::Array(xs) => Ok(xs),
        v => Err(no(format!("{v} is not a list"))),
    }
}

fn key_of(v: &Json) -> Vec<u8> {
    let mut b = vec![];
    ordered(v, &mut b);
    b
}

fn eval(e: &Expr, env: &mut Env) -> Result<Json, Refusal> {
    let ev = |e: &Expr, env: &mut Env| eval(e, env);
    Ok(match e {
        Expr::Lit(v) => v.clone(),
        Expr::Var(v, path) => {
            let (_, x) = env
                .vars
                .iter()
                .rev()
                .find(|(n, _)| n == v)
                .ok_or_else(|| no(format!("no variable {v}")))?;
            path.iter().fold(x.clone(), |x, p| at(&x, p))
        }
        Expr::Arg(a) => at(env.arg, &Json::from(a.as_str())),
        Expr::Tuple(es) => Json::Array(es.iter().map(|e| ev(e, env)).collect::<Result<_, _>>()?),
        Expr::Record(fs) => Json::Object(fs.iter().map(|(k, e)| Ok((k.clone(), ev(e, env)?))).collect::<Result<_, Refusal>>()?),
        Expr::Cmp(c, a, b) => {
            let (a, b) = (ev(a, env)?, ev(b, env)?);
            let o = key_of(&a).cmp(&key_of(&b));
            Json::from(match c {
                Cmp::Eq => o.is_eq(),
                Cmp::Ne => o.is_ne(),
                Cmp::Lt => o.is_lt(),
                Cmp::Le => o.is_le(),
                Cmp::Gt => o.is_gt(),
                Cmp::Ge => o.is_ge(),
            })
        }
        Expr::And(es) => {
            for e in es {
                if ev(e, env)? != Json::Bool(true) {
                    return Ok(Json::Bool(false));
                }
            }
            Json::Bool(true)
        }
        Expr::Or(es) => {
            for e in es {
                if ev(e, env)? == Json::Bool(true) {
                    return Ok(Json::Bool(true));
                }
            }
            Json::Bool(false)
        }
        Expr::Not(e) => Json::from(ev(e, env)? != Json::Bool(true)),
        Expr::Add(a, b) => Json::from(int(&ev(a, env)?)?.checked_add(int(&ev(b, env)?)?).ok_or_else(|| no("integer overflow"))?),
        Expr::Sub(a, b) => Json::from(int(&ev(a, env)?)?.checked_sub(int(&ev(b, env)?)?).ok_or_else(|| no("integer overflow"))?),
        Expr::Len(e) => match ev(e, env)? {
            Json::Array(xs) => Json::from(xs.len() as i64),
            Json::String(s) => Json::from(s.chars().count() as i64),
            v => return Err(no(format!("{v} has no length"))),
        },
        Expr::Sum(e) => {
            let total = list(ev(e, env)?)?.iter().try_fold(0i128, |t, x| Ok::<_, Refusal>(t + int(x)? as i128))?;
            Json::from(i64::try_from(total).map_err(|_| no("integer overflow"))?)
        }
        Expr::First(e) => list(ev(e, env)?)?.into_iter().next().unwrap_or(Json::Null),
        Expr::Else(e, d) => match ev(e, env)? {
            Json::Null => ev(d, env)?,
            v => v,
        },
        Expr::Lower(e) => Json::from(ev(e, env)?.as_str().ok_or_else(|| no("`lower` of text"))?.to_lowercase()),
        Expr::Contains(s, t) => {
            let (s, t) = (ev(s, env)?, ev(t, env)?);
            Json::from(
                s.as_str()
                    .zip(t.as_str())
                    .ok_or_else(|| no("`contains` of text"))?
                    .0
                    .contains(t.as_str().unwrap()),
            )
        }
        Expr::If(c, a, b) => match ev(c, env)? {
            Json::Bool(true) => ev(a, env)?,
            _ => ev(b, env)?,
        },
        Expr::Let(bs, body) => {
            let n = env.vars.len();
            for (k, e) in bs {
                let v = ev(e, env)?;
                env.vars.push((k.clone(), v));
            }
            let r = ev(body, env);
            env.vars.truncate(n);
            r?
        }
        Expr::Get(t, k) => {
            let k = ev(k, env)?;
            if k.is_null() {
                return Ok(Json::Null);
            }
            let s = env.schema()?;
            let tb = table(&s, t)?;
            match env.db()?.get_raw(t, key_bytes(tb, &k)?) {
                Some(b) => row(tb, &b)?,
                None => Json::Null,
            }
        }
        Expr::Related(t, c, to) => {
            let v = ev(to, env)?;
            let s = env.schema()?;
            let tb = table(&s, t)?;
            let ci = tb.column(c).ok_or_else(|| no(format!("{t} has no column {c}")))?;
            let mut p = vec![];
            typed_key(&tb.columns[ci].ty, &v, &mut p)?;
            let path = if ci == 0 {
                Some(None)
            } else {
                tb.indexes.iter().position(|ix| ix.columns.first() == Some(&ci)).map(Some)
            };
            let rows = match path {
                Some(ix) => env.db()?.scan_raw(t, ix, p).iter().map(|b| row(tb, b)).collect::<Result<Vec<_>, _>>()?,
                None => {
                    let all = env.db()?.scan_raw(t, None, vec![]);
                    let rows = all.iter().map(|b| row(tb, b)).collect::<Result<Vec<_>, _>>()?;
                    rows.into_iter().filter(|r| at(r, &Json::from(c.as_str())) == v).collect()
                }
            };
            Json::Array(rows)
        }
        Expr::Scan(t) => {
            let s = env.schema()?;
            let tb = table(&s, t)?;
            Json::Array(env.db()?.scan_raw(t, None, vec![]).iter().map(|b| row(tb, b)).collect::<Result<_, _>>()?)
        }
        Expr::Filter(i, f) => {
            let mut out = vec![];
            for x in list(ev(i, env)?)? {
                if env.bind(&f.var, x.clone(), |env| ev(&f.body, env))? == Json::Bool(true) {
                    out.push(x);
                }
            }
            Json::Array(out)
        }
        Expr::Map(i, f) => Json::Array(
            list(ev(i, env)?)?
                .into_iter()
                .map(|x| env.bind(&f.var, x, |env| ev(&f.body, env)))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Sort(i, var, keys) => {
            let mut xs = list(ev(i, env)?)?
                .into_iter()
                .map(|x| Ok((env.bind(var, x.clone(), |env| sort_key(keys, env))?, x)))
                .collect::<Result<Vec<_>, Refusal>>()?;
            xs.sort_by(|a, b| a.0.cmp(&b.0));
            Json::Array(xs.into_iter().map(|(_, x)| x).collect())
        }
        Expr::Take(i, n) => Json::Array(list(ev(i, env)?)?.into_iter().take(*n).collect()),
        Expr::Group { input, by, key, members, to } => {
            let mut groups: std::collections::BTreeMap<Vec<u8>, (Json, Vec<Json>)> = Default::default();
            for x in list(ev(input, env)?)? {
                let k = env.bind(&by.var, x.clone(), |env| ev(&by.body, env))?;
                groups.entry(key_of(&k)).or_insert_with(|| (k, vec![])).1.push(x);
            }
            let mut out = vec![];
            for (_, (k, xs)) in groups {
                let a = env.bind(key, k.clone(), |env| env.bind(members, Json::Array(xs), |env| ev(to, env)))?;
                out.push(Json::Array(vec![k, a]));
            }
            Json::Array(out)
        }
        Expr::Join(l, r, lk, rk) => {
            let (ls, rs) = (list(ev(l, env)?)?, list(ev(r, env)?)?);
            let keys = |xs: &[Json], f: &Lam, env: &mut Env| {
                xs.iter()
                    .map(|x| env.bind(&f.var, x.clone(), |env| ev(&f.body, env)).map(|k| key_of(&k)))
                    .collect::<Result<Vec<_>, _>>()
            };
            let (lks, rks) = (keys(&ls, lk, env)?, keys(&rs, rk, env)?);
            let mut out = vec![];
            for (a, ka) in ls.iter().zip(&lks) {
                for (b, kb) in rs.iter().zip(&rks) {
                    if ka == kb {
                        out.push(Json::Array(vec![a.clone(), b.clone()]));
                    }
                }
            }
            Json::Array(out)
        }
    })
}

fn sort_key(keys: &[(Expr, bool)], env: &mut Env) -> Result<Vec<u8>, Refusal> {
    let mut out = vec![];
    for (k, desc) in keys {
        let at = out.len();
        ordered(&eval(k, env)?, &mut out);
        if *desc {
            out[at..].iter_mut().for_each(|b| *b = !*b);
        }
    }
    Ok(out)
}

/// An expression evaluated over a store: a plan's answer whole.
pub fn evaluate(e: &Expr, arg: &Json, store: &Store) -> Result<Json, Refusal> {
    crate::db::read::<Dynamic, _>(store, |db| {
        eval(
            e,
            &mut Env {
                arg,
                vars: vec![],
                db: Some(db),
            },
        )
    })
}

// * Plans

fn run(f: &Lam, arg: &Json, db: Option<&Db<Dynamic>>, x: &Json) -> Json {
    let mut env = Env { arg, vars: vec![], db };
    env.bind(&f.var, x.clone(), |env| eval(&f.body, env))
        .unwrap_or_else(|e| panic!("a plan's closure refused: {e}"))
}

/// The incremental plan of `e`: its top operators as the view's, what is
/// inside them evaluated whole.
pub fn plan(e: &Expr, arg: Arc<Json>) -> Result<Plan<Dynamic, Json>, Refusal> {
    Ok(match e {
        Expr::Scan(t) => {
            let t2 = t.clone();
            scan_raw(t, move |s: &Store, b: &[u8]| {
                let tb = table(s.schema_arc(), &t2).expect("a scanned table");
                row(tb, b).unwrap_or_else(|e| panic!("a row of {t2} reads: {e}"))
            })
        }
        Expr::Filter(i, f) => {
            let f = f.clone();
            plan(i, arg.clone())?.filter_with(move |db, x| run(&f, &arg, Some(db), x) == Json::Bool(true))
        }
        Expr::Map(i, f) => {
            let f = f.clone();
            plan(i, arg.clone())?.map_with(move |db, x| run(&f, &arg, Some(db), x))
        }
        Expr::Sort(i, var, keys) => {
            let (var, keys) = (var.clone(), keys.clone());
            plan(i, arg.clone())?.sort_with(move |db, x| {
                let mut env = Env {
                    arg: &arg,
                    vars: vec![],
                    db: Some(db),
                };
                let k = env
                    .bind(&var, x.clone(), |env| sort_key(&keys, env))
                    .unwrap_or_else(|e| panic!("a sort key refused: {e}"));
                Ordered(k, Json::Null)
            })
        }
        Expr::Take(i, n) => plan(i, arg)?.take(*n),
        Expr::Group { input, by, key, members, to } => {
            let (by, key, members, to) = (by.clone(), key.clone(), members.clone(), to.clone());
            let a2 = arg.clone();
            plan(input, arg.clone())?.group(
                move |x| order_of(run(&by, &a2, None, x)),
                move |db, k: &Ordered, xs| {
                    let mut env = Env {
                        arg: &arg,
                        vars: vec![],
                        db: Some(db),
                    };
                    let xs = Json::Array(xs.cloned().collect());
                    let a = env
                        .bind(&key, k.1.clone(), |env| env.bind(&members, xs, |env| eval(&to, env)))
                        .unwrap_or_else(|e| panic!("a group's aggregate refused: {e}"));
                    Json::Array(vec![k.1.clone(), a])
                },
            )
        }
        Expr::Join(l, r, lk, rk) => {
            let (lk, rk, a2) = (lk.clone(), rk.clone(), arg.clone());
            let right = plan(r, arg.clone())?;
            plan(l, arg.clone())?
                .join(
                    right,
                    move |x| order_of(run(&lk, &arg, None, x)),
                    move |x| order_of(run(&rk, &a2, None, x)),
                )
                .map(|(a, b)| Json::Array(vec![a.clone(), b.clone()]))
        }
        e => return Err(no(format!("a view is a plan, not {e:?}"))),
    })
}

/// How a view of the plan `v` over input `arg` is made.
pub fn make(v: &Json, arg: Json) -> Result<Make<Dynamic, Json>, Refusal> {
    let e = parse(v)?;
    plan(&e, Arc::new(arg.clone()))?;
    let arg = Arc::new(arg);
    Ok(Arc::new(move |_, _| plan(&e, arg.clone())))
}
