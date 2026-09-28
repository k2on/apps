//! §2.3 and §2.5 Tables: a row is a struct with its columns said once in
//! [`Row::columns`], the module's tables are a struct of [`Table`]s, and `db.<table>` is
//! how a body reads and writes one.

use std::marker::PhantomData;

use crate::eval::{self, EvalError, EvalFault};
use crate::ir::{CmpOp, Expr, Plan, Pred as IrPred, Related, Stmt};
use crate::schema::{Column, Dir, Index, Ref, Relation, Table as IrTable, Ty};
use crate::store::{self, Change, Refusal, Store};
use crate::value::Value;

use super::cx::{self, H};
use super::raw;
use super::values::{Bool, Data, List, Opt};

/// The module's tables: a struct of the [`Table`]s it holds. `open`
/// builds it with a [`table`] per field; the order the fields are written
/// there is the schema's order. Every router of a module is over the same
/// one.
pub trait Tables: Sized + 'static {
    fn open() -> Self;
}

thread_local! {
    static RECORDING: std::cell::RefCell<Option<Vec<IrTable>>> = const { std::cell::RefCell::new(None) };
}

/// A table of the module, in [`Tables::open`]: `playlist: table()`.
pub fn table<T: Row>() -> Table<T> {
    RECORDING.with(|r| {
        if let Some(ts) = r.borrow_mut().as_mut() {
            ts.push(table_of::<T>());
        }
    });
    Table { _t: PhantomData }
}

/// The tables, in the order `open` writes them.
pub(crate) fn tables_of<S: Tables>() -> Vec<IrTable> {
    let prev = RECORDING.with(|r| r.borrow_mut().replace(Vec::new()));
    let _ = S::open();
    RECORDING.with(|r| std::mem::replace(&mut *r.borrow_mut(), prev)).unwrap_or_default()
}

/// A row of a table: the struct, its table's name, its key's shape, and its
/// columns in field order.
pub trait Row: Sized + 'static {
    const NAME: &'static str;
    /// The key, as a tuple of the key columns' value types.
    type Key: Key;
    fn columns() -> Columns<Self>;
}

/// The table a row type declares, as the schema carries it.
pub fn table_of<T: Row>() -> IrTable {
    T::columns().table(T::NAME)
}

/// A struct of the vocabulary's values that is not necessarily a row of a
/// table: what a query or a helper builds and returns (`TStruct`). Its
/// fields are said once, in [`Record::fields`], in declaration order —
/// which, for a record, is alphabetical in the canonical form, because the
/// IR's struct type is a map and cannot carry another order. Every row is
/// a record ([`Row::columns`] says its fields).
///
/// ```ignore
/// pub struct AlbumsEntry {
///     pub art: Text,
///     pub name: Text,
///     pub tracks: Int,
/// }
/// impl Record for AlbumsEntry {
///     fn fields() -> Fields<Self> {
///         fields().field("art", text()).field("name", text()).field("tracks", int())
///     }
/// }
/// ```
pub trait Record: Sized + 'static {
    fn fields() -> Fields<Self>;
}

/// A record's fields, by name and type, in declaration order.
pub struct Fields<R> {
    pub(crate) fields: Vec<(String, Ty)>,
    _r: PhantomData<fn() -> R>,
}

/// A record with no fields yet.
pub fn fields<R>() -> Fields<R> {
    Fields {
        fields: vec![],
        _r: PhantomData,
    }
}

impl<R> Fields<R> {
    /// The next field, of the type the field builder names (`text()`,
    /// `int()`, `opt(..)`, `id::<T>()`, …); a record's fields carry no checks.
    ///
    /// # Panics
    ///
    /// When the builder carries a check: a check belongs to an input.
    pub fn field<V: Data>(mut self, name: &str, f: super::input::FieldB<V>) -> Self {
        assert!(f.checks.is_empty(), "{name}: a record's field carries no checks");
        self.fields.push((name.into(), f.ty));
        self
    }
}

