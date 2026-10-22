//! The vectors D1 writes under `spec/vectors/` (`spec/RUNNERS.md` says
//! how each is run): `protocol/`, every frame as its bytes and its JSON;
//! `trace/`, every client of the scripted sessions;
//! `rebase/`, `fleet-seed-7` and the sessions the fuzzer found a bug with;
//! `views/`, a named view of a reference domain ([`crate::library`]) over a
//! store and batches of row changes, written and run here.
//! `ARK_WRITE_VECTORS` set writes them; otherwise each must be what is on
//! disk.

use std::path::PathBuf;
use std::sync::Arc;

use ark::format::Encode;
use ark::hash::hex;
use ark::log::{EntryId, Origin};
use ark::peer::Who;
use ark::row::Id;
use ark::store::{Change, Store};
use ark_peer::{Config, Live, Module};
use serde_json::{json, Value};

use crate::demo::{arrival, live, room, AddToPlaylist, CreatePlaylist, Point, Rename};
use crate::sim::{who, Net, Op};

fn dir(d: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors").join(d)
}

/// A vector written, or held to what is on disk.
pub fn vector(d: &str, name: &str, text: &str) -> Result<(), String> {
    let path = dir(d).join(name);
    if std::env::var_os("ARK_WRITE_VECTORS").is_some() {
        std::fs::create_dir_all(dir(d)).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
    }
    let on_disk = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    (on_disk == text)
        .then_some(())
        .ok_or(format!("{d}/{name} moved; ARK_WRITE_VECTORS=1 writes it again"))
}

/// Every vector file of a directory, by name.
pub fn on_disk(d: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir(d))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into(),
                std::fs::read_to_string(e.path()).unwrap_or_default(),
            )
        })
        .collect();
    out.sort();
    out
}

// * The scripted sessions

fn demo() -> Config {
    Config::of(Arc::new(crate::demo::demo()))
}

fn create(i: i64, name: &str) -> Op {
    Op::Mutate(i, "demo.create_playlist".into(), CreatePlaylist { name: name.into() }.to_bytes())
}

fn add(i: i64, owner: &str, t: &str) -> Op {
    let input = AddToPlaylist {
        playlist_id: Id::new(EntryId::of(Origin::named(owner), 1).draw(0)),
        track_id: t.into(),
    };
    Op::Mutate(i, "demo.add_to_playlist".into(), input.to_bytes())
}

/// `fleet-seed-7`'s script after its playlist (v4's, move for move).
pub fn fleet_script() -> Vec<Op> {
    let mut s = vec![];
    for k in 1..=4 {
        s.extend((0..=2).map(|i| add(i, "peer-0-session", &format!("{i}-{k}"))));
    }
    s.push(Op::Partition(2));
    s.extend((5..=7).map(|k| add(2, "peer-0-session", &format!("2-{k}"))));
    s.extend([add(0, "peer-0-session", "0-8"), add(1, "peer-0-session", "1-9")]);
    s.extend((0..60).map(|_| Op::Step));
    s.push(Op::Partition(0));
    s.extend([add(1, "peer-0-session", "1-10"), add(0, "peer-0-session", "0-11")]);
    s.extend((0..60).map(|_| Op::Step));
    s.extend([Op::Heal(0), Op::Heal(2)]);
    s.extend((0..80).map(|_| Op::Step));
    s
}

/// `fleet-seed-7` (or another seed) run: the playlist, settled, the
/// script, settled.
pub fn fleet(seed: u64) -> Net {
    let mut net = Net::new(demo(), |_| demo(), 3, seed);
    net.run(&create(0, "Fleet"));
    net.settle().expect("the playlist settles");
    for op in fleet_script() {
        net.run(&op);
    }
    net.settle().expect("the fleet settles");
    net
}

