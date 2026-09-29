//! A seeded generator of change sequences, and the contract of a maintained
//! view driven over it (`docs/plan-v4.md` §1.5, "The correctness contract").
//!
//! Shared by every crate that holds its queries to the contract: included
//! with `#[path]` from `rust/ark/tests/views.rs` (a fixture of every plan
//! feature), `rust/ark-client/src/view.rs` (the demo) and
//! `harken/domain/tests/views.rs` (harken's fifteen queries), so that one
//! generator and one set of assertions is what "maintained" means.
//!
//! The changes are raw rows, not mutations: a view has to be right about
//! any store, and raw rows reach what no mutator would write — a song whose
//! media is gone, a work whose composer is nobody, two groups merging. Each
//! change is applied to the store as it is drawn, so a batch is a
//! consistent sequence and the store after it is what the view is pushed
//! against. Values are drawn per column from its *join class* — the columns
//! a reference, a shared name or the plan's own `on`s and lookups tie it to
//! — so that most rows join something and some join nothing.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use ark::ir::{Expr, Plan, Source, Sym};
use ark::schema::{Schema, Ty};
use ark::store::{Change, MemoryStore, Row, Store};
use ark::value::{FieldName, TableName, Value};
use ark::view::{self, nodes, Env, View};

/// splitmix64: small, seeded, and the same everywhere.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// True `pct` times in a hundred.
    pub fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }

    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> Option<&'a T> {
        match xs.len() {
            0 => None,
            n => Some(&xs[self.below(n)]),
        }
    }
}

type Col = (TableName, FieldName);

/// Draws batches of changes for one plan over one schema.
pub struct Churn<'a> {
    sch: &'a Schema,
    /// The plan's source table and every table a node of it reads.
    read: Vec<TableName>,
    /// Every other table: a change there must cost the view nothing.
    other: Vec<TableName>,
    /// Each column's join class, by representative.
    class: BTreeMap<Col, Col>,
    rng: Rng,
    fresh: u64,
    /// The row last touched, which the next change prefers now and then, so
    /// that one batch adds, edits and removes the same row.
    last: Option<(TableName, Vec<Value>)>,
}