impl<T: Row> Record for T {
    fn fields() -> Fields<Self> {
        let cols = T::columns();
        Fields {
            fields: cols
                .cols
                .into_iter()
                .map(|c| {
                    let ty = c.column_ty();
                    (c.name, ty)
                })
                .collect(),
            _r: PhantomData,
        }
    }
}

fn record_names<T: Record>() -> Vec<String> {
    T::fields().fields.into_iter().map(|(n, _)| n).collect()
}

fn record_what<T: Record>() -> &'static str {
    std::any::type_name::<T>()
}

impl<T: Record> Data for T {
    fn ty() -> Ty {
        Ty::Struct(T::fields().fields.into_iter().collect())
    }
    fn from_h(h: H) -> Self {
        let names = record_names::<T>();
        let hs: Vec<H> = if cx::emitting() {
            let base = cx::expr(h);
            names.iter().map(|n| cx::e(Expr::Field(Box::new(base.clone()), n.clone()))).collect()
        } else {
            let v = cx::value(h);
            names
                .iter()
                .map(|n| {
                    cx::lit(match &v {
                        Value::Struct(m) => m.get(n).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    })
                })
                .collect()
        };
        cx::remember(&hs, h);
        raw::assemble(&hs, record_what::<T>())
    }
    fn to_h(&self) -> H {
        let names = record_names::<T>();
        let hs = raw::disassemble(self, names.len(), record_what::<T>());
        if let Some(h) = cx::origin(&hs) {
            return h;
        }
        if cx::emitting() {
            cx::e(Expr::Struct(names.into_iter().zip(hs).map(|(n, h)| (n, cx::expr(h))).collect()))
        } else {
            cx::lit(Value::Struct(names.into_iter().zip(hs).map(|(n, h)| (n, cx::value(h))).collect()))
        }
    }
}

// Columns ---------------------------------------------------------------------

/// A column of a row type, by name: `pub const id: Col<Self, Id<Self>> =
/// col("id")`. In a predicate it compares; in an order it sorts.
pub struct Col<T, V> {
    name: &'static str,
    _t: PhantomData<fn() -> (T, V)>,
}

impl<T, V> Clone for Col<T, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, V> Copy for Col<T, V> {}

/// A column, by name.
pub const fn col<T, V>(name: &'static str) -> Col<T, V> {
    Col { name, _t: PhantomData }
}

