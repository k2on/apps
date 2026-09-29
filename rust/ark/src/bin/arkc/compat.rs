//! §17 Compatibility: what a module may become (`arkc check OLD NEW`).
//!
//! The log is permanent and every retained entry is replayed by every
//! whole peer, so a module is a promise to bytes already on disk, and a
//! proposed module is held to the one that made it. The answer is a list
//! of [`Break`]s; an empty list is what lets a build through
//! ([`is_additive`]).
//!
//! **The rule is additive-only, for a reason rather than by taste.** An
//! entry carries an intent — a function by hash, arguments by name, the
//! autos its peer drew — and nothing in it can be read without the
//! vocabulary it was written in. Adding to that vocabulary costs the log
//! nothing: a new table is empty at every retained sequence, a nullable
//! column reads as `None` on every older row, a new function has no
//! entries yet, an optional argument is absent from every old intent and
//! means `None` there. Taking from it always strands somebody: a mutator's
//! name is what a peer that has not updated still authors, a required
//! argument added to one is a value every pending intent lacks, a column
//! removed is a field a retained body reads and a row every retained entry
//! wrote. So a schema grows and never shrinks, a column is retired rather
//! than removed (`docs/arkdb.md` §3.2), and a change to a live column is a
//! rebuild from snapshot plus replay, never a check that passes. Helpers
//! and queries may go: no entry names either, and a helper a retained
//! mutator still reaches travels in its closure.
//!
//! **A retained body is type-checked, not diffed.** A recorded surface
//! says what the log may carry, never what `apply` will do, and cannot see
//! a schema change at all, because a column is not part of a verb's
//! signature. Entries name their function by hash and the authority keeps
//! every closure a retained entry names (§8.3, §11), so [`check_retained`]
//! asks the verifier — the judgement that admitted the function in the
//! first place — again, against the proposed schema, and reports what it
//! says under the hash the entries carry.
//!
//! [`check`] is the half a text diff could do and the verifier cannot: the
//! schema table by table, and every mutator's name, arguments and autos.
//! Argument *order* is not compared (arguments travel keyed by name), and
//! an auto added to a mutator is not a break, because autos are drawn by
//! whichever runtime holds the function, so there is nobody to strand. An
//! id argument that names another table is [`Break::ArgRetyped`]: the
//! bytes are the same sixteen either way, which is exactly why it has to be
//! caught here.
//!
//! Both modules are assumed to verify on their own; this compares, it does
//! not re-admit.

// The library's surface; `arkc` calls `check` alone. It moves to
// `rust/ark/src/compat.rs`, where nothing is dead, once the library can
// take it.
#![allow(dead_code)]

use ark::hash::{function_hash, Closure, FnHash};
use ark::ir::{FnKind, Function, Module, Router};
use ark::schema::{Schema, Table, Ty};
use ark::value::{FieldName, TableName};
use ark::verify::{verify_function, VerifyError};

/// A change the log cannot survive. Each names what it is about, because
/// the list is read by somebody who has just made the change and needs to
/// know which of their edits was the problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Break {
    /// A mutator present before and absent now (or no longer a mutator).
    /// Entries naming it may be pending on a peer that has not updated.
    FunctionRemoved(String),
    /// Mutator, argument. Old intents carry the value; nothing takes it.
    ArgRemoved(String, String),
    /// Mutator, argument, was, now. The bytes in the log were written as
    /// one thing and would be read as another; an id naming another table
    /// is this too.
    ArgRetyped(String, String, Ty, Ty),
    /// Mutator, argument. A new argument that is not an option: every
    /// existing caller and every pending intent lacks it.
    ArgAdded(String, String),
    /// Mutator, auto. Old entries froze a value nothing reads.
    AutoRemoved(String, String),
    /// Mutator, auto. A `Now` that became a `NewId`, or an id of another
    /// table: the frozen value would be read as something it is not.
    AutoChanged(String, String),
    /// Every retained entry that wrote to it is stranded, and the state
    /// hash at every retained sequence moves.
    TableRemoved(TableName),
    /// Table, column. Retired, never removed: a retained body reads it and
    /// every retained row has it.
    ColumnRemoved(TableName, FieldName),
    /// Table, column, was, now. Rows already written hold the old type.
    ColumnRetyped(TableName, FieldName, Ty, Ty),
    /// Table, column. Every retained row holding `None` there is refused.
    /// The other direction is fine.
    ColumnMadeNonNullable(TableName, FieldName),
    /// Table, column. A new column with no default: every retained put
    /// writes a full row without it. Add it nullable.
    ColumnAddedNonNullable(TableName, FieldName),
    /// Rows are stored, deleted and referenced by key; a retained delete
    /// names the old one.
    KeyChanged(TableName),
    /// Table, column. A reference added, removed or re-pointed on a column
    /// that existed: a new constraint may refuse rows retained entries
    /// wrote, and a moved one changes what those rows mean.
    RefChanged(TableName, FieldName),
    /// Table, columns. A new unique index is a new refusal, and retained
    /// rows may already collide under it. A non-unique index may be added
    /// freely.
    UniqueAdded(TableName, Vec<FieldName>),
    /// A closure a retained entry names no longer verifies against the
    /// proposed schema, with the verifier's own complaints.
    RetainedBodyBroken(FnHash, Vec<VerifyError>),
}

