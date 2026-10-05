//! §7.2 A module from a value: the inverse of [`super::encode`]
//! (`Ark.Decode`). Strict about shape — an unknown tag, a missing field or
//! a field of the wrong type is a [`DecodeError`] naming the path — and
//! lenient about nothing. A decoded function's `names` is empty.

use std::collections::BTreeMap;

use crate::hash::Closure;
use crate::ir::{Auto, Check, CmpOp, Expr, Field, FnKind, Function, Key, Lookup, Module, Op, Plan, Pred, Related, Router, Source, StdFn, Stmt, Sym};
use crate::schema::{Column, Dir, Index, Ref, Schema, Table, Ty};
use crate::value::{FieldName, Value};

/// Where in the module the shape was wrong, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub path: Vec<String>,
    pub what: String,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.join("/"), self.what)
    }
}

impl std::error::Error for DecodeError {}

type D<T> = Result<T, DecodeError>;
type Fields = BTreeMap<FieldName, Value>;

fn err<T>(here: &[&str], what: impl Into<String>) -> D<T> {
    Err(DecodeError {
        path: here.iter().map(|s| s.to_string()).collect(),
        what: what.into(),
    })
}

/// The whole module (`Ark.Decode.fromValue`).
pub fn module_from_value(v: &Value) -> D<Module> {
    let fs = tagged(&["module"], "module", v)?;
    let spec = int(&["module", "spec"], field(&fs, "spec")?)?;
    let schema = schema_from_value(field(&fs, "schema")?)?;
    let functions = list(&["module", "functions"], function_from_value, field(&fs, "functions")?)?;
    let routers = list(&["module", "routers"], router_from_value, field(&fs, "routers")?)?;
    let live = list(
        &["module", "live"],
        |x| {
            let fs = tagged(&["frame"], "frame", x)?;
            let n = text(&["frame", "name"], field(&fs, "name")?)?;
            let t = ty_from_value(field(&fs, "ty")?)?;
            Ok((n, t))
        },
        field(&fs, "live")?,
    )?;
    Ok(Module {
        spec,
        schema,
        functions,
        routers,
        live,
    })
}

/// §1.1 A router.
pub fn router_from_value(v: &Value) -> D<Router> {
    let fs = tagged(&["router"], "router", v)?;
    let name = text(&["router", "name"], field(&fs, "name")?)?;
    let here = |k: &'static str| vec!["router", name.as_str(), k];
    let uses = list(&here("uses"), |x| text(&here("uses"), x), field(&fs, "uses")?)?;
    Ok(Router { name, uses })
}

/// A closure as an authority stores or sends one: `{ t: "closure", fn,
/// helpers }` (`Ark.Decode.closureFromValue`).
pub fn closure_from_value(v: &Value) -> D<Closure> {
    let fs = tagged(&["closure"], "closure", v)?;
    let function = function_from_value(field(&fs, "fn")?)?;
    let helpers = list(&["closure", "helpers"], function_from_value, field(&fs, "helpers")?)?;
    Ok(Closure { function, helpers })
}

pub fn schema_from_value(v: &Value) -> D<Schema> {
    let tables = list(&["schema"], table, v)?;
    Ok(Schema { tables })
}

fn table(x: &Value) -> D<Table> {
    let fs = tagged(&["table"], "table", x)?;
    let name = text(&["table", "name"], field(&fs, "name")?)?;
    let here = |k: &'static str| vec!["table", name.as_str(), k];
    let columns = list(&here("columns"), column, field(&fs, "columns")?)?;
    let key = list(&here("key"), |k| text(&here("key"), k), field(&fs, "key")?)?;
    let all = list(&here("indexes"), index, field(&fs, "indexes")?)?;
    let refs = list(&here("refs"), reference, field(&fs, "refs")?)?;
    let (mut indexes, mut texts) = (vec![], vec![]);
    for (ix, text) in all {
        match (text, &ix.columns[..]) {
            (false, _) => indexes.push(ix),
            (true, [c]) => texts.push(c.clone()),
            (true, _) => return err(&here("indexes"), "a text index is on one column"),
        }
    }
    Ok(Table::new(name, columns, key, indexes, refs).with_text(texts))
}

fn column(x: &Value) -> D<Column> {
    let fs = tagged(&["column"], "column", x)?;
    let name = text(&["column", "name"], field(&fs, "name")?)?;
    let ty = ty_from_value(field(&fs, "ty")?)?;
    let nullable = bool(&["column", &name, "nullable"], field(&fs, "nullable")?)?;
    Ok(Column { name, ty, nullable })
}

