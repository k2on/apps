//! §1.12 The one lowering: a function body's restricted Rust into Rust
//! that builds its §1.4 `Expr` through `ark5::author`. What is accepted is
//! `spec5/AUTHORING.md`'s table, row for row; anything else is an error
//! naming the construct ([`no`]).
//!
//! Binders are seen here, so variables are De Bruijn indices computed here
//! ([`Frame`]): a `let` binds one, a closure its parameters, a match arm
//! its variant's fields — all of them, in declaration order, which only
//! the derive knows, so an arm's offsets are the derive's constants, and a
//! `_` arm standing for several variants binds however many each has. A
//! variable is a Rust variable `__a_<name>` holding a witness of its type;
//! a use is `__a_<name>.at(i)`. Field names, variant names and struct
//! literals' completeness are left to rustc over what the derives wrote:
//! a field is read by name, never by position.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::punctuated::Punctuated;
use syn::{BinOp, Block, Expr, ExprClosure, ExprStruct, Ident, Lit, Macro, Member, Pat, Path, Stmt, Token, Type, UnOp};

/// A construct outside the subset, named.
pub fn no(at: impl ToTokens, what: &str) -> syn::Error {
    syn::Error::new_spanned(at, format!("{what} is not in the subset ark lowers (spec5/AUTHORING.md)"))
}

/// A binder: its names, each at an offset from its innermost, and how many
/// it binds.
pub struct Frame {
    names: Vec<(String, TokenStream)>,
    size: TokenStream,
}

