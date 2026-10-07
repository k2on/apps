//! `docs/plan-guards.md` D2: scopes — middleware that says what a person
//! holds. What the authoring emits, what the wire carries, what the
//! verifier refuses, and that a scope runs nothing.

#[path = "support/orgs.rs"]
mod orgs;

use std::collections::BTreeMap;

use ark::authoring::{router, Module as Authored, Pred as APred};
use ark::canon::{decode, encode};
use ark::eval::{self, Args, Ctx};
use ark::hash::{closures, function_hash};
use ark::ir::{self, Expr, FnKind, Hold, Pred, Projection};
use ark::store::{MemoryStore, Store};
use ark::value::Value;
use ark::verify::{verify, Complaint, VerifyError};

fn built() -> ir::Module {
    orgs::module().build().clone()
}

fn complaints(m: &ir::Module) -> Vec<Complaint> {
    match verify(m) {
        Ok(_) => vec![],
        Err(es) => es
            .into_iter()
            .filter_map(|e| match e {
                VerifyError::In(_, c) => Some(c),
                _ => None,
            })
            .collect(),
    }
}

fn edit(m: &ir::Module, name: &str, f: impl FnOnce(&mut ir::Function)) -> ir::Module {
    let mut m = m.clone();
    f(m.functions.iter_mut().find(|g| g.name == name).expect("the function"));
    m
}

/// A scope on the router is inherited by every procedure on it, and one on
/// a chain by every procedure built on the chain; each is a function of
/// kind `scope` with its holds, listed in `uses` and in the router's.
/// Falsified by emitting the scope's holds empty: the module does not
/// build (`ScopeHoldsNothing`).
#[test]
fn a_scope_is_middleware_with_holds() {
    let m = built();
    let mine = m.lookup_function("memberships").expect("the router's scope");
    assert_eq!(mine.kind, FnKind::Scope);
    assert!(mine.body.is_empty() && mine.input.is_empty() && mine.ret.is_none());
    assert_eq!(
        mine.holds.iter().map(|h| h.table.as_str()).collect::<Vec<_>>(),
        ["org", "member"],
        "one hold per table, in the order written"
    );
    let org = &mine.holds[0];
    let Some(Pred::Any(ps)) = &org.filter else { panic!("{:?}", org.filter) };
    assert!(matches!(&ps[0], Pred::Exists(t, c, _) if t == "member" && c == "org_id"));
    assert!(matches!(&ps[1], Pred::When(Expr::HasRole(r)) if r == "admin"));
    for p in ["create_org", "join", "my_orgs"] {
        assert_eq!(m.lookup_function(p).unwrap().uses, ["memberships"], "{p} inherits the router's scope");
    }
    assert_eq!(m.lookup_router("orgs").unwrap().uses, ["memberships"]);
    let own = m.lookup_function("own_account").unwrap();
    assert_eq!(own.holds[0].columns, Projection::Exclude(vec!["password".into()]));
    assert_eq!(m.lookup_function("me").unwrap().uses, ["own_account"]);
    assert_eq!(m.lookup_function("sign_up").unwrap().uses, ["is_admin", "all_accounts"]);
    assert_eq!(ir::reads(mine), ["member".to_string(), "org".to_string()].into(), "what a scope reads");
}

/// A scope is hashed into the closure of every procedure that runs it, as
/// any middleware is: the same query with the scope taken out of its
/// `uses` is another function. Falsified by leaving scopes out of
/// `reaches`: the two hashes were equal.
#[test]
fn a_scope_is_hashed_into_the_closure() {
    let m = built();
    let with = closures(&m).into_iter().find(|(_, c)| c.function.name == "my_orgs").unwrap();
    assert!(with.1.helpers.iter().any(|h| h.name == "memberships"), "the closure carries its scope");
    let without = edit(&m, "my_orgs", |f| f.uses.clear());
    let c = ir::closure(&without, without.lookup_function("my_orgs").unwrap());
    assert_ne!(function_hash(&c), with.0);
}

/// Encoded, a scope carries its holds and a module with none carries none:
/// a function of any other kind writes no `holds` key, so every module
/// before scopes is the bytes it was (`spec/vectors` holds that whole).
/// Decode of encode is the identity, `pick`, `exclude` and every leaf.
/// Falsified by writing `holds` on every function: the plain function's
/// bytes held the key.
#[test]
fn holds_are_on_the_wire_only_for_a_scope() {
    let m = built();
    let v = ir::module_value(&m);
    let back = ir::module_from_value(&decode(&encode(&v)).unwrap()).unwrap();
    assert_eq!(ir::module_value(&back), v);
    let plain = ir::function_value(&BTreeMap::new(), m.lookup_function("create_org").unwrap());
    assert!(!encode(&plain).windows(5).any(|w| w == b"holds"), "a mutator writes no holds");
    let picked = Hold {
        table: "account".into(),
        filter: None,
        columns: Projection::Pick(vec!["user".into(), "name".into()]),
    };
    assert_eq!(ir::hold_from_value(&ir::hold_value(&picked)).unwrap(), picked);
    // A scope without holds, and holds on something else, are not one form.
    let mut fv = ir::function_value(&BTreeMap::new(), m.lookup_function("memberships").unwrap());
    if let Value::Struct(fs) = &mut fv {
        fs.remove("holds");
    }
    assert!(ir::function_from_value(&fv).is_err(), "a scope with no holds key");
}

