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
//!   `account_row::<B>(a)`.
//!
//! What it cannot follow is a compile error that says so: a method called
//! on the record, `match` over it, a binding that is not one name, `while`
//! and `loop`. It writes the function's entry points into a module of the
//! same name: `view(&record)` and `record()`. Drawing it is a toolkit's,
//! an adapter's: `arkui::view::element(view(&record))` in iced,
//! `arkui_web::render(view(&record))` in the browser; the app's crate
//! depends on neither.

use std::collections::BTreeSet;

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, Error, Expr, FnArg, Ident, ItemFn, Member, Pat, ReturnType, Type};

pub fn expand(item: ItemFn) -> syn::Result<TokenStream> {
    let name = &item.sig.ident;
    let vis = &item.vis;
    let attrs = &item.attrs;
    let ret = match &item.sig.output {
        ReturnType::Type(_, t) => t.clone(),
        ReturnType::Default => return Err(Error::new(item.sig.span(), "a view returns a View<Msg>")),
    };
    // It returns a `View<Msg>`, or says so.
    view_msg(&ret)?;
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
        data: vec![params.iter().map(|(i, _)| i.to_string()).collect()],
        in_msg: 0,
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
    let plain_params: Vec<_> = params.iter().map(|(i, t)| quote!(#i: &#t)).collect();
    let names: Vec<_> = params.iter().map(|(i, _)| i).collect();
    let roots = params.iter().map(|(i, _)| {
        let s = i.to_string();
        quote!(::ark_app::view::Sym::root(#s))
    });
    Ok(quote! {
        #(#attrs)*
        #[allow(clippy::needless_lifetimes)]
        #vis fn #name<'__a, __B: ::ark_app::view::Backend>(#(#generic_params),*) -> #ret #body

        /// The view's entry points: drawn from a record, or recorded.
        #[allow(non_snake_case, dead_code)]
        #vis mod #name {
            #[allow(unused_imports)]
            use super::*;

            /// The view of the record as it is.
            pub fn view(#(#plain_params),*) -> #ret {
                super::#name::<::ark_app::view::Real>(#(#names),*)
            }

            /// What the view says, as paths: what SwiftUI and Compose are written from.
            pub fn record() -> #ret {
                super::#name::<::ark_app::view::Recorder>(#(#roots),*)
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

struct Rewrite {
    /// The record's bindings in scope, innermost last.
    data: Vec<BTreeSet<String>>,
    /// How deep inside a struct literal (a message being built).
    in_msg: usize,
    errors: Vec<Error>,
}

impl Rewrite {
    fn is_data(&self, i: &Ident) -> bool {
        let s = i.to_string();
        self.data.iter().rev().any(|d| d.contains(&s))
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
    fn as_ref(&mut self, e: &Expr, what: &str) -> Option<Expr> {
        match self.chain(e) {
            Some((root, fields)) => Some(Rewrite::reference(&root, &fields, e.span())),
            None => {
                self.errors.push(Error::new(
                    e.span(),
                    format!("a view's `{what}` is over the record's fields (`p.list`); work anything else out in the screen record"),
                ));
                None
            }
        }
    }

    fn scoped<T>(&mut self, names: impl IntoIterator<Item = String>, f: impl FnOnce(&mut Self) -> T) -> T {
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
}

impl VisitMut for Rewrite {
    fn visit_expr_mut(&mut self, e: &mut Expr) {
        let span = e.span();
        match e {
            Expr::ForLoop(f) => {
                let Some(item) = self.one_name(&f.pat) else { return };
                let Some(list) = self.as_ref(&f.expr, "for") else { return };
                let label = item.to_string();
                let body = self.scoped([label.clone()], |me| me.block_into_view(&mut f.body));
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
                    let Some(opt) = self.as_ref(&l.expr, "if let") else { return };
                    let label = item.to_string();
                    let yes = self.scoped([label.clone()], |me| me.block_into_view(&mut i.then_branch));
                    *e = parse_quote!(__B::when_some(#opt, #label, |#item| #yes, || #no));
                } else if self.chain(&i.cond).is_some() {
                    let c = self.as_ref(&i.cond, "if").expect("a chain");
                    let yes = self.block_into_view(&mut i.then_branch);
                    *e = parse_quote!(__B::when(#c, || #yes, || #no));
                } else {
                    self.errors.push(Error::new(
                        i.cond.span(),
                        "a view's `if` is over one of the record's flags (`if a.closed`); work the condition out in the screen record",
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
                    format!(
                        "a view reads the record's fields, not its methods: put `{}` in the screen record",
                        m.method
                    ),
                ));
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
                let mut shadow = BTreeSet::new();
                for p in &c.inputs {
                    if let Pat::Ident(i) = p {
                        shadow.insert(i.ident.to_string());
                    }
                }
                let saved = std::mem::take(&mut self.data);
                self.data = saved
                    .iter()
                    .map(|d| d.iter().filter(|n| !shadow.contains(*n)).cloned().collect())
                    .collect();
                self.visit_expr_mut(&mut c.body);
                self.data = saved;
            }
            Expr::Call(c) if c.args.iter().any(|a| matches!(self.chain(a), Some((_, f)) if f.is_empty())) => {
                // A sub-view handed a binding: called over the same backend.
                for a in c.args.iter_mut() {
                    match self.chain(a) {
                        Some((root, fields)) if fields.is_empty() => *a = Rewrite::reference(&root, &fields, a.span()),
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
    fn what_it_cannot_follow_says_so() {
        assert!(expanded("fn v(p: &Page) -> View<M> { list_row(p.name.to_uppercase()) }").contains("not its methods"));
        assert!(expanded("fn v(p: &Page) -> View<M> { match p.kind { _ => hero() } }").contains("`match`"));
        assert!(expanded("fn v(p: &Page) -> View<M> { if p.n > 0 { hero() } }").contains("flags"));
        assert!(expanded("fn v(p: &Page) -> View<M> { for (i, a) in p.rows { hero() } }").contains("one name"));
        assert!(expanded("fn v(p: Page) -> View<M> { hero() }").contains("by reference"));
        assert!(expanded("fn v(p: &Page) { }").contains("View<Msg>"));
    }
}
