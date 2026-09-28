//! A plan builder for tests and hand-written IR: `Plan::from(table).filter(pred)
//! .order_by(col, Dir::Asc).limit(n).related(name, parent, child, column,
//! child_plan)` and `Pred::cmp`, `in_list`, `all`, `any`, `not`. Right-hand
//! sides are `Value`s, already evaluated, carried as literals.

use crate::ir::{CmpOp, Expr, Plan, Pred, Related};
use crate::schema::{Dir, Relation};
use crate::value::Value;

impl Plan {
    /// Every row of a table, in key order, with no filter, order or limit.
    #[allow(clippy::should_implement_trait)]
    pub fn from(table: &str) -> Plan {
        Plan {
            table: table.into(),
            filter: None,
            order: vec![],
            limit: None,
            related: vec![],
        }
    }

    /// Keep the rows the predicate admits.
    pub fn filter(mut self, pred: Pred) -> Plan {
        self.filter = Some(pred);
        self
    }

    /// Order by a column, after any order already given.
    pub fn order_by(mut self, column: &str, dir: Dir) -> Plan {
        self.order.push((column.into(), dir));
        self
    }

    /// Take at most `n` rows.
    pub fn limit(mut self, n: i64) -> Plan {
        self.limit = Some(n);
        self
    }

    /// Read the relationship `child.column REFERENCES parent` beneath each
    /// row, as a field named `name` holding the child plan's rows.
    pub fn related(mut self, name: &str, parent: &str, child: &str, column: &str, plan: Plan) -> Plan {
        self.related.push(Related {
            name: name.into(),
            relation: Relation {
                parent: parent.into(),
                child: child.into(),
                column: column.into(),
            },
            plan,
        });
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
