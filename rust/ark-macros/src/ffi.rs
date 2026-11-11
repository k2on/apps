//! The typed views of the bridge (`docs/plan-v5.md`, Landed: typed views):
//! a type's **twin**, a UniFFI record (or enum) named as the type is, each
//! field its own type's twin, with the conversions both ways
//! (`ark_ffi::Twin`), and for a view's row its view object `<T>View`.
//!
//! The twin is written in the adapter, never in the type's own crate: a
//! domain or an app stays free of UniFFI and the bridge. What
//! `#[ark(ffi)]` (or `#[ark(ffi_view)]`, or `#[derive(Twin)]`) writes beside
//! the type is its shape, and nothing that needs either: each field's type
//! by position (`ark::twin::Field<V, F>`), and a hidden macro,
//! `__ark_twin_<Type>!`, that the adapter calls (through `ark::twins!`) to
//! write the twin there, its `crate::UniFfiTag` the adapter's.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, Index, Type};

use crate::domain::{err, items, Item};

/// `<t as Twin>::Ffi`, the type `t` crosses as; `t` to it; `t` from it.
fn ffi_ty(t: &TokenStream) -> TokenStream {
    quote!(<#t as ::ark_ffi::Twin<crate::UniFfiTag>>::Ffi)
}

fn to(t: &TokenStream, e: TokenStream) -> TokenStream {
    quote!(<#t as ::ark_ffi::Twin<crate::UniFfiTag>>::ffi(#e))
}

fn from(t: &TokenStream, e: TokenStream) -> TokenStream {
    quote!(<#t as ::ark_ffi::Twin<crate::UniFfiTag>>::from_ffi(#e))
}

/// A position, unsuffixed: `Field<0, 1>`.
fn n(i: usize) -> proc_macro2::Literal {
    proc_macro2::Literal::usize_unsuffixed(i)
}

/// The type's path as the adapter names it, inside the macro.
fn path() -> TokenStream {
    quote!($($p)::+)
}

/// Field `f` of variant `v` (of the struct, `v` 0), by position.
fn field(v: usize, f: usize) -> TokenStream {
    let (p, v, f) = (path(), n(v), n(f));
    quote!(<#p as ::ark::twin::Field<#v, #f>>::Ty)
}

/// Whether a type asks for its twin (`ffi`) and, as a view's row, its view
/// object too (`ffi_view`); and whether the twin compares and hashes
/// (`ffi_eq`).
pub fn asked(attrs: &[syn::Attribute]) -> (bool, bool, bool) {
    let flags: Vec<String> = items(attrs)
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .filter_map(|i| match i {
            Item::Flag(f) => Some(f.to_string()),
            _ => None,
        })
        .collect();
    let view = flags.iter().any(|f| f == "ffi_view");
    let eq = flags.iter().any(|f| f == "ffi_eq");
    (view || eq || flags.iter().any(|f| f == "ffi"), view, eq)
}

type Field = (Ident, Type, Option<Index>);

/// Whether a field says `#[serde(default)]`.
fn serde_default(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().filter(|a| a.path().is_ident("serde")).any(|a| {
        let mut found = false;
        let _ = a.parse_nested_meta(|m| {
            found |= m.path.is_ident("default");
            if m.input.peek(syn::Token![=]) {
                let _: syn::Expr = m.value()?.parse()?;
            }
            Ok(())
        });
        found
    })
}

/// A variant's or a struct's fields as `(name, type)`: a lone unnamed field
/// is `value` (as `Job::Done`'s is), more than one is refused.
fn named(fields: &Fields, span: Span) -> syn::Result<Vec<Field>> {
    match fields {
        Fields::Named(n) => Ok(n.named.iter().map(|f| (f.ident.clone().unwrap(), f.ty.clone(), None)).collect()),
        Fields::Unit => Ok(vec![]),
        Fields::Unnamed(u) if u.unnamed.len() == 1 => Ok(vec![(
            Ident::new("value", Span::call_site()),
            u.unnamed[0].ty.clone(),
            Some(Index::from(0)),
        )]),
        Fields::Unnamed(_) => err(span, "a twin names its fields: one unnamed field at most"),
    }
}

/// The twin of a struct or an enum, and for a view's row its view object,
/// when its attributes ask for them; nothing otherwise.
pub fn twin(input: &DeriveInput) -> syn::Result<TokenStream> {
    let (wanted, view, eq) = asked(&input.attrs);
    if !wanted {
        return Ok(quote!());
    }
    shape(input, view, eq)
}

/// What `#[derive(Twin)]` writes: the shape whether or not `ffi` is said.
pub fn twin_derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let (_, view, eq) = asked(&input.attrs);
    shape(input, view, eq)
}

/// Each field's type by position, and the macro that writes the twin.
fn shape(input: &DeriveInput, view: bool, eq: bool) -> syn::Result<TokenStream> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return err(name.span(), "a twin is of a type with no parameters");
    }
    let (label, tw) = (name.to_string(), format_ident!("{}Ffi", name));
    let p = path();
    let mut fields = vec![];
    let (kind, decl, out, back) = match &input.data {
        Data::Struct(s) => {
            let fs = named(&s.fields, name.span())?;
            let ns: Vec<&Ident> = fs.iter().map(|(f, _, _)| f).collect();
            let ts: Vec<TokenStream> = (0..fs.len()).map(|i| field(0, i)).collect();
            for (i, (_, t, _)) in fs.iter().enumerate() {
                let i = n(i);
                fields.push(quote!(impl ::ark::twin::Field<0, #i> for #name { type Ty = #t; }));
            }
            let (tys, tos) = (ts.iter().map(ffi_ty), fs.iter().zip(&ts).map(|((f, _, _), t)| to(t, quote!(self.#f))));
            let froms = fs.iter().zip(&ts).map(|((f, _, _), t)| from(t, quote!(t.#f)));
            // A field added with a default (`#[serde(default)]`) has its
            // type's default in Swift's and Kotlin's constructors too, so
            // what built the record before still does.
            let defaults: Vec<TokenStream> = match &s.fields {
                Fields::Named(n) => n
                    .named
                    .iter()
                    .map(|f| match serde_default(&f.attrs) {
                        true => quote!(#[uniffi(default)]),
                        false => quote!(),
                    })
                    .collect(),
                _ => fs.iter().map(|_| quote!()).collect(),
            };
            (
                quote!(::uniffi::Record),
                quote!(struct #tw { #(#defaults pub #ns: #tys,)* }),
                quote!(#tw { #(#ns: #tos,)* }),
                quote!(#p { #(#ns: #froms,)* }),
            )
        }
        Data::Enum(e) => {
            let (mut decl, mut out, mut back) = (vec![], vec![], vec![]);
            for (vn, v) in e.variants.iter().enumerate() {
                let vi = &v.ident;
                let fs = named(&v.fields, vi.span())?;
                let ns: Vec<&Ident> = fs.iter().map(|(f, _, _)| f).collect();
                let ts: Vec<TokenStream> = (0..fs.len()).map(|i| field(vn, i)).collect();
                for (i, (_, t, _)) in fs.iter().enumerate() {
                    let (vn, i) = (n(vn), n(i));
                    fields.push(quote!(impl ::ark::twin::Field<#vn, #i> for #name { type Ty = #t; }));
                }
                let tys = ts.iter().map(ffi_ty);
                let tos: Vec<_> = fs.iter().zip(&ts).map(|((f, _, _), t)| to(t, quote!(#f))).collect();
                let froms: Vec<_> = fs.iter().zip(&ts).map(|((f, _, _), t)| from(t, quote!(#f))).collect();
                decl.push(quote!(#vi { #(#ns: #tys),* }));
                match fs.first() {
                    Some((_, _, Some(_))) => {
                        out.push(quote!(#p::#vi(value) => #tw::#vi { #(#ns: #tos),* }));
                        back.push(quote!(#tw::#vi { value } => #p::#vi(#(#froms),*)));
                    }
                    _ => {
                        out.push(quote!(#p::#vi { #(#ns),* } => #tw::#vi { #(#ns: #tos),* }));
                        back.push(quote!(#tw::#vi { #(#ns),* } => #p::#vi { #(#ns: #froms),* }));
                    }
                }
            }
            (
                quote!(::uniffi::Enum),
                quote!(enum #tw { #(#decl,)* }),
                quote!(match self { #(#out,)* }),
                quote!(match t { #(#back,)* }),
            )
        }
        Data::Union(_) => return err(name.span(), "a union has no twin"),
    };
    let view = if view { view_object(name) } else { quote!() };
    let (eq_derive, eq_export) = match eq {
        true => (quote!(Eq, Hash,), quote!(#[uniffi::export(Eq, Hash)])),
        false => (quote!(), quote!()),
    };
    let mac = format_ident!("__ark_twin_{}", name);
    let doc = format!("`{name}`'s UniFFI twin, written where it is called: `ark::twins!`.");
    Ok(quote! {
        #(#fields)*

        #[doc = #doc]
        #[doc(hidden)]
        #[macro_export]
        // `crate` is the caller's on purpose: its UniFFI tag, its twins.
        #[allow(clippy::crate_in_macro_def)]
        macro_rules! #mac {
            ($($p:ident)::+) => {
                #[derive(Clone, Debug, PartialEq, #eq_derive #kind)]
                #eq_export
                #[uniffi(name = #label)]
                pub #decl

                impl ::ark_ffi::Twin<crate::UniFfiTag> for #p {
                    type Ffi = #tw;
                    fn ffi(self) -> #tw {
                        #out
                    }
                    fn from_ffi(t: #tw) -> #p {
                        #back
                    }
                }

                #view
            };
        }
    })
}

/// `<T>View`, a view of rows of `T` as UniFFI holds it, made from a view's
/// name and its input's JSON (`ark_ffi::Peer::view_typed`), and `<T>Taken`,
/// what it did with the rows that came with it.
fn view_object(name: &Ident) -> TokenStream {
    let (tw, view, taken) = (
        format_ident!("{}Ffi", name),
        format_ident!("{}View", name),
        format_ident!("{}Taken", name),
    );
    let doc = format!("A view of `{name}` rows, read as they are: no JSON.");
    let p = path();
    let twin = quote!(<#p as ::ark_ffi::Twin<crate::UniFfiTag>>::ffi);
    quote! {
        #[doc = #doc]
        #[derive(::uniffi::Object)]
        pub struct #view(::ark_ffi::Typed<#p>);

        #[derive(Clone, Debug, PartialEq, ::uniffi::Record)]
        pub struct #taken {
            pub update: ::ark_ffi::Taken,
            pub rows: ::std::vec::Vec<#tw>,
        }

        #[::uniffi::export]
        impl #view {
            /// The view `name` (whose rows are this type) on its input's JSON.
            #[uniffi::constructor]
            pub fn new(
                peer: ::std::sync::Arc<::ark_ffi::Peer>,
                name: ::std::string::String,
                input: ::std::string::String,
            ) -> ::std::result::Result<::std::sync::Arc<Self>, ::ark_ffi::Refusal> {
                Ok(::std::sync::Arc::new(#view(peer.view_typed(&name, &input)?)))
            }

            pub fn rows(&self) -> ::std::vec::Vec<#tw> {
                self.0.rows().into_iter().map(#twin).collect()
            }

            pub fn take(&self) -> #taken {
                let (update, rows) = self.0.take();
                #taken { update, rows: rows.into_iter().map(#twin).collect() }
            }
        }
    }
}

/// Each type's twin macro called on its path: `twins!(money_core::{AccountLine,
/// accounts::{AccountKind, Shelving}})`, a `use`'s tree of the types.
pub fn twins(input: TokenStream) -> syn::Result<TokenStream> {
    use syn::parse::Parser;
    use syn::punctuated::Punctuated;
    use syn::UseTree;
    fn flatten(prefix: &mut Vec<Ident>, tree: &UseTree, out: &mut Vec<(Vec<Ident>, Ident)>) -> syn::Result<()> {
        match tree {
            UseTree::Path(p) => {
                prefix.push(p.ident.clone());
                flatten(prefix, &p.tree, out)?;
                prefix.pop();
            }
            UseTree::Name(n) => out.push((prefix.clone(), n.ident.clone())),
            UseTree::Group(g) => {
                for t in &g.items {
                    flatten(prefix, t, out)?;
                }
            }
            UseTree::Rename(r) => return err(r.rename.span(), "a twin is of the type as named"),
            UseTree::Glob(g) => return err(g.star_token.span, "name each type: a twin is written for each"),
        }
        Ok(())
    }
    let trees = Punctuated::<UseTree, syn::Token![,]>::parse_terminated.parse2(input)?;
    let mut tys = vec![];
    for t in &trees {
        flatten(&mut vec![], t, &mut tys)?;
    }
    let mut calls = vec![];
    for (path, t) in &tys {
        let Some(krate) = path.first() else {
            return err(t.span(), "name the type by its crate: `money_core::AccountLine`");
        };
        let mac = format_ident!("__ark_twin_{}", t);
        calls.push(quote!(#krate::#mac!(#(#path)::* :: #t);));
    }
    Ok(quote!(#(#calls)*))
}