/// The scripted sessions whose clients are the `trace/` vectors.
pub fn sessions() -> Vec<(&'static str, Net)> {
    let room = room();
    let p = "peer-0-session";
    let run = |mut net: Net, ops: Vec<Op>| {
        for op in ops {
            net.run(&op);
        }
        net.settle().expect("a scripted session settles");
        net
    };
    let two = |seed| Net::new(demo(), |_| demo(), 2, seed);
    let mut keep = demo();
    keep.keep = Some(1);
    let anon = Who {
        actor: String::new(),
        session: "anonymous".into(),
        roles: vec![],
    };
    let alice = Who {
        actor: "alice".into(),
        session: "alice-session".into(),
        roles: vec!["listener".into()],
    };
    let mut hub = demo();
    hub.live = Some(Live {
        on_join: Some("room.on_join".into()),
        on_leave: Some("room.on_leave".into()),
        arrive: Arc::new(arrival),
        keep: Some(4),
        ..Live::new(Module::of(&room))
    });
    let device = |name: &str| Config {
        live: Some(live(name, 100)),
        ..demo()
    };
    let point = |i, x| Op::Call(i, "room.point".into(), Point { x }.to_bytes());
    let steps = |n: usize| vec![Op::Step; n];
    vec![
        (
            "the-demo",
            run(two(1), [vec![create(0, "Mix"), add(0, p, "t1"), add(1, p, "t2")], steps(12)].concat()),
        ),
        (
            "a-restart",
            run(
                Net::new(keep, |_| demo(), 2, 2),
                vec![
                    create(0, "Mix"),
                    add(0, p, "t1"),
                    Op::Partition(1),
                    add(0, p, "t2"),
                    add(0, p, "t3"),
                    Op::Restart,
                    Op::Heal(1),
                ],
            ),
        ),
        (
            "a-reopen",
            run(two(3), vec![Op::Partition(0), create(0, "Road"), add(0, p, "t5"), Op::Reopen(0)]),
        ),
        (
            "a-sign-in",
            run(
                Net::of(demo(), vec![(anon, demo()), (who(1), demo())], 4),
                vec![Op::Partition(0), create(0, "Mine"), Op::SignIn(0, alice)],
            ),
        ),
        (
            "alone-then-join",
            run(
                two(5),
                vec![Op::Alone(0), create(0, "Road"), add(0, p, "t5"), create(1, "Bob's"), Op::Join(0)],
            ),
        ),
        (
            "the-room",
            run(
                Net::of(hub, vec![(who(0), device("phone")), (who(1), device("laptop"))], 6),
                [
                    vec![
                        Op::Call(0, "room.rename".into(), Rename { name: "kitchen".into() }.to_bytes()),
                        point(0, 1),
                        point(0, 2),
                        point(0, 3),
                        Op::Wait(100),
                    ],
                    steps(4),
                    vec![Op::Partition(0)],
                ]
                .concat(),
            ),
        ),
        ("fleet-seed-7", fleet(7)),
    ]
}

/// An op as the rebase vector writes it.
pub fn op_json(op: &Op) -> Value {
    match op {
        Op::Mutate(i, f, input) => json!({"client": i, "fn": f, "input": hex(input), "t": "mutate"}),
        Op::Partition(i) => json!({"client": i, "t": "partition"}),
        Op::Heal(i) => json!({"client": i, "t": "heal"}),
        Op::Step => json!({"t": "step"}),
        other => json!({"t": format!("{other:?}")}),
    }
}

// * `views/`

/// Who a view is read as, from the vector's `{"session", "user"}`.
pub fn view_ctx(user: &str, session: &str) -> ark::peer::Ctx {
    ark::peer::Ctx {
        draws: Default::default(),
        id: EntryId::default(),
        actor: user.into(),
        session: session.into(),
        roles: vec![],
        now: 0,
    }
}

fn row_json(s: &ark::schema::Schema, table: &str, b: &[u8]) -> Result<Value, String> {
    let t = s.table(table).ok_or(format!("no table {table}"))?.1;
    ark::dynamic::row(t, b).map_err(|r| r.0)
}

fn row_of(s: &ark::schema::Schema, table: &str, v: &Value) -> Result<Vec<u8>, String> {
    let t = s.table(table).ok_or(format!("no table {table}"))?.1;
    ark::dynamic::row_bytes(t, v).map_err(|r| r.0)
}

