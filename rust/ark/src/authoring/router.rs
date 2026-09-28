//! §1.1, §1.2, §2.5 Routers, middleware, procedures and the module: what a
//! domain declares, run once under `Emit` to be the module and on every
//! entry under `Native` to apply it.

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use crate::canon;
use crate::eval::{self, Args, Checked, EvalError, EvalFault};
use crate::hash::{closure, function_hash, module_hash, Closure, FnHash};
use crate::ir::{self, Check, Expr, Field, FnKind, Function, Stmt, SPEC_VERSION};
use crate::schema::{Schema, Scope as IrScope, Table as IrTable, Ty};
use crate::store::{Change, Overlay, Refusal, Store};
use crate::value::{hex, Value};

use super::cx::{self, Cx, H};
use super::input::{CheckSpec, Input};
use super::raw;
use super::schema::{IntoEffect, Scope};
use super::values::{Ctx, Data};

type MwRun = Arc<dyn Fn(&dyn Fn(&str) -> H) -> Option<H> + Send + Sync>;
type BodyRun = Arc<dyn Fn(&[H], &[H]) -> Option<H> + Send + Sync>;
type InputRefineRun = Arc<dyn Fn(&[H]) -> H + Send + Sync>;

#[derive(Clone)]
struct MwDecl {
    name: String,
    kind: FnKind,
    input: Vec<(String, Ty)>,
    ret: Option<Ty>,
    run: MwRun,
}

#[derive(Clone)]
struct InputDecl {
    fields: Vec<(String, Ty, Vec<CheckSpec>)>,
    refine: Vec<(InputRefineRun, Option<String>)>,
}

impl InputDecl {
    fn of<I: Input>() -> InputDecl {
        let o = I::schema();
        let n = o.fields.len();
        InputDecl {
            fields: o.fields,
            refine: o
                .refine
                .into_iter()
                .map(|(f, why)| {
                    let run: InputRefineRun = Arc::new(move |hs: &[H]| {
                        let i: I = raw::assemble(&hs[..n], std::any::type_name::<I>());
                        f(&i).to_h()
                    });
                    (run, why)
                })
                .collect(),
        }
    }
}

#[derive(Clone)]
struct RouteDecl {
    name: String,
    kind: FnKind,
    chain: Vec<String>,
    input: InputDecl,
    ret: Option<Ty>,
    body: BodyRun,
}

struct Core {
    name: String,
    scope: &'static str,
    middleware: RefCell<Vec<MwDecl>>,
    routes: RefCell<Vec<RouteDecl>>,
}

/// A router over scope `S`, or a middleware chain built on one: every
/// `guard` and `provide` returns a router that runs it, and `P` is what
/// the provides so far hand a body, in order.
pub struct Router<S, P = ()> {
    core: Rc<Core>,
    chain: Vec<String>,
    _t: PhantomData<fn() -> (S, P)>,
}

impl<S, P> Clone for Router<S, P> {
    fn clone(&self) -> Self {
        Router {
            core: self.core.clone(),
            chain: self.chain.clone(),
            _t: PhantomData,
        }
    }
}

/// A router named `name`, over `S`.
pub fn router<S: Scope>(name: &str) -> Router<S> {
    Router {
        core: Rc::new(Core {
            name: name.into(),
            scope: S::NAME,
            middleware: RefCell::new(vec![]),
            routes: RefCell::new(vec![]),
        }),
        chain: vec![],
        _t: PhantomData,
    }
}

fn scope_value<S: Scope>() -> S {
    raw::assemble(&[], S::NAME)
}

impl<S: Scope, P> Router<S, P> {
    fn extend<Q>(&self, mw: MwDecl) -> Router<S, Q> {
        let name = mw.name.clone();
        self.core.middleware.borrow_mut().push(mw);
        let mut chain = self.chain.clone();
        chain.push(name);
        Router {
            core: self.core.clone(),
            chain,
            _t: PhantomData,
        }
    }

