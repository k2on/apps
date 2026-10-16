//! §3.12 Which transitions need an order (`docs/plan-live.md`, "Ordered and
//! ephemeral fields"), by static inspection, as [`super::reads`] says what a
//! query reads.
//!
//! A live state's record may mark fields ephemeral: a `PerDevice` each
//! device writes its own slot of — a pointer dragged across a queue, a
//! scrubber held under a finger. Every change to a state is a transition,
//! and the engine sorts them into two kinds from their bodies alone:
//!
//! - **ephemeral**: it reads nothing of the state, nothing of the context
//!   but `ctx.device`, and writes only the caller's own slot of ephemeral
//!   fields (`state.f[ctx.device]`). One writer per slot, so nothing needs
//!   ordering: the server relays it with no sequence and no hash, and the
//!   newest value from a device wins.
//! - **ordered**: every other one. The server sequences it per instance.
//!
//! And one rule binds the ordered kind: **it reads no ephemeral field**,
//! since devices may disagree about one at any instant and an ordered
//! transition must reach the same state on every replica. It may write the
//! caller's own slot — an ordered delivery is the stronger one — and pass
//! the rest through. Hooks are ordered.
//!
//! "Writes only" and "passes through" are read off the shape of what the
//! body returns, which is what a record built with `..state` emits: the
//! state itself, or a struct of every field of the state's record, each
//! `state.f` — or, for an ephemeral `f`, `set_slot(state.f, ctx.device, v)`
//! — through any `if` whose condition is allowed where it stands.

use std::collections::BTreeSet;

use super::{Block, Check, Expr, Stmt, Transition};
use crate::schema::Ty;
use crate::value::FieldName;

/// The two kinds of transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Sequenced per instance; the server's order is every replica's.
    Ordered,
    /// Relayed with no order: writes the caller's own ephemeral slots only.
    Ephemeral,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Ordered => "ordered",
            Kind::Ephemeral => "ephemeral",
        }
    }
}

/// The kind of a declared transition of a state whose resolved type is
/// `state` (ephemeral markers kept). A hook is always ordered.
pub fn classify(state: &Ty, t: &Transition) -> Kind {
    let eph = crate::types::ephemeral_fields(state);
    let Ty::Struct(fields) = state else { return Kind::Ordered };
    if eph.is_empty() || matches!(t.name.as_str(), super::ON_JOIN | super::ON_LEAVE | super::ON_RESTORE) {
        return Kind::Ordered;
    }
    let names: BTreeSet<&FieldName> = fields.keys().collect();
    let quiet = |e: &Expr| {
        !mentions(e, &|x| {
            matches!(x, Expr::LiveState | Expr::CtxUser | Expr::CtxSession | Expr::CtxNow | Expr::HasRole(_))
        })
    };
    let checks_quiet = t.input.iter().flat_map(|(_, f)| &f.checks).all(|c| match c {
        Check::Refine(e, _) => quiet(e),
        _ => true,
    });
    let mut wrote = false;
    if checks_quiet && t.guards.iter().all(|(e, _)| quiet(e)) && block_ephemeral(&t.body, &names, &eph, &quiet, &mut wrote) && wrote {
        Kind::Ephemeral
    } else {
        Kind::Ordered
    }
}

fn block_ephemeral(b: &Block, names: &BTreeSet<&FieldName>, eph: &BTreeSet<FieldName>, quiet: &dyn Fn(&Expr) -> bool, wrote: &mut bool) -> bool {
    b.iter().all(|s| match s {
        Stmt::Return(Some(e)) => returns_own_slots(e, names, eph, quiet, wrote),
        Stmt::If(c, a, e) => quiet(c) && block_ephemeral(a, names, eph, quiet, wrote) && block_ephemeral(e, names, eph, quiet, wrote),
        Stmt::For(_, xs, body) => quiet(xs) && block_ephemeral(body, names, eph, quiet, wrote),
        Stmt::Let(_, e) | Stmt::Refuse(e) => quiet(e),
        _ => false,
    })
}