/// The verifier holds a scope to its shape and its holds to their table:
/// each of these is a one-line edit of a module that verifies, refused
/// with its own complaint. Falsified by accepting an argument in a hold's
/// filter (`ctx_only` passing `Arg`): the module verified.
#[test]
fn the_verifier_holds_a_scope_to_ctx_and_its_tables() {
    let m = built();
    assert!(complaints(&m).is_empty(), "{:?}", verify(&m).err());
    let holds = m.lookup_function("memberships").unwrap().holds.clone();
    let set = |p: Pred| {
        edit(&m, "memberships", |f| {
            f.holds.iter_mut().find(|h| h.table == "member").unwrap().filter = Some(p);
        })
    };
    let cases: Vec<(&str, ir::Module, fn(&Complaint) -> bool)> = vec![
        (
            "an argument",
            set(Pred::Cmp("user".into(), ir::CmpOp::Eq, Expr::Arg("who".into()))),
            |c| matches!(c, Complaint::ScopeReadsBeyondCtx(w) if w == "an argument"),
        ),
        (
            "a column the table lacks",
            set(Pred::Cmp("nope".into(), ir::CmpOp::Eq, Expr::CtxUser)),
            |c| matches!(c, Complaint::UnknownColumn(t, col) if t == "member" && col == "nope"),
        ),
        (
            "an exists through no reference",
            set(Pred::Exists("account".into(), "user".into(), Box::new(Pred::All(vec![])))),
            |c| matches!(c, Complaint::ExistsNotAReference(..)),
        ),
        ("a role of the wrong type", set(Pred::When(Expr::Lit(Value::text("admin")))), |c| {
            matches!(c, Complaint::TypeMismatch(..))
        }),
        (
            "a projection dropping the key",
            edit(&m, "own_account", |f| f.holds[0].columns = Projection::Exclude(vec!["user".into()])),
            |c| matches!(c, Complaint::ProjectionDropsKey(t, k) if t == "account" && k == "user"),
        ),
        (
            "a nested exists",
            edit(&m, "memberships", |f| {
                let inner = Pred::Exists("member".into(), "org_id".into(), Box::new(Pred::All(vec![])));
                f.holds[0].filter = Some(Pred::Exists("member".into(), "org_id".into(), Box::new(inner)));
            }),
            |c| matches!(c, Complaint::NestedExists | Complaint::ExistsNotAReference(..)),
        ),
        ("a scope with no holds", edit(&m, "memberships", |f| f.holds.clear()), |c| {
            matches!(c, Complaint::ScopeHoldsNothing)
        }),
        (
            "a scope with a body",
            edit(&m, "memberships", |f| f.body = vec![ir::Stmt::Return(None)]),
            |c| matches!(c, Complaint::NotAScope(w) if w == "a body"),
        ),
        ("holds on a guard", edit(&m, "is_admin", |f| f.holds = holds.clone()), |c| {
            matches!(c, Complaint::HoldsOutsideScope)
        }),
        (
            "a scope's leaf in a query's plan",
            edit(&m, "my_orgs", |f| {
                f.plan.as_mut().unwrap().filter = Some(Pred::When(Expr::HasRole("admin".into())))
            }),
            |c| matches!(c, Complaint::ScopeLeafInPlan),
        ),
    ];
    for (what, m2, want) in cases {
        let cs = complaints(&m2);
        assert!(cs.iter().any(want), "{what}: {cs:?}");
    }
}

/// A scope runs nothing: a mutator and a query on a scoped router apply
/// natively and through the interpreter to the same store, the scope
/// skipped in both — a scope that ran would refuse as a function with no
/// body would. Falsified by not skipping scopes in the interpreter's
/// preamble: `apply_closure` failed with `NoReturn`.
#[test]
fn a_scope_runs_nothing() {
    let authored = orgs::module();
    let m = authored.build();
    let st = MemoryStore::empty(m.schema.clone());
    let (_, create) = authored.procedure("create_org").unwrap();
    let ctx = Ctx::new("alice", "s");
    let autos: Args = [("id".to_string(), Value::Id([7; 16]))].into();
    let args: Args = [("name".to_string(), Value::text("Acme"))].into();
    let applied = create.agrees(&ctx, &autos, &args, &st).expect("native and interpreted agree");
    assert_eq!(applied.expect("no bug").expect("accepted").len(), 2, "the org and the membership");
    let mut st = st;
    create.apply(&ctx, &autos, &args, &mut st).unwrap().unwrap();
    let (_, q) = authored.procedure("my_orgs").unwrap();
    let got = q.query(&ctx, &Args::new(), &st).expect("the query runs");
    assert_eq!(got, eval::query_closure(&m.schema, q.closure(), &ctx, &Args::new(), &st).unwrap());
    assert_eq!(st.scan("org").len(), 1);
}

/// `client` is a query over a scope: the same function `query` emits, and
/// refused at build on a chain that carries no scope, where the name would
/// say something untrue. Falsified by dropping the check: the module built.
#[test]
fn client_is_a_query_over_a_scope() {
    let m = built();
    assert_eq!(m.lookup_function("my_orgs").unwrap().kind, FnKind::Query);
    let bare = router::<orgs::Orgs>("bare");
    let refused = Authored::new((bare.routes((bare.client("all", |_ctx, db, ()| db.org.rows()),)),));
    let errs = refused.try_build().err().expect("refused");
    assert!(errs.iter().any(|e| e.contains("carries no scope")), "{errs:?}");
    // And a scope's filter can only be a filter.
    let r = router::<orgs::Orgs>("r");
    let s = r.server("ordered", |_ctx, db| db.org.rows().order_by(orgs::Org::name.asc()));
    let refused = Authored::new((r.routes((s.client("q", |_ctx, db, ()| db.org.rows()),)),));
    let errs = refused.try_build().err().expect("refused");
    assert!(errs.iter().any(|e| e.contains("a scope holds rows")), "{errs:?}");
    let _ = APred::<orgs::Org>::when;
}
