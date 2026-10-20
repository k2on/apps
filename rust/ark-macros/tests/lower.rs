//! Every row of `spec/AUTHORING.md`'s table lowers to the `Expr` the row
//! says: each test writes the construct in an `#[ark::*]` function and
//! compares the body built with the one written out by hand. Each test was
//! falsified once; the doc comment says how.

use ark::codec::{bytes_enc, int_enc};
use ark::ir::{CtxField, Effect, Expr, Op, Std, Un};
use ark::store::Dir;
use ark::ty::Ty;

#[allow(dead_code)]
mod src {
    use ark::author::{Input, Int, Row, Schema, Sum, Table, Text};

    #[derive(Row)]
    pub struct T {
        #[ark(key)]
        pub id: Text,
        pub n: Int,
        pub o: Option<Text>,
    }

    #[derive(Row)]
    pub struct U {
        #[ark(key)]
        pub id: Text,
        pub t: Text,
    }

    #[derive(Schema)]
    pub struct S {
        pub t: Table<T>,
        pub u: Table<U>,
    }

    #[derive(Input)]
    pub struct R {
        pub a: Int,
        pub b: Text,
    }

    #[derive(Sum)]
    pub enum E {
        P { x: Int, y: Int },
        Q(Text),
        Z,
    }

    #[ark::helper]
    pub fn params(a: Int, b: Text) -> Int {
        a
    }

    #[ark::helper]
    pub fn lets(a: Int) -> Int {
        let x = a + 1;
        x * 2
    }

    #[ark::mutator]
    pub fn statements(db: S, id: Text) {
        db.u.delete(id);
        db.t.delete(id);
    }

    #[ark::helper]
    pub fn ifs(a: Int) -> Int {
        if a > 0 {
            a
        } else {
            -a
        }
    }

    #[ark::mutator]
    pub fn if_alone(db: S, id: Text) {
        if id == "" {
            db.t.delete(id);
        }
    }

    #[ark::helper]
    pub fn if_let(o: Option<Int>) -> Int {
        if let Some(x) = o {
            x
        } else {
            0
        }
    }

    #[ark::helper]
    pub fn matches(e: E) -> Int {
        match e {
            E::P { y, x } => x - y,
            E::Q(_) => 1,
            E::Z => 2,
        }
    }

    #[ark::helper]
    pub fn wild(e: E) -> Int {
        match e {
            E::Q(t) => t.len(),
            _ => 0,
        }
    }

    #[ark::helper]
    pub fn literals() -> (Int, Text, bool) {
        (-5, "x", true)
    }

    #[ark::helper]
    pub fn operators(a: Int, b: Int) -> bool {
        a % b == 0 && !(a < b) || a != -b
    }

    #[ark::helper]
    pub fn fields(r: R, p: (Int, Text)) -> Text {
        if r.a > 0 {
            r.b
        } else {
            p.1
        }
    }

    #[ark::helper]
    pub fn literal(r: R) -> R {
        R { b: "z", ..r }
    }

    #[ark::helper]
    pub fn variants(t: Text) -> Vec<E> {
        vec![E::Q(t), E::P { y: 1, x: 2 }, E::Z]
    }

    #[ark::helper]
    pub fn options() -> Vec<Option<Int>> {
        vec![Some(1), None]
    }

    #[ark::helper]
    pub fn calls(a: Int) -> Int {
        lets(a)
    }

    #[ark::helper]
    pub fn stds(t: Text, xs: Vec<Int>) -> Text {
        concat(vec![t.trim(), (t.len() + xs.len()).to_text()])
    }

    #[ark::query]
    pub fn relational(db: S) -> Vec<(T, U)> {
        db.t.filter(|r| r.n > 0)
            .sort(|r| (Desc(r.n), r.id))
            .take(2)
            .join(db.u, |a, b| a.id == b.id)
    }

    #[ark::query]
    pub fn grouped(db: S) -> Vec<(Option<Text>, Int)> {
        db.t.group(|r| r.o, |k, rs| rs.len())
    }