    /// A guard: runs before the body of every procedure built on the router
    /// it returns, and may refuse.
    pub fn guard<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S) -> R + Send + Sync + 'static) -> Router<S, P> {
        self.extend(MwDecl {
            name: name.into(),
            kind: FnKind::Guard,
            input: vec![],
            ret: None,
            run: Arc::new(move |_| {
                let ctx = Ctx::current();
                f(&ctx, &scope_value::<S>()).into_effect();
                None
            }),
        })
    }

    fn provide_as<J: Input, T: Data, Q>(&self, name: &str, f: impl Fn(&Ctx, &S, &J) -> T + Send + Sync + 'static) -> Router<S, Q> {
        let fields: Vec<(String, Ty)> = J::schema().fields.into_iter().map(|(n, t, _)| (n, t)).collect();
        let names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
        let label = name.to_string();
        self.extend(MwDecl {
            name: name.into(),
            kind: FnKind::Provide,
            input: fields,
            ret: Some(T::ty()),
            run: Arc::new(move |lookup| {
                let hs: Vec<H> = names.iter().map(|n| lookup(n)).collect();
                let j: J = raw::assemble(&hs, &label);
                let ctx = Ctx::current();
                Some(f(&ctx, &scope_value::<S>(), &j).to_h())
            }),
        })
    }

    /// The input the next procedure takes.
    pub fn input<I: Input>(&self) -> Proc<S, I, P> {
        Proc {
            router: self.clone(),
            _t: PhantomData,
        }
    }

    /// The router, with these procedures on it.
    pub fn routes(&self, rs: impl Routes<S>) -> Router<S> {
        self.core.routes.borrow_mut().extend(rs.decls());
        Router {
            core: self.core.clone(),
            chain: vec![],
            _t: PhantomData,
        }
    }
}

impl<S: Scope> Router<S, ()> {
    /// A provide: runs before the body and hands it a value, or refuses.
    pub fn provide<J: Input, T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, &J) -> T + Send + Sync + 'static) -> Router<S, (T,)> {
        self.provide_as(name, f)
    }
    /// A mutator with no input.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, ()) -> R + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().mutation(name, f)
    }
    /// A query with no input.
    pub fn query<T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, ()) -> T + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().query(name, f)
    }
}

impl<S: Scope, A: Data> Router<S, (A,)> {
    /// A second provide.
    pub fn provide<J: Input, T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, &J) -> T + Send + Sync + 'static) -> Router<S, (A, T)> {
        self.provide_as(name, f)
    }
    /// A mutator with no input.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A) -> R + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().mutation(name, f)
    }
    /// A query with no input.
    pub fn query<T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A) -> T + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().query(name, f)
    }
}

impl<S: Scope, A: Data, B: Data> Router<S, (A, B)> {
    /// A mutator with no input.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A, B) -> R + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().mutation(name, f)
    }
    /// A query with no input.
    pub fn query<T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A, B) -> T + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().query(name, f)
    }
}

/// A router with its next procedure's input chosen.
pub struct Proc<S, I, P> {
    router: Router<S, P>,
    _t: PhantomData<fn() -> I>,
}

/// A procedure, declared: what [`Router::routes`] takes.
pub struct Route<S> {
    decl: RouteDecl,
    _t: PhantomData<fn() -> S>,
}

fn input_of<I: Input>(hs: &[H]) -> I {
    raw::assemble(hs, std::any::type_name::<I>())
}

impl<S: Scope, I: Input, P> Proc<S, I, P> {
    fn route(&self, name: &str, kind: FnKind, ret: Option<Ty>, body: BodyRun) -> Route<S> {
        Route {
            decl: RouteDecl {
                name: name.into(),
                kind,
                chain: self.router.chain.clone(),
                input: InputDecl::of::<I>(),
                ret,
                body,
            },
            _t: PhantomData,
        }
    }
}

impl<S: Scope, I: Input> Proc<S, I, ()> {
    /// A mutator: `|ctx, db, input| effect`.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, I) -> R + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, _| {
            f(&Ctx::current(), &scope_value::<S>(), input_of::<I>(ins)).into_effect();
            None
        });
        self.route(name, FnKind::Mutator, None, body)
    }
    /// A query: `|ctx, db, input| value`.
    pub fn query<T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, I) -> T + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, _| Some(f(&Ctx::current(), &scope_value::<S>(), input_of::<I>(ins)).to_h()));
        self.route(name, FnKind::Query, Some(T::ty()), body)
    }
}

