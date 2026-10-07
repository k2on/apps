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
use crate::ir::{self, Check, Expr, Field, FnKind, Function, Hold, Plan, Stmt, SPEC_VERSION};
use crate::schema::{Schema, Table as IrTable, Ty};
use crate::store::{Change, Overlay, Refusal, Store};
use crate::value::{hex, Value};

use super::cx::{self, Cx, H};
use super::input::{CheckSpec, Input};
use super::raw;
use super::schema::{Binders, Holds, IntoEffect, Query, Tables};
use super::values::{Ctx, Data};

type MwRun = Arc<dyn Fn(&dyn Fn(&str) -> H) -> Option<H> + Send + Sync>;
/// A scope's closure (`docs/plan-guards.md` D2): what it holds, described
/// once under `Emit` and never run natively.
type HoldsRun = Arc<dyn Fn() -> Vec<Hold> + Send + Sync>;
type BodyRun = Arc<dyn Fn(&[H], &[H]) + Send + Sync>;
/// A query's closure: the plan it returns, and the type of its nodes.
type PlanRun = Arc<dyn Fn(&[H], &[H]) -> (Plan, Ty) + Send + Sync>;

/// What a route's closure is: a mutator's body, run under both modes, or
/// a query's plan, described once under `Emit` (§1.4).
#[derive(Clone)]
enum Run {
    Mutator(BodyRun),
    Query(PlanRun),
}
type InputRefineRun = Arc<dyn Fn(&[H]) -> H + Send + Sync>;

#[derive(Clone)]
struct MwDecl {
    name: String,
    kind: FnKind,
    input: Vec<(String, Ty)>,
    ret: Option<Ty>,
    run: MwRun,
    /// A scope's holds; `None` for a guard or a provide.
    holds: Option<HoldsRun>,
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
    body: Run,
    /// Declared with [`Proc::client`]: a query over a chain that carries a
    /// scope, which the build holds it to (`docs/plan-guards.md` D2).
    client: bool,
}

struct Core {
    name: String,
    /// The Rust type of the tables, so that a module whose routers are
    /// over two different ones is refused rather than half-described.
    over: &'static str,
    tables: fn() -> Vec<IrTable>,
    middleware: RefCell<Vec<MwDecl>>,
    routes: RefCell<Vec<RouteDecl>>,
}

/// A router over the tables `S`, or a middleware chain built on one: every
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
pub fn router<S: Tables>(name: &str) -> Router<S> {
    Router {
        core: Rc::new(Core {
            name: name.into(),
            over: std::any::type_name::<S>(),
            tables: super::schema::tables_of::<S>,
            middleware: RefCell::new(vec![]),
            routes: RefCell::new(vec![]),
        }),
        chain: vec![],
        _t: PhantomData,
    }
}

fn tables_value<S: Tables>() -> S {
    S::open()
}

impl<S: Tables, P> Router<S, P> {
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
                f(&ctx, &tables_value::<S>()).into_effect();
                None
            }),
            holds: None,
        })
    }

    /// `docs/plan-guards.md` D2 A scope: what the person a procedure built
    /// on the router it returns is called by holds — `|ctx, db|` to one
    /// table's rows and columns, or a tuple of them:
    /// `db.users.filter(User::id.eq(ctx.user)).exclude(User::password)`.
    /// A function of `ctx` alone, run once under `Emit` and never natively:
    /// it decides what the authority serves, not what a run does. Named, as
    /// every middleware is, because a procedure's `uses` lists it. Declared
    /// on the router itself, every procedure on it inherits it; on a chain,
    /// every procedure built on the chain does.
    pub fn server<H: Holds>(&self, name: &str, f: impl Fn(&Ctx, &S) -> H + Send + Sync + 'static) -> Router<S, P> {
        self.extend(MwDecl {
            name: name.into(),
            kind: FnKind::Scope,
            input: vec![],
            ret: None,
            run: Arc::new(|_| None),
            holds: Some(Arc::new(move || f(&Ctx::current(), &tables_value::<S>()).holds())),
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
                Some(f(&ctx, &tables_value::<S>(), &j).to_h())
            }),
            holds: None,
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
        self.core.routes.borrow_mut().extend(rs.decls().into_iter().map(|RouteDeclBox(d)| d));
        Router {
            core: self.core.clone(),
            chain: vec![],
            _t: PhantomData,
        }
    }
}

impl<S: Tables> Router<S, ()> {
    /// A provide: runs before the body and hands it a value, or refuses.
    pub fn provide<J: Input, T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, &J) -> T + Send + Sync + 'static) -> Router<S, (T,)> {
        self.provide_as(name, f)
    }
    /// A mutator with no input.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, ()) -> R + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().mutation(name, f)
    }
    /// A query with no input.
    pub fn query<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, ()) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().query(name, f)
    }
    /// `docs/plan-guards.md` D2 A query with no input, over a scope
    /// ([`Proc::client`]).
    pub fn client<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, ()) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().client(name, f)
    }
}