impl<T, V> Col<T, V> {
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl<T: Row, V: Data> Col<T, V> {
    fn cmp(self, op: CmpOp, x: V) -> Pred<T> {
        Pred::new(IrPred::Cmp(self.name.into(), op, rhs(x.to_h())))
    }
    /// `PCmp c Eq x`.
    pub fn eq(self, x: impl Into<V>) -> Pred<T> {
        self.cmp(CmpOp::Eq, x.into())
    }
    /// `PCmp c Ne x`.
    pub fn ne(self, x: impl Into<V>) -> Pred<T> {
        self.cmp(CmpOp::Ne, x.into())
    }
    /// `PCmp c Lt x`.
    pub fn lt(self, x: impl Into<V>) -> Pred<T> {
        self.cmp(CmpOp::Lt, x.into())
    }
    /// `PCmp c Le x`.
    pub fn le(self, x: impl Into<V>) -> Pred<T> {
        self.cmp(CmpOp::Le, x.into())
    }
    /// `PCmp c Gt x`.
    pub fn gt(self, x: impl Into<V>) -> Pred<T> {
        self.cmp(CmpOp::Gt, x.into())
    }
    /// `PCmp c Ge x`.
    pub fn ge(self, x: impl Into<V>) -> Pred<T> {
        self.cmp(CmpOp::Ge, x.into())
    }
    /// `PIn c [x…]`.
    pub fn in_<X: Into<V>>(self, xs: impl IntoIterator<Item = X>) -> Pred<T> {
        Pred::new(IrPred::In(self.name.into(), xs.into_iter().map(|x| rhs(x.into().to_h())).collect()))
    }
    /// Ascending, in an order.
    pub fn asc(self) -> Order<T> {
        Order {
            by: (self.name.into(), Dir::Asc),
            _t: PhantomData,
        }
    }
    /// Descending, in an order.
    pub fn desc(self) -> Order<T> {
        Order {
            by: (self.name.into(), Dir::Desc),
            _t: PhantomData,
        }
    }
}

// A right-hand side: the expression, or the value as a literal.
fn rhs(h: H) -> Expr {
    if cx::emitting() {
        cx::expr(h)
    } else {
        Expr::Lit(cx::value(h))
    }
}

/// A relationship read beneath a row: `pub const playlist_item:
/// Rel<Self, PlaylistItem> = rel("playlist_item")`, named after the child.
pub struct Rel<T, C> {
    name: &'static str,
    _t: PhantomData<fn() -> (T, C)>,
}

impl<T, C> Clone for Rel<T, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, C> Copy for Rel<T, C> {}

/// A relationship, by name.
pub const fn rel<T, C>(name: &'static str) -> Rel<T, C> {
    Rel { name, _t: PhantomData }
}

/// The value types a column of each kind may hold: the type, and the type
/// through an option (a nullable column).
pub trait ColumnOf<V> {}
impl ColumnOf<super::values::Text> for super::values::Text {}
impl ColumnOf<super::values::Text> for Opt<super::values::Text> {}
impl ColumnOf<super::values::Int> for super::values::Int {}
impl ColumnOf<super::values::Int> for Opt<super::values::Int> {}
impl ColumnOf<super::values::Bool> for super::values::Bool {}
impl ColumnOf<super::values::Bool> for Opt<super::values::Bool> {}
impl ColumnOf<super::values::Bytes> for super::values::Bytes {}
impl ColumnOf<super::values::Bytes> for Opt<super::values::Bytes> {}
impl<X: Row> ColumnOf<super::values::Id<X>> for super::values::Id<X> {}
impl<X: Row> ColumnOf<super::values::Id<X>> for Opt<super::values::Id<X>> {}

/// A row's columns, as [`Row::columns`] builds them: each column in field
/// order, `.nullable()` and `.refs::<P>()` applying to the one just added,
/// then the key and the indexes.
pub struct Columns<T> {
    cols: Vec<Column>,
    /// Whether each column's value type is an option, to hold `.nullable()`
    /// to it.
    optional: Vec<bool>,
    key: Vec<String>,
    indexes: Vec<Index>,
    refs: Vec<Ref>,
    _t: PhantomData<fn() -> T>,
}

/// An empty column list.
pub fn columns<T>() -> Columns<T> {
    Columns {
        cols: vec![],
        optional: vec![],
        key: vec![],
        indexes: vec![],
        refs: vec![],
        _t: PhantomData,
    }
}

impl<T> Columns<T> {
    fn add<V: Data>(mut self, c: &'static str, ty: Ty) -> Self {
        self.cols.push(Column {
            name: c.into(),
            ty,
            nullable: false,
        });
        self.optional.push(matches!(V::ty(), Ty::Option(_)));
        self
    }

