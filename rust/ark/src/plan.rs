//! A plan builder for tests and hand-written IR: `Plan::from(table)
//! .filter(pred).order_by(col, Dir::Asc).limit(n)`, and for the v4 parts
//! `.row(sym)`, `.lookup(..)`, `.related(..)`, `.having(e)`, `.project(e)`,
//! `.sort_by(e, dir)` and `Plan::group(table, by, members)`; `Pred::cmp`,
//! `in_list`, `all`, `any`, `not`. Right-hand sides are `Value`s, already
//! evaluated, carried as literals. Symbols are the caller's to choose.

use crate::ir::{CmpOp, Expr, Key, Lookup, Plan, Pred, Related, Source, Sym};
use crate::schema::Dir;
use crate::value::Value;

impl Plan {
    /// Every row of a table, in key order, with no filter, order or limit.
    #[allow(clippy::should_implement_trait)]
    pub fn from(table: &str) -> Plan {
        Plan {
            source: Source::Table(table.into()),
            filter: None,
            row: None,
            members: None,
            lookups: vec![],
            related: vec![],
            having: None,
            project: None,
            order: vec![],
            limit: None,
        }
    }

    /// The rows of a table grouped by columns: the row binder is the key
    /// struct, `members` the group's rows.
    pub fn group(table: &str, by: &[&str], row: Sym, members: Sym) -> Plan {
        Plan {
            source: Source::Group {
                table: table.into(),
                by: by.iter().map(|c| c.to_string()).collect(),
            },
            row: Some(row),
            members: Some(members),
            ..Plan::from(table)
        }
    }

    /// Keep the rows the predicate admits.
    pub fn filter(mut self, pred: Pred) -> Plan {
        self.filter = Some(pred);
        self
    }

    /// Bind the source row to `sym`.
    pub fn row(mut self, sym: Sym) -> Plan {
        self.row = Some(sym);
        self
    }

    /// Order by a column, after any order already given.
    pub fn order_by(mut self, column: &str, dir: Dir) -> Plan {
        self.order.push((Key::Column(column.into()), dir));
        self
    }

    /// Order by an expression over the node's binders, after any order
    /// already given.
    pub fn sort_by(mut self, key: Expr, dir: Dir) -> Plan {
        self.order.push((Key::Expr(key), dir));
        self
    }

    /// Take at most `n` nodes.
    pub fn limit(mut self, n: i64) -> Plan {
        self.limit = Some(n);
        self
    }

    /// Bind `sym` to the row of `table` under the key the expressions
    /// compute.
    pub fn lookup(mut self, name: &str, sym: Sym, table: &str, key: Vec<Expr>) -> Plan {
        self.lookups.push(Lookup {
            name: name.into(),
            sym,
            table: table.into(),
            key,
        });
        self
    }

    /// Bind `sym` to the child plan's nodes where each `column` equals the
    /// expression over this node, and name the list `name` in the default
    /// node.
    pub fn related(mut self, name: &str, sym: Sym, on: Vec<(&str, Expr)>, plan: Plan) -> Plan {
        self.related.push(Related {
            name: name.into(),
            sym,
            on: on.into_iter().map(|(c, e)| (c.to_string(), e)).collect(),
            plan,
        });
        self
    }

    /// Keep a node only when the expression holds.
    pub fn having(mut self, e: Expr) -> Plan {
        self.having = Some(e);
        self
    }

    /// The node's value.
    pub fn project(mut self, e: Expr) -> Plan {
        self.project = Some(e);
        self
    }
}

impl Pred {
    /// `column <op> value`.
    pub fn cmp(column: &str, op: CmpOp, value: Value) -> Pred {
        Pred::Cmp(column.into(), op, Expr::Lit(value))
    }

    /// `column IN values`.
    pub fn in_list(column: &str, values: Vec<Value>) -> Pred {
        Pred::In(column.into(), values.into_iter().map(Expr::Lit).collect())
    }

    /// Every predicate holds.
    pub fn all(preds: Vec<Pred>) -> Pred {
        Pred::All(preds)
    }

    /// Some predicate holds.
    pub fn any(preds: Vec<Pred>) -> Pred {
        Pred::Any(preds)
    }

    /// The predicate does not hold.
    #[allow(clippy::should_implement_trait)]
    pub fn not(pred: Pred) -> Pred {
        Pred::Not(Box::new(pred))
    }
}