impl<S: Tables, A: Data> Router<S, (A,)> {
    /// A second provide.
    pub fn provide<J: Input, T: Data>(&self, name: &str, f: impl Fn(&Ctx, &S, &J) -> T + Send + Sync + 'static) -> Router<S, (A, T)> {
        self.provide_as(name, f)
    }
    /// A mutator with no input.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A) -> R + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().mutation(name, f)
    }
    /// A query with no input.
    pub fn query<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().query(name, f)
    }
}

impl<S: Tables, A: Data, B: Data> Router<S, (A, B)> {
    /// A mutator with no input.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, (), A, B) -> R + Send + Sync + 'static) -> Route<S> {
        self.input::<()>().mutation(name, f)
    }
    /// A query with no input.
    pub fn query<R: 'static, Bs: Binders, N>(
        &self,
        name: &str,
        f: impl Fn(&Ctx, &S, (), A, B) -> Query<R, Bs, N> + Send + Sync + 'static,
    ) -> Route<S> {
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

impl<S: Tables, I: Input, P> Proc<S, I, P> {
    fn route(&self, name: &str, body: Run) -> Route<S> {
        Route {
            decl: RouteDecl {
                name: name.into(),
                kind: match body {
                    Run::Mutator(_) => FnKind::Mutator,
                    Run::Query(_) => FnKind::Query,
                },
                chain: self.router.chain.clone(),
                input: InputDecl::of::<I>(),
                body,
                client: false,
            },
            _t: PhantomData,
        }
    }
}

impl<S: Tables, I: Input> Proc<S, I, ()> {
    /// A mutator: `|ctx, db, input| effect`.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, I) -> R + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, _| {
            f(&Ctx::current(), &tables_value::<S>(), input_of::<I>(ins)).into_effect();
        });
        self.route(name, Run::Mutator(body))
    }
    /// A query: `|ctx, db, input| plan`, the plan returned whole (§1.9).
    pub fn query<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, I) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        let run: PlanRun = Arc::new(move |ins, _| f(&Ctx::current(), &tables_value::<S>(), input_of::<I>(ins)).finish());
        self.route(name, Run::Query(run))
    }
    /// `docs/plan-guards.md` D2 A query on a chain that carries a scope: the
    /// client's half of a read, run over what the person holds. Exactly
    /// [`Proc::query`] — the same IR, the same hash — under the name that
    /// says what it runs over; the build refuses one whose chain carries no
    /// scope, where the name would say something untrue. Mutations keep
    /// their own name: a write is a preview on the device whatever it runs
    /// over, and its server half is `ctx.private` (D3), not a second verb.
    pub fn client<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, I) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        let mut r = self.query(name, f);
        r.decl.client = true;
        r
    }
}

impl<S: Tables, I: Input, A: Data> Proc<S, I, (A,)> {
    /// A mutator: `|ctx, db, input, provided| effect`.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A) -> R + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, ps| {
            f(&Ctx::current(), &tables_value::<S>(), input_of::<I>(ins), A::from_h(ps[0])).into_effect();
        });
        self.route(name, Run::Mutator(body))
    }
    /// A query: `|ctx, db, input, provided| plan`.
    pub fn query<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        let run: PlanRun = Arc::new(move |ins, ps| f(&Ctx::current(), &tables_value::<S>(), input_of::<I>(ins), A::from_h(ps[0])).finish());
        self.route(name, Run::Query(run))
    }
    /// `docs/plan-guards.md` D2 A query over a scope ([`Proc::client`]).
    pub fn client<R: 'static, B: Binders, N>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A) -> Query<R, B, N> + Send + Sync + 'static) -> Route<S> {
        let mut r = self.query(name, f);
        r.decl.client = true;
        r
    }
}

impl<S: Tables, I: Input, A: Data, B: Data> Proc<S, I, (A, B)> {
    /// A mutator: `|ctx, db, input, a, b| effect`.
    pub fn mutation<R: IntoEffect>(&self, name: &str, f: impl Fn(&Ctx, &S, I, A, B) -> R + Send + Sync + 'static) -> Route<S> {
        let body: BodyRun = Arc::new(move |ins, ps| {
            f(
                &Ctx::current(),
                &tables_value::<S>(),
                input_of::<I>(ins),
                A::from_h(ps[0]),
                B::from_h(ps[1]),
            )
            .into_effect();
        });
        self.route(name, Run::Mutator(body))
    }
    /// A query: `|ctx, db, input, a, b| plan`.
    pub fn query<R: 'static, Bs: Binders, N>(
        &self,
        name: &str,
        f: impl Fn(&Ctx, &S, I, A, B) -> Query<R, Bs, N> + Send + Sync + 'static,
    ) -> Route<S> {
        let run: PlanRun = Arc::new(move |ins, ps| {
            f(
                &Ctx::current(),
                &tables_value::<S>(),
                input_of::<I>(ins),
                A::from_h(ps[0]),
                B::from_h(ps[1]),
            )
            .finish()
        });
        self.route(name, Run::Query(run))
    }
}