    /// A text column.
    pub fn text<V: Data + ColumnOf<super::values::Text>>(self, c: Col<T, V>) -> Self {
        self.add::<V>(c.name, Ty::Text)
    }
    /// An int column.
    pub fn int<V: Data + ColumnOf<super::values::Int>>(self, c: Col<T, V>) -> Self {
        self.add::<V>(c.name, Ty::Int)
    }
    /// A bool column.
    pub fn bool<V: Data + ColumnOf<super::values::Bool>>(self, c: Col<T, V>) -> Self {
        self.add::<V>(c.name, Ty::Bool)
    }
    /// A bytes column.
    pub fn bytes<V: Data + ColumnOf<super::values::Bytes>>(self, c: Col<T, V>) -> Self {
        self.add::<V>(c.name, Ty::Bytes)
    }
    /// An id column, naming the table its type names.
    pub fn id<X: Row, V: Data + ColumnOf<super::values::Id<X>>>(self, c: Col<T, V>) -> Self {
        self.add::<V>(c.name, Ty::Id(X::NAME.into()))
    }
    /// An enum column: text restricted to the variants.
    pub fn enum_<V: Data + ColumnOf<super::values::Text>, S: AsRef<str>>(self, c: Col<T, V>, variants: impl IntoIterator<Item = S>) -> Self {
        let vs = variants.into_iter().map(|s| s.as_ref().to_string()).collect();
        self.add::<V>(c.name, Ty::Enum(vs))
    }
    /// The column just added holds `None` too.
    pub fn nullable(mut self) -> Self {
        let c = self.cols.last_mut().expect(".nullable() after a column");
        c.nullable = true;
        self
    }
    /// The id column just added references `P`'s key.
    pub fn refs<P: Row>(mut self) -> Self {
        let c = self.cols.last().expect(".refs() after a column");
        self.refs.push(Ref {
            column: c.name.clone(),
            table: P::NAME.into(),
        });
        self
    }
    /// The key.
    pub fn key(mut self, cols: impl Cols<T>) -> Self {
        self.key = cols.names();
        self
    }
    /// A unique index.
    pub fn unique(mut self, cols: impl Cols<T>) -> Self {
        self.indexes.push(Index {
            columns: cols.names(),
            unique: true,
        });
        self
    }
    /// An index that is only a statement about performance.
    pub fn index(mut self, cols: impl Cols<T>) -> Self {
        self.indexes.push(Index {
            columns: cols.names(),
            unique: false,
        });
        self
    }

    /// The table, as the schema carries it.
    ///
    /// # Panics
    ///
    /// When a column's option-ness and its `.nullable()` disagree.
    pub fn table(self, name: &str) -> IrTable {
        for (c, opt) in self.cols.iter().zip(&self.optional) {
            assert_eq!(
                c.nullable, *opt,
                "{name}.{}: a column is nullable exactly when its value type is an Opt",
                c.name
            );
        }
        IrTable {
            name: name.into(),
            columns: self.cols,
            key: self.key,
            indexes: self.indexes,
            refs: self.refs,
        }
    }
}

/// One column, or a tuple of them: a key, an index, an `on`.
pub trait Cols<T> {
    fn names(&self) -> Vec<String>;
}

impl<T, A> Cols<T> for Col<T, A> {
    fn names(&self) -> Vec<String> {
        vec![self.name.into()]
    }
}

macro_rules! cols_tuple {
    ($($v:ident . $i:tt),+) => {
        impl<T, $($v),+> Cols<T> for ($(Col<T, $v>,)+) {
            fn names(&self) -> Vec<String> {
                vec![$(self.$i.name.into()),+]
            }
        }
    };
}
cols_tuple!(A.0);
cols_tuple!(A.0, B.1);
cols_tuple!(A.0, B.1, C.2);
cols_tuple!(A.0, B.1, C.2, D.3);
cols_tuple!(A.0, B.1, C.2, D.3, E.4);

/// A key's values: a tuple of the key columns' value types.
pub trait Key {
    #[doc(hidden)]
    fn handles(&self) -> Vec<H>;
}

macro_rules! key_tuple {
    ($($v:ident . $i:tt),+) => {
        impl<$($v: Data),+> Key for ($($v,)+) {
            fn handles(&self) -> Vec<H> {
                vec![$(self.$i.to_h()),+]
            }
        }
    };
}
key_tuple!(A.0);
key_tuple!(A.0, B.1);
key_tuple!(A.0, B.1, C.2);
key_tuple!(A.0, B.1, C.2, D.3);

// Predicates and orders ---------------------------------------------------------

/// A filter over rows of `T`.
pub struct Pred<T> {
    p: IrPred,
    _t: PhantomData<fn() -> T>,
}

impl<T> Pred<T> {
    fn new(p: IrPred) -> Pred<T> {
        Pred { p, _t: PhantomData }
    }
    /// `PAll`.
    pub fn and(self, q: Pred<T>) -> Pred<T> {
        let mut ps = match self.p {
            IrPred::All(ps) => ps,
            p => vec![p],
        };
        ps.push(q.p);
        Pred::new(IrPred::All(ps))
    }
    /// `PAny`.
    pub fn or(self, q: Pred<T>) -> Pred<T> {
        let mut ps = match self.p {
            IrPred::Any(ps) => ps,
            p => vec![p],
        };
        ps.push(q.p);
        Pred::new(IrPred::Any(ps))
    }
    /// `PNot`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Pred<T> {
        Pred::new(IrPred::Not(Box::new(self.p)))
    }
}