// An index, and whether it is of kind `text` (D4): `kind` is written only
// for one, and names no other kind.
fn index(x: &Value) -> D<(Index, bool)> {
    let fs = tagged(&["index"], "index", x)?;
    let columns = list(&["index", "columns"], |c| text(&["index", "columns"], c), field(&fs, "columns")?)?;
    let unique = bool(&["index", "unique"], field(&fs, "unique")?)?;
    let text_kind = match fs.get("kind") {
        None => false,
        Some(k) if text(&["index", "kind"], k)? == "text" && !unique => true,
        Some(k) => return err(&["index", "kind"], format!("unknown index kind {k:?}")),
    };
    Ok((Index { columns, unique }, text_kind))
}

fn reference(x: &Value) -> D<Ref> {
    let fs = tagged(&["ref"], "ref", x)?;
    let column = text(&["ref", "column"], field(&fs, "column")?)?;
    let table = text(&["ref", "table"], field(&fs, "table")?)?;
    Ok(Ref { column, table })
}

pub fn ty_from_value(v: &Value) -> D<Ty> {
    let (t, fs) = tagged_any(&["ty"], v)?;
    Ok(match t.as_str() {
        "bool" => Ty::Bool,
        "int" => Ty::Int,
        "text" => Ty::Text,
        "bytes" => Ty::Bytes,
        "id" => Ty::Id(text(&["ty", "id"], field(&fs, "table")?)?),
        "enum" => Ty::Enum(list(&["ty", "enum"], |x| text(&["ty", "enum"], x), field(&fs, "variants")?)?),
        "option" => Ty::Option(Box::new(ty_from_value(field(&fs, "of")?)?)),
        "list" => Ty::List(Box::new(ty_from_value(field(&fs, "of")?)?)),
        "struct" => {
            let m = struct_map(&["ty", "struct"], field(&fs, "fields")?)?;
            let mut out = BTreeMap::new();
            for (k, v) in m {
                out.insert(k.clone(), ty_from_value(v)?);
            }
            Ty::Struct(out)
        }
        other => return err(&["ty"], format!("unknown type tag {other}")),
    })
}

pub fn function_from_value(v: &Value) -> D<Function> {
    let fs = tagged(&["fn"], "fn", v)?;
    let name = text(&["fn", "name"], field(&fs, "name")?)?;
    let here: Vec<&str> = vec!["fn", &name];
    let kind_path = [here.as_slice(), &["kind"]].concat();
    let kind_text = text(&kind_path, field(&fs, "kind")?)?;
    let kind = match FnKind::parse(&kind_text) {
        Some(k) => k,
        None => return err(&here, format!("unknown kind {kind_text}")),
    };
    let p = |k: &'static str| [here.as_slice(), &[k]].concat();
    let router = optional(|x| text(&p("router"), x), field(&fs, "router")?)?;
    let uses = list(&p("uses"), |x| text(&p("uses"), x), field(&fs, "uses")?)?;
    let autos = list(&p("autos"), auto, field(&fs, "autos")?)?;
    let input = list(&p("input"), |x| input_field(&p("input"), x), field(&fs, "input")?)?;
    let refine = list(
        &p("refine"),
        |x| {
            let rp = p("refine");
            let rs = tagged(&rp, "refine", x)?;
            Ok((expr(&rp, field(&rs, "e")?)?, why(&rp, field(&rs, "why")?)?))
        },
        field(&fs, "refine")?,
    )?;
    let ret = optional(ty_from_value, field(&fs, "ret")?)?;
    let body = list(&p("body"), |x| stmt(&here, x), field(&fs, "body")?)?;
    let plan = match fs.get("plan") {
        None => None,
        Some(v) => Some(plan(&p("plan"), v)?),
    };
    Ok(Function {
        name,
        kind,
        router,
        uses,
        autos,
        input,
        refine,
        ret,
        body,
        plan,
        names: BTreeMap::new(),
    })
}

fn why(here: &[&str], v: &Value) -> D<Option<String>> {
    optional(|x| text(here, x), v)
}

fn input_field(here: &[&str], x: &Value) -> D<(String, Field)> {
    let fs = tagged(here, "field", x)?;
    let n = text(here, field(&fs, "name")?)?;
    let p: Vec<&str> = [here, &[n.as_str()]].concat();
    let ty = ty_from_value(field(&fs, "ty")?)?;
    let checks = list(&p, |c| check(&p, c), field(&fs, "checks")?)?;
    Ok((n, Field { ty, checks }))
}

