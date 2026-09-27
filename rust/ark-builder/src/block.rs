//! A block of statements. Every read — `select`, `exists`, `get` — is bound
//! by a `let` here, because that is the only place the IR lets one appear;
//! what the author gets back is the variable. Nested blocks are built by
//! closures over a fresh [`Block`], so a statement cannot land outside the
//! branch it was written in.

use std::collections::BTreeMap;

use ark::ir::{Expr as IrExpr, Stmt, Sym};
use ark::value::Value;

use crate::expr::Expr;
use crate::plan::Plan;
use crate::sym::fresh;

/// Statements under construction, with the author's names for what they
/// bind. The names are kept for a printable form; the encoding carries
/// numbers only.
#[derive(Default)]
pub struct Block {
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) names: BTreeMap<Sym, String>,
}

fn keys(ks: impl IntoIterator<Item = Expr>) -> Vec<IrExpr> {
    ks.into_iter().map(|e| e.0).collect()
}

impl Block {
    pub(crate) fn new() -> Block {
        Block::default()
    }

    fn bind(&mut self, name: Option<&str>, e: IrExpr) -> Expr {
        let x = fresh();
        if let Some(n) = name {
            self.names.insert(x, n.to_string());
        }
        self.stmts.push(Stmt::Let(x, e));
        Expr(IrExpr::Var(x))
    }

    fn nested(&mut self, build: impl FnOnce(&mut Block)) -> Vec<Stmt> {
        let mut inner = Block::new();
        build(&mut inner);
        self.names.append(&mut inner.names);
        inner.stmts
    }

    /// `let name = e`; the variable.
    pub fn let_(&mut self, name: &str, e: impl Into<Expr>) -> Expr {
        self.bind(Some(name), e.into().0)
    }

    /// `let x = select plan`; the rows, as a variable.
    pub fn select(&mut self, plan: Plan) -> Expr {
        self.bind(None, IrExpr::Select(Box::new(plan.0)))
    }

    /// `let x = exists table key`; whether the row is there, as a variable.
    pub fn exists(&mut self, table: &str, key: impl IntoIterator<Item = Expr>) -> Expr {
        self.bind(None, IrExpr::Exists(table.to_string(), keys(key)))
    }

    /// `let x = get table key`; the row or `None`, as a variable.
    pub fn get(&mut self, table: &str, key: impl IntoIterator<Item = Expr>) -> Expr {
        self.bind(None, IrExpr::Get(table.to_string(), keys(key)))
    }

    /// `if cond { then }`.
    pub fn if_(&mut self, cond: impl Into<Expr>, then: impl FnOnce(&mut Block)) {
        let c = cond.into().0;
        let a = self.nested(then);
        self.stmts.push(Stmt::If(c, a, vec![]));
    }

    /// `if cond { then } else { otherwise }`.
    pub fn if_else(&mut self, cond: impl Into<Expr>, then: impl FnOnce(&mut Block), otherwise: impl FnOnce(&mut Block)) {
        let c = cond.into().0;
        let a = self.nested(then);
        let b = self.nested(otherwise);
        self.stmts.push(Stmt::If(c, a, b));
    }

    /// `for x in xs { body }`; the body is handed the element.
    pub fn for_each(&mut self, xs: impl Into<Expr>, body: impl FnOnce(&mut Block, Expr)) {
        let list = xs.into().0;
        let x = fresh();
        let stmts = self.nested(|b| body(b, Expr(IrExpr::Var(x))));
        self.stmts.push(Stmt::For(x, list, stmts));
    }

    /// Write a full row.
    pub fn put(&mut self, table: &str, row: impl Into<Expr>) {
        self.stmts.push(Stmt::Put(table.to_string(), row.into().0));
    }

    /// Delete by key.
    pub fn delete(&mut self, table: &str, key: impl IntoIterator<Item = Expr>) {
        self.stmts.push(Stmt::Delete(table.to_string(), keys(key)));
    }

    /// End the mutator with a verdict.
    pub fn refuse(&mut self, why: &str) {
        self.stmts.push(Stmt::Refuse(IrExpr::Lit(Value::text(why))));
    }

    /// End the mutator with a verdict computed from an expression.
    pub fn refuse_with(&mut self, why: impl Into<Expr>) {
        self.stmts.push(Stmt::Refuse(why.into().0));
    }

    /// Leave a mutator.
    pub fn ret(&mut self) {
        self.stmts.push(Stmt::Return(None));
    }

    /// Leave a query or helper with its value.
    pub fn ret_value(&mut self, e: impl Into<Expr>) {
        self.stmts.push(Stmt::Return(Some(e.into().0)));
    }
}