impl<S: Scope, I: Input, A: Data> Proc<S, I, (A,)> {
    /// A mutator: `|ctx, db, input, provided| effect`.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A) -> R + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, ps| {
            f(&Ctx::current(), &scope_value::<S>(), input_of::<I>(ins), A::from_h(ps[0])).into_effect();
            None
        });
        self.route(name, FnKind::Mutator, None, body)
    }
    /// A query: `|ctx, db, input, provided| value`.
    pub fn query<T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A) -> T + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, ps| Some(f(&Ctx::current(), &scope_value::<S>(), input_of::<I>(ins), A::from_h(ps[0])).to_h()));
        self.route(name, FnKind::Query, Some(T::ty()), body)
    }
}

impl<S: Scope, I: Input, A: Data, B: Data> Proc<S, I, (A, B)> {
    /// A mutator: `|ctx, db, input, a, b| effect`.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A, B) -> R + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, ps| {
            f(&Ctx::current(), &scope_value::<S>(), input_of::<I>(ins), A::from_h(ps[0]), B::from_h(ps[1])).into_effect();
            None
        });
        self.route(name, FnKind::Mutator, None, body)
    }
    /// A query: `|ctx, db, input, a, b| value`.
    pub fn query<T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A, B) -> T + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, ps| {
            Some(f(&Ctx::current(), &scope_value::<S>(), input_of::<I>(ins), A::from_h(ps[0]), B::from_h(ps[1])).to_h())
        });
        self.route(name, FnKind::Query, Some(T::ty()), body)
    }
}

/// A tuple of routes of one scope.
pub trait Routes<S> {
    #[doc(hidden)]
    fn decls(self) -> Vec<RouteDeclBox>;
}

#[doc(hidden)]
pub struct RouteDeclBox(RouteDecl);

impl<S> Routes<S> for Route<S> {
    fn decls(self) -> Vec<RouteDeclBox> {
        vec![RouteDeclBox(self.decl)]
    }
}

macro_rules! routes_tuple {
    ($($i:tt),+) => {
        impl<S> Routes<S> for ($(routes_tuple!(@t $i S),)+) {
            fn decls(self) -> Vec<RouteDeclBox> {
                vec![$(RouteDeclBox(self.$i.decl)),+]
            }
        }
    };
    (@t $i:tt $S:ident) => { Route<$S> };
}
routes_tuple!(0);
routes_tuple!(0, 1);
routes_tuple!(0, 1, 2);
routes_tuple!(0, 1, 2, 3);
routes_tuple!(0, 1, 2, 3, 4);
routes_tuple!(0, 1, 2, 3, 4, 5);
routes_tuple!(0, 1, 2, 3, 4, 5, 6);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11);

impl<S> Extend<RouteDeclBox> for Vec<RouteDecl> {
    fn extend<T: IntoIterator<Item = RouteDeclBox>>(&mut self, iter: T) {
        for RouteDeclBox(d) in iter {
            self.push(d);
        }
    }
}

// ---------------------------------------------------------------------------
// The module

/// A tuple of routers: what [`Module::new`] takes.
pub trait Routers {
    #[doc(hidden)]
    fn cores(self) -> Vec<CoreBox>;
}

#[doc(hidden)]
pub struct CoreBox(Rc<Core>);

impl<S, P> Routers for Router<S, P> {
    fn cores(self) -> Vec<CoreBox> {
        vec![CoreBox(self.core)]
    }
}