impl Frame {
    pub fn of(names: impl IntoIterator<Item = (String, u32)>) -> Frame {
        let names: Vec<_> = names.into_iter().map(|(n, o)| (n, quote!(#o))).collect();
        let k = names.len() as u32;
        Frame { names, size: quote!(#k) }
    }
}

pub struct Lower {
    pub scope: Vec<Frame>,
    pub ctx: Option<String>,
    pub db: Option<(String, Type)>,
    pub me: String,
}

/// The methods an expression may be called with: `ark5::author`'s.
const METHODS: &[&str] = &[
    "trim", "is_empty", "lower", "is_alnum", "chars", "len", "starts_with", "split_once", "fnv1a64", "utf8", "to_text", "min", "max",
    "clamp", "abs", "hex", "sha256", "text_of_id", "first", "last", "contains", "reverse", "concat", "insert", "remove_at", "move_at",
    "slice", "take", "filter", "map", "join", "group", "sort", "is_some", "is_none", "unwrap", "unwrap_or", "map_or", "sum",
];

fn rv(id: &impl std::fmt::Display) -> Ident {
    format_ident!("__a_{}", id.to_string())
}

fn upper(i: &Ident) -> bool {
    i.to_string().starts_with(|c: char| c.is_uppercase())
}

/// `E::V`: a variant, its sum's path and its name.
fn variant_of(p: &Path) -> Option<(Path, Ident)> {
    let n = p.segments.len();
    (n >= 2 && upper(&p.segments[n - 1].ident) && upper(&p.segments[n - 2].ident)).then(|| {
        let mut e = p.clone();
        let v = e.segments.pop().unwrap().into_value().ident;
        e.segments.pop_punct();
        (e, v)
    })
}

fn seq(first: Vec<TokenStream>, last: TokenStream) -> TokenStream {
    match first.is_empty() {
        true => last,
        false => quote!(::ark5::author::seq(vec![#(#first),*], #last)),
    }
}

fn is_ok_unit(e: &Expr) -> bool {
    matches!(e, Expr::Call(c) if matches!(&*c.func, Expr::Path(p) if p.path.is_ident("Ok"))
        && matches!(c.args.first(), Some(Expr::Tuple(t)) if t.elems.is_empty()))
}

/// What an expression outside the subset is called.
fn what(e: &Expr) -> &'static str {
    match e {
        Expr::Assign(_) => "an assignment",
        Expr::Async(_) | Expr::Await(_) => "async",
        Expr::Break(_) | Expr::Continue(_) => "`break` or `continue`",
        Expr::Cast(_) => "a cast (`as`)",
        Expr::ForLoop(_) | Expr::Loop(_) | Expr::While(_) => "a loop",
        Expr::Index(_) => "indexing (`x[i]`)",
        Expr::Range(_) => "a range",
        Expr::Reference(_) => "a reference (`&`)",
        Expr::Return(_) => "`return`",
        Expr::Try(_) | Expr::TryBlock(_) => "the `?` operator",
        Expr::Unsafe(_) => "an `unsafe` block",
        Expr::Closure(_) => "a closure outside a combinator",
        Expr::Repeat(_) => "an array repeat",
        Expr::Let(_) => "`let` in an expression",
        _ => "this expression",
    }
}

enum Body<'a> {
    E(&'a Expr),
    B(&'a Block),
    U,
}

impl Lower {
    fn under<T>(&mut self, f: Frame, go: impl FnOnce(&mut Self) -> T) -> T {
        self.scope.push(f);
        let r = go(self);
        self.scope.pop();
        r
    }

    /// A bound variable's use: its witness at its De Bruijn index.
    fn var(&self, id: &Ident) -> Option<TokenStream> {
        let mut inner = vec![];
        for f in self.scope.iter().rev() {
            if let Some((_, off)) = f.names.iter().rev().find(|(n, _)| *id == n) {
                let w = rv(id);
                return Some(quote!(#w.at(#off #(+ #inner)*)));
            }
            inner.push(f.size.clone());
        }
        None
    }

    fn is(&self, e: &Expr, name: &Option<String>) -> bool {
        matches!((e, name), (Expr::Path(p), Some(n)) if p.path.is_ident(n))
    }

    fn handle(&self) -> TokenStream {
        let ty = &self.db.as_ref().unwrap().1;
        quote!(<#ty>::__ark_open())
    }

    /// `db.t`: the table's name, when `e` is one.
    fn table<'a>(&self, e: &'a Expr) -> Option<&'a Member> {
        let db = self.db.as_ref().map(|d| d.0.clone());
        match e {
            Expr::Field(f) if self.is(&f.base, &db) => Some(&f.member),
            _ => None,
        }
    }

    pub fn block(&mut self, b: &Block) -> syn::Result<TokenStream> {
        self.stmts(&b.stmts)
    }

    /// Statements: each one's value dropped but the last's; a `let` binds
    /// the rest; a body's last `Ok(())` is nothing.
    fn stmts(&mut self, ss: &[Stmt]) -> syn::Result<TokenStream> {
        let ss = match ss.last() {
            Some(Stmt::Expr(e, None)) if is_ok_unit(e) => &ss[..ss.len() - 1],
            _ => ss,
        };
        let mut first = vec![];
        for (i, s) in ss.iter().enumerate() {
            let (t, value) = match s {
                Stmt::Local(l) => return Ok(seq(first, self.let_(l, &ss[i + 1..])?)),
                Stmt::Item(it) => return Err(no(it, "an item inside a body")),
                Stmt::Macro(m) => (self.mac(&m.mac, true)?, false),
                Stmt::Expr(Expr::Macro(m), semi) => (self.mac(&m.mac, semi.is_some())?, semi.is_none()),
                Stmt::Expr(e, semi) => (self.expr(e)?, semi.is_none()),
            };
            match (i + 1 == ss.len(), value) {
                (true, true) => return Ok(seq(first, t)),
                (true, false) => return Ok(seq(first, quote!(::ark5::author::stmt(#t)))),
                _ => first.push(quote!(::ark5::author::stmt(#t).e)),
            }
        }
        Ok(seq(first, quote!(::ark5::author::unit())))
    }

    fn let_(&mut self, l: &syn::Local, rest: &[Stmt]) -> syn::Result<TokenStream> {
        let (pat, ty) = match &l.pat {
            Pat::Type(t) => (&*t.pat, Some(&t.ty)),
            p => (p, None),
        };
        let id = match pat {
            Pat::Ident(p) if p.mutability.is_some() => return Err(no(p, "a mutable binding")),
            Pat::Ident(p) if p.subpat.is_none() && p.by_ref.is_none() => &p.ident,
            p => return Err(no(p, "a `let` pattern other than a name")),
        };
        let init = l.init.as_ref().ok_or_else(|| no(l, "a `let` without a value"))?;
        if let Some((_, e)) = &init.diverge {
            return Err(no(e, "`let … else`"));
        }
        let v = self.expr(&init.expr)?;
        let rest = self.under(Frame::of([(id.to_string(), 0)]), |s| s.stmts(rest))?;
        let (w, annot) = (rv(id), ty.map(|t| quote!(: ::ark5::author::Ex<<#t as ::ark5::author::Shape>::Base>)));
        Ok(quote!({ let __v #annot = #v; let #w = ::ark5::author::w(&__v); ::ark5::author::let_(__v, #rest) }))
    }

    pub fn expr(&mut self, e: &Expr) -> syn::Result<TokenStream> {
        Ok(match e {
            Expr::Lit(l) => lit(&l.lit, false)?,
            Expr::Paren(p) => self.expr(&p.expr)?,
            Expr::Group(g) => self.expr(&g.expr)?,
            Expr::Unary(u) => match (&u.op, &*u.expr) {
                (UnOp::Neg(_), Expr::Lit(l)) => lit(&l.lit, true)?,
                (UnOp::Neg(_), x) => self.expr(x).map(|x| quote!(::ark5::author::neg(#x)))?,
                (UnOp::Not(_), x) => self.expr(x).map(|x| quote!(::ark5::author::not(#x)))?,
                _ => return Err(no(u, "a dereference")),
            },
            Expr::Binary(b) => {
                let (x, y) = (self.expr(&b.left)?, self.expr(&b.right)?);
                let (f, op) = match b.op {
                    BinOp::Add(_) => ("arith", "Add"),
                    BinOp::Sub(_) => ("arith", "Sub"),
                    BinOp::Mul(_) => ("arith", "Mul"),
                    BinOp::Div(_) => ("arith", "Div"),
                    BinOp::Rem(_) => ("arith", "Mod"),
                    BinOp::Eq(_) => ("cmp", "Eq"),
                    BinOp::Ne(_) => ("cmp", "Ne"),
                    BinOp::Lt(_) => ("cmp", "Lt"),
                    BinOp::Le(_) => ("cmp", "Le"),
                    BinOp::Gt(_) => ("cmp", "Gt"),
                    BinOp::Ge(_) => ("cmp", "Ge"),
                    BinOp::And(_) => ("logic", "And"),
                    BinOp::Or(_) => ("logic", "Or"),
                    BinOp::BitAnd(_) | BinOp::BitOr(_) | BinOp::BitXor(_) | BinOp::Shl(_) | BinOp::Shr(_) => {
                        return Err(no(&b.op, "a bitwise operator"))
                    }
                    _ => return Err(no(&b.op, "an assignment")),
                };
                let (f, op) = (format_ident!("{f}"), format_ident!("{op}"));
                quote!(::ark5::author::#f(::ark5::ir::Op::#op, #x, #y))
            }
            Expr::Path(p) if p.qself.is_none() => self.path(&p.path)?,
            Expr::Field(f) => {
                let m = &f.member;
                if self.is(&f.base, &self.ctx) {
                    match m.to_token_stream().to_string().as_str() {
                        "user" | "session" | "roles" | "now" => quote!(::ark5::author::Ctx.#m()),
                        _ => return Err(no(f, &format!("the context field `{}`", m.to_token_stream()))),
                    }
                } else if let Some(t) = self.table(e) {
                    let h = self.handle();
                    quote!(#h.#t.all())
                } else {
                    let b = self.expr(&f.base)?;
                    quote!(::ark5::author::view(#b).#m)
                }
            }
            Expr::MethodCall(m) => self.method(m)?,
            Expr::Call(c) => self.call(c)?,
            Expr::Struct(s) => self.struct_(s)?,
            Expr::Tuple(t) => match t.elems.len() {
                0 => quote!(::ark5::author::unit()),
                2..=4 => {
                    let xs = t.elems.iter().map(|x| self.expr(x)).collect::<syn::Result<Vec<_>>>()?;
                    quote!(::ark5::author::tup((#(#xs,)*)))
                }
                _ => return Err(no(t, "a tuple of one or of more than four")),
            },
            Expr::If(i) => {
                let els = match &i.else_branch {
                    Some((_, e)) => Some(&**e),
                    None => None,
                };
                if let Expr::Let(l) = &*i.cond {
                    return self.arms(&l.expr, &[(&l.pat, Body::B(&i.then_branch))], els.map(Body::E).or(Some(Body::U)));
                }
                let c = self.expr(&i.cond)?;
                let a = self.block(&i.then_branch)?;
                let b = match els {
                    Some(e) => self.expr(e)?,
                    None => quote!(::ark5::author::unit()),
                };
                quote!(::ark5::author::if_(#c, #a, #b))
            }
            Expr::Match(m) => {
                let (mut arms, mut wild) = (vec![], None);
                for a in &m.arms {
                    if let Some((_, g)) = &a.guard {
                        return Err(no(g, "a match guard"));
                    }
                    match &a.pat {
                        Pat::Wild(_) => wild = Some(Body::E(&a.body)),
                        p => arms.push((p, Body::E(&a.body))),
                    }
                }
                self.arms(&m.expr, &arms, wild)?
            }
            Expr::Block(b) if b.label.is_none() => self.block(&b.block)?,
            Expr::Macro(m) => self.mac(&m.mac, false)?,
            Expr::Array(a) => self.list(a.elems.iter())?,
            e => return Err(no(e, what(e))),
        })
    }

    fn list<'a>(&mut self, xs: impl Iterator<Item = &'a Expr>) -> syn::Result<TokenStream> {
        let xs = xs.map(|x| self.expr(x)).collect::<syn::Result<Vec<_>>>()?;
        Ok(quote!(::ark5::author::list(vec![#(#xs),*])))
    }

    fn path(&mut self, p: &Path) -> syn::Result<TokenStream> {
        if let Some(id) = p.get_ident() {
            if self.ctx.as_deref() == Some(&id.to_string()) {
                return Ok(quote!(::ark5::author::Ctx));
            }
            if self.db.as_ref().is_some_and(|d| *id == d.0) {
                return Ok(self.handle());
            }
            if id == "None" {
                return Ok(quote!(::ark5::author::none()));
            }
            return self.var(id).ok_or_else(|| no(id, &format!("`{id}`, which is no variable bound here,")));
        }
        let (en, v) = variant_of(p).ok_or_else(|| no(p, "a path that is no variable and no variant"))?;
        let (i, n) = (format_ident!("__ark_i_{v}"), format_ident!("__ark_n_{v}"));
        Ok(quote!({ if false { let _ = #p; } ::ark5::author::variant::<#en>(#en::#i, #en::#n, vec![]) }))
    }

    fn mac(&mut self, m: &Macro, stmt: bool) -> syn::Result<TokenStream> {
        let name = m.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
        match name.as_str() {
            "vec" => self.list(m.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)?.iter()),
            "refuse" => {
                let x = self.expr(&m.parse_body::<Expr>()?)?;
                Ok(match stmt {
                    true => quote!(::ark5::author::refuse::<()>(#x)),
                    false => quote!(::ark5::author::refuse(#x)),
                })
            }
            _ => Err(no(m, &format!("the macro `{}!`", m.path.to_token_stream()))),
        }
    }

    fn closure(&mut self, c: &ExprClosure) -> syn::Result<TokenStream> {
        if c.capture.is_some() || c.asyncness.is_some() || c.constness.is_some() {
            return Err(no(c, "a `move`, async or const closure"));
        }
        let mut names = vec![];
        for p in &c.inputs {
            let p = if let Pat::Type(t) = p { &*t.pat } else { p };
            names.push(match p {
                Pat::Ident(i) if i.subpat.is_none() && i.mutability.is_none() => i.ident.to_string(),
                Pat::Wild(_) => "_".into(),
                p => return Err(no(p, "a closure parameter pattern")),
            });
        }
        let n = names.len() as u32;
        let pats: Vec<_> = names.iter().map(|n| if n == "_" { quote!(_) } else { rv(n).to_token_stream() }).collect();
        let frame = Frame::of(names.into_iter().enumerate().map(|(i, x)| (x, n - 1 - i as u32)));
        let body = self.under(frame, |s| s.expr(&c.body))?;
        Ok(quote!(|#(#pats),*| #body))
    }

    /// An argument: a closure as a lambda, anything else as an expression.
    fn arg(&mut self, a: &Expr) -> syn::Result<TokenStream> {
        match a {
            Expr::Closure(c) => self.closure(c),
            a => self.expr(a),
        }
    }

    /// A table's key: the tuple of its columns' values, written as one.
    fn key(&mut self, k: &Expr) -> syn::Result<TokenStream> {
        let xs = match k {
            Expr::Tuple(t) => t.elems.iter().map(|x| self.expr(x)).collect::<syn::Result<Vec<_>>>()?,
            k => vec![self.expr(k)?],
        };
        Ok(quote!(vec![#(#xs.e),*]))
    }

    fn method(&mut self, m: &syn::ExprMethodCall) -> syn::Result<TokenStream> {
        let (name, f) = (m.method.to_string(), &m.method);
        let args: Vec<&Expr> = m.args.iter().collect();
        if self.is(&m.receiver, &self.ctx) {
            let tf = &m.turbofish;
            return match (name.as_str(), &args[..]) {
                ("user" | "session" | "roles" | "now", []) | ("new_id", []) => Ok(quote!(::ark5::author::Ctx.#f #tf ())),
                ("has_role", [r]) => self.expr(r).map(|r| quote!(::ark5::author::Ctx.has_role(#r))),
                _ => Err(no(m, &format!("`ctx.{name}(..)`"))),
            };
        }
        if m.turbofish.is_some() {
            return Err(no(&m.turbofish, "a turbofish"));
        }
        let recv = match self.table(&m.receiver) {
            Some(t) => {
                let h = self.handle();
                let h = quote!(#h.#t);
                match (name.as_str(), &args[..]) {
                    ("get" | "delete", [k]) => return self.key(k).map(|k| quote!(#h.#f(#k))),
                    ("insert", [r]) => return self.expr(r).map(|r| quote!(#h.insert(#r))),
                    ("update", [k, l]) => {
                        let (k, l) = (self.key(k)?, self.arg(l)?);
                        return Ok(quote!(#h.update(#k, #l)));
                    }
                    ("all", []) => return Ok(quote!(#h.all())),
                    _ => quote!(#h.all()),
                }
            }
            None => self.expr(&m.receiver)?,
        };
        if !METHODS.contains(&name.as_str()) {
            return Err(no(f, &format!("the method `{name}`")));
        }
        if let ("sort", [Expr::Closure(c)]) = (name.as_str(), &args[..]) {
            return self.sort(recv, c);
        }
        let args = args.into_iter().map(|a| self.arg(a)).collect::<syn::Result<Vec<_>>>()?;
        Ok(quote!((#recv).#f(#(#args),*)))
    }

    /// `.sort(|r| key)`: a tuple of keys, each `Desc(k)` or `Asc(k)` or `k`.
    fn sort(&mut self, recv: TokenStream, c: &ExprClosure) -> syn::Result<TokenStream> {
        let mut body = &*c.body;
        while let Expr::Block(b) = body {
            match &b.block.stmts[..] {
                [Stmt::Expr(e, None)] => body = e,
                _ => break,
            }
        }
        let keys: Vec<&Expr> = match body {
            Expr::Tuple(t) => t.elems.iter().collect(),
            k => vec![k],
        };
        let mut dirs = vec![];
        let keys: Vec<&Expr> = keys
            .into_iter()
            .map(|k| match k {
                Expr::Call(d) if d.args.len() == 1 && matches!(&*d.func, Expr::Path(p) if p.path.is_ident("Desc") || p.path.is_ident("Asc")) => {
                    let desc = matches!(&*d.func, Expr::Path(p) if p.path.is_ident("Desc"));
                    dirs.push(if desc { quote!(Desc) } else { quote!(Asc) });
                    &d.args[0]
                }
                k => {
                    dirs.push(quote!(Asc));
                    k
                }
            })
            .collect();
        let body: Expr = match &keys[..] {
            [k] => (*k).clone(),
            ks => syn::parse_quote!((#(#ks),*)),
        };
        let mut c = c.clone();
        c.body = Box::new(body);
        let l = self.closure(&c)?;
        Ok(quote!((#recv).sort(vec![#(::ark5::store::Dir::#dirs),*], #l)))
    }

    fn call(&mut self, c: &syn::ExprCall) -> syn::Result<TokenStream> {
        let Expr::Path(p) = &*c.func else {
            return Err(no(&c.func, "a call of a computed function"));
        };
        let p = &p.path;
        let args = c.args.iter().map(|a| self.expr(a)).collect::<syn::Result<Vec<_>>>()?;
        let free = |n: &str| p.is_ident(n);
        if free("Some") && args.len() == 1 {
            return Ok(quote!(::ark5::author::some(#(#args)*)));
        }
        if free("concat") || free("nil_id") {
            return Ok(quote!(::ark5::author::#p(#(#args),*)));
        }
        if free("Ok") || free("Desc") || free("Asc") {
            return Err(no(c, &format!("`{}(..)` here", p.to_token_stream())));
        }
        if let Some((en, v)) = variant_of(p) {
            let set = (0..args.len()).map(|i| format_ident!("__ark_set_{v}_{i}"));
            let (i, n, dead) = (format_ident!("__ark_i_{v}"), format_ident!("__ark_n_{v}"), args.iter().map(|_| quote!(unreachable!())));
            return Ok(quote!({ if false { let _ = #p(#(#dead),*); } ::ark5::author::variant::<#en>(#en::#i, #en::#n, vec![#(#en::#set(#args)),*]) }));
        }
        match p.get_ident() {
            Some(id) if upper(id) => Err(no(c, "a refined type's constructor (a refinement holds where bytes enter, at parse)")),
            Some(id) if *id == self.me => Err(no(c, "a call of the function itself (calls are a DAG)")),
            _ => Ok(quote!(#p(#(#args),*))),
        }
    }

    fn struct_(&mut self, s: &ExprStruct) -> syn::Result<TokenStream> {
        let p = &s.path;
        let mut fields = vec![];
        for f in &s.fields {
            let Member::Named(m) = &f.member else { return Err(no(f, "a field by position")) };
            fields.push((m, self.expr(&f.expr)?));
        }
        let dead = fields.iter().map(|(m, _)| quote!(#m: unreachable!()));
        let rest = s.rest.as_ref().map(|r| self.expr(r)).transpose()?;
        let (vals, dead_rest) = (fields.iter().map(|f| &f.1), s.rest.as_ref().map(|_| quote!(..unreachable!())));
        let check = quote!(if false { let _ = #p { #(#dead,)* #dead_rest }; });
        Ok(match variant_of(p) {
            Some((en, v)) => {
                if let Some(r) = &s.rest {
                    return Err(no(r, "`..base` in a variant"));
                }
                let set = fields.iter().map(|(m, _)| format_ident!("__ark_set_{v}_{m}"));
                let (i, n) = (format_ident!("__ark_i_{v}"), format_ident!("__ark_n_{v}"));
                quote!({ #check ::ark5::author::variant::<#en>(#en::#i, #en::#n, vec![#(#en::#set(#vals)),*]) })
            }
            None => {
                let set = fields.iter().map(|(m, _)| format_ident!("__ark_set_{m}"));
                let base = match rest {
                    Some(r) => quote!(Some(#r)),
                    None => quote!(None),
                };
                quote!({ #check ::ark5::author::record::<#p>(#p::__ark_n, vec![#(#p::#set(#vals)),*], #base) })
            }
        })
    }

    /// A match: on an option (`None`, `Some(x)`) or a declared sum (`E::V`,
    /// `E::V(a, _)`, `E::V { f, g: h, .. }`), `wild` standing for the rest.
    fn arms(&mut self, scrut: &Expr, arms: &[(&Pat, Body)], wild: Option<Body>) -> syn::Result<TokenStream> {
        let s = self.expr(scrut)?;
        let (mut sum, mut opt, mut out, mut pats) = (None::<Path>, [false; 2], vec![], vec![]);
        for (pat, body) in arms {
            let (idx, frame, lets) = match pat {
                Pat::Ident(i) if i.ident == "None" && i.subpat.is_none() => (quote!(0u8), Frame::of([]), quote!()),
                Pat::Path(p) if p.path.is_ident("None") => (quote!(0u8), Frame::of([]), quote!()),
                Pat::TupleStruct(t) if t.path.is_ident("Some") && t.elems.len() == 1 => {
                    let (name, lets) = match &t.elems[0] {
                        Pat::Ident(i) if i.subpat.is_none() => {
                            let w = rv(&i.ident);
                            (i.ident.to_string(), quote!(let #w = ::ark5::author::some_w(&__s);))
                        }
                        Pat::Wild(_) => ("_".into(), quote!()),
                        p => return Err(no(p, "a nested pattern")),
                    };
                    (quote!(1u8), Frame::of([(name, 0)]), lets)
                }
                p => {
                    let path = match p {
                        Pat::Path(x) => &x.path,
                        Pat::TupleStruct(x) => &x.path,
                        Pat::Struct(x) => &x.path,
                        p => return Err(no(p, "a pattern other than a variant, `Some`, `None` or `_`")),
                    };
                    let (en, v) = variant_of(path).ok_or_else(|| no(path, "a pattern naming no variant `E::V`"))?;
                    sum = Some(en.clone());
                    pats.push(p);
                    let (n, arm) = (format_ident!("__ark_n_{v}"), format_ident!("__ark_arm_{v}"));
                    let mut names = vec![];
                    let mut lets = vec![quote!(let __arm = #en::#arm();)];
                    let mut bind = |pat: &Pat, field: TokenStream, pos: TokenStream| -> syn::Result<()> {
                        match pat {
                            Pat::Ident(i) if i.subpat.is_none() => {
                                let w = rv(&i.ident);
                                lets.push(quote!(let #w = __arm.#field;));
                                names.push((i.ident.to_string(), quote!((#en::#n - 1 - #pos))));
                                Ok(())
                            }
                            Pat::Wild(_) | Pat::Rest(_) => Ok(()),
                            p => Err(no(p, "a nested pattern")),
                        }
                    };
                    match p {
                        Pat::TupleStruct(t) => {
                            for (k, e) in t.elems.iter().enumerate() {
                                let k = syn::Index::from(k);
                                bind(e, quote!(#k), quote!(#k))?;
                            }
                        }
                        Pat::Struct(st) => {
                            for f in &st.fields {
                                let Member::Named(m) = &f.member else { return Err(no(f, "a field by position")) };
                                let pos = format_ident!("__ark_p_{v}_{m}");
                                bind(&f.pat, quote!(#m), quote!(#en::#pos))?;
                            }
                        }
                        _ => {}
                    }
                    let i = format_ident!("__ark_i_{v}");
                    (quote!(#en::#i), Frame { names, size: quote!(#en::#n) }, quote!(#(#lets)*))
                }
            };
            if sum.is_none() {
                opt[idx.to_string().starts_with('1') as usize] = true;
            }
            let body = self.under(frame, |s| s.body(body))?;
            out.push(quote!((#idx, { #lets #body })));
        }
        let wild = match wild {
            Some(b) => {
                let w = self.under(Frame { names: vec![], size: quote!(__wild_n) }, |s| s.body(&b))?;
                quote!(Some(&|__wild_n: u32| #w))
            }
            None if sum.is_none() && opt != [true; 2] => return Err(no(scrut, "a match on an option without `None`, `Some` or `_`")),
            None => quote!(None),
        };
        let rest = (wild.to_string() != "None").then(|| quote!(_ => {}));
        let (arities, check) = match &sum {
            Some(en) => (
                quote!(&#en::__ark_arities()),
                quote!(if false { let __x: #en = unreachable!(); match __x { #(#pats => {})* #rest } }),
            ),
            None => (quote!(&[0u32, 1u32]), quote!()),
        };
        Ok(quote!({ let __s = #s; #check ::ark5::author::match_(__s.clone(), #arities, vec![#(#out),*], #wild) }))
    }

    fn body(&mut self, b: &Body) -> syn::Result<TokenStream> {
        match b {
            Body::E(e) => self.expr(e),
            Body::B(b) => self.block(b),
            Body::U => Ok(quote!(::ark5::author::unit())),
        }
    }
}

fn lit(l: &Lit, neg: bool) -> syn::Result<TokenStream> {
    Ok(match l {
        Lit::Int(i) => {
            let n = i.base10_parse::<i128>()?;
            let n = i64::try_from(if neg { -n } else { n }).map_err(|_| no(i, "an int beyond i64"))?;
            quote!(::ark5::author::int(#n))
        }
        Lit::Str(s) if !neg => quote!(::ark5::author::text(#s)),
        Lit::Bool(b) if !neg => quote!(::ark5::author::bool(#b)),
        Lit::Float(_) => return Err(no(l, "a float literal")),
        Lit::Char(_) => return Err(no(l, "a char literal")),
        l => return Err(no(l, "this literal")),
    })
}