    #[ark::query]
    pub fn reads(db: S, k: Text) -> Vec<Text> {
        match db.t.get(k) {
            Some(r) => db.u.map(|u| u.t),
            None => vec![],
        }
    }

    #[ark::mutator]
    pub fn writes(db: S, k: Text) {
        db.t.insert(T { id: k, n: 1, o: None });
        db.t.update(k, |r| T { n: r.n + 1, ..r });
    }

    #[ark::mutator]
    pub fn refuses(k: Text) {
        refuse!("no");
    }

    #[ark::helper]
    pub fn context(ctx: Ctx) -> (Text, Text, Int, bool) {
        (ctx.user, ctx.session(), ctx.now, ctx.has_role("x"))
    }

    #[ark::mutator]
    pub fn ok(db: S, k: Text) -> Result<(), Refuse> {
        db.t.delete(k);
        Ok(())
    }

    #[ark::helper]
    pub fn optional(o: Option<Int>) -> Vec<Option<Int>> {
        vec![o.map(|x| x + 1), o.filter(|x| x > 0), Some(o.map_or(0, |x| x)), Some(o.unwrap_or(0))]
    }

    #[ark::helper]
    pub fn reader(db: S, k: Text) -> bool {
        db.t.get(k).is_some()
    }

    #[ark::scope]
    pub fn scope(ctx: Ctx, row: SRow) -> bool {
        match row {
            SRow::T(t) => t.n > 0,
            SRow::U(u) => u.t == ctx.user,
        }
    }
}

use src::*;

fn b(e: Expr) -> Box<Expr> {
    Box::new(e)
}
fn v(i: u32) -> Expr {
    Expr::Var(i)
}
fn int(n: i64) -> Expr {
    Expr::Lit(Ty::Int, int_enc(n).to_vec())
}
fn text_ty() -> Ty {
    Ty::Named("Text".into())
}
fn text(s: &str) -> Expr {
    Expr::Lit(text_ty(), bytes_enc(s.as_bytes()))
}
fn op(o: Op, x: Expr, y: Expr) -> Expr {
    Expr::Op(o, b(x), b(y))
}
fn field(x: Expr, i: u32) -> Expr {
    Expr::Field(b(x), i)
}
fn lam(n: u32, body: Expr) -> Box<Expr> {
    b(Expr::Lambda(n, b(body)))
}
fn opt(t: &str) -> Ty {
    Ty::Named(format!("Option<{t}>"))
}
fn e_ty() -> Ty {
    Ty::Named("E".into())
}

/// Parameters in order, the last innermost; `ctx` and `db` are no
/// parameters. Falsified by binding them first-innermost: `a` read `Var(0)`.
#[test]
fn parameters() {
    let f = &params::built().f;
    assert_eq!(f.params, [Ty::Int, text_ty()]);
    assert_eq!((f.ret.clone(), f.effect), (Ty::Int, Effect::Pure));
    assert_eq!(f.body, v(1));
}

/// `let x = e; rest` is `Let(e, rest)`. Falsified by lowering the init
/// under its own binder: `a` read `Var(1)` in it.
#[test]
fn let_binds_the_rest() {
    assert_eq!(
        lets::built().f.body,
        Expr::Let(b(op(Op::Add, v(0), int(1))), b(op(Op::Mul, v(0), int(2))))
    );
}

/// Statements are a `Seq`; a mutator is `Write` and returns `()`.
/// Falsified by dropping the first statement: one delete.
#[test]
fn statements_are_a_seq() {
    let f = &statements::built().f;
    let key = || b(Expr::Tuple(vec![v(0)]));
    assert_eq!(f.body, Expr::Seq(vec![Expr::Delete("u".into(), key()), Expr::Delete("t".into(), key())]));
    assert_eq!((f.ret.clone(), f.effect), (Ty::unit(), Effect::Write));
}

