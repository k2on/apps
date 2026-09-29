//! §2.2 `ECall`: a helper is an ordinary Rust function of the vocabulary's
//! types whose body is `helper(..)`. Called under `Emit`, it is
//! `ECall name args`, and the first call in a module build emits the helper
//! itself (its body over `EArg`s of its parameters) into the module,
//! immediately before the first function that calls it; under `Native` it
//! is its body, run on the values.
//!
//! ```ignore
//! pub fn movement_key(work_id: Text, no: Int) -> Text {
//!     helper("movement_key", (("work_id", work_id), ("no", no)), |work_id: Text, no: Int| {
//!         concat(list([work_id, "#".into(), no.to_text()]))
//!     })
//! }
//! ```
//!
//! The names are the helper's parameters in the IR (`fnInput`), written
//! once beside the values; the closure's parameters are the same names,
//! typed, because a closure passed through a trait bound cannot have them
//! inferred. A helper reads nothing and refuses nothing (the verifier holds
//! it to that); a body that captures a value from its caller rather than
//! taking it as a parameter emits a symbol or an argument its own function
//! does not have, and the verifier says so.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{Expr, Field, FnKind, Function, Stmt};
use crate::schema::Ty;

use super::cx::{self, H};
use super::values::Data;

/// A helper's parameters: `("name", value)`, or a tuple of them.
pub trait Params {
    /// The values the body takes, as a tuple.
    type Vals;
    #[doc(hidden)]
    fn names(&self) -> Vec<&'static str>;
    #[doc(hidden)]
    fn tys() -> Vec<Ty>;
    #[doc(hidden)]
    fn handles(&self) -> Vec<H>;
    #[doc(hidden)]
    fn from_hs(hs: &[H]) -> Self::Vals;
}

/// A helper's body: a closure of as many values as it has parameters.
pub trait Body<V, R> {
    #[doc(hidden)]
    fn run(self, v: V) -> R;
}

impl<A: Data> Params for (&'static str, A) {
    type Vals = (A,);
    fn names(&self) -> Vec<&'static str> {
        vec![self.0]
    }
    fn tys() -> Vec<Ty> {
        vec![A::ty()]
    }
    fn handles(&self) -> Vec<H> {
        vec![self.1.to_h()]
    }
    fn from_hs(hs: &[H]) -> (A,) {
        (A::from_h(hs[0]),)
    }
}

macro_rules! params_tuple {
    ($($v:ident . $i:tt),+) => {
        impl<$($v: Data),+> Params for ($((&'static str, $v),)+) {
            type Vals = ($($v,)+);
            fn names(&self) -> Vec<&'static str> {
                vec![$(self.$i.0),+]
            }
            fn tys() -> Vec<Ty> {
                vec![$($v::ty()),+]
            }
            fn handles(&self) -> Vec<H> {
                vec![$(self.$i.1.to_h()),+]
            }
            fn from_hs(hs: &[H]) -> Self::Vals {
                ($($v::from_h(hs[$i]),)+)
            }
        }
        impl<$($v,)+ R, F: FnOnce($($v),+) -> R> Body<($($v,)+), R> for F {
            fn run(self, v: ($($v,)+)) -> R {
                self($(v.$i),+)
            }
        }
    };
}
params_tuple!(A.0);
params_tuple!(A.0, B.1);
params_tuple!(A.0, B.1, C.2);
params_tuple!(A.0, B.1, C.2, D.3);
params_tuple!(A.0, B.1, C.2, D.3, E.4);
params_tuple!(A.0, B.1, C.2, D.3, E.4, F2.5);

/// `ECall name args` under `Emit` (emitting the helper on its first call in
/// a build); the body on the values under `Native`.
pub fn helper<P: Params, R: Data>(name: &'static str, params: P, body: impl Body<P::Vals, R>) -> R {
    let hs = params.handles();
    if !cx::emitting() {
        return body.run(P::from_hs(&hs));
    }
    let names = params.names();
    if first_call(name) {
        let args: Vec<H> = names.iter().map(|n| cx::e(Expr::Arg((*n).into()))).collect();
        let (ret, mut block) = cx::detached(|| cx::expr(body.run(P::from_hs(&args)).to_h()));
        block.push(Stmt::Return(Some(ret)));
        let f = Function {
            name: name.into(),
            kind: FnKind::Helper,
            router: None,
            uses: vec![],
            autos: vec![],
            input: names.iter().zip(P::tys()).map(|(n, t)| ((*n).into(), Field::plain(t))).collect(),
            refine: vec![],
            ret: Some(R::ty()),
            body: block,
            plan: None,
            names: BTreeMap::new(),
        };
        HELPERS.with(|h| {
            if let Some(reg) = h.borrow_mut().as_mut() {
                reg.ready.push(f);
            }
        });
    }
    R::from_h(cx::e(Expr::Call(name.into(), hs.iter().map(|h| cx::expr(*h)).collect())))
}

#[derive(Default)]
pub(crate) struct Registry {
    seen: BTreeSet<String>,
    ready: Vec<Function>,
}

thread_local! {
    static HELPERS: RefCell<Option<Registry>> = const { RefCell::new(None) };
}

// Whether this is the helper's first call in the build in progress (and
// mark it called). Outside a build nothing is emitted.
fn first_call(name: &str) -> bool {
    HELPERS.with(|h| match h.borrow_mut().as_mut() {
        Some(reg) => reg.seen.insert(name.into()),
        None => false,
    })
}

/// Start recording helpers for a module build.
pub(crate) fn begin() -> Option<Registry> {
    HELPERS.with(|h| h.borrow_mut().replace(Registry::default()))
}

/// The helpers emitted since the last call, in the order they finished:
/// a helper a helper calls finishes first.
pub(crate) fn drain() -> Vec<Function> {
    HELPERS.with(|h| h.borrow_mut().as_mut().map(|r| std::mem::take(&mut r.ready)).unwrap_or_default())
}

/// Stop recording, restoring what was there before.
pub(crate) fn end(prev: Option<Registry>) {
    HELPERS.with(|h| *h.borrow_mut() = prev);
}
