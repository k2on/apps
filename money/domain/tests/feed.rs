//! What a connection fed `session.feed` holds (§1.8), table by table, is
//! what the scope listed before each row said its owner: every table whole
//! — the budget is the household's — but the sign-ins and the roles, its
//! person's. Each table is asked of a row its reader's and a row somebody
//! else's.

use ark::peer::Ctx;
use ark::row::Type;
use serde_json::{json, Value};

/// The tables the scope fed only its person's rows of, and the one it fed
/// none of; every other one it fed whole.
const THEIRS: &[&str] = &["sign_in", "role"];
const NONE: &[&str] = &["asking", "syncing"];

/// A value of `t` as JSON, every `user` in it `user`.
fn zero(t: &Type, user: &str) -> Value {
    match t {
        Type::Int => json!(0),
        Type::Text => json!(""),
        Type::Bool => json!(false),
        Type::Bytes | Type::List(_) => json!([]),
        Type::Unit | Type::Option(_) => Value::Null,
        Type::Id(_) => json!("00".repeat(16)),
        Type::Tuple(ts) => ts.iter().map(|t| zero(t, user)).collect(),
        Type::Record(_, fs) => fs
            .iter()
            .map(|(n, t)| (n.clone(), if n == "user" { json!(user) } else { zero(t, user) }))
            .collect::<serde_json::Map<_, _>>()
            .into(),
        Type::Sum(_, vs) => match &vs[0] {
            (v, fs) if fs.is_empty() => json!(v),
            (v, fs) => json!({ v: zero(&Type::Record(v.clone(), fs.clone()), user) }),
        },
    }
}

/// Falsified by `Role` saying no owner: bob's role was held for alice.
#[test]
fn the_feed_holds_what_its_list_held() {
    let d = money_domain::domain();
    let holds = d.get("session.feed").and_then(|f| f.scope.clone()).expect("the scope");
    let alice = Ctx::who("alice", "a", vec![]);
    for t in &d.schema.tables {
        for whose in ["alice", "bob"] {
            let row = ark::json::from_text(&t.row_type(), &zero(&t.row_type(), whose).to_string()).unwrap();
            let was = !NONE.contains(&t.name.as_str()) && (whose == "alice" || !THEIRS.contains(&t.name.as_str()));
            assert_eq!(holds(&alice, &t.name, &row), Ok(was), "{}, {whose}'s", t.name);
        }
    }
}