impl<'a> Churn<'a> {
    pub fn new(sch: &'a Schema, plan: &Plan, seed: u64) -> Churn<'a> {
        let mut read: Vec<TableName> = vec![plan.table().clone()];
        for (_, n) in nodes(plan) {
            if !read.contains(n.table()) {
                read.push(n.table().clone());
            }
        }
        let other = sch.tables().map(|t| t.name.clone()).filter(|t| !read.contains(t)).collect();
        let mut c = Churn {
            sch,
            read,
            other,
            class: BTreeMap::new(),
            rng: Rng::new(seed),
            fresh: 0,
            last: None,
        };
        // References and shared column names.
        let mut by_name: BTreeMap<FieldName, Col> = BTreeMap::new();
        for t in sch.tables() {
            for r in &t.refs {
                if let Some(p) = sch.lookup_table(&r.table) {
                    if p.key.len() == 1 {
                        c.union((t.name.clone(), r.column.clone()), (p.name.clone(), p.key[0].clone()));
                    }
                }
            }
            for col in &t.columns {
                let here = (t.name.clone(), col.name.clone());
                match by_name.get(&col.name) {
                    Some(first) => c.union(first.clone(), here),
                    None => {
                        by_name.insert(col.name.clone(), here);
                    }
                }
            }
        }
        // The plan's own joins, at every depth.
        c.plan_joins(plan);
        c
    }

    fn root(&self, x: &Col) -> Col {
        let mut x = x.clone();
        while let Some(p) = self.class.get(&x) {
            if *p == x {
                break;
            }
            x = p.clone();
        }
        x
    }

    fn union(&mut self, a: Col, b: Col) {
        let (ra, rb) = (self.root(&a), self.root(&b));
        if ra != rb {
            self.class.insert(ra, rb.clone());
            self.class.entry(rb.clone()).or_insert(rb);
        }
    }

    fn plan_joins(&mut self, p: &Plan) {
        // The columns a symbol's fields are columns of.
        let mut syms: BTreeMap<Sym, TableName> = BTreeMap::new();
        if let Some(r) = p.row {
            syms.insert(r, p.table().clone());
        }
        for l in &p.lookups {
            if let Some(t) = self.sch.lookup_table(&l.table) {
                for (k, e) in t.key.iter().zip(&l.key) {
                    if let Some(c) = column_of(e, &syms) {
                        self.union((l.table.clone(), k.clone()), c);
                    }
                }
            }
            syms.insert(l.sym, l.table.clone());
        }
        for r in &p.related {
            for (c, e) in &r.on {
                if let Some(pc) = column_of(e, &syms) {
                    self.union((r.plan.table().clone(), c.clone()), pc);
                }
            }
            self.plan_joins(&r.plan);
        }
    }

    // The values a column's class holds now.
    fn pool(&self, st: &MemoryStore, col: &Col) -> Vec<Value> {
        let root = self.root(col);
        let mut out: BTreeSet<Value> = BTreeSet::new();
        for t in self.sch.tables() {
            for c in &t.columns {
                let here = (t.name.clone(), c.name.clone());
                if here == *col || self.root(&here) == root {
                    for r in st.rows(&t.name).values() {
                        if let Some(v) = r.get(&c.name) {
                            if !v.is_null() {
                                out.insert(v.clone());
                            }
                        }
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    fn fresh(&mut self, ty: &Ty) -> Value {
        self.fresh += 1;
        let n = self.fresh;
        match ty {
            Ty::Bool => Value::Bool(self.rng.chance(50)),
            // Small, so that order keys tie and a limit's edge moves.
            Ty::Int => Value::int(self.rng.below(12) as i64 - 2),
            Ty::Text => Value::text(format!("x{n}")),
            Ty::Bytes => Value::Bytes(n.to_be_bytes().to_vec()),
            Ty::Id(_) => {
                let mut b = [0u8; 16];
                b[..8].copy_from_slice(&0xfeed_u64.to_be_bytes());
                b[8..].copy_from_slice(&n.to_be_bytes());
                Value::Id(b)
            }
            Ty::Enum(vs) => Value::text(self.rng.pick(vs).cloned().unwrap_or_default()),
            Ty::Option(t) => self.fresh(t),
            Ty::List(_) => Value::List(vec![]),
            Ty::Struct(_) => Value::Struct(BTreeMap::new()),
        }
    }

    // A value for a column: mostly one its class already holds, so that it
    // joins; sometimes a fresh one, so that it joins nothing; `Null` where
    // the column allows it.
    fn value(&mut self, st: &MemoryStore, t: &str, c: &str) -> Value {
        let col = self
            .sch
            .lookup_table(t)
            .and_then(|tb| tb.column(c))
            .cloned()
            .expect("a column of the schema");
        if col.nullable && self.rng.chance(12) {
            return Value::Null;
        }
        let pool = self.pool(st, &(t.to_string(), c.to_string()));
        if !pool.is_empty() && !matches!(col.ty, Ty::Bool) && self.rng.chance(80) {
            return self.rng.pick(&pool).cloned().unwrap_or(Value::Null);
        }
        self.fresh(&col.ty)
    }

    fn table(&mut self) -> TableName {
        if !self.other.is_empty() && self.rng.chance(8) {
            return self.rng.pick(&self.other).cloned().unwrap_or_default();
        }
        self.rng.pick(&self.read).cloned().unwrap_or_default()
    }

    // An existing row of a table: the last one touched now and then.
    fn row_of(&mut self, st: &MemoryStore, t: &str) -> Option<Row> {
        if let Some((lt, k)) = &self.last {
            if lt == t && self.rng.chance(35) {
                if let Some(r) = st.get(t, k) {
                    return Some(r);
                }
            }
        }
        let rows: Vec<Row> = st.rows(t).into_values().collect();
        self.rng.pick(&rows).cloned()
    }

    /// One change, drawn and applied: an add, an edit of one or two
    /// columns that are not the key, or a remove.
    pub fn change(&mut self, st: &mut MemoryStore) -> Option<Change> {
        let t = self.table();
        let tbl = self.sch.lookup_table(&t)?.clone();
        let ch = match self.rng.below(10) {
            // Add: a fresh key (or one drawn from its class, so that a
            // composite key joins), every column drawn.
            0..=3 => {
                let mut row = Row::new();
                for c in &tbl.columns {
                    row.insert(c.name.clone(), self.value(st, &t, &c.name));
                }
                for k in &tbl.key {
                    if row[k].is_null() || self.rng.chance(50) {
                        let ty = tbl.column(k)?.ty.clone();
                        let v = self.fresh(&ty);
                        row.insert(k.clone(), v);
                    }
                }
                if st.get(&t, &tbl.key_of(&row)).is_some() {
                    return None;
                }
                Change::Add(t.clone(), row)
            }
            // Edit.
            4..=7 => {
                let old = self.row_of(st, &t)?;
                let mut new = old.clone();
                let cols: Vec<FieldName> = tbl.columns.iter().map(|c| c.name.clone()).filter(|c| !tbl.key.contains(c)).collect();
                for _ in 0..1 + self.rng.below(2) {
                    if let Some(c) = self.rng.pick(&cols).cloned() {
                        let v = self.value(st, &t, &c);
                        new.insert(c, v);
                    }
                }
                if new == old {
                    return None;
                }
                Change::Edit(t.clone(), old, new)
            }
            _ => Change::Remove(t.clone(), self.row_of(st, &t)?),
        };
        let row = match &ch {
            Change::Add(_, r) | Change::Edit(_, _, r) | Change::Remove(_, r) => r,
        };
        self.last = Some((t, tbl.key_of(row)));
        st.apply_change(&ch);
        Some(ch)
    }

    /// One settle's worth: one to four changes, each applied as drawn.
    pub fn batch(&mut self, st: &mut MemoryStore) -> Vec<Change> {
        let n = 1 + self.rng.below(4);
        let mut out = vec![];
        let mut tries = 0;
        while out.len() < n && tries < 40 {
            tries += 1;
            if let Some(c) = self.change(st) {
                out.push(c);
            }
        }
        out
    }
}

// A column an expression reads, when it is one: `sym.column`, optionally
// under `Some`, or an option's `map` to a column.
fn column_of(e: &Expr, syms: &BTreeMap<Sym, TableName>) -> Option<Col> {
    match e {
        Expr::Some(x) => column_of(x, syms),
        Expr::Field(x, c) => match &**x {
            Expr::Var(s) => syms.get(s).map(|t| (t.clone(), c.clone())),
            _ => None,
        },
        Expr::Match(x, bound, some, _) => match (&**x, &**some) {
            (Expr::Var(s), body) => {
                let mut inner = syms.clone();
                inner.insert(*bound, syms.get(s)?.clone());
                column_of(body, &inner)
            }
            _ => None,
        },
        _ => None,
    }
}

/// What a run exercised, so that a test can say it reached each case the
/// contract is about rather than passing on an idle generator.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    pub steps: usize,
    pub batches: usize,
    pub inserts: usize,
    pub removes: usize,
    pub updates: usize,
    /// Changes to a table a node reads whose row some entry depended on…
    pub joined: usize,
    /// …and ones no entry depended on.
    pub unjoined: usize,
    /// Entries a having refused and then admitted, or the other way.
    pub flips: usize,
    /// Edits that moved a row from one group to another.
    pub moves: usize,
    /// Steps where an admitted entry crossed the limit's edge while staying
    /// a candidate: entered the window from beyond it, or was pushed out.
    pub window: usize,
}

/// Drive `steps` batches through a view of `plan` in `env` over `st`,
/// asserting after every one that the view is a fresh hydrate
/// ([`view::contract`]), that its answer is what [`view::read`] gives now,
/// and that the patches splice the answer before into the answer after.
/// A failure names the query, the seed, the step and the batch.
pub fn drive(name: &str, sch: &Schema, plan: &Plan, env: &Env, mut st: MemoryStore, seed: u64, steps: usize) -> Tally {
    let mut churn = Churn::new(sch, plan, seed);
    let mut v: View = view::hydrate(sch, plan, env.clone(), &st).unwrap_or_else(|e| panic!("{name}: hydrate: {e:?}"));
    let ns = nodes(plan);
    let lim = plan.limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX);
    let mut t = Tally::default();
    for i in 0..steps {
        let before = v.clone();
        let rows_before = v.rows();
        let batch = churn.batch(&mut st);
        let at = || format!("{name}, seed {seed}, step {i}: {batch:?}");
        let ps = view::push_all(sch, &st, &batch, &mut v).unwrap_or_else(|e| panic!("{}: {e:?}", at()));
        assert!(view::contract(sch, &st, &v), "{}: the view is not a fresh hydrate", at());
        let fresh = view::read(sch, plan, &env.scope(sch), &st).unwrap_or_else(|e| panic!("{}: read: {e:?}", at()));
        assert_eq!(v.rows(), fresh, "{}: the answer", at());
        assert_eq!(view::splice(&ps, &rows_before), fresh, "{}: the patches {ps:?}", at());

        t.steps += 1;
        t.batches += usize::from(batch.len() > 1);
        for p in &ps {
            match p {
                view::Patch::Insert { .. } => t.inserts += 1,
                view::Patch::Remove { .. } => t.removes += 1,
                view::Patch::Update { .. } => t.updates += 1,
            }
        }
        for ch in &batch {
            if ch.table() != plan.table().as_str() && ns.iter().any(|(_, n)| n.table() == ch.table()) {
                let rows: Vec<&Row> = match ch {
                    Change::Add(_, r) | Change::Remove(_, r) => vec![r],
                    Change::Edit(_, o, n) => vec![o, n],
                };
                let hit = ns
                    .iter()
                    .filter(|(_, n)| n.table() == ch.table())
                    .any(|(id, n)| rows.iter().any(|r| before.by_dep.contains_key(&(*id, n.dependency(sch, r)))));
                if hit {
                    t.joined += 1;
                } else {
                    t.unjoined += 1;
                }
            }
            if let (Source::Group { by, .. }, Change::Edit(tn, o, n)) = (&plan.source, ch) {
                let g = |r: &Row| by.iter().map(|c| r.get(c).cloned()).collect::<Vec<_>>();
                if tn == plan.table() && g(o) != g(n) {
                    t.moves += 1;
                }
            }
        }
        t.flips += before
            .by_key
            .iter()
            .filter(|(k, e)| v.by_key.get(*k).is_some_and(|e2| e2.admitted != e.admitted))
            .count();
        if lim != usize::MAX {
            let window = |w: &View| w.entries.iter().take(lim).cloned().collect::<BTreeSet<_>>();
            let beyond = |w: &View| w.entries.iter().skip(lim).cloned().collect::<BTreeSet<_>>();
            let (w0, w1, b0, b1) = (window(&before), window(&v), beyond(&before), beyond(&v));
            if w1.iter().any(|k| b0.contains(k)) || w0.iter().any(|k| b1.contains(k)) {
                t.window += 1;
            }
        }
    }
    t
}