/// `if`/`else` is `If`; an `if` alone has `()` for its else. Falsified by
/// an `if` alone's else being its then.
#[test]
fn if_and_else() {
    assert_eq!(
        ifs::built().f.body,
        Expr::If(b(op(Op::Gt, v(0), int(0))), b(v(0)), b(Expr::Un(Un::Neg, b(v(0)))))
    );
    let del = Expr::Delete("t".into(), b(Expr::Tuple(vec![v(0)])));
    assert_eq!(
        if_alone::built().f.body,
        Expr::If(b(op(Op::Eq, v(0), text(""))), b(del), b(Expr::Seq(vec![])))
    );
}

/// `if let Some(x) = o { a } else { b }` is a `Match` whose `None` arm is
/// the else. Falsified by taking `Some` for variant 0: the `match` in
/// `reads` was refused as having no `Some`.
#[test]
fn if_let_is_a_match() {
    assert_eq!(if_let::built().f.body, Expr::Match(b(v(0)), vec![int(0), v(0)]));
}

/// A match on a declared sum: one arm per variant in declaration order,
/// each binding all its fields, the last innermost, whatever the pattern
/// names; `_` stands for the rest. Falsified by binding a struct variant's
/// fields in the order the pattern names them: `x - y` read `y - x`.
#[test]
fn match_on_a_sum() {
    assert_eq!(
        matches::built().f.body,
        Expr::Match(b(v(0)), vec![op(Op::Sub, v(1), v(0)), int(1), int(2)])
    );
    let q = Expr::Std(Std::TextLen, vec![v(0)]);
    assert_eq!(wild::built().f.body, Expr::Match(b(v(0)), vec![int(0), q, int(0)]));
}

/// Literals: an int (a negative one included), text, a bool; a tuple.
/// Falsified by lowering `-5` as `Neg(5)`.
#[test]
fn literals_and_tuples() {
    let t = Expr::Tuple(vec![int(-5), text("x"), Expr::Lit(Ty::bool(), vec![1])]);
    assert_eq!(literals::built().f.body, t);
}

/// The operators, checked arithmetic, comparison by bytes and the
/// short-circuit booleans. Falsified by `!=` lowered as `Eq`.
#[test]
fn operators_lower_to_ops() {
    let left = op(
        Op::And,
        op(Op::Eq, op(Op::Mod, v(1), v(0)), int(0)),
        Expr::Un(Un::Not, b(op(Op::Lt, v(1), v(0)))),
    );
    let right = op(Op::Ne, v(1), Expr::Un(Un::Neg, b(v(0))));
    assert_eq!(operators::built().f.body, op(Op::Or, left, right));
}

/// A field by name, a tuple's by number: `Field` at the declared position.
/// Falsified by the derive numbering fields from 1.
#[test]
fn fields_by_name() {
    assert_eq!(
        fields::built().f.body,
        Expr::If(b(op(Op::Gt, field(v(1), 0), int(0))), b(field(v(1), 1)), b(field(v(0), 1)))
    );
}

/// A struct literal is the tuple of its fields in declaration order, the
/// rest from `..base`. Falsified by `..base` filling a field from the
/// next one: `a` read `b`.
#[test]
fn struct_literals_by_name() {
    assert_eq!(literal::built().f.body, Expr::Tuple(vec![field(v(0), 0), text("z")]));
}

/// Variants by name, their fields by name; `vec![..]` is a `List`.
/// Falsified by `variant` not ordering a struct variant's fields by
/// position: `P { y: 1, x: 2 }` was built as missing `x`.
#[test]
fn variants_and_lists() {
    let es = vec![
        Expr::Variant(e_ty(), 1, vec![v(0)]),
        Expr::Variant(e_ty(), 0, vec![int(2), int(1)]),
        Expr::Variant(e_ty(), 2, vec![]),
    ];
    assert_eq!(variants::built().f.body, Expr::List(e_ty(), es));
    let os = vec![Expr::Variant(opt("Int"), 1, vec![int(1)]), Expr::Variant(opt("Int"), 0, vec![])];
    assert_eq!(options::built().f.body, Expr::List(opt("Int"), os));
    assert!(
        options::built().f.types.iter().any(|(n, _)| n == "Option<Int>"),
        "an option is declared by its name"
    );
}