/// One column of an order.
pub struct Order<T> {
    by: (String, Dir),
    _t: PhantomData<fn() -> T>,
}

/// One order, or a tuple of them.
pub trait Orders<T> {
    fn orders(self) -> Vec<(String, Dir)>;
}

impl<T> Orders<T> for Order<T> {
    fn orders(self) -> Vec<(String, Dir)> {
        vec![self.by]
    }
}

macro_rules! orders_tuple {
    ($($i:tt),+) => {
        impl<T> Orders<T> for ($(orders_tuple!(@t $i T),)+) {
            fn orders(self) -> Vec<(String, Dir)> {
                vec![$(self.$i.by),+]
            }
        }
    };
    (@t $i:tt $T:ident) => { Order<$T> };
}
orders_tuple!(0);
orders_tuple!(0, 1);
orders_tuple!(0, 1, 2);
orders_tuple!(0, 1, 2, 3);
orders_tuple!(0, 1, 2, 3, 4);
orders_tuple!(0, 1, 2, 3, 4, 5);

// Tables -------------------------------------------------------------------------

/// `db.<table>`: one table of the module.
pub struct Table<T> {
    _t: PhantomData<fn() -> T>,
}

fn key_values(hs: &[H]) -> Vec<Value> {
    hs.iter().map(|h| cx::value(*h)).collect()
}

fn store_fault(r: Refusal) {
    cx::halt(EvalFault::Verdict(r));
}

// A native write: through the store of the run, its change recorded, its
// refusal halting the run.
fn write(f: impl FnOnce(&mut dyn Store) -> Result<Option<Change>, Refusal>) {
    if cx::halted() {
        return;
    }
    match raw::store(f) {
        Ok(ch) => cx::native(|n| n.changes.extend(ch)),
        Err(r) => store_fault(r),
    }
}

fn row_of(h: H) -> Option<crate::store::Row> {
    match cx::value(h) {
        Value::Struct(m) => Some(m),
        other => {
            cx::halt(EvalFault::Bug(EvalError::TypeError(format!("a row is a struct, not {other:?}"))));
            None
        }
    }
}

impl<T: Row> Table<T> {
    /// `EGet t k`, bound: the row under the key, if any.
    pub fn get(&self, key: T::Key) -> Opt<T> {
        let ks = key.handles();
        if cx::emitting() {
            return Opt::from_h(cx::bind(Expr::Get(T::NAME.into(), ks.iter().map(|h| cx::expr(*h)).collect())));
        }
        if cx::halted() {
            return Opt::from_h(cx::lit(Value::Null));
        }
        let k = key_values(&ks);
        Opt::from_h(cx::lit(raw::store(|st| st.get_value(T::NAME, &k))))
    }

    /// `EExists t k`, bound.
    pub fn exists(&self, key: T::Key) -> Bool {
        let ks = key.handles();
        if cx::emitting() {
            return Bool::from_h(cx::bind(Expr::Exists(T::NAME.into(), ks.iter().map(|h| cx::expr(*h)).collect())));
        }
        if cx::halted() {
            return Bool::from_h(cx::lit(Value::Null));
        }
        let k = key_values(&ks);
        Bool::from_h(cx::lit(Value::Bool(raw::store(|st| st.exists(T::NAME, &k)))))
    }

    fn query(&self) -> Query<T> {
        Query {
            plan: Plan::from(T::NAME),
            _t: PhantomData,
        }
    }