macro_rules! routers_tuple {
    ($($v:ident . $i:tt),+) => {
        impl<$($v: Routers),+> Routers for ($($v,)+) {
            fn cores(self) -> Vec<CoreBox> {
                let mut out = Vec::new();
                $(out.extend(self.$i.cores());)+
                out
            }
        }
    };
}
routers_tuple!(A.0);
routers_tuple!(A.0, B.1);
routers_tuple!(A.0, B.1, C.2);
routers_tuple!(A.0, B.1, C.2, D.3);
routers_tuple!(A.0, B.1, C.2, D.3, E.4);
routers_tuple!(A.0, B.1, C.2, D.3, E.4, F.5);
routers_tuple!(A.0, B.1, C.2, D.3, E.4, F.5, G.6);
routers_tuple!(A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7);

struct Built {
    module: ir::Module,
    procedures: Vec<(FnHash, Procedure)>,
}

/// A domain: its routers, in order. Run under `Emit` once, it is the
/// module ([`Module::emit`]); its procedures run under `Native`
/// ([`Module::procedures`]).
pub struct Module {
    cores: Vec<Rc<Core>>,
    built: OnceCell<Result<Built, Vec<String>>>,
}

impl Module {
    /// Every router, in the order the module lists them.
    pub fn new(routers: impl Routers) -> Module {
        Module {
            cores: routers.cores().into_iter().map(|CoreBox(c)| c).collect(),
            built: OnceCell::new(),
        }
    }

    fn built(&self) -> &Result<Built, Vec<String>> {
        self.built.get_or_init(|| build(&self.cores))
    }

    /// The module as the IR, verified — orders completed and every function
    /// normalised — or everything wrong with it.
    pub fn try_build(&self) -> Result<&ir::Module, Vec<String>> {
        self.built().as_ref().map(|b| &b.module).map_err(|e| e.clone())
    }

    /// The module as the IR, verified.
    ///
    /// # Panics
    ///
    /// On an authoring error or a module the verifier refuses: a domain
    /// that does not emit is a bug in the domain, found the first time it
    /// runs.
    pub fn build(&self) -> &ir::Module {
        match self.built() {
            Ok(b) => &b.module,
            Err(es) => panic!("the domain does not emit a module that verifies:\n  {}", es.join("\n  ")),
        }
    }

    /// The module's canonical bytes: what an `.ark` file holds.
    pub fn emit(&self) -> Vec<u8> {
        canon::encode(&ir::module_value(self.build()))
    }

    /// The module hash.
    pub fn hash(&self) -> Vec<u8> {
        module_hash(self.build())
    }

    /// Every procedure, by the hash of its closure, to run natively.
    pub fn procedures(&self) -> Vec<(FnHash, Procedure)> {
        self.build();
        match self.built() {
            Ok(b) => b.procedures.clone(),
            Err(_) => unreachable!("built above"),
        }
    }

    /// One procedure, by name.
    pub fn procedure(&self, name: &str) -> Option<(FnHash, Procedure)> {
        self.procedures().into_iter().find(|(_, p)| p.name() == name)
    }
}

fn build(cores: &[Rc<Core>]) -> Result<Built, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut functions: Vec<Function> = Vec::new();
    let mut routers: Vec<ir::Router> = Vec::new();
    let mut touched: Vec<(String, IrTable)> = Vec::new();
    let mut scopes: Vec<String> = Vec::new();
    let mut decls: Vec<(String, RouteDecl, Vec<MwDecl>)> = Vec::new();
    for core in cores {
        if !scopes.iter().any(|s| s == core.scope) {
            scopes.push(core.scope.into());
        }
        let mws = core.middleware.borrow().clone();
        routers.push(ir::Router {
            name: core.name.clone(),
            scope: core.scope.into(),
            uses: mws.iter().map(|m| m.name.clone()).collect(),
        });
        for mw in &mws {
            let (f, t, es) = emit_middleware(core, mw);
            functions.push(f);
            touched.extend(t);
            errors.extend(es);
        }
        for r in core.routes.borrow().iter() {
            let (f, t, es) = emit_route(core, r);
            functions.push(f);
            touched.extend(t);
            errors.extend(es);
            let chain = r.chain.iter().filter_map(|n| mws.iter().find(|m| m.name == *n).cloned()).collect();
            decls.push((core.scope.into(), r.clone(), chain));
        }
    }
    let schema = schema_of(&scopes, &touched, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    let raw_module = ir::Module {
        spec: SPEC_VERSION,
        schema,
        functions,
        routers,
        live: vec![],
    };
    let module = crate::verify::verify(&raw_module).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>())?;
    let module = named(module);
    let procedures = decls
        .into_iter()
        .map(|(_, route, middleware)| {
            let f = module.lookup_function(&route.name).expect("emitted above");
            let c = closure(&module, f);
            let h = function_hash(&c);
            let p = Procedure(Arc::new(Inner {
                hash: h.clone(),
                closure: c,
                schema: module.schema.clone(),
                route,
                middleware,
            }));
            (h, p)
        })
        .collect();
    Ok(Built { module, procedures })
}