/// A call of another `#[ark::*]` function is a `Call` by its hash, its
/// declarations the caller's. Falsified by calling by a hash of the name.
#[test]
fn calls_by_hash() {
    assert_eq!(calls::built().f.body, Expr::Call(lets::built().hash, vec![v(0)]));
}

/// The standard library by method and by `concat`. Falsified by a text's
/// `len` lowered as a list's.
#[test]
fn the_standard_library() {
    let n = op(Op::Add, Expr::Std(Std::TextLen, vec![v(1)]), Expr::Std(Std::Len, vec![v(0)]));
    let parts = Expr::List(text_ty(), vec![Expr::Std(Std::Trim, vec![v(1)]), Expr::Std(Std::TextOfInt, vec![n])]);
    assert_eq!(stds::built().f.body, Expr::Std(Std::Concat, vec![parts]));
}

/// The relational combinators on `db.<table>` with closures as lambdas:
/// `filter`, `sort` with `Desc`, `take`, `join`, `group`, `map`, `get`.
/// Falsified by `Desc` left as the key's direction for every key.
#[test]
fn the_relational_combinators() {
    let filter = Expr::Filter(b(Expr::Scan("t".into())), lam(1, op(Op::Gt, field(v(0), 1), int(0))));
    let sort = Expr::Sort(
        b(filter),
        lam(1, Expr::Tuple(vec![field(v(0), 1), field(v(0), 0)])),
        vec![Dir::Desc, Dir::Asc],
    );
    let take = Expr::Take(b(sort), b(int(2)));
    let join = Expr::Join(b(take), b(Expr::Scan("u".into())), lam(2, op(Op::Eq, field(v(1), 0), field(v(0), 0))));
    let q = &relational::built().f;
    assert_eq!((q.body.clone(), q.effect), (join, Effect::Read));
    let group = Expr::Group(b(Expr::Scan("t".into())), lam(1, field(v(0), 2)), lam(2, Expr::Std(Std::Len, vec![v(0)])));
    assert_eq!(grouped::built().f.body, group);
    let get = Expr::Lookup("t".into(), b(Expr::Tuple(vec![v(0)])));
    let map = Expr::Map(b(Expr::Scan("u".into())), lam(1, field(v(0), 1)));
    assert_eq!(reads::built().f.body, Expr::Match(b(get), vec![Expr::List(text_ty(), vec![]), map]));
}

/// The writes: `insert` of a row by name, `update` of a key through a
/// lambda. Falsified by `update`'s lambda declaring no parameter.
#[test]
fn the_writes() {
    let row = Expr::Tuple(vec![v(0), int(1), Expr::Variant(opt("Text"), 0, vec![])]);
    let new = Expr::Tuple(vec![field(v(0), 0), op(Op::Add, field(v(0), 1), int(1)), field(v(0), 2)]);
    let update = Expr::Update("t".into(), b(Expr::Tuple(vec![v(0)])), lam(1, new));
    assert_eq!(writes::built().f.body, Expr::Seq(vec![Expr::Insert("t".into(), b(row)), update]));
}

/// `refuse!(m)` is `Refuse(m)`. Falsified by a refusal's message dropped
/// for the empty text.
#[test]
fn refuse_is_refuse() {
    assert_eq!(refuses::built().f.body, Expr::Refuse(b(text("no"))));
}

/// `ctx.user`, `.session`, `.now`, `.has_role(r)`; `ctx.new_id()` is the
/// demo's (`tests/demo.rs`). Falsified by `has_role` asking the session.
#[test]
fn the_context() {
    let roles = Expr::Std(Std::Contains, vec![Expr::Ctx(CtxField::Roles), text("x")]);
    let t = Expr::Tuple(vec![
        Expr::Ctx(CtxField::User),
        Expr::Ctx(CtxField::Session),
        Expr::Ctx(CtxField::Now),
        roles,
    ]);
    assert_eq!(context::built().f.body, t);
}

