//! One function under construction: its autos and arguments, declared
//! before the body that mentions them, and the body itself.

use ark::ir::{Auto, Expr as IrExpr, FnKind, Function};
use ark::schema::Ty as IrTy;

use crate::block::Block;
use crate::expr::Expr;
use crate::ty::Ty;

/// A mutator, query or helper under construction.
pub struct FunctionBuilder {
    name: String,
    kind: FnKind,
    scope: Option<String>,
    autos: Vec<(String, Auto)>,
    args: Vec<(String, IrTy)>,
    ret: Option<IrTy>,
    body: Block,
}

impl FunctionBuilder {
    pub(crate) fn new(name: &str, kind: FnKind, scope: Option<&str>, ret: Option<Ty>) -> FunctionBuilder {
        FunctionBuilder {
            name: name.to_string(),
            kind,
            scope: scope.map(str::to_string),
            autos: vec![],
            args: vec![],
            ret: ret.map(|t| t.to_ir()),
            body: Block::new(),
        }
    }

    /// A fresh id naming a row of `table`, drawn once at the originating
    /// peer; the auto, as an expression.
    pub fn new_id(&mut self, name: &str, table: &str) -> Expr {
        self.autos.push((name.to_string(), Auto::NewId(table.to_string())));
        Expr(IrExpr::Auto(name.to_string()))
    }

    /// Milliseconds since the epoch, drawn once at the originating peer;
    /// the auto, as an expression.
    pub fn now(&mut self, name: &str) -> Expr {
        self.autos.push((name.to_string(), Auto::Now));
        Expr(IrExpr::Auto(name.to_string()))
    }

    /// A caller's argument; the argument, as an expression.
    pub fn arg(&mut self, name: &str, ty: Ty) -> Expr {
        self.args.push((name.to_string(), ty.to_ir()));
        Expr(IrExpr::Arg(name.to_string()))
    }

    /// The body: statements are appended to what is already there.
    pub fn body(&mut self) -> &mut Block {
        &mut self.body
    }

    pub(crate) fn finish(self) -> Function {
        Function {
            name: self.name,
            kind: self.kind,
            scope: self.scope,
            autos: self.autos,
            args: self.args,
            ret: self.ret,
            body: self.body.stmts,
            names: self.body.names,
        }
    }
}