// §2.5 `fnNames`: the host exposes no names, so each symbol is `_n`.
fn named(mut m: ir::Module) -> ir::Module {
    for f in &mut m.functions {
        f.names = crate::ir::normalize::binders(f).into_iter().map(|s| (s, format!("_{s}"))).collect();
    }
    m
}

// The scopes in the order the routers name them; in each, the tables its
// functions touched, in the order they were first touched, parents before
// children. A table touched from two scopes is an error.
fn schema_of(scopes: &[String], touched: &[(String, IrTable)], errors: &mut Vec<String>) -> Schema {
    let mut home: BTreeMap<String, String> = BTreeMap::new();
    for (s, t) in touched {
        match home.get(&t.name) {
            Some(h) if h != s => errors.push(format!("the table {} is used from the scopes {h} and {s}; a table is in one scope", t.name)),
            _ => {
                home.insert(t.name.clone(), s.clone());
            }
        }
    }
    let mut out = Vec::new();
    for s in scopes {
        let mut pending: Vec<IrTable> = Vec::new();
        for (sc, t) in touched {
            if sc == s && !pending.iter().any(|x| x.name == t.name) {
                pending.push(t.clone());
            }
        }
        let mut placed: Vec<IrTable> = Vec::new();
        while !pending.is_empty() {
            let names: Vec<String> = pending.iter().map(|t| t.name.clone()).collect();
            let i = pending
                .iter()
                .position(|t| t.refs.iter().all(|r| r.table == t.name || !names.contains(&r.table)))
                .unwrap_or(0);
            placed.push(pending.remove(i));
        }
        out.push(IrScope {
            name: s.clone(),
            tables: placed,
        });
    }
    Schema { scopes: out }
}

type Emitted = (Function, Vec<(String, IrTable)>, Vec<String>);

fn finish(name: &str, cx: Cx) -> (Vec<(String, ir::Auto)>, ir::Block, Vec<(String, IrTable)>, Vec<String>) {
    let mut em = cx.into_emit();
    let body = em.body();
    let errors = em.errors.iter().map(|e| format!("{name}: {e}")).collect();
    (em.autos, body, em.touched, errors)
}

fn emit_middleware(core: &Core, mw: &MwDecl) -> Emitted {
    let (ret, cx) = cx::run(Cx::emit(core.scope), || {
        let lookup = |n: &str| cx::e(Expr::Arg(n.into()));
        let r = (mw.run)(&lookup);
        r.map(cx::expr)
    });
    let (autos, mut body, touched, errors) = finish(&mw.name, cx);
    if let Some(e) = ret {
        body.push(Stmt::Return(Some(e)));
    }
    let f = Function {
        name: mw.name.clone(),
        kind: mw.kind,
        scope: Some(core.scope.into()),
        router: None,
        uses: vec![],
        autos,
        input: mw.input.iter().map(|(n, t)| (n.clone(), Field::plain(t.clone()))).collect(),
        refine: vec![],
        ret: mw.ret.clone(),
        body,
        names: BTreeMap::new(),
    };
    (f, touched, errors)
}