fn check(here: &[&str], v: &Value) -> D<Check> {
    let (t, fs) = tagged_any(here, v)?;
    let p: Vec<&str> = [here, &[t.as_str()]].concat();
    let w = || why(&p, field(&fs, "why")?);
    let opt_int = |k: &str| optional(|x| int(&p, x), field(&fs, k)?);
    Ok(match t.as_str() {
        "trim" => Check::Trim,
        "min_len" => Check::MinLen(int(&p, field(&fs, "n")?)?, w()?),
        "max_len" => Check::MaxLen(int(&p, field(&fs, "n")?)?, w()?),
        "range" => Check::Range(opt_int("lo")?, opt_int("hi")?, w()?),
        "non_empty" => Check::NonEmpty(w()?),
        "exists" => Check::Exists(w()?),
        "refine" => Check::Refine(expr(&p, field(&fs, "e")?)?, w()?),
        other => return err(here, format!("unknown check {other}")),
    })
}

fn auto(x: &Value) -> D<(String, Auto)> {
    let (t, fs) = tagged_any(&["auto"], x)?;
    let n = text(&["auto", "name"], field(&fs, "name")?)?;
    match t.as_str() {
        "new_id" => {
            let tb = text(&["auto", &n], field(&fs, "table")?)?;
            Ok((n, Auto::NewId(tb)))
        }
        "now" => Ok((n, Auto::Now)),
        other => err(&["auto", &n], format!("unknown auto {other}")),
    }
}

fn stmt(here: &[&str], v: &Value) -> D<Stmt> {
    let (t, fs) = tagged_any(here, v)?;
    let p: Vec<&str> = [here, &[t.as_str()]].concat();
    Ok(match t.as_str() {
        "let" => Stmt::Let(sym(&p, field(&fs, "sym")?)?, expr(&p, field(&fs, "e")?)?),
        "if" => Stmt::If(
            expr(&p, field(&fs, "c")?)?,
            list(&p, |x| stmt(&p, x), field(&fs, "then")?)?,
            list(&p, |x| stmt(&p, x), field(&fs, "else")?)?,
        ),
        "for" => Stmt::For(
            sym(&p, field(&fs, "sym")?)?,
            expr(&p, field(&fs, "in")?)?,
            list(&p, |x| stmt(&p, x), field(&fs, "body")?)?,
        ),
        "insert" => Stmt::Insert(
            text(&p, field(&fs, "table")?)?,
            expr(&p, field(&fs, "row")?)?,
            list(&p, |x| text(&p, x), field(&fs, "on")?)?,
        ),
        "upsert" => Stmt::Upsert(
            text(&p, field(&fs, "table")?)?,
            expr(&p, field(&fs, "row")?)?,
            list(&p, |x| text(&p, x), field(&fs, "on")?)?,
        ),
        "update" => Stmt::Update(
            text(&p, field(&fs, "table")?)?,
            list(&p, |x| expr(&p, x), field(&fs, "key")?)?,
            sym(&p, field(&fs, "sym")?)?,
            expr(&p, field(&fs, "row")?)?,
        ),
        "delete" => Stmt::Delete(text(&p, field(&fs, "table")?)?, list(&p, |x| expr(&p, x), field(&fs, "key")?)?),
        "refuse" => Stmt::Refuse(expr(&p, field(&fs, "e")?)?),
        "return" => Stmt::Return(optional(|x| expr(&p, x), field(&fs, "e")?)?),
        other => return err(here, format!("unknown statement {other}")),
    })
}

