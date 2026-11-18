//! `#[view]`: a function of a screen's record that returns a `View`, made
//! generic over `ark_app::view::Backend` so the same function is drawn
//! (iced, the browser) and recorded (SwiftUI and Compose are written from
//! the recording). Only what reads the record is rewritten:
//!
//! - `p.f.g` (a parameter's, or a `for`/`if let` binding's, fields) into
//!   `B::get` reads, handed to a part as `B::val(…)`, or, inside a struct
//!   literal (a message being built), read as the value itself (`B::probe`);
//! - `for x in p.list { … }` into `B::each`;
//! - `if p.flag { … } else { … }` into `B::when` (no `else` is nothing);
//! - `if let Some(x) = p.opt { … }` into `B::when_some`;
//! - a call handed a binding itself (`account_row(a)`) into a sub-view's,
//!   `account_row::<B>(a)`;
//! - a `let` that reads a source or a binding into the backend's: `let s =
//!   cell(STATUS)` a cell's value, `let p = row(playback())` and `let q =
//!   list(up_next(3))` a function returning a plan, kept as a watch by its
//!   name and arguments, `let line = text!("{}  {}", t.artist, t.at)`
//!   words whose holes are kept apart, and any other `let` over bindings
//!   (`let title = title(p)`) a value worked out from them, its bindings
//!   handed to it by reference. A source's binding, or a let's, handed to
//!   a part is its value (`text(title)`); what is worked out goes in a
//!   `let`, never in a part's argument.
//!
//! What it cannot follow is a compile error that says so: a method called
//! on the record, `match` over it, a binding that is not one name, `while`
//! and `loop`. It writes the function's entry points into a module of the
//! same name: `view(&record)` (`view(db, &record)` when it reads sources),
//! `record()` and `recording()`. Drawing it is a toolkit's,
//! an adapter's: `arkui::view::element(view(&record))` in iced,
//! `arkui_web::render(view(&record))` in the browser; the app's crate
//! depends on neither.

use std::collections::BTreeMap;

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, Error, Expr, FnArg, Ident, ItemFn, LitStr, Member, Pat, ReturnType, Stmt, Token, Type};

