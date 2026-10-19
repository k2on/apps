//! The function attributes' common expansion (§1.12). `fn f(ctx: Ctx, db:
//! S, a: A, …) -> R { body }` becomes a module `f` whose `built()` is the
//! function built once — its parameters `a, …` in order, its body lowered
//! by [`crate::lower`] — and a Rust `fn f` taking and giving expressions,
//! which is what a call to it from another body is. `ctx` is the parameter
//! of type `Ctx`, `db` the one named `db` (its type the schema's table
//! set); neither is a parameter of the function, both are ambient.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse::Parser, punctuated::Punctuated, FnArg, ItemFn, Pat, ReturnType, Token, Type};

use crate::lower::{Frame, Lower};

#[derive(Clone, Copy)]
pub enum Kind {
    Mutator,
    Query,
    Scope,
    Live,
    Helper,
}

pub fn expand(kind: Kind, args: TokenStream, item: TokenStream) -> TokenStream {
    go(kind, args, item).unwrap_or_else(|e| e.to_compile_error())
}

/// `Result<(), _>` and nothing are `()`.
fn ret(r: &ReturnType) -> Type {
    match r {
        ReturnType::Type(_, t) if !matches!(&**t, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Result")) => (**t).clone(),
        _ => syn::parse_quote!(()),
    }
}

fn go(kind: Kind, args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let f: ItemFn = syn::parse2(item)?;
    let (vis, name) = (&f.vis, &f.sig.ident);
    let binding = match args.is_empty() {
        true => name.to_string(),
        false => syn::parse2::<syn::LitStr>(args)?.value(),
    };
    let mut cx = Lower { scope: vec![], ctx: None, db: None, me: name.to_string() };
    let (mut params, mut sig, mut pass) = (vec![], vec![], vec![]);
    for a in &f.sig.inputs {
        let FnArg::Typed(a) = a else { return Err(syn::Error::new_spanned(a, "a method is not in the subset")) };
        let Pat::Ident(id) = &*a.pat else { return Err(crate::lower::no(&a.pat, "a parameter pattern")) };
        let (id, ty) = (&id.ident, &a.ty);
        let is = |n: &str| matches!(&**ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == n));
        if is("Ctx") {
            cx.ctx = Some(id.to_string());
            sig.push(quote!(#id: ::ark5::author::Ctx));
        } else if id == "db" || id == "_db" {
            cx.db = Some((id.to_string(), (**ty).clone()));
            sig.push(quote!(#id: #ty));
        } else {
            let rv = format_ident!("__a_{}", id);
            params.push((id.to_string(), quote!(let #rv = ::ark5::author::param::<#ty>();)));
            sig.push(quote!(#rv: ::ark5::author::Ex<<#ty as ::ark5::author::Shape>::Base>));
            pass.push(quote!(#rv.e));
        }
    }
    let n = params.len();
    let frame = Frame::of(params.iter().enumerate().map(|(i, p)| (p.0.clone(), (n - 1 - i) as u32)));
    cx.scope.push(frame);
    let body = cx.block(&f.block)?;
    let lets = params.iter().map(|p| &p.1);
    let r = ret(&f.sig.output);
    let k = format_ident!("{}", ["Mutator", "Query", "Scope", "Live", "Helper"][kind as usize]);
    let doc = f.attrs.iter().filter(|a| a.path().is_ident("doc"));
    Ok(quote! {
        #(#doc)*
        #[allow(non_snake_case, unused, unreachable_code, clippy::all)]
        #vis mod #name {
            use super::*;
            pub const NAME: &str = #binding;
            pub fn built() -> &'static ::ark5::author::Built {
                static B: ::std::sync::OnceLock<::ark5::author::Built> = ::std::sync::OnceLock::new();
                ::ark5::author::once(&B, || ::ark5::author::build::<#r>(NAME, ::ark5::author::Kind::#k, || {
                    #(#lets)*
                    let __body: ::ark5::author::Ex<<#r as ::ark5::author::Shape>::Base> = #body;
                    __body
                }))
            }
        }
        #[allow(non_snake_case, unused, clippy::all)]
        #vis fn #name(#(#sig),*) -> ::ark5::author::Ex<<#r as ::ark5::author::Shape>::Base> {
            ::ark5::author::call(#name::built(), vec![#(#pass),*])
        }
    })
}

/// `module!{ schema: S, functions: [a, b::c] }`.
pub fn module(input: TokenStream) -> TokenStream {
    let parse = |input: syn::parse::ParseStream| -> syn::Result<TokenStream> {
        let (mut schema, mut fns) = (None, vec![]);
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "schema" => schema = Some(input.parse::<syn::Path>()?),
                "functions" => {
                    let c;
                    syn::bracketed!(c in input);
                    fns = Punctuated::<syn::Path, Token![,]>::parse_terminated(&c)?.into_iter().collect();
                }
                _ => return Err(syn::Error::new_spanned(key, "module! takes schema: and functions:")),
            }
            let _ = input.parse::<Option<Token![,]>>()?;
        }
        let schema = schema.ok_or_else(|| input.error("module! needs schema:"))?;
        Ok(quote!(::ark5::author::assemble(#schema::schema(), &[#(#fns::built()),*])))
    };
    parse.parse2(input).unwrap_or_else(|e| e.to_compile_error())
}