// What an ephemeral transition may return: the state unchanged, or every
// field passed through but ephemeral ones written at the caller's slot,
// through any `if` that reads nothing it may not.
fn returns_own_slots(e: &Expr, names: &BTreeSet<&FieldName>, eph: &BTreeSet<FieldName>, quiet: &dyn Fn(&Expr) -> bool, wrote: &mut bool) -> bool {
    match e {
        Expr::LiveState => true,
        Expr::If(c, a, b) => quiet(c) && returns_own_slots(a, names, eph, quiet, wrote) && returns_own_slots(b, names, eph, quiet, wrote),
        Expr::Struct(fs) => {
            fs.keys().collect::<BTreeSet<_>>() == *names
                && fs.iter().all(|(k, v)| {
                    if passed_through(k, v) {
                        return true;
                    }
                    match own_slot_write(k, v) {
                        Some(x) if eph.contains(k) && quiet(x) => {
                            *wrote = true;
                            true
                        }
                        _ => false,
                    }
                })
        }
        _ => false,
    }
}

// `state.k`, as `k` itself.
fn passed_through(k: &str, v: &Expr) -> bool {
    matches!(v, Expr::Field(s, f) if **s == Expr::LiveState && f == k)
}

// `set_slot(state.k, ctx.device, x)`: the value written, `x`.
fn own_slot_write<'e>(k: &str, v: &'e Expr) -> Option<&'e Expr> {
    match v {
        Expr::SetSlot(m, key, x) if passed_through(k, m) && **key == Expr::CtxDevice => Some(x),
        _ => None,
    }
}

/// The first ephemeral field an ordered transition reads, by name, or
/// `"the whole state"` where it uses the state as a value; `None` where it
/// reads none. What the verifier refuses an ordered transition for.
pub fn ordered_reads_ephemeral(state: &Ty, t: &Transition) -> Option<String> {
    let eph = crate::types::ephemeral_fields(state);
    if eph.is_empty() {
        return None;
    }
    let names: BTreeSet<&FieldName> = match state {
        Ty::Struct(fs) => fs.keys().collect(),
        _ => return None,
    };
    for (_, f) in &t.input {
        for c in &f.checks {
            if let Check::Refine(e, _) = c {
                if let Some(r) = reads(e, &eph) {
                    return Some(r);
                }
            }
        }
    }
    for (e, _) in &t.guards {
        if let Some(r) = reads(e, &eph) {
            return Some(r);
        }
    }
    block_reads(&t.body, &names, &eph)
}

fn block_reads(b: &Block, names: &BTreeSet<&FieldName>, eph: &BTreeSet<FieldName>) -> Option<String> {
    b.iter().find_map(|s| match s {
        Stmt::Return(Some(e)) => returned_reads(e, names, eph),
        Stmt::If(c, a, e) => reads(c, eph)
            .or_else(|| block_reads(a, names, eph))
            .or_else(|| block_reads(e, names, eph)),
        Stmt::For(_, xs, body) => reads(xs, eph).or_else(|| block_reads(body, names, eph)),
        Stmt::Let(_, e) | Stmt::Refuse(e) => reads(e, eph),
        _ => None,
    })
}

// A returned value: the state itself is a pass-through; a struct of every
// field may pass an ephemeral field through or write the caller's slot of
// it; anything else is read as any expression is.
fn returned_reads(e: &Expr, names: &BTreeSet<&FieldName>, eph: &BTreeSet<FieldName>) -> Option<String> {
    match e {
        Expr::LiveState => None,
        Expr::If(c, a, b) => reads(c, eph)
            .or_else(|| returned_reads(a, names, eph))
            .or_else(|| returned_reads(b, names, eph)),
        Expr::Struct(fs) if fs.keys().collect::<BTreeSet<_>>() == *names => fs.iter().find_map(|(k, v)| {
            if eph.contains(k) {
                if passed_through(k, v) {
                    return None;
                }
                if let Some(x) = own_slot_write(k, v) {
                    return reads(x, eph);
                }
            }
            reads(v, eph)
        }),
        _ => reads(e, eph),
    }
}