/// A change as the protocol's facts read: its kind, its table, its rows.
pub fn change_json(s: &ark::schema::Schema, c: &Change) -> Result<Value, String> {
    let r = |b: &[u8]| row_json(s, c.table(), b);
    Ok(match c {
        Change::Add { table, row } => json!({ "t": "add", "table": table, "row": r(row)? }),
        Change::Remove { table, row } => json!({ "t": "remove", "table": table, "row": r(row)? }),
        Change::Edit { table, old, new } => json!({ "t": "edit", "table": table, "old": r(old)?, "new": r(new)? }),
    })
}

fn change_of(s: &ark::schema::Schema, v: &Value) -> Result<Change, String> {
    let table = v["table"].as_str().ok_or("a change names its table")?.to_string();
    let r = |k: &str| row_of(s, &table, &v[k]);
    Ok(match v["t"].as_str() {
        Some("add") => Change::Add { row: r("row")?, table },
        Some("remove") => Change::Remove { row: r("row")?, table },
        Some("edit") => Change::Edit {
            old: r("old")?,
            new: r("new")?,
            table,
        },
        _ => return Err(format!("{v}: no change")),
    })
}

/// The store, every table by name, rows in key order.
pub fn store_json(s: &ark::schema::Schema, store: &Store) -> Result<Value, String> {
    let mut m = serde_json::Map::new();
    for t in &s.tables {
        m.insert(
            t.name.clone(),
            store.rows(&t.name).map(|r| row_json(s, &t.name, r)).collect::<Result<_, _>>()?,
        );
    }
    Ok(Value::Object(m))
}