/// §17.1 Every way `new` breaks the promise `old` made: the schema first,
/// then the mutators. Empty is additive.
pub fn check(old: &Module, new: &Module) -> Vec<Break> {
    let mut out = schema_breaks(&old.schema, &new.schema);
    out.extend(function_breaks(old, new));
    out
}

/// `check(old, new).is_empty()`.
pub fn is_additive(old: &Module, new: &Module) -> bool {
    check(old, new).is_empty()
}

/// §17.2 Re-verify every retained closure against `new`'s schema. The
/// closure is complete — its function and every helper and middleware it
/// reaches, in declaration order — so the module it is checked in is those
/// followed by the function, checked at its own index. A router is a
/// grouping, not a meaning, and may have been renamed since the closure
/// was hashed: where `new` lacks the closure's router, one is supplied that
/// allows exactly the middleware the function runs, so that what is judged
/// is the body against the schema.
pub fn check_retained(new: &Module, retained: &[Closure]) -> Vec<Break> {
    retained
        .iter()
        .filter_map(|c| {
            let f = &c.function;
            let mut functions = c.helpers.clone();
            functions.push(f.clone());
            let mut routers = new.routers.clone();
            if let Some(r) = &f.router {
                if new.lookup_router(r).is_none() {
                    routers.push(Router {
                        name: r.clone(),
                        uses: f.uses.clone(),
                    });
                }
            }
            let m = Module {
                functions,
                routers,
                ..new.clone()
            };
            verify_function(&m, c.helpers.len(), f)
                .err()
                .map(|cs| Break::RetainedBodyBroken(function_hash(c), cs.into_iter().map(|x| VerifyError::In(f.name.clone(), x)).collect()))
        })
        .collect()
}

// Schema ------------------------------------------------------------------

fn schema_breaks(old: &Schema, new: &Schema) -> Vec<Break> {
    let mut out = Vec::new();
    for t in old.tables() {
        let Some(t2) = new.lookup_table(&t.name) else {
            out.push(Break::TableRemoved(t.name.clone()));
            continue;
        };
        for c in &t.columns {
            let Some(c2) = t2.column(&c.name) else {
                out.push(Break::ColumnRemoved(t.name.clone(), c.name.clone()));
                continue;
            };
            if c.ty != c2.ty {
                out.push(Break::ColumnRetyped(t.name.clone(), c.name.clone(), c.ty.clone(), c2.ty.clone()));
            }
            if c.nullable && !c2.nullable {
                out.push(Break::ColumnMadeNonNullable(t.name.clone(), c.name.clone()));
            }
            if ref_on(t, &c.name) != ref_on(t2, &c.name) {
                out.push(Break::RefChanged(t.name.clone(), c.name.clone()));
            }
        }
        for c2 in &t2.columns {
            if !c2.nullable && t.column(&c2.name).is_none() {
                out.push(Break::ColumnAddedNonNullable(t.name.clone(), c2.name.clone()));
            }
        }
        if t.key != t2.key {
            out.push(Break::KeyChanged(t.name.clone()));
        }
        for ix in &t2.indexes {
            if ix.unique && !t.indexes.contains(ix) {
                out.push(Break::UniqueAdded(t.name.clone(), ix.columns.clone()));
            }
        }
    }
    out
}

/// The table a column references, if any.
fn ref_on<'a>(t: &'a Table, column: &str) -> Option<&'a str> {
    t.refs.iter().find(|r| r.column == column).map(|r| r.table.as_str())
}

// Functions ----------------------------------------------------------------

