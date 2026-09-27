//! Plans and predicates, with expression right-hand sides — the `ark`
//! crate's own builders take evaluated values, because generated code
//! builds a plan at run time; an author builds one at write time.

use ark::ir::{CmpOp, Plan as IrPlan, Pred as IrPred, Related};
use ark::schema::{Dir, Relation};

use crate::expr::Expr;

/// §3.3 What a `select` pulls: a table, a filter, an order, a limit, and
/// the relationships read beneath each row.
#[derive(Clone, Debug)]
pub struct Plan(pub(crate) IrPlan);

impl Plan {
    /// Every row of a table. The verifier completes the order with the
    /// table's key columns, so an author need not.
    #[allow(clippy::should_implement_trait)]
    pub fn from(table: &str) -> Plan {
        Plan(IrPlan {
            table: table.to_string(),
            filter: None,
            order: vec![],
            limit: None,
            related: vec![],
        })
    }

    /// Keep the rows the predicate admits; a second filter is conjoined
    /// with the first.
    pub fn filter(mut self, pred: Pred) -> Plan {
        self.0.filter = Some(match self.0.filter.take() {
            None => pred.0,
            Some(IrPred::All(mut ps)) => {
                ps.push(pred.0);
                IrPred::All(ps)
            }
            Some(p) => IrPred::All(vec![p, pred.0]),
        });
        self
    }

    /// Order by a column, after any order already given.
    pub fn order_by(mut self, column: &str, dir: Dir) -> Plan {
        self.0.order.push((column.to_string(), dir));
        self
    }

    /// Take at most `n` rows.
    pub fn limit(mut self, n: i64) -> Plan {
        self.0.limit = Some(n);
        self
    }

    /// Read `child.column REFERENCES parent` beneath each row, as a field
    /// named `name` holding the child plan's rows.
    pub fn related(mut self, name: &str, parent: &str, child: &str, column: &str, plan: Plan) -> Plan {
        self.0.related.push(Related {
            name: name.to_string(),
            relation: Relation {
                parent: parent.to_string(),
                child: child.to_string(),
                column: column.to_string(),
            },
            plan: plan.0,
        });
        self
    }

    /// The `ark` crate's plan.
    pub fn into_ir(self) -> IrPlan {
        self.0
    }
}

/// A filter over one row; the right-hand sides may not mention the row.
#[derive(Clone, Debug)]
pub struct Pred(pub(crate) IrPred);

impl Pred {
    /// `column <op> e`.
    pub fn cmp(column: &str, op: CmpOp, e: impl Into<Expr>) -> Pred {
        Pred(IrPred::Cmp(column.to_string(), op, e.into().0))
    }

    /// `column IN [e, …]`.
    pub fn in_list(column: &str, es: impl IntoIterator<Item = Expr>) -> Pred {
        Pred(IrPred::In(column.to_string(), es.into_iter().map(|e| e.0).collect()))
    }

    /// Every predicate holds.
    pub fn all(preds: impl IntoIterator<Item = Pred>) -> Pred {
        Pred(IrPred::All(preds.into_iter().map(|p| p.0).collect()))
    }

    /// Some predicate holds.
    pub fn any(preds: impl IntoIterator<Item = Pred>) -> Pred {
        Pred(IrPred::Any(preds.into_iter().map(|p| p.0).collect()))
    }

    /// The predicate does not hold.
    #[allow(clippy::should_implement_trait)]
    pub fn not(pred: Pred) -> Pred {
        Pred(IrPred::Not(Box::new(pred.0)))
    }

    /// The `ark` crate's predicate.
    pub fn into_ir(self) -> IrPred {
        self.0
    }
}