// The first ephemeral field `e` reads: `state.f` for an ephemeral `f`, or the
// state used whole.
fn reads(e: &Expr, eph: &BTreeSet<FieldName>) -> Option<String> {
    match e {
        Expr::Field(s, f) if **s == Expr::LiveState => eph.contains(f).then(|| f.clone()),
        Expr::LiveState => Some("the whole state".into()),
        _ => crate::view::children(e).into_iter().find_map(|c| reads(c, eph)),
    }
}

// Whether any subexpression of `e` is one `hit` answers for.
fn mentions(e: &Expr, hit: &dyn Fn(&Expr) -> bool) -> bool {
    hit(e) || crate::view::children(e).into_iter().any(|c| mentions(c, hit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Field;

    fn state() -> Ty {
        let ptr = Ty::Ephemeral(Box::new(Ty::PerDevice(Box::new(Ty::Int))));
        Ty::Struct([("count".to_string(), Ty::Int), ("pointer".to_string(), ptr)].into())
    }

    fn field(f: &str) -> Expr {
        Expr::Field(Box::new(Expr::LiveState), f.into())
    }

    fn returning(e: Expr) -> Transition {
        Transition {
            name: "t".into(),
            input: vec![("at".into(), Field::plain(Ty::Int))],
            guards: vec![],
            body: vec![Stmt::Return(Some(e))],
            names: Default::default(),
        }
    }

    fn with(count: Expr, pointer: Expr) -> Expr {
        Expr::Struct([("count".to_string(), count), ("pointer".to_string(), pointer)].into())
    }

    fn set(key: Expr) -> Expr {
        Expr::SetSlot(
            Box::new(field("pointer")),
            Box::new(key),
            Box::new(Expr::Some(Box::new(Expr::Arg("at".into())))),
        )
    }

    /// The three cases the plan names, and the rule. Falsified by dropping
    /// the key's check in `own_slot_write`: writing another device's slot
    /// came out ephemeral.
    #[test]
    fn a_transition_is_ephemeral_only_when_it_writes_its_own_slot_and_reads_nothing() {
        let s = state();
        let point = returning(with(field("count"), set(Expr::CtxDevice)));
        assert_eq!(classify(&s, &point), Kind::Ephemeral, "its own slot, nothing read");
        let other = returning(with(field("count"), set(Expr::Lit(crate::value::Value::text("tv")))));
        assert_eq!(classify(&s, &other), Kind::Ordered, "another device's slot");
        let reading = returning(with(
            Expr::Op(super::super::Op::Add, vec![field("count"), Expr::Lit(crate::value::Value::Int(1))]),
            set(Expr::CtxDevice),
        ));
        assert_eq!(classify(&s, &reading), Kind::Ordered, "it reads the state");
        assert_eq!(
            ordered_reads_ephemeral(&s, &reading),
            None,
            "and reads nothing ephemeral, so it may be ordered"
        );
        let peeks = returning(with(
            Expr::Std(super::super::StdFn::Len, vec![Expr::Slots(Box::new(field("pointer")))]),
            field("pointer"),
        ));
        assert_eq!(classify(&s, &peeks), Kind::Ordered);
        assert_eq!(
            ordered_reads_ephemeral(&s, &peeks),
            Some("pointer".into()),
            "an ordered read of an ephemeral field"
        );
        let whole = Transition {
            body: vec![Stmt::Let(0, Expr::LiveState), Stmt::Return(Some(Expr::Var(0)))],
            ..returning(Expr::LiveState)
        };
        assert_eq!(ordered_reads_ephemeral(&s, &whole), Some("the whole state".into()));
    }
}