/// A body's last `Ok(())` is nothing, and `-> Result<(), Refuse>` is `()`.
/// Falsified by not dropping it: `Ok(..)` was refused as a call.
#[test]
fn ok_is_nothing() {
    let f = &ok::built().f;
    assert_eq!(
        (f.body.clone(), f.ret.clone()),
        (Expr::Delete("t".into(), b(Expr::Tuple(vec![v(0)]))), Ty::unit())
    );
}

/// An option's `map`, `filter`, `map_or`, `unwrap_or`, as matches and the
/// standard library. Falsified by `filter` keeping the value it refused.
#[test]
fn options_methods() {
    let none = || Expr::Variant(opt("Int"), 0, vec![]);
    let some = |e: Expr| Expr::Variant(opt("Int"), 1, vec![e]);
    let map = Expr::Match(b(v(0)), vec![none(), some(op(Op::Add, v(0), int(1)))]);
    let filter = Expr::Match(b(v(0)), vec![none(), Expr::If(b(op(Op::Gt, v(0), int(0))), b(some(v(0))), b(none()))]);
    let map_or = some(Expr::Match(b(v(0)), vec![int(0), v(0)]));
    let unwrap_or = some(Expr::Std(Std::UnwrapOr, vec![v(0), int(0)]));
    assert_eq!(optional::built().f.body, Expr::List(opt("Int"), vec![map, filter, map_or, unwrap_or]));
}

/// A helper's effect is the least its body has. Falsified by every helper
/// being `Pure`: `reader` would not check.
#[test]
fn a_helper_is_at_its_least_effect() {
    assert_eq!(reader::built().f.effect, Effect::Read);
    assert_eq!(lets::built().f.effect, Effect::Pure);
    assert_eq!(statements::built().f.effect, Effect::Write);
}

/// A scope is `Pure` over the schema's row sum, `<Schema>Row`, and holds a
/// row as V1's peer asks it: the table's index, then the row. Falsified by
/// the row sum's variants out of schema order: `T`'s rows were judged as
/// `U`'s.
#[test]
fn a_scope_holds_rows() {
    use ark::author::Node;
    use ark::peer::Ctx;
    let d = ark::author::assemble(S::schema(), &[scope::built()]).unwrap();
    let p = d.lib.procedure(&scope::built().hash).unwrap();
    let ctx = Ctx {
        id: Default::default(),
        actor: "me".into(),
        session: "s".into(),
        roles: vec![],
        now: 0,
    };
    let t = |n: i64| [vec![0u8], ("a".to_string(), n).to_bytes(), vec![0]].concat();
    let u = |who: &str| [vec![1u8], ("a".to_string(), who.to_string()).to_bytes()].concat();
    assert_eq!(scope::built().f.effect, Effect::Pure);
    assert_eq!(p.holds(&ctx, &t(1)), Ok(true));
    assert_eq!(p.holds(&ctx, &t(0)), Ok(false));
    assert_eq!(p.holds(&ctx, &u("me")), Ok(true));
    assert_eq!(p.holds(&ctx, &u("you")), Ok(false));
}

/// Every function above is one the checker passes over the schema, so each
/// lowering is a well-typed one. Falsified by every function returning
/// `Int`: `params` alone checked.
#[test]
fn every_lowering_checks() {
    let all = [
        params::built(),
        lets::built(),
        statements::built(),
        ifs::built(),
        if_alone::built(),
        if_let::built(),
        matches::built(),
        wild::built(),
        literals::built(),
        operators::built(),
        fields::built(),
        literal::built(),
        variants::built(),
        options::built(),
        calls::built(),
        stds::built(),
        relational::built(),
        grouped::built(),
        reads::built(),
        writes::built(),
        refuses::built(),
        context::built(),
        ok::built(),
        optional::built(),
        reader::built(),
        scope::built(),
    ];
    ark::author::assemble(S::schema(), &all).unwrap_or_else(|r| panic!("{r}"));
}