    /// Keep the rows the predicate admits.
    pub fn filter(&self, p: Pred<T>) -> Query<T> {
        self.query().filter(p)
    }
    /// Order by one column or a tuple of them; the key completes it.
    pub fn order_by(&self, o: impl Orders<T>) -> Query<T> {
        self.query().order_by(o)
    }
    /// At most `n` rows.
    pub fn limit(&self, n: i64) -> Query<T> {
        self.query().limit(n)
    }
    /// Read a relationship beneath each row.
    pub fn with<C: Row>(&self, r: Rel<T, C>) -> Query<T> {
        self.query().with(r)
    }
    /// Every row, in key order.
    pub fn all(&self) -> List<T> {
        self.query().all()
    }
    /// The first row in key order, if any.
    pub fn first(&self) -> Opt<T> {
        self.query().first()
    }

    /// `SInsert t row []`: write the row unless one has its key; `.on(cols)`
    /// matches on a unique index instead.
    pub fn insert(&self, row: T) -> Write<T> {
        Write::new(WriteKind::Insert, row.to_h())
    }

    /// `SUpsert t row []`: write the row; `.on(cols)` keeps the key of a
    /// row matching on a unique index.
    pub fn upsert(&self, row: T) -> Write<T> {
        Write::new(WriteKind::Upsert, row.to_h())
    }

    /// `SUpdate t k row new`: the row under the key replaced by `f` of it;
    /// nothing when there is none.
    pub fn update(&self, key: T::Key, f: impl FnOnce(T) -> T) -> Effect {
        let ks = key.handles();
        if cx::emitting() {
            let k: Vec<Expr> = ks.iter().map(|h| cx::expr(*h)).collect();
            let x = cx::fresh();
            let new = cx::in_expr(|| cx::expr(f(T::from_h(cx::e(Expr::Var(x)))).to_h()));
            cx::stmt(Stmt::Update(T::NAME.into(), k, x, new));
            return Effect(());
        }
        if cx::halted() {
            return Effect(());
        }
        let k = key_values(&ks);
        if let Some(old) = raw::store(|st| st.get(T::NAME, &k)) {
            let new = f(T::from_h(cx::lit(Value::Struct(old))));
            if let Some(row) = row_of(new.to_h()) {
                write(|st| store::update(st, T::NAME, &k, row));
            }
        }
        Effect(())
    }

    /// `SDelete t k`.
    pub fn delete(&self, key: T::Key) -> Effect {
        let ks = key.handles();
        if cx::emitting() {
            cx::stmt(Stmt::Delete(T::NAME.into(), ks.iter().map(|h| cx::expr(*h)).collect()));
            return Effect(());
        }
        if cx::halted() {
            return Effect(());
        }
        let k = key_values(&ks);
        write(|st| st.delete(T::NAME, &k));
        Effect(())
    }
}

/// A read being described: `db.t.filter(p).order_by(o).limit(n)`, then
/// `.all()` or `.first()`.
pub struct Query<T> {
    plan: Plan,
    _t: PhantomData<fn() -> T>,
}

impl<T: Row> Query<T> {
    /// Keep the rows the predicate admits (and any filter before it).
    pub fn filter(mut self, p: Pred<T>) -> Query<T> {
        self.plan.filter = Some(match self.plan.filter.take() {
            None => p.p,
            Some(q) => IrPred::All(vec![q, p.p]),
        });
        self
    }
    /// Order by one column or a tuple of them, after any order before.
    pub fn order_by(mut self, o: impl Orders<T>) -> Query<T> {
        self.plan.order.extend(o.orders());
        self
    }
    /// At most `n` rows.
    pub fn limit(mut self, n: i64) -> Query<T> {
        self.plan.limit = Some(n);
        self
    }
    /// Read a relationship beneath each row, as a field of its name.
    pub fn with<C: Row>(mut self, r: Rel<T, C>) -> Query<T> {
        let child = table_of::<C>();
        let column = child
            .refs
            .iter()
            .find(|x| x.table == T::NAME)
            .map(|x| x.column.clone())
            .unwrap_or_else(|| panic!("{}: {} has no reference to {}", r.name, C::NAME, T::NAME));
        self.plan.related.push(Related {
            name: r.name.into(),
            relation: Relation {
                parent: T::NAME.into(),
                child: C::NAME.into(),
                column,
            },
            plan: Plan::from(C::NAME),
        });
        self
    }

