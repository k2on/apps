//! §2.3 and §2.5 Tables: a row is a struct with its columns said once in
//! [`Row::columns`], the module's tables are a struct of [`Table`]s, and `db.<table>` is
//! how a body reads and writes one.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use std::rc::Rc;

use crate::eval::{self, EvalError, EvalFault};
use crate::ir::{CmpOp, Expr, Key as IrKey, Lookup, Plan, Pred as IrPred, Related, Source, Stmt, Sym};
use crate::schema::{Column, Dir, Index, Ref, Table as IrTable, Ty};
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

/// A record's field names, computed once per type: taking a row apart
/// happens once per element of every loop, and `fields()` builds the
/// declaration each time it is asked.
fn record_names<T: Record>() -> std::rc::Rc<[std::rc::Rc<str>]> {
    use std::any::TypeId;
    use std::collections::HashMap;
    thread_local! {
        static NAMES: std::cell::RefCell<HashMap<TypeId, std::rc::Rc<[std::rc::Rc<str>]>>> = std::cell::RefCell::new(HashMap::new());
    }
    NAMES.with(|c| {
        c.borrow_mut()
            .entry(TypeId::of::<T>())
            .or_insert_with(|| T::fields().fields.into_iter().map(|(n, _)| n.into()).collect())
            .clone()
    })
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
            let hs: Vec<H> = names.iter().map(|n| cx::e(Expr::Field(Box::new(base.clone()), n.to_string()))).collect();
            cx::remember(&hs, h);
            hs
        } else {
            // Each field read in place: nothing is copied until a field is
            // used, and comparing two is not using either.
            names.iter().map(|n| cx::field(h, n.clone())).collect()
        };
        raw::assemble(&hs, record_what::<T>())
    }
    fn to_h(&self) -> H {
        let names = record_names::<T>();
        let hs = raw::disassemble(self, names.len(), record_what::<T>());
        if cx::emitting() {
            if let Some(h) = cx::origin(&hs) {
                return h;
            }
            cx::e(Expr::Struct(names.iter().zip(hs).map(|(n, h)| (n.to_string(), cx::expr(h))).collect()))
        } else {
            if let Some(h) = cx::whole(&hs, &names) {
                return h;
            }
            cx::lit(Value::Struct(names.iter().zip(hs).map(|(n, h)| (n.to_string(), cx::value(h))).collect()))
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
        IrTable::new(name, self.cols, self.key, self.indexes, self.refs)
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

// A row written, as the struct it was built as: the store lays it out as
// its table's (`store::row_for`).
fn row_of(h: H) -> Option<BTreeMap<String, Value>> {
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
            if cx::planning() {
                read_in_plan(&format!("db.{}.get(..)", T::NAME));
                return Opt::from_h(cx::e(Expr::None(T::ty())));
            }
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
            if cx::planning() {
                read_in_plan(&format!("db.{}.exists(..)", T::NAME));
                return Bool::from_h(cx::lit(Value::Bool(false)));
            }
            return Bool::from_h(cx::bind(Expr::Exists(T::NAME.into(), ks.iter().map(|h| cx::expr(*h)).collect())));
        }
        if cx::halted() {
            return Bool::from_h(cx::lit(Value::Null));
        }
        let k = key_values(&ks);
        Bool::from_h(cx::lit(Value::Bool(raw::store(|st| st.exists(T::NAME, &k)))))
    }

    fn query(&self) -> Query<T> {
        let row = if cx::emitting() { cx::fresh() } else { NATIVE_ROW };
        Query {
            plan: Plan::from(T::NAME),
            sorts: vec![],
            row,
            binds: vec![],
            row_ty: T::ty(),
            rel_fields: vec![],
            project_ty: None,
            on: vec![],
            make: Rc::new(T::from_h),
            _t: PhantomData,
        }
    }

    /// Every row, in key order, as a plan to build on: what a query whose
    /// first step is a lookup starts from (`db.song.rows().get(..)`),
    /// `get` on a table being the bound read of one row.
    pub fn rows(&self) -> Query<T> {
        self.query()
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
    pub fn with<C: Row>(&self, r: Rel<T, C>) -> Query<T, (List<C>,)> {
        self.query().with(r)
    }
    /// §1.9 Every row, as a child plan pinned to its parent; see
    /// [`Query::on`].
    pub fn on(&self, p: Pred<T>) -> Query<T> {
        self.query().on(p)
    }
    /// §1.9 A related plan beneath each row; see [`Query::each`].
    pub fn each<C: 'static, CB, CN: Data>(&self, f: impl FnOnce(T, ()) -> Query<C, CB, CN>) -> Query<T, (List<CN>,)> {
        self.query().each(f)
    }
    /// §1.9 Keep a row's node only when `f` holds; see [`Query::having`].
    pub fn having(&self, f: impl FnOnce(T, ()) -> Bool) -> Query<T> {
        self.query().having(f)
    }
    /// §1.9 Order by an expression; see [`Query::sort_by`].
    pub fn sort_by<K: Data>(&self, f: impl FnOnce(T, ()) -> K) -> Query<T> {
        self.query().sort_by(f)
    }
    /// §1.9 Each row's node; see [`Query::map`].
    pub fn map<U: Data>(&self, f: impl FnOnce(T, ()) -> U) -> Query<T, (), U> {
        self.query().map(f)
    }
    /// §1.9 The rows grouped by one column or a tuple of them; see
    /// [`Query::group_by`].
    pub fn group_by<G: GroupCols<T>>(&self, g: G) -> Query<G::Key, (List<T>,), G::Key> {
        self.query().group_by(g)
    }
    /// §1.9, D4 The distinct values of one column or a tuple of them; see
    /// [`Query::distinct`].
    pub fn distinct<G: GroupCols<T>>(&self, g: G) -> Query<G::Key, (List<T>,), G::Key> {
        self.query().distinct(g)
    }
    /// §1.9 A lookup of one row of this table by key, for a plan's
    /// [`Query::get`]: `db.media.by((song.media_id,))`. `get` spelt for a
    /// node, so that a plan's lookup and a mutator's bound read differ.
    pub fn by(&self, key: T::Key) -> By<T> {
        By {
            key: key.handles().into_iter().map(rhs).collect(),
            _t: PhantomData,
        }
    }
    /// [`Table::by`] with the key's one part an option, as a nullable
    /// reference holds it: `None` looks up nothing.
    pub fn by_opt<V: Data>(&self, key: Opt<V>) -> By<T>
    where
        T: Row<Key = (V,)>,
    {
        By {
            key: vec![rhs(key.to_h())],
            _t: PhantomData,
        }
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
            let new = f(T::from_h(cx::lit(old.into_value())));
            if let Some(row) = row_of(new.to_h()) {
                write(|st| {
                    let row = store::row_for(st.as_store(), T::NAME, row);
                    store::update(st, T::NAME, &k, row)
                });
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

/// §1.9 A plan being described: `db.t.filter(p).order_by(o).limit(n)`,
/// then `.all()` or `.first()` in a mutator's body, where it is a read; or
/// returned whole from a query's closure, where it is the query.
///
/// `R` is what a node's closures are handed for the source row (the row
/// type, or for a group its key), `B` the binders the plan has accumulated
/// — `.get` appends an `Opt<U>`, `.each` and `.with` a `List<N>`, a group
/// starts with its members — and `N` the node's type, the row until
/// `.map`. Every closure takes `(row, (a, b, ..))` and is run once, under
/// `Emit`: a query is recorded, never run natively, and `ark::view::pull`
/// is what it means.
pub struct Query<R, B = (), N = R> {
    plan: Plan,
    /// Expression order keys, after the column keys (§1.9).
    sorts: Vec<(Expr, Dir)>,
    row: Sym,
    /// The symbols of `B`, in order.
    binds: Vec<Sym>,
    /// The row binder's type: the table's row, or a group's key struct.
    row_ty: Ty,
    /// The related lists a node without a projection carries.
    rel_fields: Vec<(String, Ty)>,
    project_ty: Option<Ty>,
    /// As a child plan: `child.col == expr(parent)`, from [`Query::on`].
    on: Vec<(String, Expr)>,
    make: Rc<dyn Fn(H) -> R>,
    _t: PhantomData<fn() -> (B, N)>,
}

/// The row symbol a plan built natively uses. Native builds only v3-shaped
/// reads, whose one binder is the row an `on` reads, and scope is flat per
/// node, so one number serves every node of such a plan.
const NATIVE_ROW: Sym = 0;

/// §1.9 A lookup being described: the table and its key, over a node's
/// binders. [`Table::by`] makes one; [`Query::get`] takes it.
pub struct By<T> {
    key: Vec<Expr>,
    _t: PhantomData<fn() -> T>,
}

/// The binders a plan's closures are handed, as a tuple: `()`, `(A,)`, …
/// up to six. More is a nested plan.
pub trait Binders: Sized {
    #[doc(hidden)]
    fn at(syms: &[Sym]) -> Self;
}

/// A tuple of binders with one more on the end.
pub trait Append<X> {
    type Out;
}

impl Binders for () {
    fn at(_: &[Sym]) -> Self {}
}

impl<X: Data> Append<X> for () {
    type Out = (X,);
}

macro_rules! binders_tuple {
    ($($v:ident . $i:tt),+) => {
        impl<$($v: Data),+> Binders for ($($v,)+) {
            fn at(syms: &[Sym]) -> Self {
                ($($v::from_h(cx::e(Expr::Var(syms[$i]))),)+)
            }
        }
    };
}
binders_tuple!(A.0);
binders_tuple!(A.0, B.1);
binders_tuple!(A.0, B.1, C.2);
binders_tuple!(A.0, B.1, C.2, D.3);
binders_tuple!(A.0, B.1, C.2, D.3, E.4);
binders_tuple!(A.0, B.1, C.2, D.3, E.4, F.5);

macro_rules! append_tuple {
    ($($v:ident),+) => {
        impl<$($v: Data,)+ X: Data> Append<X> for ($($v,)+) {
            type Out = ($($v,)+ X);
        }
    };
}
append_tuple!(A);
append_tuple!(A, B);
append_tuple!(A, B, C);
append_tuple!(A, B, C, D);
append_tuple!(A, B, C, D, E);

/// What a group is keyed by: one column (the key is its value) or a tuple
/// of them (the key is the tuple of their values).
pub trait GroupCols<T> {
    type Key: 'static;
    #[doc(hidden)]
    fn columns(&self) -> Vec<(&'static str, Ty)>;
    #[doc(hidden)]
    fn key(names: &[&'static str], h: H) -> Self::Key;
}

// The value of a group key's column, read off the key struct.
fn key_field<V: Data>(h: H, name: &str) -> V {
    V::from_h(cx::e(Expr::Field(Box::new(cx::expr(h)), name.into())))
}

fn column_ty<T: Row>(name: &str) -> Ty {
    table_of::<T>()
        .column(name)
        .map(|c| c.column_ty())
        .unwrap_or_else(|| panic!("{}.{name}: no such column", T::NAME))
}

impl<T: Row, V: Data> GroupCols<T> for Col<T, V> {
    type Key = V;
    fn columns(&self) -> Vec<(&'static str, Ty)> {
        vec![(self.name, column_ty::<T>(self.name))]
    }
    fn key(names: &[&'static str], h: H) -> V {
        key_field(h, names[0])
    }
}

macro_rules! group_tuple {
    ($($v:ident . $i:tt),+) => {
        impl<T: Row, $($v: Data),+> GroupCols<T> for ($(Col<T, $v>,)+) {
            type Key = ($($v,)+);
            fn columns(&self) -> Vec<(&'static str, Ty)> {
                vec![$((self.$i.name, column_ty::<T>(self.$i.name))),+]
            }
            fn key(names: &[&'static str], h: H) -> Self::Key {
                ($(key_field::<$v>(h, names[$i]),)+)
            }
        }
    };
}
group_tuple!(A.0, B.1);
group_tuple!(A.0, B.1, C.2);

// The one message for a read written where a plan is being described.
fn read_in_plan(what: &str) {
    cx::complain(format!(
        "{what} inside a query's plan: a plan reads through `.get(|..| db.t.by(key))` (a lookup) and `.each(|..| db.t…)` (a related plan), never through an expression"
    ));
}

impl<R, B, N> Query<R, B, N> {
    fn retype<B2, N2>(self) -> Query<R, B2, N2> {
        Query {
            plan: self.plan,
            sorts: self.sorts,
            row: self.row,
            binds: self.binds,
            row_ty: self.row_ty,
            rel_fields: self.rel_fields,
            project_ty: self.project_ty,
            on: self.on,
            make: self.make,
            _t: PhantomData,
        }
    }

    fn fresh() -> Sym {
        if cx::emitting() {
            cx::fresh()
        } else {
            NATIVE_ROW + 1
        }
    }

    // A related plan, named after its table (or its reference), numbered
    // `_2`, `_3`… when an earlier one here has the name: without a
    // projection each is a field of the node, and two of one name would be
    // one field.
    fn related(&mut self, base: String, on: Vec<(String, Expr)>, plan: Plan, node: Ty) {
        let mut name = base.clone();
        for n in 2.. {
            if !self.plan.related.iter().any(|r| r.name == name) {
                break;
            }
            name = format!("{base}_{n}");
        }
        let sym = Self::fresh();
        let list = Ty::List(Box::new(node));
        self.plan.related.push(Related {
            name: name.clone(),
            sym,
            on,
            plan,
        });
        self.rel_fields.push((name, list));
        self.binds.push(sym);
    }

    /// The plan, with its order assembled and its row binder dropped when
    /// nothing could read it, and the type of its nodes.
    pub(crate) fn finish(self) -> (Plan, Ty) {
        let mut plan = self.plan;
        plan.order.extend(self.sorts.into_iter().map(|(e, d)| (IrKey::Expr(e), d)));
        plan.row = Some(self.row);
        if plan.is_bare() {
            plan.row = None;
        }
        let node = self.project_ty.unwrap_or_else(|| {
            let Ty::Struct(mut fs) = self.row_ty else {
                unreachable!("a row type is a struct")
            };
            fs.extend(self.rel_fields);
            Ty::Struct(fs)
        });
        (plan, node)
    }
}

impl<R: 'static, B: Binders, N> Query<R, B, N> {
    // A node closure, run once over the node's binders: in the plan's
    // context, where nothing may be written and nothing read.
    fn run<X>(&self, what: &str, f: impl FnOnce(R, B) -> X) -> X {
        assert!(
            cx::emitting(),
            "{what}: a query's plan is described under Emit and never run natively (ark::view::pull is what it means)"
        );
        cx::in_plan(|| f((self.make)(cx::e(Expr::Var(self.row))), B::at(&self.binds)))
    }

    /// §1.3 A lookup: the row of another table under the key `f` computes
    /// over this node, appended to the binders as an `Opt<U>` — `None`
    /// when there is none, or when a part of the key is `None`.
    /// `.get(|song, ()| db.media.by((song.media_id,)))`. May chain: a later
    /// `get` sees the earlier ones.
    pub fn get<U: Row>(mut self, f: impl FnOnce(R, B) -> By<U>) -> Query<R, B::Out, N>
    where
        B: Append<Opt<U>>,
    {
        let by = self.run("get", f);
        let sym = Self::fresh();
        self.plan.lookups.push(Lookup {
            name: U::NAME.into(),
            sym,
            table: U::NAME.into(),
            key: by.key,
        });
        self.binds.push(sym);
        self.retype()
    }

    /// §1.3 A related plan: the child query `f` returns, its rows pinned to
    /// this node by the child's [`Query::on`], appended to the binders as
    /// the list of its nodes (in its own order, cut to its own limit).
    /// `.each(|song, ()| db.credit.order_by(..).on(Credit::recording_id.eq(song.recording_id)))`.
    /// The child's own closures see the child's binders only.
    pub fn each<C: 'static, CB, CN: Data>(mut self, f: impl FnOnce(R, B) -> Query<C, CB, CN>) -> Query<R, B::Out, N>
    where
        B: Append<List<CN>>,
    {
        let child = self.run("each", f);
        let name = child.plan.table().clone();
        let on = child.on.clone();
        let (plan, node) = child.finish();
        self.related(name, on, plan, node);
        self.retype()
    }

    /// §1.3 Keep a node only when `f` holds (and any `having` before it).
    /// A node it refuses still exists to a view, so it appears when a
    /// child arrives: `.having(|album, (songs,)| songs.len().gt(0))`.
    pub fn having(mut self, f: impl FnOnce(R, B) -> Bool) -> Self {
        let h = cx::expr(self.run("having", f).0);
        self.plan.having = Some(match self.plan.having.take() {
            None => h,
            Some(g) => Expr::Op(crate::ir::Op::And, vec![g, h]),
        });
        self
    }

    /// §1.3 Order by an expression over the node, ascending. Keys compare
    /// in the order given — every `order_by` column first, then every
    /// `sort_by` expression, then the key columns — so the *first* call is
    /// the primary key. That is not the list `sort_by` of v3, where a
    /// stable sort made the last call primary.
    pub fn sort_by<K: Data>(self, f: impl FnOnce(R, B) -> K) -> Self {
        self.sort(Dir::Asc, f)
    }

    /// [`Query::sort_by`], descending.
    pub fn sort_by_desc<K: Data>(self, f: impl FnOnce(R, B) -> K) -> Self {
        self.sort(Dir::Desc, f)
    }

    fn sort<K: Data>(mut self, d: Dir, f: impl FnOnce(R, B) -> K) -> Self {
        let k = cx::expr(self.run("sort_by", f).to_h());
        self.sorts.push((k, d));
        self
    }

    /// §1.3 The node's value: `f` over the row and the binders. The query's
    /// value is the list of these.
    pub fn map<U: Data>(mut self, f: impl FnOnce(R, B) -> U) -> Query<R, B, U> {
        let e = cx::expr(self.run("map", f).to_h());
        self.plan.project = Some(e);
        self.project_ty = Some(U::ty());
        self.retype()
    }

    /// At most `n` nodes (per parent, in a child plan).
    pub fn limit(mut self, n: i64) -> Self {
        self.plan.limit = Some(n);
        self
    }
}

impl<T: Row, B, N> Query<T, B, N> {
    /// Keep the rows the predicate admits (and any filter before it). The
    /// right-hand sides are constant for the read.
    pub fn filter(mut self, p: Pred<T>) -> Self {
        self.plan.filter = Some(match self.plan.filter.take() {
            None => p.p,
            Some(q) => IrPred::All(vec![q, p.p]),
        });
        self
    }
    /// Order by one column or a tuple of them, after any order before.
    pub fn order_by(mut self, o: impl Orders<T>) -> Self {
        self.plan.order.extend(o.orders().into_iter().map(|(c, d)| (IrKey::Column(c), d)));
        self
    }
    /// Read a relationship beneath each row, as a field of its name: a
    /// related plan on the reference the schema declares, appended to the
    /// binders as a `List<C>`.
    pub fn with<C: Row>(mut self, r: Rel<T, C>) -> Query<T, B::Out, N>
    where
        B: Append<List<C>>,
    {
        let child = table_of::<C>();
        let column = child
            .refs
            .iter()
            .find(|x| x.table == T::NAME)
            .map(|x| x.column.clone())
            .unwrap_or_else(|| panic!("{}: {} has no reference to {}", r.name, C::NAME, T::NAME));
        let parent = table_of::<T>();
        let [key] = &parent.key[..] else {
            panic!("{}: {} is referenced through a key of one column", r.name, T::NAME)
        };
        let on = vec![(column, Expr::Field(Box::new(Expr::Var(self.row)), key.clone()))];
        self.related(r.name.into(), on, Plan::from(C::NAME), C::ty());
        self.retype()
    }
    /// §1.3 As a child plan: pin each row to its parent, `child.col ==
    /// expr(parent)`, one equality per column (`.and` them for several).
    /// `Credit::recording_id.eq(song.recording_id)`. Options are flat, so
    /// an `Opt<T>` column joins a `T` as plain equality.
    pub fn on(mut self, p: Pred<T>) -> Self {
        fn pairs(p: IrPred, out: &mut Vec<(String, Expr)>) -> bool {
            match p {
                IrPred::Cmp(c, CmpOp::Eq, e) => {
                    out.push((c, e));
                    true
                }
                IrPred::All(ps) => ps.into_iter().all(|q| pairs(q, out)),
                _ => false,
            }
        }
        if !pairs(p.p, &mut self.on) {
            cx::complain(format!("{}: .on(..) takes column equalities joined with .and", T::NAME));
        }
        self
    }

    fn pull(self) -> H {
        let (mut plan, _) = self.finish();
        if cx::emitting() {
            if cx::planning() {
                read_in_plan(&format!("a read of {}", T::NAME));
                return cx::lit(Value::List(vec![]));
            }
            if !plan.is_v3_shaped() {
                cx::complain(format!(
                    "a read of {} with a lookup, a related plan on something other than a reference, a group, a having, a projection or an expression order: only a query's plan may have one (§1.4)",
                    T::NAME
                ));
            }
            return cx::bind(Expr::Select(Box::new(plan)));
        }
        if cx::halted() {
            return cx::lit(Value::List(vec![]));
        }
        let rows = raw::store(|st| {
            let sch = st.schema().clone();
            eval::complete_order(&sch, &mut plan);
            eval::select_plan(&sch, &plan, st.as_store())
        });
        match rows {
            Ok(rows) => cx::lit(Value::List(rows)),
            Err(f) => {
                cx::halt(f);
                cx::lit(Value::List(vec![]))
            }
        }
    }

    /// `SLet s (ESelect plan)`: every node the plan pulls. A mutator's (or
    /// a middleware's) read; a query returns its plan instead.
    pub fn all(self) -> List<N>
    where
        N: Data,
    {
        List::from_h(self.pull())
    }

    /// `SLet s (ESelect plan{limit = 1})`, `SLet s' (EStd First [EVar s])`,
    /// and the value `EVar s'`.
    pub fn first(mut self) -> Opt<N>
    where
        N: Data,
    {
        self.plan.limit = Some(1);
        let rows = self.pull();
        let first = List::<N>::from_h(rows).first();
        if cx::emitting() {
            return Opt::from_h(cx::bind(cx::expr(first.to_h())));
        }
        first
    }
}

impl<T: Row> Query<T> {
    /// §1.3 The rows the filter admits, grouped by one column or a tuple of
    /// them: one node per distinct key. The closures are handed the key
    /// (the column's value, or the tuple of them) and, as the first binder,
    /// the group's rows in key order: `db.media.group_by(Media::creator)` is
    /// a `Query<Text, (List<Media>,)>`. Grouped rows are ordered by the key.
    pub fn group_by<G: GroupCols<T>>(self, g: G) -> Query<G::Key, (List<T>,), G::Key> {
        assert!(
            self.plan.related.is_empty() && self.plan.order.is_empty(),
            "group_by comes straight after the filter"
        );
        let cols = g.columns();
        let names: Vec<&'static str> = cols.iter().map(|(n, _)| *n).collect();
        let members = Self::fresh();
        let plan = Plan {
            source: Source::Group {
                table: T::NAME.into(),
                by: names.iter().map(|n| n.to_string()).collect(),
            },
            members: Some(members),
            ..self.plan
        };
        let names: Rc<[&'static str]> = names.into();
        Query {
            plan,
            sorts: self.sorts,
            row: self.row,
            binds: vec![members],
            row_ty: Ty::Struct(cols.into_iter().map(|(n, t)| (n.to_string(), t)).collect()),
            rel_fields: vec![],
            project_ty: None,
            on: self.on,
            make: Rc::new(move |h| G::key(&names, h)),
            _t: PhantomData,
        }
    }

    /// §1.9, `docs/plan-db.md` D4 The distinct values of one column (or a
    /// tuple of them) among the rows the filter admits, in key order: a
    /// group with no aggregate, so it is [`Query::group_by`] whose node is
    /// its key — the column's value, or for a tuple the key struct of the
    /// columns — and nothing new in the engine. `db.song.distinct(Song::creator)`
    /// is `db.song.group_by(Song::creator).map(|creator, _| creator)`, the
    /// same plan byte for byte, and is maintained as a group is: a value
    /// appears with its first row and goes with its last. The members are
    /// still the first binder, for a `.having` over them.
    pub fn distinct<G: GroupCols<T>>(self, g: G) -> Query<G::Key, (List<T>,), G::Key> {
        let cols = g.columns();
        let mut q = self.group_by(g);
        let (e, ty) = match cols.as_slice() {
            [(name, ty)] => (Expr::Field(Box::new(Expr::Var(q.row)), name.to_string()), ty.clone()),
            _ => (Expr::Var(q.row), q.row_ty.clone()),
        };
        q.plan.project = Some(e);
        q.project_ty = Some(ty);
        q
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
            WriteKind::Insert => write(|st| {
                let row = store::row_for(st.as_store(), T::NAME, row);
                store::insert(st, T::NAME, row, &on)
            }),
            WriteKind::Upsert => write(|st| {
                let row = store::row_for(st.as_store(), T::NAME, row);
                store::upsert(st, T::NAME, row, &on)
            }),
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