fn expr(here: &[&str], v: &Value) -> D<Expr> {
    let (t, fs) = tagged_any(here, v)?;
    let p: Vec<&str> = [here, &[t.as_str()]].concat();
    let e = |k: &str| -> D<Box<Expr>> { Ok(Box::new(expr(&p, field(&fs, k)?)?)) };
    let s = |k: &str| -> D<Sym> { sym(&p, field(&fs, k)?) };
    let es = |k: &str| -> D<Vec<Expr>> { list(&p, |x| expr(&p, x), field(&fs, k)?) };
    Ok(match t.as_str() {
        "lit" => Expr::Lit(field(&fs, "v")?.clone()),
        "arg" => Expr::Arg(text(&p, field(&fs, "name")?)?),
        "auto" => Expr::Auto(text(&p, field(&fs, "name")?)?),
        "var" => Expr::Var(s("sym")?),
        "ctx_user" => Expr::CtxUser,
        "ctx_session" => Expr::CtxSession,
        "provided" => Expr::Provided(text(&p, field(&fs, "fn")?)?),
        "field" => Expr::Field(e("e")?, text(&p, field(&fs, "name")?)?),
        "struct" => {
            let m = struct_map(&p, field(&fs, "fields")?)?;
            let mut out = BTreeMap::new();
            for (k, v) in m {
                out.insert(k.clone(), expr(&p, v)?);
            }
            Expr::Struct(out)
        }
        "list" => Expr::List(es("items")?),
        "some" => Expr::Some(e("e")?),
        "none" => Expr::None(ty_from_value(field(&fs, "ty")?)?),
        "match" => Expr::Match(e("e")?, s("sym")?, e("some")?, e("none")?),
        "ife" => Expr::If(e("c")?, e("then")?, e("else")?),
        "op" => {
            let name = text(&p, field(&fs, "op")?)?;
            let op = Op::parse(&name).ok_or_else(|| DecodeError {
                path: strings(&p),
                what: format!("unknown operator {name}"),
            })?;
            Expr::Op(op, es("args")?)
        }
        "cmp" => Expr::Cmp(cmp_op(&p, field(&fs, "op")?)?, e("l")?, e("r")?),
        "call" => Expr::Call(text(&p, field(&fs, "fn")?)?, es("args")?),
        "std" => {
            let name = text(&p, field(&fs, "fn")?)?;
            let f = StdFn::parse(&name).ok_or_else(|| DecodeError {
                path: strings(&p),
                what: format!("unknown standard function {name}"),
            })?;
            Expr::Std(f, es("args")?)
        }
        "map" => Expr::Map(e("in")?, s("sym")?, e("body")?),
        "filter" => Expr::Filter(e("in")?, s("sym")?, e("body")?),
        "any" => Expr::Any(e("in")?, s("sym")?, e("body")?),
        "all" => Expr::All(e("in")?, s("sym")?, e("body")?),
        "sort_by" => Expr::SortBy(e("in")?, s("sym")?, e("key")?),
        "fold" => Expr::Fold(e("in")?, e("init")?, s("acc")?, s("sym")?, e("body")?),
        "select" => Expr::Select(Box::new(plan(&p, field(&fs, "plan")?)?)),
        "get" => Expr::Get(text(&p, field(&fs, "table")?)?, es("key")?),
        "exists" => Expr::Exists(text(&p, field(&fs, "table")?)?, es("key")?),
        other => return err(here, format!("unknown expression {other}")),
    })
}

/// §1.8 A plan: the v3 keys always, the v4 ones read when present.
fn plan(here: &[&str], v: &Value) -> D<Plan> {
    let fs = tagged(here, "plan", v)?;
    let table = text(here, field(&fs, "table")?)?;
    let inner: Vec<&str> = [here, &[table.as_str()]].concat();
    let here = inner.as_slice();
    let filter = optional(|x| pred(here, x), field(&fs, "filter")?)?;
    let order = list(
        here,
        |x| {
            let fs = tagged(here, "by", x)?;
            let key = match (fs.get("column"), fs.get("expr")) {
                (Some(c), None) => Key::Column(text(here, c)?),
                (None, Some(e)) => Key::Expr(expr(here, e)?),
                _ => return err(here, "an order key is a column or an expression"),
            };
            let d = match text(here, field(&fs, "dir")?)?.as_str() {
                "asc" => Dir::Asc,
                "desc" => Dir::Desc,
                other => return err(here, format!("unknown direction {other}")),
            };
            Ok((key, d))
        },
        field(&fs, "order")?,
    )?;
    let limit = optional(|x| int(here, x), field(&fs, "limit")?)?;
    let related = list(
        here,
        |x| {
            let fs = tagged(here, "related", x)?;
            let name = text(here, field(&fs, "name")?)?;
            let sub: Vec<&str> = [here, &[name.as_str()]].concat();
            let sym = sym(&sub, field(&fs, "sym")?)?;
            let on = list(
                &sub,
                |pair| match pair {
                    Value::List(xs) if xs.len() == 2 => Ok((text(&sub, &xs[0])?, expr(&sub, &xs[1])?)),
                    _ => err(&sub, "an on pair is [column, expr]"),
                },
                field(&fs, "on")?,
            )?;
            let pl = plan(&sub, field(&fs, "plan")?)?;
            Ok(Related { name, sym, on, plan: pl })
        },
        field(&fs, "related")?,
    )?;
    let source = match fs.get("group") {
        None => Source::Table(table.clone()),
        Some(g) => Source::Group {
            table: table.clone(),
            by: list(here, |c| text(here, c), g)?,
        },
    };
    let opt_sym = |k: &str| fs.get(k).map(|x| sym(here, x)).transpose();
    let row = opt_sym("row")?;
    let members = opt_sym("members")?;
    let lookups = match fs.get("lookups") {
        None => vec![],
        Some(ls) => list(
            here,
            |x| {
                let fs = tagged(here, "lookup", x)?;
                Ok(Lookup {
                    name: text(here, field(&fs, "name")?)?,
                    sym: sym(here, field(&fs, "sym")?)?,
                    table: text(here, field(&fs, "table")?)?,
                    key: list(here, |k| expr(here, k), field(&fs, "key")?)?,
                })
            },
            ls,
        )?,
    };
    let having = fs.get("having").map(|x| expr(here, x)).transpose()?;
    let project = fs.get("project").map(|x| expr(here, x)).transpose()?;
    Ok(Plan {
        source,
        filter,
        row,
        members,
        lookups,
        related,
        having,
        project,
        order,
        limit,
    })
}