    fn pull(mut self) -> H {
        if cx::emitting() {
            return cx::bind(Expr::Select(Box::new(self.plan)));
        }
        if cx::halted() {
            return cx::lit(Value::List(vec![]));
        }
        let rows = raw::store(|st| {
            let sch = st.schema().clone();
            eval::complete_order(&sch, &mut self.plan);
            eval::select_plan(&sch, &self.plan, st.as_store())
        });
        match rows {
            Ok(rows) => cx::lit(Value::List(rows)),
            Err(f) => {
                cx::halt(f);
                cx::lit(Value::List(vec![]))
            }
        }
    }

    /// `SLet s (ESelect plan)`: every row the plan pulls.
    pub fn all(self) -> List<T> {
        List::from_h(self.pull())
    }

    /// `SLet s (ESelect plan{limit = 1})`, `SLet s' (EStd First [EVar s])`,
    /// and the value `EVar s'`.
    pub fn first(mut self) -> Opt<T> {
        self.plan.limit = Some(1);
        let rows = self.pull();
        let first = List::<T>::from_h(rows).first();
        if cx::emitting() {
            return Opt::from_h(cx::bind(cx::expr(first.to_h())));
        }
        first
    }
}

// Writes and effects ------------------------------------------------------------

/// What a mutator's body is: the writes it made. A body, a `when` and a
/// `for_each` each end in one.
pub struct Effect(pub(crate) ());

/// Anything a body may end in: an [`Effect`], a pending [`Write`] (which
/// this finishes), or nothing at all.
pub trait IntoEffect {
    fn into_effect(self) -> Effect;
}

impl IntoEffect for Effect {
    fn into_effect(self) -> Effect {
        self
    }
}

impl IntoEffect for () {
    fn into_effect(self) -> Effect {
        Effect(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteKind {
    Insert,
    Upsert,
}

/// An insert or an upsert, written when it is finished: returned from a
/// body or a `when`, or dropped at the end of its statement — so `.on(..)`
/// can still be said after it.
pub struct Write<T: Row> {
    kind: WriteKind,
    row: H,
    on: Vec<String>,
    done: bool,
    _t: PhantomData<fn() -> T>,
}

impl<T: Row> Write<T> {
    fn new(kind: WriteKind, row: H) -> Write<T> {
        Write {
            kind,
            row,
            on: vec![],
            done: false,
            _t: PhantomData,
        }
    }

    /// Match on these columns — a declared unique index — rather than the key.
    pub fn on(mut self, cols: impl Cols<T>) -> Write<T> {
        self.on = cols.names();
        self
    }

    fn finish(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        if cx::emitting() {
            let row = cx::expr(self.row);
            let on = std::mem::take(&mut self.on);
            cx::stmt(match self.kind {
                WriteKind::Insert => Stmt::Insert(T::NAME.into(), row, on),
                WriteKind::Upsert => Stmt::Upsert(T::NAME.into(), row, on),
            });
            return;
        }
        if cx::halted() {
            return;
        }
        let Some(row) = row_of(self.row) else { return };
        let on = self.on.clone();
        match self.kind {
            WriteKind::Insert => write(|st| store::insert(st, T::NAME, row, &on)),
            WriteKind::Upsert => write(|st| store::upsert(st, T::NAME, row, &on)),
        }
    }
}

impl<T: Row> Drop for Write<T> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.finish();
        }
    }
}

impl<T: Row> IntoEffect for Write<T> {
    fn into_effect(mut self) -> Effect {
        self.finish();
        Effect(())
    }
}
