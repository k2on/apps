//! The typed path through the bridge (`docs/plan-v5.md`, Experiment: one
//! generator): what `#[ark(ffi)]` on a type, `#[ffi]` on a function and
//! `#[ark(ffi)]` on a job add, for `ark_ffi::typed` — a type's **twin**, a
//! UniFFI record or enum named as the type is, each field its type's twin,
//! with the conversions both ways (`ark_ffi::Twin`); for a view's row
//! (`#[ark(ffi_view)]`) its view object too; for a function, a UniFFI
//! function calling it by name with its typed input. What is written is in
//! the domain's crate, whose UniFFI scaffolding (`crate::UniFfiTag`) and
//! `uniffi` and `ark_ffi` dependencies are behind the feature that asks
//! for any of it (`#[cfg_attr(feature = "ffi", ark(ffi))]`).

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, FnArg, GenericArgument, Ident, ItemFn, LitStr, PathArguments, ReturnType, Type};

use crate::domain::{items, Item};

fn err(span: Span, msg: &str) -> TokenStream {
    syn::Error::new(span, msg).to_compile_error()
}

/// `<t as Twin>::Ffi`, the type `t` crosses as.
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

/// A variant's or a struct's fields as `(name, type)`: a lone unnamed field
/// is `value` (as `Job::Done`'s is), more than one is refused.
fn named(fields: &Fields, span: Span) -> Result<Vec<(Ident, Type, Option<syn::Index>)>, TokenStream> {
    match fields {
        Fields::Named(n) => Ok(n.named.iter().map(|f| (f.ident.clone().unwrap(), f.ty.clone(), None)).collect()),
        Fields::Unit => Ok(vec![]),
        Fields::Unnamed(u) if u.unnamed.len() == 1 => Ok(vec![(Ident::new("value", span), u.unnamed[0].ty.clone(), Some(syn::Index::from(0)))]),
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
    let body = match &input.data {
        Data::Struct(s) => {
            let fs = match named(&s.fields, name.span()) {
                Ok(f) => f,
                Err(e) => return e,
            };
            let decl = fs.iter().map(|(f, t, _)| {
                let ft = ffi_ty(t);
                quote!(pub #f: #ft)
            });
            let out = fs.iter().map(|(f, t, _)| {
                let e = to(t, quote!(self.#f));
                quote!(#f: #e)
            });
            let back = fs.iter().map(|(f, t, _)| {
                let e = from(t, quote!(t.#f));
                quote!(#f: #e)
            });
            quote! {
                #[derive(Clone, Debug, PartialEq, ::uniffi::Record)]
                #[uniffi(name = #label)]
                pub struct #tw { #(#decl,)* }

                impl ::ark_ffi::Twin<crate::UniFfiTag> for #name {
                    type Ffi = #tw;
                    fn ffi(self) -> #tw {
                        #tw { #(#out,)* }
                    }
                    fn from_ffi(t: #tw) -> #name {
                        #name { #(#back,)* }
                    }
                }
            }
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
                let tos = fs.iter().map(|(f, t, _)| to(t, quote!(#f)));
                let froms = fs.iter().map(|(f, t, _)| from(t, quote!(#f)));
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
            quote! {
                #[derive(Clone, Debug, PartialEq, ::uniffi::Enum)]
                #[uniffi(name = #label)]
                pub enum #tw { #(#decl,)* }

                impl ::ark_ffi::Twin<crate::UniFfiTag> for #name {
                    type Ffi = #tw;
                    fn ffi(self) -> #tw {
                        match self { #(#out,)* }
                    }
                    fn from_ffi(t: #tw) -> #name {
                        match t { #(#back,)* }
                    }
                }
            }
        }
        Data::Union(_) => return err(name.span(), "a union has no twin"),
    };
    let view = if view { view_object(name) } else { quote!() };
    quote!(#body #view)
}

/// `<T>View`, a view of rows of `T` as UniFFI holds it, and `<T>Taken`,
/// what it did with the rows that came with it.
fn view_object(name: &Ident) -> TokenStream {
    let (tw, view, taken) = (
        format_ident!("{}Ffi", name),
        format_ident!("{}View", name),
        format_ident!("{}Taken", name),
    );
    let doc = format!("A view of `{name}` rows, read as they are.");
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
            pub fn rows(&self) -> ::std::vec::Vec<#tw> {
                self.0.rows().into_iter().map(<#name as ::ark_ffi::Twin<crate::UniFfiTag>>::ffi).collect()
            }

            pub fn take(&self) -> #taken {
                let (update, rows) = self.0.take();
                #taken {
                    update,
                    rows: rows.into_iter().map(<#name as ::ark_ffi::Twin<crate::UniFfiTag>>::ffi).collect(),
                }
            }
        }

        impl #view {
            pub fn of(t: ::ark_ffi::Typed<#name>) -> ::std::sync::Arc<#view> {
                ::std::sync::Arc::new(#view(t))
            }
        }
    }
}

/// The `n`-th generic argument of `t`'s last segment, counting from the
/// end when `last`.
fn arg(t: &Type, last: bool) -> Option<Type> {
    let Type::Path(p) = t else { return None };
    let PathArguments::AngleBracketed(a) = &p.path.segments.last()?.arguments else {
        return None;
    };
    let mut tys = a.args.iter().filter_map(|g| match g {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    });
    if last {
        tys.next_back()
    } else {
        tys.next()
    }
}

fn is_unit(t: &Type) -> bool {
    matches!(t, Type::Tuple(t) if t.elems.is_empty())
}

/// `#[ffi]` above `#[mutator("name")]`, `#[view("name")]` or
/// `#[query("name")]`: the function as it is, and beside it a UniFFI
/// function `<name>` (dots as underscores) over a peer — a mutator's
/// answering its intent's id, and `<name>_check` its input's first
/// failing refinement; a view's its `<T>View`; a query's its answer.
pub fn function(item: TokenStream) -> TokenStream {
    let f: ItemFn = match syn::parse2(item.clone()) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    let kinds = ["mutator", "view", "query"];
    let found = f.attrs.iter().find_map(|a| {
        let k = a.path().segments.last()?.ident.to_string();
        kinds.contains(&k.as_str()).then(|| a.parse_args::<LitStr>().map(|n| (k, n)))
    });
    let (kind, name) = match found {
        Some(Ok(kn)) => kn,
        Some(Err(e)) => return e.to_compile_error(),
        None => return err(f.sig.ident.span(), "#[ffi] goes above #[mutator(..)], #[view(..)] or #[query(..)]"),
    };
    let input = match f.sig.inputs.iter().nth(2) {
        Some(FnArg::Typed(p)) => (*p.ty).clone(),
        _ => return err(f.sig.ident.span(), "a function of (&Ctx, &Db, input)"),
    };
    let ok = match &f.sig.output {
        ReturnType::Type(_, t) => arg(t, false),
        ReturnType::Default => None,
    };
    let export = format_ident!("{}", name.value().replace('.', "_"));
    let (param, value) = match is_unit(&input) {
        true => (quote!(), quote!(())),
        false => {
            let (ft, back) = (ffi_ty(&input), from(&input, quote!(input)));
            (quote!(, input: #ft), back)
        }
    };
    let peer = quote!(peer: ::std::sync::Arc<::ark_ffi::Peer>);
    let refusal = quote!(::ark_ffi::Refusal);
    let call = match (kind.as_str(), ok) {
        ("mutator", _) => {
            let check = format_ident!("{}_check", export);
            quote! {
                #[::uniffi::export]
                pub fn #export(#peer #param) -> ::std::result::Result<::std::string::String, #refusal> {
                    peer.mutate_value(#name, &#value)
                }

                #[::uniffi::export]
                pub fn #check(#peer #param) -> ::std::option::Option<::std::string::String> {
                    peer.check_value(#name, &#value)
                }
            }
        }
        ("view", Some(plan)) => {
            let Some(Type::Path(row)) = arg(&plan, true) else {
                return err(f.sig.ident.span(), "a view answers Result<Plan<T>, Refusal>");
            };
            let r = &row.path.segments.last().unwrap().ident;
            let view = format_ident!("{}View", r);
            quote! {
                #[::uniffi::export]
                pub fn #export(#peer #param) -> ::std::result::Result<::std::sync::Arc<#view>, #refusal> {
                    Ok(#view::of(peer.view_value::<#row>(#name, &#value)?))
                }
            }
        }
        ("query", Some(t)) => {
            let (ft, out) = (ffi_ty(&t), to(&t, quote!(peer.query_value::<#t>(#name, &#value)?)));
            quote! {
                #[::uniffi::export]
                pub fn #export(#peer #param) -> ::std::result::Result<#ft, #refusal> {
                    Ok(#out)
                }
            }
        }
        _ => return err(f.sig.ident.span(), "a view or a query answers Result<T, Refusal>"),
    };
    quote!(#item #call)
}

/// A job's twins and functions, when its row asks for its twin: `Job<T>`
/// as `Job<T's name>`, `Result<T, String>` as `<Row>Outcome`, the inputs
/// `<Row>Claim`, `<Row>Finish`, `<Row>Cancel` and the answer `<Row>Due`,
/// and `<table>_claim`, `_finish`, `_cancel` and `_due` over a peer.
pub fn job(input: &DeriveInput, outcome: &Type, key_ty: &Type, table: &str) -> TokenStream {
    if !asked(&input.attrs).0 {
        return quote!();
    }
    let row = &input.ident;
    let Type::Path(o) = outcome else {
        return err(row.span(), "a job's outcome is a named type");
    };
    let oname = o.path.segments.last().unwrap().ident.to_string();
    let (job_label, job_tw) = (format!("Job{oname}"), format_ident!("Job{}Ffi", oname));
    let n = |what: &str| (format!("{row}{what}"), format_ident!("{}{}Ffi", row, what));
    let ((out_label, out_tw), (claim_l, claim_tw), (finish_l, finish_tw), (cancel_l, cancel_tw), (due_l, due_tw)) =
        (n("Outcome"), n("Claim"), n("Finish"), n("Cancel"), n("Due"));
    let (o_ffi, key_ffi, row_ffi) = (ffi_ty(outcome), ffi_ty(key_ty), ffi_ty(&syn::parse_quote!(#row)));
    let tag = quote!(crate::UniFfiTag);
    let tw = quote!(::ark_ffi::Twin<#tag>);
    let fname = |f: &str| format_ident!("{}_{}", table, f);
    let (claim, finish, cancel, due) = (fname("claim"), fname("finish"), fname("cancel"), fname("due"));
    let (claim_n, finish_n, cancel_n, due_n) = (
        format!("{table}.claim"),
        format!("{table}.finish"),
        format!("{table}.cancel"),
        format!("{table}.due"),
    );
    quote! {
        #[derive(Clone, Debug, PartialEq, ::uniffi::Enum)]
        #[uniffi(name = #job_label)]
        pub enum #job_tw {
            Pending { due_ms: i64 },
            Claimed { by: ::std::string::String, at_ms: i64 },
            Done { value: #o_ffi },
            Failed { why: ::std::string::String, retry_ms: i64 },
            Cancelled,
        }

        impl #tw for ::ark::effect::Job<#outcome> {
            type Ffi = #job_tw;
            fn ffi(self) -> #job_tw {
                use ::ark::effect::Job as J;
                match self {
                    J::Pending { due_ms } => #job_tw::Pending { due_ms },
                    J::Claimed { by, at_ms } => #job_tw::Claimed { by, at_ms },
                    J::Done(v) => #job_tw::Done { value: <#outcome as #tw>::ffi(v) },
                    J::Failed { why, retry_ms } => #job_tw::Failed { why, retry_ms },
                    J::Cancelled => #job_tw::Cancelled,
                }
            }
            fn from_ffi(t: #job_tw) -> Self {
                use ::ark::effect::Job as J;
                match t {
                    #job_tw::Pending { due_ms } => J::Pending { due_ms },
                    #job_tw::Claimed { by, at_ms } => J::Claimed { by, at_ms },
                    #job_tw::Done { value } => J::Done(<#outcome as #tw>::from_ffi(value)),
                    #job_tw::Failed { why, retry_ms } => J::Failed { why, retry_ms },
                    #job_tw::Cancelled => J::Cancelled,
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, ::uniffi::Enum)]
        #[uniffi(name = #out_label)]
        pub enum #out_tw {
            Ok { value: #o_ffi },
            Err { why: ::std::string::String },
        }

        impl #tw for ::std::result::Result<#outcome, ::std::string::String> {
            type Ffi = #out_tw;
            fn ffi(self) -> #out_tw {
                match self {
                    Ok(v) => #out_tw::Ok { value: <#outcome as #tw>::ffi(v) },
                    Err(why) => #out_tw::Err { why },
                }
            }
            fn from_ffi(t: #out_tw) -> Self {
                match t {
                    #out_tw::Ok { value } => Ok(<#outcome as #tw>::from_ffi(value)),
                    #out_tw::Err { why } => Err(why),
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, ::uniffi::Record)]
        #[uniffi(name = #claim_l)]
        pub struct #claim_tw { pub id: #key_ffi, pub by: ::std::string::String }

        #[derive(Clone, Debug, PartialEq, ::uniffi::Record)]
        #[uniffi(name = #finish_l)]
        pub struct #finish_tw {
            pub id: #key_ffi,
            pub by: ::std::string::String,
            pub outcome: #out_tw,
        }

        #[derive(Clone, Debug, PartialEq, ::uniffi::Record)]
        #[uniffi(name = #cancel_l)]
        pub struct #cancel_tw { pub id: #key_ffi }

        #[derive(Clone, Debug, PartialEq, ::uniffi::Record)]
        #[uniffi(name = #due_l)]
        pub struct #due_tw { pub rows: ::std::vec::Vec<#row_ffi>, pub next_ms: ::std::option::Option<i64> }

        #[::uniffi::export]
        pub fn #claim(peer: ::std::sync::Arc<::ark_ffi::Peer>, input: #claim_tw) -> ::std::result::Result<::std::string::String, ::ark_ffi::Refusal> {
            let input = ::ark::effect::Claim::<#row> { id: <#key_ty as #tw>::from_ffi(input.id), by: input.by };
            peer.mutate_value(#claim_n, &input)
        }

        #[::uniffi::export]
        pub fn #finish(peer: ::std::sync::Arc<::ark_ffi::Peer>, input: #finish_tw) -> ::std::result::Result<::std::string::String, ::ark_ffi::Refusal> {
            let input = ::ark::effect::Finish::<#row> {
                id: <#key_ty as #tw>::from_ffi(input.id),
                by: input.by,
                outcome: <::std::result::Result<#outcome, ::std::string::String> as #tw>::from_ffi(input.outcome),
            };
            peer.mutate_value(#finish_n, &input)
        }

        #[::uniffi::export]
        pub fn #cancel(peer: ::std::sync::Arc<::ark_ffi::Peer>, input: #cancel_tw) -> ::std::result::Result<::std::string::String, ::ark_ffi::Refusal> {
            let input = ::ark::effect::Cancel::<#row> { id: <#key_ty as #tw>::from_ffi(input.id) };
            peer.mutate_value(#cancel_n, &input)
        }

        #[::uniffi::export]
        pub fn #due(peer: ::std::sync::Arc<::ark_ffi::Peer>) -> ::std::result::Result<#due_tw, ::ark_ffi::Refusal> {
            let d: ::ark::effect::Due<#row> = peer.query_value(#due_n, &())?;
            Ok(#due_tw { rows: d.rows.into_iter().map(<#row as #tw>::ffi).collect(), next_ms: d.next_ms })
        }
    }
}