fn pred(here: &[&str], v: &Value) -> D<Pred> {
    let (t, fs) = tagged_any(here, v)?;
    Ok(match t.as_str() {
        "pcmp" => Pred::Cmp(
            text(here, field(&fs, "column")?)?,
            cmp_op(here, field(&fs, "op")?)?,
            expr(here, field(&fs, "e")?)?,
        ),
        "pin" => Pred::In(text(here, field(&fs, "column")?)?, list(here, |x| expr(here, x), field(&fs, "items")?)?),
        "pall" => Pred::All(list(here, |x| pred(here, x), field(&fs, "items")?)?),
        "pany" => Pred::Any(list(here, |x| pred(here, x), field(&fs, "items")?)?),
        "pnot" => Pred::Not(Box::new(pred(here, field(&fs, "e")?)?)),
        "phas" => Pred::Has(text(here, field(&fs, "column")?)?, expr(here, field(&fs, "e")?)?),
        other => return err(here, format!("unknown predicate {other}")),
    })
}

fn cmp_op(here: &[&str], v: &Value) -> D<CmpOp> {
    let name = text(here, v)?;
    CmpOp::parse(&name).ok_or_else(|| DecodeError {
        path: strings(here),
        what: format!("unknown comparison {name}"),
    })
}

// Primitives ------------------------------------------------------------

fn strings(here: &[&str]) -> Vec<String> {
    here.iter().map(|s| s.to_string()).collect()
}

fn tagged(here: &[&str], want: &str, v: &Value) -> D<Fields> {
    let (t, fs) = tagged_any(here, v)?;
    if t == want {
        Ok(fs)
    } else {
        err(here, format!("expected {want}, found {t}"))
    }
}

fn tagged_any(here: &[&str], v: &Value) -> D<(String, Fields)> {
    match v {
        Value::Struct(fs) => match fs.get("t") {
            Some(Value::Text(t)) => Ok((t.to_string(), (**fs).clone())),
            _ => err(here, "a node needs a text tag \"t\""),
        },
        _ => err(here, "expected a struct"),
    }
}

fn field<'a>(fs: &'a Fields, k: &str) -> D<&'a Value> {
    fs.get(k).ok_or_else(|| DecodeError {
        path: vec![k.to_string()],
        what: "missing field".into(),
    })
}

fn optional<T>(f: impl FnOnce(&Value) -> D<T>, v: &Value) -> D<Option<T>> {
    match v {
        Value::Null => Ok(None),
        other => Ok(Some(f(other)?)),
    }
}

fn list<T>(here: &[&str], f: impl Fn(&Value) -> D<T>, v: &Value) -> D<Vec<T>> {
    match v {
        Value::List(xs) => xs.iter().map(f).collect(),
        _ => err(here, "expected a list"),
    }
}

fn struct_map<'a>(here: &[&str], v: &'a Value) -> D<&'a Fields> {
    match v {
        Value::Struct(m) => Ok(m),
        _ => err(here, "expected a struct"),
    }
}

fn text(here: &[&str], v: &Value) -> D<String> {
    match v {
        Value::Text(t) => Ok(t.to_string()),
        _ => err(here, "expected text"),
    }
}

fn int(here: &[&str], v: &Value) -> D<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        _ => err(here, "expected an int"),
    }
}

fn bool(here: &[&str], v: &Value) -> D<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => err(here, "expected a bool"),
    }
}

fn sym(here: &[&str], v: &Value) -> D<Sym> {
    int(here, v)
}
