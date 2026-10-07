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

// One refusal case: what it is, the module, and the complaint it wants.
type Case = (&'static str, ir::Module, fn(&Complaint) -> bool);

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
    let cases: Vec<Case> = vec![
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
    let errs = refused.try_build().expect_err("refused");
    assert!(errs.iter().any(|e| e.contains("carries no scope")), "{errs:?}");
    // And a scope's filter can only be a filter.
    let r = router::<orgs::Orgs>("r");
    let s = r.server("ordered", |_ctx, db| db.org.rows().order_by(orgs::Org::name.asc()));
    let refused = Authored::new((r.routes((s.client("q", |_ctx, db, ()| db.org.rows()),)),));
    let errs = refused.try_build().expect_err("refused");
    assert!(errs.iter().any(|e| e.contains("a scope holds rows")), "{errs:?}");
    let _ = APred::<orgs::Org>::when;
}

// The union --------------------------------------------------------------------

use ark::scope::{Scopes, Who};
use std::collections::BTreeSet;

fn who<'a>(user: &'a str, roles: &'a BTreeSet<String>) -> Who<'a> {
    Who { user, session: "s", roles }
}

fn row(t: &ark::schema::Table, pairs: &[(&str, Value)]) -> ark::store::Row {
    ark::store::Row::of(t, pairs.iter().map(|(k, v)| (k.to_string(), v.clone())))
}

// Two orgs, alice a member of the first and bob of both, and three
// accounts.
fn store(m: &ir::Module) -> MemoryStore {
    let mut st = MemoryStore::empty(m.schema.clone());
    let t = |n: &str| m.schema.lookup_table(n).unwrap().clone();
    let (org, member, account) = (t("org"), t("member"), t("account"));
    for (k, name) in [(1u8, "Acme"), (2, "Bolt")] {
        st.put("org", row(&org, &[("id", Value::Id([k; 16])), ("name", Value::text(name))]))
            .unwrap();
    }
    for (k, user) in [(1u8, "alice"), (1, "bob"), (2, "bob")] {
        st.put("member", row(&member, &[("org_id", Value::Id([k; 16])), ("user", Value::text(user))]))
            .unwrap();
    }
    for user in ["alice", "bob", "carol"] {
        let pairs = [
            ("user", Value::text(user)),
            ("name", Value::text(user.to_uppercase())),
            ("password", Value::text("hunter2")),
        ];
        st.put("account", row(&account, &pairs)).unwrap();
    }
    st
}

/// What a person holds is the union of the module's scopes with them put
/// in: alice holds the org she is a member of (the `exists` form), her own
/// membership rows and her own account without its password, and her
/// device's schema has no `password` column and no reference from
/// `member` to an `org` it does not hold whole. An admin holds every table
/// whole, and is the whole peer a module with no scope makes of everybody.
/// Falsified by unioning columns of scopes that fold to nothing as well:
/// alice's `account` kept its password.
#[test]
fn a_person_holds_the_union_of_the_scopes() {
    let m = built();
    let scopes = Scopes::of(&m);
    assert_eq!(scopes.roles(), ["admin".to_string()].into());
    let none = BTreeSet::new();
    let alice = scopes.holdings(who("alice", &none));
    assert!(!alice.is_whole());
    let cols = |cs: &[&str]| cs.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    assert_eq!(
        alice.columns(),
        [
            ("account".to_string(), cols(&["user", "name"])),
            ("member".to_string(), cols(&["org_id", "user"])),
            ("org".to_string(), cols(&["id", "name"])),
        ]
        .into(),
        "every table held in part, with what is held of it"
    );
    let acc = alice.schema().lookup_table("account").unwrap();
    assert!(acc.column("password").is_none(), "an excluded column does not exist on the device");
    assert!(
        alice.schema().lookup_table("member").unwrap().refs.is_empty(),
        "no reference to a parent not held whole"
    );
    let st = store(&m);
    let held = alice.held_rows(&st);
    assert_eq!(held["org"].len(), 1, "the org she is a member of");
    assert_eq!(held["member"].len(), 1, "her own membership");
    assert_eq!(held["account"].len(), 1);
    assert_eq!(held["account"][0].get("password"), None);
    let admin_roles: BTreeSet<String> = ["admin".to_string()].into();
    let admin = scopes.holdings(who("root", &admin_roles));
    assert!(admin.is_whole(), "{:?}", admin.columns());
    assert_eq!(admin.schema(), &m.schema);
    assert_eq!(admin.digest(&st), ark::hash::state_hash(&st), "a whole person's digest is the store's");
    // A module with no scope holds everything for everybody.
    assert!(Scopes::of(&ir::Module {
        functions: vec![],
        ..m.clone()
    })
    .holdings(who("x", &none))
    .is_whole());
}

/// The union digest is the state hash of a store holding exactly what the
/// person holds, laid out as their device lays it — what their own hash is
/// when they are right. Falsified by summing unprojected rows: the digests
/// differ.
#[test]
fn the_union_digest_is_the_held_store_hashed() {
    let m = built();
    let none = BTreeSet::new();
    let alice = Scopes::of(&m).holdings(who("alice", &none));
    let st = store(&m);
    let mut mine = MemoryStore::empty(alice.schema().clone());
    for (t, rs) in alice.held_rows(&st) {
        for r in rs {
            mine.apply_change(&ark::store::Change::Add(t.clone(), r));
        }
    }
    assert_eq!(alice.digest(&st), ark::hash::state_hash(&mine));
    assert_ne!(alice.digest(&st), ark::hash::state_hash(&st));
}

/// An entry's facts as a person holds them: bob adding alice to his
/// second org is, to alice, her new membership and the org it makes hers
/// — a row the entry never touched, arriving through the `exists` form;
/// an edit of a password is nothing to her; an account she does not hold
/// is not sent. Applied to what she held before, they reach what she holds
/// after. Falsified by not sending the `exists` form's rows: the org did
/// not arrive.
#[test]
fn facts_arrive_as_the_person_holds_them() {
    let m = built();
    let none = BTreeSet::new();
    let alice = Scopes::of(&m).holdings(who("alice", &none));
    let before = store(&m);
    let member = m.schema.lookup_table("member").unwrap();
    let account = m.schema.lookup_table("account").unwrap();
    let joined = row(member, &[("org_id", Value::Id([2; 16])), ("user", Value::text("alice"))]);
    let old_pw = before.get("account", &[Value::text("alice")]).unwrap();
    let new_pw = old_pw.clone().with("password", Value::text("swordfish"));
    let carol = before.get("account", &[Value::text("carol")]).unwrap();
    let facts = vec![
        ark::store::Change::Add("member".into(), joined.clone()),
        ark::store::Change::Edit("account".into(), old_pw, new_pw),
        ark::store::Change::Remove("account".into(), carol),
    ];
    let mut after = before.clone();
    after.apply_changes(&facts);
    let seen = alice.filter_facts(&before, &after, &facts);
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(matches!(&seen[0], ark::store::Change::Add(t, r) if t == "member" && *r == joined));
    assert!(matches!(&seen[1], ark::store::Change::Add(t, r) if t == "org" && r.get("name") == Some(&Value::text("Bolt"))));
    let _ = account;
    // Applied over what she held, they are what she holds.
    let mut mine = MemoryStore::empty(alice.schema().clone());
    for (t, rs) in alice.held_rows(&before) {
        for r in rs {
            mine.apply_change(&ark::store::Change::Add(t.clone(), r));
        }
    }
    mine.apply_changes(&seen);
    assert_eq!(ark::hash::state_hash(&mine), alice.digest(&after));
}

/// The verifier's rule: a client-run part of a procedure that names a
/// column outside the union of some role set the module names is
/// `NotHeld`, naming the table, the column and the roles — unless a guard
/// the procedure runs refuses that role set outright. `sign_up` writes a
/// password and builds, behind `is_admin`; the same mutation with the
/// guard taken out does not, for the person holding no role; a query on
/// the own-account chain filtering by password does not either. Falsified
/// by not folding guards: the module with `is_admin` was refused.
#[test]
fn a_client_never_names_a_column_it_does_not_hold() {
    let m = built();
    assert!(verify(&m).is_ok());
    let unguarded = edit(&m, "sign_up", |f| f.uses.retain(|u| u != "is_admin"));
    let cs = complaints(&unguarded);
    assert!(
        cs.iter()
            .any(|c| matches!(c, Complaint::NotHeld(t, col, roles) if t == "account" && col == "password" && roles.is_empty())),
        "{cs:?}"
    );
    let filtered = edit(&m, "me", |f| {
        f.plan.as_mut().unwrap().filter = Some(Pred::Cmp("password".into(), ir::CmpOp::Eq, Expr::Lit(Value::text("x"))));
    });
    let cs = complaints(&filtered);
    assert!(
        cs.iter()
            .any(|c| matches!(c, Complaint::NotHeld(t, col, _) if t == "account" && col == "password")),
        "{cs:?}"
    );
    // A whole-row read names no column: `me` reads every held column of
    // the account and builds.
    assert!(m.lookup_function("me").unwrap().plan.as_ref().unwrap().filter.is_none());
}

// Serving the union ----------------------------------------------------------

use ark::live::Silent;
use ark::peer::{Authority, Replica};
use ark::protocol::{open_access, trusting, Client, Mode, Server, ServerMsg};

struct Fleet {
    m: ir::Module,
    sv: Server<Silent>,
    clients: Vec<Client>,
    /// Every frame the server sent each client, in order.
    sent: Vec<Vec<ServerMsg>>,
}

impl Fleet {
    fn new(scoped: bool) -> Fleet {
        let authored = orgs::module();
        let m = authored.build().clone();
        let mut a = Authority::new(m.schema.clone(), closures(&m));
        a.hold(authored.procedures());
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        if scoped {
            sv = sv.with_scopes(Scopes::of(&m));
        }
        Fleet {
            m,
            sv,
            clients: vec![],
            sent: vec![],
        }
    }

    // A client at 0, connected with this token.
    fn join(&mut self, token: &str) -> usize {
        let r = Replica::open(
            self.m.schema.clone(),
            closures(&self.m),
            MemoryStore::empty(self.m.schema.clone()),
            0,
            vec![],
        );
        let mut c = Client::open(r, Mode::Whole, Some(token.into()));
        c.connected();
        self.clients.push(c);
        self.sent.push(vec![]);
        let i = self.clients.len() - 1;
        self.pump();
        i
    }

    fn pump(&mut self) {
        for _ in 0..8 {
            for (i, c) in self.clients.iter_mut().enumerate() {
                for m in c.take_outgoing() {
                    self.sv.recv(i as i64 + 1, m);
                }
            }
            for (to, m) in self.sv.take_outgoing() {
                let i = (to - 1) as usize;
                self.sent[i].push(m.clone());
                self.clients[i].recv(m);
            }
            for c in &mut self.clients {
                c.settle();
            }
        }
    }

    fn mutate(&mut self, i: usize, user: &str, roles: &[&str], name: &str, id: u8, args: Args) {
        let ctx = Ctx::new(user, "dev").with_roles(roles.iter().map(|r| r.to_string()));
        let fh = closures(&self.m).into_iter().find(|(_, c)| c.function.name == name).unwrap().0;
        let autos: Args = [("id".to_string(), Value::Id([id; 16]))].into();
        let mut eid = [0u8; 16];
        eid[0] = id;
        eid[15] = i as u8;
        self.clients[i].mutate(eid, &ctx, &fh, &autos, &args).expect("accepted on the device");
        self.pump();
    }

    fn holdings(&self, user: &str, roles: &[&str]) -> ark::scope::Holdings {
        let rs: BTreeSet<String> = roles.iter().map(|r| r.to_string()).collect();
        Scopes::of(&self.m).holdings(who(user, &rs))
    }
}

fn text_args(pairs: &[(&str, Value)]) -> Args {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// `docs/plan-guards.md` D2 The authority serves each connection its
/// union. Alice holds no role: she is started from a snapshot of what she
/// holds — no password, no org she is not in — and her device's store has
/// no `password` column. Bob creates an org and adds alice to it: what
/// alice is sent is her membership and the org it gives her, which the
/// entry never touched, and nothing of bob's other org; her hash is the
/// union digest the authority answers her `verify` with. An admin is
/// whole, paged by intents with no `after`/`upto` and no facts. Falsified
/// by serving every connection whole (no `with_scopes`): alice was paged
/// by intents and held bob's org.
#[test]
fn the_authority_serves_each_person_their_union() {
    let mut f = Fleet::new(true);
    let alice = f.join("alice");
    let bob = f.join("bob");
    let root = f.join("root:admin");
    f.mutate(
        root,
        "root",
        &["admin"],
        "sign_up",
        1,
        text_args(&[
            ("user", Value::text("alice")),
            ("name", Value::text("Alice")),
            ("password", Value::text("pw")),
        ]),
    );
    f.mutate(bob, "bob", &[], "create_org", 2, text_args(&[("name", Value::text("Bolt"))]));
    f.mutate(bob, "bob", &[], "create_org", 3, text_args(&[("name", Value::text("Cove"))]));
    assert!(
        f.sent[alice].iter().any(|m| matches!(m, ServerMsg::SnapshotOf { held: Some(_), .. })),
        "started from a partial snapshot"
    );
    let r = &f.clients[alice].replica;
    assert!(r.partial.is_some());
    assert!(r.confirmed.schema().lookup_table("account").unwrap().column("password").is_none());
    assert_eq!(r.confirmed.scan("account").len(), 1, "her own account");
    assert!(r.confirmed.scan("org").is_empty(), "no org of bob's");
    // Bob adds alice to Bolt.
    f.mutate(
        bob,
        "bob",
        &[],
        "join",
        4,
        text_args(&[("org_id", Value::Id([2; 16])), ("user", Value::text("alice"))]),
    );
    let r = &f.clients[alice].replica;
    assert_eq!(r.confirmed.scan("org").len(), 1, "the org the membership gives her");
    assert_eq!(r.confirmed.scan("member").len(), 1);
    let head = f.sv.authority.log.head_seq();
    let digest = f.holdings("alice", &[]).digest(&f.sv.authority.store);
    assert_eq!(r.verify_at(), (head, digest.clone()), "at the head, at the union digest");
    let pages: Vec<&ServerMsg> = f.sent[alice].iter().filter(|m| matches!(m, ServerMsg::Batch { .. })).collect();
    assert!(pages.iter().all(|m| matches!(m, ServerMsg::Batch { covers: Some(_), .. })));
    for m in &pages {
        if let ServerMsg::Batch { items, .. } = m {
            for (_, e, facts) in items {
                assert!(e.args.is_empty() || e.actor == "alice", "another's intent as its envelope");
                assert!(facts.is_some());
            }
        }
    }
    // Her verify is answered from the union digest.
    f.clients[alice].verify_all();
    f.pump();
    assert_eq!(f.clients[alice].agreed.last(), Some(&(head, Some(true))));
    // The admin is whole.
    let root_pages: Vec<&ServerMsg> = f.sent[root].iter().filter(|m| matches!(m, ServerMsg::Batch { .. })).collect();
    assert!(!root_pages.is_empty());
    assert!(root_pages.iter().all(|m| matches!(m, ServerMsg::Batch { covers: None, .. })));
    assert!(f.clients[root].replica.partial.is_none());
    assert_eq!(f.clients[root].replica.verify_at().1, ark::hash::state_hash(&f.sv.authority.store));
}

/// A partial peer's own intent is previewed over what it holds and
/// confirmed by the authority's facts: alice creates an org and holds it
/// and her membership; she then adds bob to it, which her device previews
/// as bob's membership row — a row she does not hold — and which the
/// authority's facts for her leave out. She is confirmed by those, the
/// preview taken back, at the union digest: nothing pending, and no
/// divergence, since a difference from her record is the authority's
/// answer about what she holds. Falsified by confirming a partial
/// replica's own intents through `confirm_inbox` (the whole path): the
/// difference was recorded as a divergence.
#[test]
fn a_partial_peer_previews_and_is_confirmed_by_facts() {
    let mut f = Fleet::new(true);
    let alice = f.join("alice");
    f.mutate(alice, "alice", &[], "create_org", 5, text_args(&[("name", Value::text("Mine"))]));
    let r = &f.clients[alice].replica;
    assert!(r.pending.is_empty() && r.diverged.is_empty(), "{:?} {:?}", r.pending, r.diverged);
    assert_eq!(r.confirmed.scan("org").len(), 1);
    f.mutate(
        alice,
        "alice",
        &[],
        "join",
        6,
        text_args(&[("org_id", Value::Id([5; 16])), ("user", Value::text("bob"))]),
    );
    let r = &f.clients[alice].replica;
    assert!(r.pending.is_empty() && r.diverged.is_empty(), "{:?} {:?}", r.pending, r.diverged);
    assert_eq!(r.view.scan("member").len(), 1, "her own membership, and not bob's");
    assert_eq!(f.sv.authority.store.scan("member").len(), 2, "both, at the authority");
    assert_eq!(r.verify_at().1, f.holdings("alice", &[]).digest(&f.sv.authority.store));
}

/// A role granted between connections moves a person from a union to
/// everything: alice reconnects holding `admin`, is answered with the
/// whole snapshot at the head, and holds the log whole — her store the
/// module's schema again. Falsified by keeping a peer that says `partial`
/// on the paged path (`elsewhere` ignoring it): she stayed partial-shaped
/// with the password column missing.
#[test]
fn a_role_granted_makes_a_partial_peer_whole() {
    let mut f = Fleet::new(true);
    let alice = f.join("alice");
    let root = f.join("root:admin");
    f.mutate(
        root,
        "root",
        &["admin"],
        "sign_up",
        1,
        text_args(&[
            ("user", Value::text("carol")),
            ("name", Value::text("C")),
            ("password", Value::text("pw")),
        ]),
    );
    assert!(f.clients[alice].replica.partial.is_some());
    f.clients[alice].disconnected();
    f.sv.disconnect(alice as i64 + 1);
    f.clients[alice].token = Some("alice:admin".into());
    f.clients[alice].connected();
    f.pump();
    let r = &f.clients[alice].replica;
    assert!(r.partial.is_none(), "whole now");
    assert_eq!(r.confirmed.schema(), &f.m.schema);
    assert_eq!(r.verify_at().1, ark::hash::state_hash(&f.sv.authority.store));
    assert_eq!(r.confirmed.scan("account").len(), 1, "carol's account, password and all");
}

/// What the scopes change about a whole peer: nothing. A module with
/// scopes served to a person who holds everything sends the frames a
/// server with no scopes sends, byte for byte. Falsified by sending a
/// whole connection's pages with `covers` all the same: the frames
/// differed.
#[test]
fn a_whole_peer_is_served_the_bytes_it_always_was() {
    let run = |scoped: bool| {
        let mut f = Fleet::new(scoped);
        let root = f.join("root:admin");
        let other = f.join("dave:admin");
        f.mutate(other, "dave", &["admin"], "create_org", 9, text_args(&[("name", Value::text("Dove"))]));
        f.mutate(
            root,
            "root",
            &["admin"],
            "sign_up",
            1,
            text_args(&[("user", Value::text("eve")), ("name", Value::text("E")), ("password", Value::text("pw"))]),
        );
        f.sent[root].iter().map(|m| encode(&m.to_value())).collect::<Vec<_>>()
    };
    assert_eq!(run(true), run(false));
}
