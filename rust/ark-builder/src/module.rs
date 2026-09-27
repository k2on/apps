//! The module: a schema, then functions in declaration order, then what
//! [`ModuleBuilder::build`] makes of them — an `ark::ir::Module` with every
//! plan's order completed the way the verifier would, so that the file's
//! hash is the hash `arkc verify` prints.

use std::fs;
use std::io;
use std::path::Path;

use ark::canon;
use ark::ir::{module_value, Expr, FnKind, Function, Module, Plan, Related, Stmt, SPEC_VERSION};
use ark::schema::{Dir, Schema, Ty as IrTy};
use ark::value::hex;

use crate::function::FunctionBuilder;
use crate::schema::ScopeBuilder;
use crate::ty::Ty;

/// A module under construction.
pub struct ModuleBuilder {
    schema: Schema,
    functions: Vec<Function>,
    live: Vec<(String, IrTy)>,
}

impl Default for ModuleBuilder {
    fn default() -> ModuleBuilder {
        ModuleBuilder {
            schema: Schema::empty(),
            functions: vec![],
            live: vec![],
        }
    }
}

impl ModuleBuilder {
    pub fn new() -> ModuleBuilder {
        ModuleBuilder::default()
    }

    /// Declare a scope and its tables.
    pub fn scope(&mut self, name: &str, build: impl FnOnce(&mut ScopeBuilder)) {
        let mut s = ScopeBuilder::new(name);
        build(&mut s);
        self.schema.scopes.push(s.scope);
    }

    /// A mutator of `scope`: reads and writes that scope, may refuse,
    /// returns nothing.
    pub fn mutator(&mut self, name: &str, scope: &str, build: impl FnOnce(&mut FunctionBuilder)) {
        self.function(FunctionBuilder::new(name, FnKind::Mutator, Some(scope), None), build);
    }

    /// A query: reads any scope, returns a value of `ret`.
    pub fn query(&mut self, name: &str, ret: Ty, build: impl FnOnce(&mut FunctionBuilder)) {
        self.function(FunctionBuilder::new(name, FnKind::Query, None, Some(ret)), build);
    }

    /// A helper: pure, callable by every function declared after it.
    pub fn helper(&mut self, name: &str, ret: Ty, build: impl FnOnce(&mut FunctionBuilder)) {
        self.function(FunctionBuilder::new(name, FnKind::Helper, None, Some(ret)), build);
    }

    fn function(&mut self, mut f: FunctionBuilder, build: impl FnOnce(&mut FunctionBuilder)) {
        build(&mut f);
        self.functions.push(f.finish());
    }

    /// A frame type of the live section.
    pub fn live(&mut self, name: &str, ty: Ty) {
        self.live.push((name.to_string(), ty.to_ir()));
    }

    /// The type of a whole row of a table declared so far: a struct of its
    /// columns, the nullable ones as options.
    ///
    /// # Panics
    ///
    /// If no scope so far declares the table — the schema comes first.
    pub fn row_ty(&self, table: &str) -> Ty {
        match self.schema.lookup_table(table) {
            Some(t) => Ty::from(t.row_ty()),
            None => panic!("row_ty({table:?}): no table of that name has been declared"),
        }
    }

    /// The schema declared so far.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The functions declared so far, in declaration order.
    pub fn functions(&self) -> &[Function] {
        &self.functions
    }

    /// The module, at this crate's spec version, every order completed.
    pub fn build(&self) -> Module {
        let m = Module {
            spec: SPEC_VERSION,
            schema: self.schema.clone(),
            functions: self.functions.clone(),
            live: self.live.clone(),
        };
        complete_orders(m)
    }

    /// The module's canonical bytes: `canon::encode` of `module_value`.
    pub fn bytes(&self) -> Vec<u8> {
        canon::encode(&module_value(&self.build()))
    }

    /// The module hash, as `arkc verify` prints it.
    pub fn hash(&self) -> String {
        hex(&ark::ir::module_hash(&self.build()))
    }

    /// Write the module's canonical bytes to a file.
    pub fn write(&self, path: impl AsRef<Path>) -> io::Result<()> {
        fs::write(path, self.bytes())
    }
}

/// §9.4 `Ark.Verify.completeOrders`: append each plan's key columns,
/// ascending, after whatever the author ordered by, omitting any already
/// present — in every plan of every function, related plans included.
fn complete_orders(mut m: Module) -> Module {
    let sch = m.schema.clone();
    for f in &mut m.functions {
        for s in &mut f.body {
            complete_stmt(&sch, s);
        }
    }
    m
}

fn complete_stmt(sch: &Schema, s: &mut Stmt) {
    match s {
        Stmt::Let(_, e) | Stmt::Put(_, e) | Stmt::Refuse(e) | Stmt::Return(Some(e)) => complete_expr(sch, e),
        Stmt::If(c, a, b) => {
            complete_expr(sch, c);
            a.iter_mut().chain(b.iter_mut()).for_each(|s| complete_stmt(sch, s));
        }
        Stmt::For(_, xs, b) => {
            complete_expr(sch, xs);
            b.iter_mut().for_each(|s| complete_stmt(sch, s));
        }
        Stmt::Delete(_, ks) => ks.iter_mut().for_each(|e| complete_expr(sch, e)),
        Stmt::Return(None) => {}
    }
}

fn complete_expr(sch: &Schema, e: &mut Expr) {
    match e {
        Expr::Select(p) => complete_plan(sch, p),
        Expr::Field(e, _) | Expr::Some(e) => complete_expr(sch, e),
        Expr::Struct(fs) => fs.values_mut().for_each(|e| complete_expr(sch, e)),
        Expr::List(es) | Expr::Op(_, es) | Expr::Call(_, es) | Expr::Std(_, es) | Expr::Get(_, es) | Expr::Exists(_, es) => {
            es.iter_mut().for_each(|e| complete_expr(sch, e))
        }
        Expr::Match(a, _, b, c) | Expr::If(a, b, c) => {
            complete_expr(sch, a);
            complete_expr(sch, b);
            complete_expr(sch, c);
        }
        Expr::Cmp(_, a, b) | Expr::Map(a, _, b) | Expr::Filter(a, _, b) | Expr::Any(a, _, b) | Expr::All(a, _, b) | Expr::SortBy(a, _, b) => {
            complete_expr(sch, a);
            complete_expr(sch, b);
        }
        Expr::Fold(xs, z, _, _, b) => {
            complete_expr(sch, xs);
            complete_expr(sch, z);
            complete_expr(sch, b);
        }
        Expr::Lit(_) | Expr::Arg(_) | Expr::Auto(_) | Expr::Var(_) | Expr::CtxUser | Expr::CtxSession | Expr::None(_) => {}
    }
}

fn complete_plan(sch: &Schema, p: &mut Plan) {
    if let Some(t) = sch.lookup_table(&p.table) {
        for k in &t.key {
            if !p.order.iter().any(|(c, _)| c == k) {
                p.order.push((k.clone(), Dir::Asc));
            }
        }
    }
    for Related { plan, .. } in &mut p.related {
        complete_plan(sch, plan);
    }
}