pub fn expand(item: ItemFn) -> syn::Result<TokenStream> {
    let name = &item.sig.ident;
    let vis = &item.vis;
    let attrs = &item.attrs;
    let ret = match &item.sig.output {
        ReturnType::Type(_, t) => t.clone(),
        ReturnType::Default => return Err(Error::new(item.sig.span(), "a view returns a View<Msg>")),
    };
    // It returns a `View<Msg>`, or says so.
    let msg = view_msg(&ret)?;
    if !item.sig.generics.params.is_empty() {
        return Err(Error::new(
            item.sig.generics.span(),
            "a view takes no generics: #[view] adds the backend's",
        ));
    }
    let mut params = vec![];
    for a in &item.sig.inputs {
        let FnArg::Typed(t) = a else {
            return Err(Error::new(a.span(), "a view is a free function of its record"));
        };
        let Pat::Ident(id) = &*t.pat else {
            return Err(Error::new(t.pat.span(), "a view's parameter is one name"));
        };
        let Type::Reference(r) = &*t.ty else {
            return Err(Error::new(t.ty.span(), "a view takes its record by reference: `p: &Page`"));
        };
        params.push((id.ident.clone(), (*r.elem).clone()));
    }
    let mut body = item.block.clone();
    let mut rw = Rewrite {
        data: vec![params.iter().map(|(i, _)| (i.to_string(), Kind::Record)).collect()],
        in_msg: 0,
        sources: false,
        errors: vec![],
    };
    rw.visit_block_mut(&mut body);
    if let Some(e) = rw.errors.into_iter().reduce(|mut a, b| {
        a.combine(b);
        a
    }) {
        return Err(e);
    }
    let generic_params = params.iter().map(|(i, t)| quote!(#i: __B::Ref<'__a, #t>));
    let plain_params: Vec<_> = params.iter().map(|(i, t)| quote!(#i: &'__a #t)).collect();
    let borrowed: Vec<_> = params.iter().map(|(i, _)| quote!(::std::borrow::Cow::Borrowed(#i))).collect();
    let roots: Vec<_> = params
        .iter()
        .map(|(i, _)| {
            let s = i.to_string();
            quote!(::ark_app::view::Sym::root(#s))
        })
        .collect();
    // A view that reads sources reads them through the backend's `Db`.
    let (db_param, db_plain, db_real, db_rec) = if rw.sources {
        (
            quote!(__db: __B::Db<'__a>,),
            quote!(__db: &'__a dyn ::ark_app::view::Db,),
            quote!(__db,),
            quote!((),),
        )
    } else {
        (quote!(), quote!(), quote!(), quote!())
    };
    Ok(quote! {
        #(#attrs)*
        #[allow(clippy::needless_lifetimes, unused_variables)]
        #vis fn #name<'__a, __B: ::ark_app::view::Backend>(#db_param #(#generic_params),*) -> #ret #body

        /// The view's entry points: drawn from a record and the node's
        /// sources, or recorded.
        #[allow(non_snake_case, dead_code)]
        #vis mod #name {
            #[allow(unused_imports)]
            use super::*;

            /// The view of the record as it is, and of the sources as the
            /// reader reads them.
            pub fn view<'__a>(#db_plain #(#plain_params),*) -> #ret {
                super::#name::<::ark_app::view::Real>(#db_real #(#borrowed),*)
            }

            /// What the view says, as paths: what SwiftUI and Compose are written from.
            pub fn record() -> #ret {
                recording().view
            }

            /// What the view says, as paths, and the lets it reads: what
            /// every emitter is written from.
            pub fn recording() -> ::ark_app::view::Recording<#msg> {
                ::ark_app::view::recording(|| super::#name::<::ark_app::view::Recorder>(#db_rec #(#roots),*))
            }
        }
    })
}

/// `M` of `View<M>`.
fn view_msg(t: &Type) -> syn::Result<Type> {
    if let Type::Path(p) = t {
        if let Some(last) = p.path.segments.last() {
            if let syn::PathArguments::AngleBracketed(a) = &last.arguments {
                if let Some(syn::GenericArgument::Type(m)) = a.args.first() {
                    return Ok(m.clone());
                }
            }
        }
    }
    Err(Error::new(t.span(), "a view returns a View<Msg>"))
}

/// What a binding in a view is: a part of the record, or read from sources
/// (a `let`, or an item of one).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Record,
    Source,
}

struct Rewrite {
    /// The bindings in scope, innermost last.
    data: Vec<BTreeMap<String, Kind>>,
    /// How deep inside a struct literal (a message being built).
    in_msg: usize,
    /// Whether the view reads a source (`cell`, `row`, `list`).
    sources: bool,
    errors: Vec<Error>,
}

/// FNV-1a: `ark_kernel::source::key`, worked out as the view is expanded.
fn key(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The bindings an expression names, in order: an expression's own
/// closures' parameters left out.
struct Names<'r> {
    rw: &'r Rewrite,
    found: Vec<Ident>,
    shadow: Vec<String>,
}

impl VisitMut for Names<'_> {
    fn visit_expr_mut(&mut self, e: &mut Expr) {
        match e {
            Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
                let i = &p.path.segments[0].ident;
                if self.rw.kind(i).is_some() && !self.shadow.contains(&i.to_string()) && !self.found.contains(i) {
                    self.found.push(i.clone());
                }
            }
            Expr::Closure(c) => {
                let n = self.shadow.len();
                for p in &c.inputs {
                    if let Pat::Ident(i) = p {
                        self.shadow.push(i.ident.to_string());
                    }
                }
                self.visit_expr_mut(&mut c.body);
                self.shadow.truncate(n);
            }
            Expr::Macro(m) => {
                // A macro's arguments, where they are expressions (`text!`'s holes).
                if let Ok(args) = m.mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
                    for mut a in args {
                        self.visit_expr_mut(&mut a);
                    }
                }
            }
            _ => visit_mut::visit_expr_mut(self, e),
        }
    }
}

impl Rewrite {
    fn kind(&self, i: &Ident) -> Option<Kind> {
        let s = i.to_string();
        self.data.iter().rev().find_map(|d| d.get(&s).copied())
    }

    fn is_data(&self, i: &Ident) -> bool {
        self.kind(i).is_some()
    }

    /// The bindings `e` names.
    fn names(&self, e: &Expr) -> Vec<Ident> {
        let mut n = Names {
            rw: self,
            found: vec![],
            shadow: vec![],
        };
        n.visit_expr_mut(&mut e.clone());
        n.found
    }

    /// `e` as a read of the record: its root binding and its fields.
    fn chain(&self, e: &Expr) -> Option<(Ident, Vec<Ident>)> {
        match e {
            Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
                let i = &p.path.segments[0].ident;
                self.is_data(i).then(|| (i.clone(), vec![]))
            }
            Expr::Field(f) => {
                let (root, mut fields) = self.chain(&f.base)?;
                match &f.member {
                    Member::Named(n) => fields.push(n.clone()),
                    Member::Unnamed(_) => return None,
                }
                Some((root, fields))
            }
            Expr::Paren(p) => self.chain(&p.expr),
            _ => None,
        }
    }

    /// The read as the backend's `Ref`, owned.
    fn reference(root: &Ident, fields: &[Ident], span: Span) -> Expr {
        let mut acc: Expr = parse_quote!(::core::clone::Clone::clone(&#root));
        for f in fields {
            let s = f.to_string();
            acc = syn::parse2(quote_spanned!(span=> __B::get(&#acc, #s, |__v| &__v.#f))).expect("a read");
        }
        acc
    }

    /// `e` as a `Ref` for `for`/`if`, or an error that says what is not one.
    fn as_ref(&mut self, e: &Expr, what: &str) -> Option<(Expr, Kind)> {
        match self.chain(e) {
            Some((root, fields)) => {
                let kind = self.kind(&root).expect("a binding");
                Some((Rewrite::reference(&root, &fields, e.span()), kind))
            }
            None => {
                self.errors.push(Error::new(
                    e.span(),
                    format!("a view's `{what}` is over a binding's fields (`p.list`); work anything else out in a `let`"),
                ));
                None
            }
        }
    }

    fn scoped<T>(&mut self, names: impl IntoIterator<Item = (String, Kind)>, f: impl FnOnce(&mut Self) -> T) -> T {
        self.data.push(names.into_iter().collect());
        let out = f(self);
        self.data.pop();
        out
    }

    fn one_name(&mut self, p: &Pat) -> Option<Ident> {
        match p {
            Pat::Ident(i) if i.subpat.is_none() => Some(i.ident.clone()),
            _ => {
                self.errors
                    .push(Error::new(p.span(), "a view binds one name here (`for a in p.accounts`)"));
                None
            }
        }
    }

    fn block_into_view(&mut self, b: &mut syn::Block) -> Expr {
        self.visit_block_mut(b);
        parse_quote!(::core::convert::Into::into(#b))
    }

    /// A `let` of a view: a source, or a value worked out from bindings,
    /// as the backend's; none for a `let` that reads neither.
    fn binding(&mut self, l: &syn::Local) -> Option<Stmt> {
        let init = l.init.as_ref()?;
        if init.diverge.is_some() {
            return None;
        }
        let Pat::Ident(pi) = &l.pat else { return None };
        let name = pi.ident.clone();
        let label = name.to_string();
        let e = &*init.expr;
        let span = e.span();
        // A source: `cell(C)`, `row(f(…))`, `list(f(…))`.
        if let Expr::Call(c) = e {
            if let Expr::Path(f) = &*c.func {
                let which = f.path.get_ident().map(Ident::to_string);
                if let (Some(which @ ("cell" | "row" | "list")), 1) = (which.as_deref(), c.args.len()) {
                    let arg = &c.args[0];
                    self.sources = true;
                    if !self.names(arg).is_empty() {
                        self.errors.push(Error::new(
                            arg.span(),
                            "a source's arguments are not read from other sources yet: a constant, or the record's",
                        ));
                        return None;
                    }
                    self.data.last_mut().expect("a scope").insert(label.clone(), Kind::Source);
                    if which == "cell" {
                        let text = quote!(#arg).to_string();
                        return Some(parse_quote_spanned(span, quote!(let #name = __B::cell(__db, #label, #text, #arg);)));
                    }
                    let Expr::Call(call) = arg else {
                        self.errors.push(Error::new(
                            arg.span(),
                            "a source is a call of a function that returns a plan: `list(up_next(3))`",
                        ));
                        return None;
                    };
                    let func = &call.func;
                    let fname = quote!(#func).to_string().replace(' ', "");
                    let k = key(&fname);
                    let text = quote!(#call).to_string();
                    let args: Vec<_> = call.args.iter().collect();
                    let arg_texts: Vec<_> = args.iter().map(|a| quote!(#a).to_string()).collect();
                    let method = Ident::new(which, span);
                    return Some(parse_quote_spanned(
                        span,
                        quote!(let #name = __B::#method(
                            __db,
                            #label,
                            (#k, #text, &[#(#arg_texts),*]),
                            &{
                                #[allow(unused_mut)]
                                let mut __a = ::std::vec::Vec::new();
                                #(::ark_app::view::kernel::Encode::encode(&(#args), &mut __a);)*
                                __a
                            },
                            || #call,
                        );),
                    ));
                }
            }
        }
        // A value worked out from bindings.
        let deps = self.names(e);
        if deps.is_empty() {
            return None;
        }
        let what = match e {
            Expr::Macro(m) if is_text(m) => {
                let Ok(args) = m.mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) else {
                    self.errors.push(Error::new(m.span(), "`text!` is a literal and its holes"));
                    return None;
                };
                let mut args = args.into_iter();
                let Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(t), .. })) = args.next() else {
                    self.errors.push(Error::new(m.span(), "`text!`'s template is a string literal"));
                    return None;
                };
                let holes: Vec<String> = args.map(|h| quote!(#h).to_string()).collect();
                quote!(::ark_app::view::Source::Text { template: #t, holes: ::std::vec![#(#holes),*] })
            }
            _ => {
                let text = LitStr::new(&quote!(#e).to_string(), span);
                quote!(::ark_app::view::Source::Derived(#text))
            }
        };
        let dep_names: Vec<String> = deps.iter().map(Ident::to_string).collect();
        self.data.last_mut().expect("a scope").insert(label.clone(), Kind::Source);
        Some(parse_quote_spanned(
            span,
            quote!(let #name = __B::derive(&[#(#dep_names),*], #label, #what, || {
                #(let #deps = __B::value(&#deps);)*
                #e
            });),
        ))
    }
}

/// Whether a macro is `text!` (or `ark_app::text!`).
fn is_text(m: &syn::ExprMacro) -> bool {
    m.mac.path.segments.last().is_some_and(|s| s.ident == "text")
}

/// A statement the expansion writes, or the error it makes.
fn parse_quote_spanned(span: Span, t: TokenStream) -> Stmt {
    syn::parse2(t).unwrap_or_else(|e| {
        let msg = e.to_string();
        syn::parse2(quote_spanned!(span=> ::core::compile_error!(#msg);)).expect("an error")
    })
}

impl VisitMut for Rewrite {
    fn visit_block_mut(&mut self, b: &mut syn::Block) {
        for s in b.stmts.iter_mut() {
            match s {
                Stmt::Local(l) => match self.binding(l) {
                    Some(new) => *s = new,
                    None => visit_mut::visit_stmt_mut(self, s),
                },
                _ => visit_mut::visit_stmt_mut(self, s),
            }
        }
    }

    fn visit_expr_mut(&mut self, e: &mut Expr) {
        let span = e.span();
        match e {
            Expr::ForLoop(f) => {
                let Some(item) = self.one_name(&f.pat) else { return };
                let Some((list, kind)) = self.as_ref(&f.expr, "for") else { return };
                let label = item.to_string();
                let body = self.scoped([(label.clone(), kind)], |me| me.block_into_view(&mut f.body));
                *e = parse_quote!(__B::each(#list, #label, |#item| #body));
            }
            Expr::If(i) => {
                let no: Expr = match i.else_branch.take() {
                    Some((_, mut els)) => {
                        self.visit_expr_mut(&mut els);
                        parse_quote!(::core::convert::Into::into(#els))
                    }
                    None => parse_quote!(::ark_app::view::View::Empty),
                };
                if let Expr::Let(l) = &*i.cond {
                    // `if let Some(x) = p.opt`
                    let item = match &*l.pat {
                        Pat::TupleStruct(ts) if ts.path.is_ident("Some") && ts.elems.len() == 1 => self.one_name(&ts.elems[0]),
                        p => {
                            self.errors.push(Error::new(p.span(), "a view's `if let` is `if let Some(x) = p.field`"));
                            None
                        }
                    };
                    let Some(item) = item else { return };
                    let Some((opt, kind)) = self.as_ref(&l.expr, "if let") else { return };
                    let label = item.to_string();
                    let yes = self.scoped([(label.clone(), kind)], |me| me.block_into_view(&mut i.then_branch));
                    *e = parse_quote!(__B::when_some(#opt, #label, |#item| #yes, || #no));
                } else if self.chain(&i.cond).is_some() {
                    let (c, _) = self.as_ref(&i.cond, "if").expect("a chain");
                    let yes = self.block_into_view(&mut i.then_branch);
                    *e = parse_quote!(__B::when(#c, || #yes, || #no));
                } else {
                    self.errors.push(Error::new(
                        i.cond.span(),
                        "a view's `if` is over a flag (`if a.closed`); work the condition out in a `let`",
                    ));
                }
            }
            Expr::Match(m) => {
                if self.chain(&m.expr).is_some() {
                    self.errors.push(Error::new(
                        m.span(),
                        "`match` over the record is not supported in a view yet: use `if` or `if let Some`",
                    ));
                } else {
                    visit_mut::visit_expr_mut(self, e)
                }
            }
            Expr::While(_) | Expr::Loop(_) => {
                self.errors.push(Error::new(span, "a view repeats with `for` over the record"));
            }
            Expr::MethodCall(m) if self.chain(&m.receiver).is_some() => {
                self.errors.push(Error::new(
                    m.method.span(),
                    format!("a view reads its bindings' fields, not their methods: work `{}` out in a `let`", m.method),
                ));
            }
            Expr::Macro(m) if is_text(m) && !self.names(&Expr::Macro(m.clone())).is_empty() => {
                self.errors
                    .push(Error::new(span, "`text!` over bindings goes in a `let`: `let line = text!(…)`"));
            }
            Expr::Struct(s) => {
                self.in_msg += 1;
                for f in s.fields.iter_mut() {
                    self.visit_expr_mut(&mut f.expr);
                }
                self.in_msg -= 1;
            }
            Expr::Closure(c) => {
                // A closure's own parameters are not the record's.
                let mut shadow = Vec::new();
                for p in &c.inputs {
                    if let Pat::Ident(i) = p {
                        shadow.push(i.ident.to_string());
                    }
                }
                let saved = std::mem::take(&mut self.data);
                self.data = saved
                    .iter()
                    .map(|d| d.iter().filter(|(n, _)| !shadow.contains(*n)).map(|(n, k)| (n.clone(), *k)).collect())
                    .collect();
                self.visit_expr_mut(&mut c.body);
                self.data = saved;
            }
            Expr::Call(c)
                if c.args
                    .iter()
                    .any(|a| matches!(self.chain(a), Some((r, f)) if f.is_empty() && self.kind(&r) == Some(Kind::Record))) =>
            {
                // A sub-view handed a binding: called over the same backend.
                for a in c.args.iter_mut() {
                    match self.chain(a) {
                        Some((root, fields)) if fields.is_empty() && self.kind(&root) == Some(Kind::Record) => {
                            *a = Rewrite::reference(&root, &fields, a.span())
                        }
                        _ => self.visit_expr_mut(a),
                    }
                }
                if let Expr::Path(p) = &mut *c.func {
                    if let Some(last) = p.path.segments.last_mut() {
                        last.arguments = syn::PathArguments::AngleBracketed(parse_quote!(::<__B>));
                    }
                }
            }
            _ => match self.chain(e) {
                Some((root, fields)) if !fields.is_empty() => {
                    let (last, init) = fields.split_last().expect("a field");
                    let base = Rewrite::reference(&root, init, span);
                    let s = last.to_string();
                    *e = match self.in_msg {
                        0 => syn::parse2(quote_spanned!(span=> __B::val(__B::get(&#base, #s, |__v| &__v.#last)))).expect("a read"),
                        _ => syn::parse2(quote_spanned!(span=> __B::probe(&#base, #s, |__v| &__v.#last))).expect("a probe"),
                    };
                }
                Some((root, _)) if self.kind(&root) == Some(Kind::Source) && self.in_msg == 0 => {
                    // A source's value, or a let's, handed to a part.
                    *e = syn::parse2(quote_spanned!(span=> __B::val(::core::clone::Clone::clone(&#root)))).expect("a value");
                }
                Some((root, _)) => {
                    self.errors.push(Error::new(
                        root.span(),
                        format!("`{root}` is the record: hand a part one of its fields, or a sub-view the whole (`row({root})`)"),
                    ));
                }
                None => visit_mut::visit_expr_mut(self, e),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expanded(src: &str) -> String {
        let f: ItemFn = syn::parse_str(src).unwrap();
        expand(f).map(|t| t.to_string()).unwrap_or_else(|e| format!("error: {e}"))
    }

    #[test]
    fn a_for_over_a_field_is_each() {
        let out = expanded("fn v(p: &Page) -> View<M> { section(for a in p.rows { list_row(a.name) }) }");
        assert!(out.contains("__B :: each"), "{out}");
        assert!(out.contains("\"rows\""), "{out}");
        assert!(out.contains("__B :: val (__B :: get"), "{out}");
        assert!(out.contains("pub fn record"), "{out}");
    }

    #[test]
    fn if_and_if_let_are_when_and_when_some() {
        let out = expanded("fn v(p: &Page) -> View<M> { (if p.flag { hero() }, if let Some(f) = p.form { sheet(f.title) } else { hero() }) }");
        assert!(out.contains("__B :: when ("), "{out}");
        assert!(out.contains("__B :: when_some"), "{out}");
        assert!(out.contains("\"f\""), "{out}");
    }

    #[test]
    fn a_message_reads_its_fields_as_values() {
        let out = expanded("fn v(a: &Item) -> View<M> { list_row(a.name).on_tap(M::Open { id: a.id }) }");
        assert!(out.contains("__B :: probe"), "{out}");
    }

    #[test]
    fn a_closure_parameter_is_not_the_record() {
        let out = expanded("fn v(p: &Page) -> View<M> { name_field(\"Name\", p.name, p.problem, |name| M::Named { name }) }");
        assert!(!out.contains("probe"), "{out}");
    }

    #[test]
    fn a_sub_view_is_called_over_the_same_backend() {
        let out = expanded("fn v(p: &Page) -> View<M> { for a in p.rows { row(a) } }");
        assert!(out.contains("row :: < __B >"), "{out}");
    }

    #[test]
    fn a_let_reads_sources_and_works_values_out() {
        let out = expanded(
            "fn v() -> View<M> { let s = cell(STATUS); let q = list(up_next(3)); let w = status_line(s); \
             card_section(for t in q { let line = text!(\"{}  {}\", t.artist, t.ms); list_row(line) }).header(w) }",
        );
        assert!(out.contains("__B :: cell (__db , \"s\" , \"STATUS\" , STATUS)"), "{out}");
        assert!(out.contains("__B :: list (__db , \"q\""), "{out}");
        assert!(out.contains("\"up_next (3)\""), "{out}");
        assert!(out.contains("__B :: derive (& [\"s\"] , \"w\""), "{out}");
        assert!(out.contains("Source :: Text"), "{out}");
        assert!(out.contains("__B :: val (:: core :: clone :: Clone :: clone (& w))"), "{out}");
        assert!(out.contains("__db : __B :: Db < '__a >"), "{out}");
        assert!(out.contains("pub fn recording"), "{out}");
    }

    #[test]
    fn what_it_cannot_follow_says_so() {
        assert!(expanded("fn v() -> View<M> { let s = cell(S); let q = list(f(s)); hero() }").contains("not read from other sources"));
        assert!(expanded("fn v() -> View<M> { let s = cell(S); text(text!(\"{}\", s)) }").contains("goes in a `let`"));
        assert!(expanded("fn v(p: &Page) -> View<M> { list_row(p.name.to_uppercase()) }").contains("not their methods"));
        assert!(expanded("fn v(p: &Page) -> View<M> { match p.kind { _ => hero() } }").contains("`match`"));
        assert!(expanded("fn v(p: &Page) -> View<M> { if p.n > 0 { hero() } }").contains("a flag"));
        assert!(expanded("fn v(p: &Page) -> View<M> { for (i, a) in p.rows { hero() } }").contains("one name"));
        assert!(expanded("fn v(p: Page) -> View<M> { hero() }").contains("by reference"));
        assert!(expanded("fn v(p: &Page) { }").contains("View<Msg>"));
    }
}