fn function_breaks(old: &Module, new: &Module) -> Vec<Break> {
    let mut out = Vec::new();
    for f in old.functions.iter().filter(|f| f.kind == FnKind::Mutator) {
        match new.lookup_function(&f.name) {
            Some(f2) if f2.kind == FnKind::Mutator => out.extend(same(f, f2)),
            _ => out.push(Break::FunctionRemoved(f.name.clone())),
        }
    }
    out
}

fn same(f: &Function, f2: &Function) -> Vec<Break> {
    let n = &f.name;
    let (args, args2) = (f.arg_types(), f2.arg_types());
    let lookup = |xs: &[(String, Ty)], a: &str| xs.iter().find(|(k, _)| k == a).map(|(_, t)| t.clone());
    let mut out = Vec::new();
    for (a, t) in &args {
        match lookup(&args2, a) {
            None => out.push(Break::ArgRemoved(n.clone(), a.clone())),
            Some(t2) if t2 != *t => out.push(Break::ArgRetyped(n.clone(), a.clone(), t.clone(), t2)),
            Some(_) => {}
        }
    }
    for (a, t) in &args2 {
        if lookup(&args, a).is_none() && !matches!(t, Ty::Option(_)) {
            out.push(Break::ArgAdded(n.clone(), a.clone()));
        }
    }
    for (a, u) in &f.autos {
        match f2.autos.iter().find(|(k, _)| k == a) {
            None => out.push(Break::AutoRemoved(n.clone(), a.clone())),
            Some((_, u2)) if u2 != u => out.push(Break::AutoChanged(n.clone(), a.clone())),
            Some(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! One test per [`Break`], each over a small module and one edit of it.
    //! Every test was falsified once by breaking the rule it holds; the
    //! doc comment on each says how.

    use std::collections::BTreeMap;

    use ark::hash::closure;
    use ark::ir::{Auto, Expr, Field, Stmt, SPEC_VERSION};
    use ark::schema::{Column, Index, Ref};

    use super::*;

    fn col(name: &str, ty: Ty, nullable: bool) -> Column {
        Column {
            name: name.into(),
            ty,
            nullable,
        }
    }

    /// `list(id, name, note?)` and `item(list_id → list, pos, label)`, keyed
    /// `(list_id, pos)`; the mutator `add(list_id, pos)` with an auto `at`,
    /// which writes an item, and the query `lists`.
    fn base() -> Module {
        let list = Table {
            name: "list".into(),
            columns: vec![
                col("id", Ty::Id("list".into()), false),
                col("name", Ty::Text, false),
                col("note", Ty::Text, true),
            ],
            key: vec!["id".into()],
            indexes: vec![],
            refs: vec![],
        };
        let item = Table {
            name: "item".into(),
            columns: vec![
                col("list_id", Ty::Id("list".into()), false),
                col("pos", Ty::Int, false),
                col("label", Ty::Text, true),
            ],
            key: vec!["list_id".into(), "pos".into()],
            indexes: vec![Index {
                columns: vec!["label".into()],
                unique: false,
            }],
            refs: vec![Ref {
                column: "list_id".into(),
                table: "list".into(),
            }],
        };
        let row = Expr::Struct(BTreeMap::from([
            ("list_id".to_string(), Expr::Arg("list_id".into())),
            ("pos".to_string(), Expr::Arg("pos".into())),
            ("label".to_string(), Expr::None(Ty::Text)),
        ]));
        let add = Function {
            name: "add".into(),
            kind: FnKind::Mutator,
            router: Some("r".into()),
            uses: vec![],
            autos: vec![("at".into(), Auto::Now)],
            input: vec![
                ("list_id".into(), Field::plain(Ty::Id("list".into()))),
                ("pos".into(), Field::plain(Ty::Int)),
            ],
            refine: vec![],
            ret: None,
            body: vec![Stmt::Insert("item".into(), row, vec![])],
            names: BTreeMap::new(),
        };
        let lists = Function {
            name: "lists".into(),
            kind: FnKind::Query,
            autos: vec![],
            input: vec![],
            ret: Some(Ty::Int),
            body: vec![Stmt::Return(Some(Expr::Lit(ark::value::Value::Int(0))))],
            ..add.clone()
        };
        Module {
            spec: SPEC_VERSION,
            schema: Schema { tables: vec![list, item] },
            functions: vec![add, lists],
            routers: vec![Router {
                name: "r".into(),
                uses: vec![],
            }],
            live: vec![],
        }
    }

    fn table<'a>(m: &'a mut Module, name: &str) -> &'a mut Table {
        m.schema.tables.iter_mut().find(|t| t.name == name).expect("a table of the fixture")
    }

    fn add(m: &mut Module) -> &mut Function {
        m.functions.iter_mut().find(|f| f.name == "add").expect("add")
    }

    /// The fixture is a module the verifier admits, and adding to it — a
    /// table, a nullable column, an optional argument, an auto, a function,
    /// a non-unique index — breaks nothing. Falsified by treating a
    /// nullable new column as a break, which fails the second assertion.
    #[test]
    fn growing_is_additive() {
        let old = base();
        ark::verify::verify(&old).expect("the fixture verifies");
        assert!(is_additive(&old, &old));
        let mut new = base();
        new.schema.tables.push(Table {
            name: "tag".into(),
            columns: vec![col("name", Ty::Text, false)],
            key: vec!["name".into()],
            indexes: vec![],
            refs: vec![],
        });
        table(&mut new, "list").columns.push(col("colour", Ty::Text, true));
        table(&mut new, "item").indexes.push(Index {
            columns: vec!["pos".into()],
            unique: false,
        });
        add(&mut new).input.push(("why".into(), Field::plain(Ty::Option(Box::new(Ty::Text)))));
        add(&mut new).autos.push(("id".into(), Auto::NewId("list".into())));
        new.functions.retain(|f| f.name != "lists"); // a query may go
        assert_eq!(check(&old, &new), vec![]);
    }

    /// Falsified by looking a mutator up in the old module instead of the
    /// new, which finds it every time.
    #[test]
    fn a_mutator_removed_or_made_a_query() {
        let mut new = base();
        new.functions.retain(|f| f.name != "add");
        assert_eq!(check(&base(), &new), vec![Break::FunctionRemoved("add".into())]);
        let mut new = base();
        add(&mut new).kind = FnKind::Query;
        assert_eq!(check(&base(), &new), vec![Break::FunctionRemoved("add".into())]);
    }

    /// Falsified by skipping arguments the new mutator lacks.
    #[test]
    fn an_argument_removed() {
        let mut new = base();
        add(&mut new).input.retain(|(n, _)| n != "pos");
        assert_eq!(check(&base(), &new), vec![Break::ArgRemoved("add".into(), "pos".into())]);
    }

    /// An id naming another table is a retype though the bytes are the
    /// same sixteen. Falsified by comparing the types' shapes (`Id(_)`)
    /// rather than the types, which lets the second edit through.
    #[test]
    fn an_argument_retyped() {
        let mut new = base();
        add(&mut new).input[1].1 = Field::plain(Ty::Text);
        assert_eq!(
            check(&base(), &new),
            vec![Break::ArgRetyped("add".into(), "pos".into(), Ty::Int, Ty::Text)]
        );
        let mut new = base();
        add(&mut new).input[0].1 = Field::plain(Ty::Id("item".into()));
        assert_eq!(
            check(&base(), &new),
            vec![Break::ArgRetyped(
                "add".into(),
                "list_id".into(),
                Ty::Id("list".into()),
                Ty::Id("item".into())
            )]
        );
    }

    /// A required argument is a break and an optional one is not.
    /// Falsified by dropping the `Option` exemption, which reports `maybe`.
    #[test]
    fn a_required_argument_added() {
        let mut new = base();
        add(&mut new).input.push(("why".into(), Field::plain(Ty::Text)));
        add(&mut new).input.push(("maybe".into(), Field::plain(Ty::Option(Box::new(Ty::Text)))));
        assert_eq!(check(&base(), &new), vec![Break::ArgAdded("add".into(), "why".into())]);
    }

    /// Falsified by skipping autos the new mutator lacks.
    #[test]
    fn an_auto_removed() {
        let mut new = base();
        add(&mut new).autos.clear();
        assert_eq!(check(&base(), &new), vec![Break::AutoRemoved("add".into(), "at".into())]);
    }

    /// Falsified by comparing autos by name only.
    #[test]
    fn an_auto_changed() {
        let mut new = base();
        add(&mut new).autos[0].1 = Auto::NewId("list".into());
        assert_eq!(check(&base(), &new), vec![Break::AutoChanged("add".into(), "at".into())]);
    }

    /// Falsified by skipping tables the new schema lacks.
    #[test]
    fn a_table_removed() {
        let mut new = base();
        new.schema.tables.retain(|t| t.name != "item");
        assert_eq!(check(&base(), &new), vec![Break::TableRemoved("item".into())]);
    }

    /// Falsified by skipping columns the new table lacks.
    #[test]
    fn a_column_removed() {
        let mut new = base();
        table(&mut new, "list").columns.retain(|c| c.name != "note");
        assert_eq!(check(&base(), &new), vec![Break::ColumnRemoved("list".into(), "note".into())]);
    }

    /// Falsified by comparing column names only.
    #[test]
    fn a_column_retyped() {
        let mut new = base();
        table(&mut new, "list").columns[1].ty = Ty::Bytes;
        assert_eq!(
            check(&base(), &new),
            vec![Break::ColumnRetyped("list".into(), "name".into(), Ty::Text, Ty::Bytes)]
        );
    }

    /// Required to nullable is fine; nullable to required is not.
    /// Falsified by reporting any change of nullability, which fails the
    /// first assertion.
    #[test]
    fn a_column_made_non_nullable() {
        let mut new = base();
        table(&mut new, "list").columns[1].nullable = true;
        assert_eq!(check(&base(), &new), vec![]);
        let mut new = base();
        table(&mut new, "list").columns[2].nullable = false;
        assert_eq!(check(&base(), &new), vec![Break::ColumnMadeNonNullable("list".into(), "note".into())]);
    }

    /// Falsified by reading the new column's nullability the wrong way
    /// round.
    #[test]
    fn a_required_column_added() {
        let mut new = base();
        table(&mut new, "list").columns.push(col("colour", Ty::Text, false));
        assert_eq!(check(&base(), &new), vec![Break::ColumnAddedNonNullable("list".into(), "colour".into())]);
    }

    /// Falsified by comparing the key's length rather than its columns.
    #[test]
    fn a_key_changed() {
        let mut new = base();
        table(&mut new, "item").key = vec!["pos".into(), "list_id".into()];
        assert_eq!(check(&base(), &new), vec![Break::KeyChanged("item".into())]);
    }

    /// A reference re-pointed and one dropped, on a column that existed.
    /// Falsified by comparing whether a column has a reference rather than
    /// what it names, which misses the first edit.
    #[test]
    fn a_reference_changed() {
        let mut new = base();
        table(&mut new, "item").refs[0].table = "item".into();
        assert_eq!(check(&base(), &new), vec![Break::RefChanged("item".into(), "list_id".into())]);
        let mut new = base();
        table(&mut new, "item").refs.clear();
        assert_eq!(check(&base(), &new), vec![Break::RefChanged("item".into(), "list_id".into())]);
    }

    /// A unique index is a new refusal; the same columns made unique count
    /// too. Falsified by comparing indexes by their columns alone, which
    /// misses the second edit.
    #[test]
    fn a_unique_index_added() {
        let mut new = base();
        table(&mut new, "list").indexes.push(Index {
            columns: vec!["name".into()],
            unique: true,
        });
        assert_eq!(check(&base(), &new), vec![Break::UniqueAdded("list".into(), vec!["name".into()])]);
        let mut new = base();
        table(&mut new, "item").indexes[0].unique = true;
        assert_eq!(check(&base(), &new), vec![Break::UniqueAdded("item".into(), vec!["label".into()])]);
    }

    /// A retained closure writing a column the proposal retyped no longer
    /// verifies, and is named by the hash the entries carry; the same
    /// closure against its own module, and against a module whose router
    /// was renamed, still does. Falsified by returning nothing from
    /// `check_retained`, and separately by leaving out the supplied router,
    /// which reports `UnknownRouter` for the renamed one.
    #[test]
    fn a_retained_body_broken() {
        let old = base();
        let c = closure(&old, old.lookup_function("add").unwrap());
        assert_eq!(check_retained(&old, std::slice::from_ref(&c)), vec![]);
        let mut renamed = base();
        renamed.routers[0].name = "s".into();
        add(&mut renamed).router = Some("s".into());
        assert_eq!(check_retained(&renamed, std::slice::from_ref(&c)), vec![]);
        let mut new = base();
        table(&mut new, "item").columns[1].ty = Ty::Text;
        match check_retained(&new, std::slice::from_ref(&c)).as_slice() {
            [Break::RetainedBodyBroken(h, es)] => {
                assert_eq!(*h, function_hash(&c));
                assert!(
                    !es.is_empty() && es.iter().all(|e| matches!(e, VerifyError::In(n, _) if n == "add")),
                    "{es:?}"
                );
            }
            other => panic!("{other:?}"),
        }
    }
}
