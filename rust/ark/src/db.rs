//! The store as generated code sees it: `db` in a generated mutator or
//! query. A [`Db`] is an [`Overlay`] over a base store that records what it
//! changed, so that a generated body can be run as one transaction — the
//! base is untouched until the caller commits the changes, and a verdict
//! drops the overlay and reports nothing, exactly as `Ark.Eval.applyClosure`
//! rolls back.

use crate::fault::Fault;
use crate::ir::Plan;
use crate::store::{Change, Overlay, Refusal, Store};
use crate::value::Value;

pub struct Db<'a> {
    overlay: Overlay<'a>,
    changes: Vec<Change>,
    /// The last refusal the store gave, so that the structured verdict is
    /// recoverable from the `Fault::Refuse` a generated body propagates.
    refusal: Option<Refusal>,
}

impl<'a> Db<'a> {
    /// An overlay over a base store, with nothing changed yet.
    pub fn new(base: &'a dyn Store) -> Db<'a> {
        Db {
            overlay: Overlay::new(base),
            changes: Vec::new(),
            refusal: None,
        }
    }

    /// `db.get(table, [key…])` → the row or `Value::null()`.
    pub fn get(&self, table: &str, key: Vec<Value>) -> Value {
        self.overlay.get_value(table, &key)
    }

    /// `db.exists(table, [key…])` → `Value::bool`.
    pub fn exists(&self, table: &str, key: Vec<Value>) -> Value {
        Value::Bool(self.overlay.exists(table, &key))
    }

    /// `db.select(plan)` → `Value::list` of nodes as `Ark.Eval.select` builds
    /// them.
    pub fn select(&self, plan: &Plan) -> Value {
        self.overlay.select(plan)
    }

    /// `db.put(table, row)`: faults with the store's refusal.
    pub fn put(&mut self, table: &str, row: Value) -> Result<(), Fault> {
        let row = match row {
            Value::Struct(m) => m,
            other => return Err(Fault::bug(format!("put: a row is a struct, not {other:?}"))),
        };
        match self.overlay.put(table, row) {
            Ok(ch) => {
                self.changes.extend(ch);
                Ok(())
            }
            Err(r) => {
                let text = r.to_string();
                self.refusal = Some(r);
                Err(Fault::refuse(text))
            }
        }
    }

    /// `db.delete(table, [key…])`: faults with the store's refusal.
    pub fn delete(&mut self, table: &str, key: Vec<Value>) -> Result<(), Fault> {
        match self.overlay.delete(table, &key) {
            Ok(ch) => {
                self.changes.extend(ch);
                Ok(())
            }
            Err(r) => {
                let text = r.to_string();
                self.refusal = Some(r);
                Err(Fault::refuse(text))
            }
        }
    }

    /// What has been changed so far, in order.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// The changes, consuming the transaction.
    pub fn into_changes(self) -> Vec<Change> {
        self.changes
    }

    /// The store's last refusal, if a write was refused.
    pub fn last_refusal(&self) -> Option<&Refusal> {
        self.refusal.as_ref()
    }

    /// The verdict a fault a generated body returned stands for: the
    /// store's own structured refusal when the fault is the one it raised,
    /// otherwise `Refused(text)`; a `Fault::Bug` is `Err`.
    pub fn verdict(&self, fault: Fault) -> Result<Refusal, String> {
        match fault {
            Fault::Refuse(text) => Ok(match &self.refusal {
                Some(r) if r.to_string() == text => r.clone(),
                _ => Refusal::Refused(text),
            }),
            Fault::Bug(text) => Err(text),
        }
    }
}

/// Run a generated mutator body as one transaction over a store: `Ok(Ok)`
/// is the changes, committed; `Ok(Err)` is the verdict, with the store
/// untouched; `Err` is a bug's text. The shape `apply_closure` has, for a
/// body that is native code rather than IR.
pub fn run_mutator(store: &mut dyn Store, body: impl FnOnce(&mut Db) -> Result<(), Fault>) -> Result<Result<Vec<Change>, Refusal>, String> {
    let outcome = {
        let mut db = Db::new(&*store);
        let r = body(&mut db);
        match r {
            Ok(()) => Ok(db.into_changes()),
            Err(f) => Err(db.verdict(f)),
        }
    };
    match outcome {
        Ok(changes) => {
            store.apply_changes(&changes);
            Ok(Ok(changes))
        }
        Err(Ok(refusal)) => Ok(Err(refusal)),
        Err(Err(bug)) => Err(bug),
    }
}