fn emit_route(core: &Core, r: &RouteDecl) -> Emitted {
    let provides: Vec<String> = {
        let mws = core.middleware.borrow();
        r.chain
            .iter()
            .filter(|n| mws.iter().any(|m| m.name == **n && m.kind == FnKind::Provide))
            .cloned()
            .collect()
    };
    let ((input, refine, ret), cx) = cx::run(Cx::emit(core.scope), || {
        let ins: Vec<H> = r.input.fields.iter().map(|(n, _, _)| cx::e(Expr::Arg(n.clone()))).collect();
        let mut input = Vec::with_capacity(r.input.fields.len());
        for (n, ty, checks) in &r.input.fields {
            let mut out = Vec::with_capacity(checks.len());
            for c in checks {
                out.push(match c {
                    CheckSpec::Plain(c) => c.clone(),
                    CheckSpec::Refine(f, why) => {
                        let arg = cx::e(Expr::Arg(n.clone()));
                        Check::Refine(cx::in_expr(|| cx::expr(f(arg))), why.clone())
                    }
                });
            }
            input.push((n.clone(), Field { ty: ty.clone(), checks: out }));
        }
        let refine: Vec<(Expr, Option<String>)> = r.input.refine.iter().map(|(f, why)| (cx::in_expr(|| cx::expr(f(&ins))), why.clone())).collect();
        let prov: Vec<H> = provides.iter().map(|n| cx::e(Expr::Provided(n.clone()))).collect();
        let ret = (r.body)(&ins, &prov).map(cx::expr);
        (input, refine, ret)
    });
    let (autos, mut body, touched, errors) = finish(&r.name, cx);
    if let Some(e) = ret {
        body.push(Stmt::Return(Some(e)));
    }
    let f = Function {
        name: r.name.clone(),
        kind: r.kind,
        scope: Some(core.scope.into()),
        router: Some(core.name.clone()),
        uses: r.chain.clone(),
        autos,
        input,
        refine,
        ret: r.ret.clone(),
        body,
        names: BTreeMap::new(),
    };
    (f, touched, errors)
}

// ---------------------------------------------------------------------------
// A procedure, native

struct Inner {
    hash: FnHash,
    closure: Closure,
    schema: Schema,
    route: RouteDecl,
    middleware: Vec<MwDecl>,
}

/// One mutator or query of a domain, run `Native`: the domain's own Rust
/// applying an entry, as [`eval::apply_closure`] applies the closure it
/// emitted — and held to it by the runtime's tests.
#[derive(Clone)]
pub struct Procedure(Arc<Inner>);

impl std::fmt::Debug for Procedure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Procedure({} {})", self.name(), hex(&self.0.hash))
    }
}

impl PartialEq for Procedure {
    fn eq(&self, other: &Procedure) -> bool {
        self.0.hash == other.0.hash
    }
}

impl Eq for Procedure {}

type Outcome<T> = Result<(Vec<Change>, T), EvalFault>;

impl Procedure {
    pub fn name(&self) -> &str {
        &self.0.route.name
    }

    pub fn kind(&self) -> FnKind {
        self.0.route.kind
    }

    /// The hash an entry names this procedure by.
    pub fn hash(&self) -> &FnHash {
        &self.0.hash
    }

    /// The closure it emitted: what a peer without it replays instead.
    pub fn closure(&self) -> &Closure {
        &self.0.closure
    }

    /// The function it emitted.
    pub fn function(&self) -> &Function {
        &self.0.closure.function
    }

    /// The schema of the module it is from.
    pub fn schema(&self) -> &Schema {
        &self.0.schema
    }

    /// §6.1 Apply the mutator to a store, as `apply_closure` would: `Ok(Ok)`
    /// is the changes, applied; `Ok(Err)` the verdict, the store untouched;
    /// `Err` a bug.
    pub fn apply(&self, ctx: &eval::Ctx, autos: &Args, args: &Args, store: &mut dyn Store) -> Result<Result<Vec<Change>, Refusal>, EvalError> {
        if self.kind() != FnKind::Mutator {
            return Err(EvalError::WrongKind(self.name().into(), self.kind()));
        }
        for (a, _) in &self.function().autos {
            if !autos.contains_key(a) {
                return Err(EvalError::MissingAuto(a.clone()));
            }
        }
        let outcome = {
            let mut overlay = Overlay::new(&*store);
            raw::with_store(&mut overlay, || self.run(ctx, autos, args))
        };
        match outcome {
            Ok((changes, _)) => {
                store.apply_changes(&changes);
                Ok(Ok(changes))
            }
            Err(EvalFault::Verdict(r)) => Ok(Err(r)),
            Err(EvalFault::Bug(e)) => Err(e),
        }
    }