/// A tuple of routes of one router.
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
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22);
routes_tuple!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23);

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

    /// The module's canonical bytes: what an `.ark` file holds — the module
    /// a client loads, every private block stripped (`docs/plan-guards.md`
    /// D3, [`ir::strip_module`]). A module with none is the bytes it was.
    pub fn emit(&self) -> Vec<u8> {
        canon::encode(&ir::module_value(&ir::strip_module(self.build())))
    }

    /// The hash of the module a client loads ([`Module::emit`]): what a
    /// server says on every page, and what a client compares its own with.
    pub fn hash(&self) -> Vec<u8> {
        module_hash(&ir::strip_module(self.build()))
    }

    /// `docs/plan-guards.md` D3 The server's module, private blocks kept,
    /// as canonical bytes: what [`Module::build`] is. Every function is named
    /// by the hash it has in [`Module::emit`]'s.
    pub fn emit_server(&self) -> Vec<u8> {
        canon::encode(&ir::module_value(self.build()))
    }

    /// The hash of the server's module, beside [`Module::hash`]: the same
    /// where no function has a private block.
    pub fn server_hash(&self) -> Vec<u8> {
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
    let mut decls: Vec<(RouteDecl, Vec<MwDecl>)> = Vec::new();
    let tables = cores.first().map(|c| (c.tables)()).unwrap_or_default();
    if let Some(first) = cores.first() {
        for core in cores.iter().filter(|c| c.over != first.over) {
            errors.push(format!(
                "router {} is over {} and router {} over {}: a module has one set of tables",
                first.name, first.over, core.name, core.over
            ));
        }
    }
    let prev = super::helper::begin();
    for core in cores {
        let mws = core.middleware.borrow().clone();
        routers.push(ir::Router {
            name: core.name.clone(),
            uses: mws.iter().map(|m| m.name.clone()).collect(),
        });
        for mw in &mws {
            let (f, es) = emit_middleware(mw);
            functions.extend(super::helper::drain());
            functions.push(f);
            errors.extend(es);
        }
        for r in core.routes.borrow().iter() {
            if r.client && !r.chain.iter().any(|n| mws.iter().any(|m| m.name == *n && m.kind == FnKind::Scope)) {
                errors.push(format!(
                    "{}: `client` is a query over what a scope holds, and this chain carries no scope; it is a `query`",
                    r.name
                ));
            }
            let (f, es) = emit_route(core, r);
            functions.extend(super::helper::drain());
            functions.push(f);
            errors.extend(es);
            let chain = r.chain.iter().filter_map(|n| mws.iter().find(|m| m.name == *n).cloned()).collect();
            decls.push((r.clone(), chain));
        }
    }
    super::helper::end(prev);
    let schema = Schema { tables };
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
        .map(|(route, middleware)| {
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

type Emitted = (Function, Vec<String>);

fn finish(name: &str, cx: Cx) -> (Vec<(String, ir::Auto)>, ir::Block, Vec<String>) {
    let mut em = cx.into_emit();
    let body = em.body();
    let errors = em.errors.iter().map(|e| format!("{name}: {e}")).collect();
    (em.autos, body, errors)
}

fn emit_middleware(mw: &MwDecl) -> Emitted {
    let ((ret, holds), cx) = cx::run(Cx::emit(), || {
        // A scope is its holds, described as a plan is: no statement and no
        // read inside it.
        if let Some(h) = &mw.holds {
            return (None, cx::in_plan(|| h()));
        }
        let lookup = |n: &str| cx::e(Expr::Arg(n.into()));
        let r = (mw.run)(&lookup);
        (r.map(cx::expr), vec![])
    });
    let (autos, mut body, errors) = finish(&mw.name, cx);
    if let Some(e) = ret {
        body.push(Stmt::Return(Some(e)));
    }
    let f = Function {
        name: mw.name.clone(),
        kind: mw.kind,
        router: None,
        uses: vec![],
        autos,
        input: mw.input.iter().map(|(n, t)| (n.clone(), Field::plain(t.clone()))).collect(),
        refine: vec![],
        ret: mw.ret.clone(),
        body,
        plan: None,
        holds,
        private: false,
        names: BTreeMap::new(),
    };
    (f, errors)
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
    let ((input, refine, planned), cx) = cx::run(Cx::emit(), || {
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
        let refine: Vec<(Expr, Option<String>)> = r
            .input
            .refine
            .iter()
            .map(|(f, why)| (cx::in_expr(|| cx::expr(f(&ins))), why.clone()))
            .collect();
        let prov: Vec<H> = provides.iter().map(|n| cx::e(Expr::Provided(n.clone()))).collect();
        // A mutator's body is its statements; a query's is its plan, and
        // the plan's context forbids a statement or a read inside it.
        let planned = match &r.body {
            Run::Mutator(run) => {
                run(&ins, &prov);
                None
            }
            Run::Query(run) => Some(cx::in_plan(|| run(&ins, &prov))),
        };
        (input, refine, planned)
    });
    let (autos, body, errors) = finish(&r.name, cx);
    let (plan, ret) = match planned {
        Some((p, node)) => (Some(p), Some(Ty::List(Box::new(node)))),
        None => (None, None),
    };
    // `docs/plan-guards.md` D3 A body that wrote a `ctx.private` block says
    // so.
    let private = ir::has_private(&body);
    let f = Function {
        name: r.name.clone(),
        kind: r.kind,
        router: Some(core.name.clone()),
        uses: r.chain.clone(),
        autos,
        input,
        refine,
        ret,
        body,
        plan,
        holds: vec![],
        private,
        names: BTreeMap::new(),
    };
    (f, errors)
}

/// Run pure vocabulary natively, outside any procedure: the value `f`
/// computes, or the refusal (or bug) it reached. What lets a host compute
/// what a helper computes — a derived key, say — with the helper's own
/// definition rather than a copy of it. There is no store: a read inside
/// panics, as a read outside a procedure does.
pub fn evaluate<T: Data>(f: impl FnOnce() -> T) -> Result<Value, EvalFault> {
    let (out, _) = cx::run(Cx::native(eval::Ctx::default(), Args::new()), || {
        let h = f().to_h();
        match cx::native(|n| n.halt.clone()) {
            Some(fault) => Err(fault),
            None => Ok(cx::value(h)),
        }
    });
    out
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

/// What applying a mutator comes to: the changes, the verdict, or a bug.
pub type Applied = Result<Result<Vec<Change>, Refusal>, EvalError>;

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

    /// §6.2 Run the query: its input checked, its middleware, and its plan
    /// pulled — `eval::query_closure` over the closure it emitted, because
    /// a query has no native half (§1.4): `ark::view::pull` is what one
    /// means, the one evaluator of plans.
    pub fn query(&self, ctx: &eval::Ctx, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
        eval::query_closure(&self.0.schema, &self.0.closure, ctx, args, store)
    }

    /// Hold this procedure to the interpreter on one entry: run it natively
    /// and through `apply_closure` on the closure it emitted, each over its
    /// own copy of `store`, and compare the verdicts, the changes and the
    /// stores afterwards. What a peer may run in debug builds, and what the
    /// agreement tests run on every procedure.
    pub fn agrees(&self, ctx: &eval::Ctx, autos: &Args, args: &Args, store: &crate::store::MemoryStore) -> Result<Applied, String> {
        let mut native_store = store.clone();
        let mut ir_store = store.clone();
        let native = self.apply(ctx, autos, args, &mut native_store);
        let ir = eval::apply_closure(&self.0.schema, &self.0.closure, ctx, autos, args, &mut ir_store);
        if native != ir {
            return Err(format!("{}: native {native:?}, interpreted {ir:?}", self.name()));
        }
        if native_store != ir_store {
            return Err(format!("{}: the stores differ afterwards", self.name()));
        }
        Ok(native)
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
                    (v, None) => {
                        checked.insert(n.clone(), v);
                    }
                    (_, Some(msg)) => return Err(EvalFault::Verdict(Refusal::Refused(msg))),
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
            for mw in inner.middleware.iter().filter(|m| m.kind != FnKind::Scope) {
                let lookup = |n: &str| cx::lit(checked.get(n).cloned().unwrap_or(Value::Null));
                let r = (mw.run)(&lookup);
                if let Some(fault) = halt() {
                    return Err(fault);
                }
                if mw.kind == FnKind::Provide {
                    provided.push(r.ok_or_else(|| EvalFault::Bug(EvalError::NoReturn(mw.name.clone())))?);
                }
            }
            let Run::Mutator(body) = &inner.route.body else {
                return Err(EvalFault::Bug(EvalError::WrongKind(inner.route.name.clone(), inner.route.kind)));
            };
            body(&ins, &provided);
            if let Some(fault) = halt() {
                return Err(fault);
            }
            // `docs/plan-guards.md` D3 The private blocks the body reached,
            // at the authority alone, last and in order: a refusal in one is
            // the entry's verdict.
            for private in cx::native(|n| std::mem::take(&mut n.deferred)) {
                private();
                if let Some(fault) = halt() {
                    return Err(fault);
                }
            }
            Ok(None)
        });
        let native = cx.into_native();
        out.map(|v| (native.changes, v))
    }
}
