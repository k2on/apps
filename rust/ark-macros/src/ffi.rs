//! The typed views of the bridge (`docs/plan-v5.md`, Landed: typed views):
//! what `#[ark(ffi_view)]` on a view's row type adds for `ark_ffi`'s typed
//! half — the type's **twin**, a UniFFI record (or enum) named as the type
//! is, each field its own type's twin, with the conversions both ways
//! (`ark_ffi::Twin`), and its view object `<T>View`. `#[ark(ffi)]` asks for
//! the twin alone, for a type a row holds. What is written is in the
//! domain's crate, whose UniFFI scaffolding (`crate::UniFfiTag`) and `uniffi`
//! and `ark_ffi` dependencies are behind the feature that asks for any of it
//! (`#[cfg_attr(feature = "ffi", ark(ffi_view))]`).

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, Index, Type};

use crate::domain::{items, Item};

fn err(span: Span, msg: &str) -> TokenStream {
    syn::Error::new(span, msg).to_compile_error()
}

/// `<t as Twin>::Ffi`, the type `t` crosses as; `t` to it; `t` from it.
fn ffi_ty(t: &Type) -> TokenStream {
    quote!(<#t as ::ark_ffi::Twin<crate::UniFfiTag>>::Ffi)
}

fn to(t: &Type, e: TokenStream) -> TokenStream {
    quote!(<#t as ::ark_ffi::Twin<crate::UniFfiTag>>::ffi(#e))
}

fn from(t: &Type, e: TokenStream) -> TokenStream {
    quote!(<#t as ::ark_ffi::Twin<crate::UniFfiTag>>::from_ffi(#e))
}

/// Whether a type asks for its twin (`ffi`) and, as a view's row, its view
/// object too (`ffi_view`).
pub fn asked(attrs: &[syn::Attribute]) -> (bool, bool) {
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
    (view || flags.iter().any(|f| f == "ffi"), view)
}

type Field = (Ident, Type, Option<Index>);

/// A variant's or a struct's fields as `(name, type)`: a lone unnamed field
/// is `value` (as `Job::Done`'s is), more than one is refused.
fn named(fields: &Fields, span: Span) -> Result<Vec<Field>, TokenStream> {
    match fields {
        Fields::Named(n) => Ok(n.named.iter().map(|f| (f.ident.clone().unwrap(), f.ty.clone(), None)).collect()),
        Fields::Unit => Ok(vec![]),
        Fields::Unnamed(u) if u.unnamed.len() == 1 => Ok(vec![(Ident::new("value", span), u.unnamed[0].ty.clone(), Some(Index::from(0)))]),
        Fields::Unnamed(_) => Err(err(span, "a twin names its fields: one unnamed field at most")),
    }
}

/// The twin of a struct or an enum, and for a view's row its view object,
/// when its attributes ask for them; nothing otherwise.
pub fn twin(input: &DeriveInput) -> TokenStream {
    let (wanted, view) = asked(&input.attrs);
    if !wanted {
        return quote!();
    }
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return err(name.span(), "a twin is of a type with no parameters");
    }
    let (label, tw) = (name.to_string(), format_ident!("{}Ffi", name));
    let (kind, decl, out, back) = match &input.data {
        Data::Struct(s) => {
            let fs = match named(&s.fields, name.span()) {
                Ok(f) => f,
                Err(e) => return e,
            };
            let ns: Vec<&Ident> = fs.iter().map(|(f, _, _)| f).collect();
            let (tys, tos) = (fs.iter().map(|(_, t, _)| ffi_ty(t)), fs.iter().map(|(f, t, _)| to(t, quote!(self.#f))));
            let froms = fs.iter().map(|(f, t, _)| from(t, quote!(t.#f)));
            (
                quote!(::uniffi::Record),
                quote!(struct #tw { #(pub #ns: #tys,)* }),
                quote!(#tw { #(#ns: #tos,)* }),
                quote!(#name { #(#ns: #froms,)* }),
            )
        }
        Data::Enum(e) => {
            let (mut decl, mut out, mut back) = (vec![], vec![], vec![]);
            for v in &e.variants {
                let vi = &v.ident;
                let fs = match named(&v.fields, vi.span()) {
                    Ok(f) => f,
                    Err(e) => return e,
                };
                let ns: Vec<&Ident> = fs.iter().map(|(f, _, _)| f).collect();
                let tys = fs.iter().map(|(_, t, _)| ffi_ty(t));
                let tos: Vec<_> = fs.iter().map(|(f, t, _)| to(t, quote!(#f))).collect();
                let froms: Vec<_> = fs.iter().map(|(f, t, _)| from(t, quote!(#f))).collect();
                decl.push(quote!(#vi { #(#ns: #tys),* }));
                match fs.first() {
                    Some((_, _, Some(_))) => {
                        out.push(quote!(#name::#vi(value) => #tw::#vi { #(#ns: #tos),* }));
                        back.push(quote!(#tw::#vi { value } => #name::#vi(#(#froms),*)));
                    }
                    _ => {
                        out.push(quote!(#name::#vi { #(#ns),* } => #tw::#vi { #(#ns: #tos),* }));
                        back.push(quote!(#tw::#vi { #(#ns),* } => #name::#vi { #(#ns: #froms),* }));
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
    quote! {
        #[derive(Clone, Debug, PartialEq, #kind)]
        #[uniffi(name = #label)]
        pub #decl

        impl ::ark_ffi::Twin<crate::UniFfiTag> for #name {
            type Ffi = #tw;
            fn ffi(self) -> #tw {
                #out
            }
            fn from_ffi(t: #tw) -> #name {
                #back
            }
        }

        #view
    }
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
    let twin = quote!(<#name as ::ark_ffi::Twin<crate::UniFfiTag>>::ffi);
    quote! {
        #[doc = #doc]
        #[derive(::uniffi::Object)]
        pub struct #view(::ark_ffi::Typed<#name>);

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