    /// §6.2 Run the query, as `query_closure` would.
    pub fn query(&self, ctx: &eval::Ctx, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
        if self.kind() != FnKind::Query {
            return Err(EvalFault::Bug(EvalError::WrongKind(self.name().into(), self.kind())));
        }
        let mut overlay = Overlay::new(store);
        let (_, v) = raw::with_store(&mut overlay, || self.run(ctx, &Args::new(), args))?;
        v.ok_or_else(|| EvalFault::Bug(EvalError::NoReturn(self.name().into())))
    }

    /// §1.3 The form validator over a partial input.
    pub fn check(&self, ctx: &eval::Ctx, partial: &Args, store: &dyn Store) -> Result<Checked, EvalError> {
        eval::check(&self.0.schema, &self.0.closure, ctx, partial, store)
    }

    // The whole procedure under Native: the input checked, the middleware,
    // the body; the changes it made and what it returned.
    fn run(&self, ctx: &eval::Ctx, autos: &Args, args: &Args) -> Outcome<Option<Value>> {
        let inner = &*self.0;
        let (out, cx) = cx::run(Cx::native(ctx.clone(), autos.clone()), || -> Result<Option<Value>, EvalFault> {
            let halt = || cx::native(|n| n.halt.clone());
            let fields = &inner.route.input.fields;
            for (n, _, _) in fields {
                if !args.contains_key(n) {
                    return Err(EvalFault::Bug(EvalError::MissingArg(n.clone())));
                }
            }
            let mut checked = args.clone();
            for (i, (n, _, specs)) in fields.iter().enumerate() {
                let field = &inner.closure.function.input[i].1;
                let exists = |t: &str, k: &Value| raw::store(|st| st.exists(t, std::slice::from_ref(k)));
                let mut refine = |j: usize, v: &Value| -> Result<bool, EvalFault> {
                    let CheckSpec::Refine(f, _) = &specs[j] else { return Ok(true) };
                    let r = cx::value(f(cx::lit(v.clone())));
                    if let Some(fault) = halt() {
                        return Err(fault);
                    }
                    Ok(r == Value::Bool(true))
                };
                match eval::check_field_with(n, field, checked[n].clone(), &exists, &mut refine)? {
                    Ok(v) => {
                        checked.insert(n.clone(), v);
                    }
                    Err(msg) => return Err(EvalFault::Verdict(Refusal::Refused(msg))),
                }
            }
            let ins: Vec<H> = fields.iter().map(|(n, _, _)| cx::lit(checked[n].clone())).collect();
            for (f, why) in &inner.route.input.refine {
                let ok = cx::value(f(&ins));
                if let Some(fault) = halt() {
                    return Err(fault);
                }
                if ok != Value::Bool(true) {
                    return Err(EvalFault::Verdict(Refusal::Refused(why.clone().unwrap_or_else(|| "invalid".into()))));
                }
            }
            let mut provided = Vec::new();
            for mw in &inner.middleware {
                let lookup = |n: &str| cx::lit(checked.get(n).cloned().unwrap_or(Value::Null));
                let r = (mw.run)(&lookup);
                if let Some(fault) = halt() {
                    return Err(fault);
                }
                if mw.kind == FnKind::Provide {
                    provided.push(r.ok_or_else(|| EvalFault::Bug(EvalError::NoReturn(mw.name.clone())))?);
                }
            }
            let r = (inner.route.body)(&ins, &provided);
            if let Some(fault) = halt() {
                return Err(fault);
            }
            Ok(r.map(cx::value))
        });
        let native = cx.into_native();
        out.map(|v| (native.changes, v))
    }
}