/// A view of `view` kept over `store` while `batches` move it: its rows at
/// the start, and after each batch its patches and rows. The contract is
/// held after every push — the rows are the view's answer afresh and a view
/// made again's, and the patches splice the rows before into them — and a
/// breach is the error.
pub fn view_steps(
    d: &ark::Domain,
    view: &str,
    ctx: &ark::peer::Ctx,
    input: &Value,
    mut store: Store,
    batches: &[Vec<Change>],
) -> Result<(Value, Vec<Value>), String> {
    use ark::plan::{splice, Patch};
    let input = d.input_json(view, &input.to_string()).map_err(|r| r.0)?;
    let json = |b: &[u8]| -> Result<Value, String> { serde_json::from_str(&d.element_json(view, b).map_err(|r| r.0)?).map_err(|e| e.to_string()) };
    let list = |rows: Vec<Vec<u8>>| rows.iter().map(|r| json(r)).collect::<Result<Vec<_>, _>>();
    let holds = |store: &Store, rows: &[Value], before: &[Value], patches: &[Patch<Value>]| -> Result<(), String> {
        let fresh: Value = serde_json::from_str(&d.answer_json(view, &d.ask(store, ctx, view, &input).map_err(|r| r.0)?).map_err(|r| r.0)?)
            .map_err(|e| e.to_string())?;
        let again = list(d.watch(store, ctx, view, &input).map_err(|r| r.0)?.rows())?;
        match () {
            _ if fresh != Value::Array(rows.to_vec()) => Err("the view is not its answer afresh".into()),
            _ if again != rows => Err("the view is not a view made again".into()),
            _ if splice(patches, before) != rows => Err("the patches do not splice".into()),
            _ => Ok(()),
        }
    };
    let mut v = d.watch(&store, ctx, view, &input).map_err(|r| r.0)?;
    let first = list(v.rows())?;
    holds(&store, &first, &first, &[])?;
    let (mut was, mut steps) = (first.clone(), vec![]);
    for (i, batch) in batches.iter().enumerate() {
        store.apply_all(batch).map_err(|r| format!("batch {i}: {r}"))?;
        let patches = v.push(batch, &store).map_err(|r| format!("batch {i}: {r}"))?;
        let patches = patches
            .into_iter()
            .map(|p| {
                Ok(match p {
                    Patch::Insert { at, node } => Patch::Insert { at, node: json(&node)? },
                    Patch::Update { at, node } => Patch::Update { at, node: json(&node)? },
                    Patch::Remove { at } => Patch::Remove { at },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let rows = list(v.rows())?;
        holds(&store, &rows, &was, &patches).map_err(|e| format!("batch {i}: {e}"))?;
        let p = |p: &Patch<Value>| match p {
            Patch::Insert { at, node } => json!({ "t": "insert", "at": at, "node": node }),
            Patch::Remove { at } => json!({ "t": "remove", "at": at }),
            Patch::Update { at, node } => json!({ "t": "update", "at": at, "node": node }),
        };
        steps.push(json!({ "patches": patches.iter().map(p).collect::<Vec<_>>(), "rows": rows }));
        was = rows;
    }
    Ok((Value::Array(first), steps))
}

/// A `views/` vector's text: the view named in its domain, who reads it and
/// with what input, the store and the batches, the rows at the start and
/// after each batch its patches and rows.
pub fn view_vector(domain: &str, view: &str, ctx: &ark::peer::Ctx, input: &Value, store: Store, batches: &[Vec<Change>]) -> Result<String, String> {
    let d = crate::library::named(domain).ok_or(format!("no domain {domain}"))?;
    let before = store_json(&d.schema, &store)?;
    let (rows, steps) = view_steps(&d, view, ctx, input, store, batches)?;
    let batches: Vec<Value> = batches
        .iter()
        .map(|b| {
            b.iter()
                .map(|c| change_json(&d.schema, c))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::from)
        })
        .collect::<Result<_, _>>()?;
    let compact = |v: &Value| v.to_string();
    let parts = [
        ("batches", compact(&Value::Array(batches))),
        ("ctx", compact(&json!({ "session": ctx.session, "user": ctx.actor }))),
        ("domain", compact(&json!(domain))),
        ("input", compact(input)),
        ("rows_before", compact(&rows)),
        ("schema", serde_json::to_string(&*d.schema).map_err(|e| e.to_string())?),
        ("steps", format!("[{}]", steps.iter().map(compact).collect::<Vec<_>>().join("\n,"))),
        ("store_before", compact(&before)),
        ("view", compact(&json!(view))),
    ];
    let lines: Vec<String> = parts.iter().map(|(k, v)| format!("  \"{k}\": {v}")).collect();
    Ok(format!("{{\n{}\n}}\n", lines.join(",\n")))
}

/// The `views/` runner (`spec/RUNNERS.md` § views): the vector's view of its
/// reference domain ([`crate::library`]) over its store, its batches pushed
/// one at a time, must give its rows and, after each, its patches and rows;
/// its schema must be the domain's.
pub fn run_view(text: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let name = v["domain"].as_str().ok_or("a vector names its domain")?;
    let d = crate::library::named(name).ok_or(format!("no domain {name}"))?;
    if serde_json::to_value(&*d.schema).map_err(|e| e.to_string())? != v["schema"] {
        return Err(format!("the schema is not {name}'s"));
    }
    let mut by = vec![];
    for t in &d.schema.tables {
        let rows = v["store_before"][&t.name].as_array().ok_or(format!("store_before has no {}", t.name))?;
        by.push((
            t.name.clone(),
            rows.iter().map(|r| row_of(&d.schema, &t.name, r)).collect::<Result<_, _>>()?,
        ));
    }
    let store = Store::from_rows(d.schema.clone(), &by).map_err(|r| r.0)?;
    let batches = (v["batches"].as_array().ok_or("no batches")?.iter())
        .map(|b| {
            b.as_array()
                .ok_or("a batch is a list")?
                .iter()
                .map(|c| change_of(&d.schema, c))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ctx = view_ctx(v["ctx"]["user"].as_str().unwrap_or(""), v["ctx"]["session"].as_str().unwrap_or(""));
    let view = v["view"].as_str().ok_or("a vector names its view")?;
    let (rows, steps) = view_steps(&d, view, &ctx, &v["input"], store, &batches)?;
    if rows != v["rows_before"] {
        return Err("the rows before are not the vector's".into());
    }
    match steps.iter().zip(v["steps"].as_array().ok_or("no steps")?).position(|(a, b)| a != b) {
        Some(i) => Err(format!("step {i} is not the vector's")),
        None if steps.len() != v["steps"].as_array().map_or(0, Vec::len) => Err("the steps are not the vector's".into()),
        None => Ok(()),
    }
}
