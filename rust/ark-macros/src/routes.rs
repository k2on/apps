//! `#[derive(Routes)]`: an app's `Route` enum spelled as paths, one pattern
//! per variant — `#[route("/album/:key")]`, `#[route("/join?server=:server&code=:code")]`
//! — the way a web router names its routes. What it writes: `path()` and
//! `from_path()` (each `:name` the variant's field of that name,
//! percent-encoded by `ark_router::url`), the route's identity as its path
//! (`PartialEq`, `Eq`, `Hash`), and `ark_router::Routable`, whose `home` is
//! the variant marked `#[home]`.
//!
//! A variant may have more than one `#[route]`: the first is how it is
//! written, the rest are read too (an old link's spelling). A field that is
//! in no pattern — a title — is not part of the path, and a route read back
//! has it as its type's default. A pattern's last `:name` in the path takes
//! the rest of it, so a slash an old link left unescaped in it survives.
//!
//! A query field is a `String`, which a path must carry, or an
//! `Option<String>`, which is left out of the path when it is `None` and
//! read as `None` when it is missing or empty. A query is read in any order.
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Type};

enum Seg {
    Lit(String),
    Param(syn::Ident),
}

struct Pattern {
    segs: Vec<Seg>,
    query: Vec<(String, syn::Ident)>,
}

impl Pattern {
    fn fields(&self) -> impl Iterator<Item = &syn::Ident> {
        self.segs
            .iter()
            .filter_map(|s| match s {
                Seg::Param(i) => Some(i),
                Seg::Lit(_) => None,
            })
            .chain(self.query.iter().map(|(_, f)| f))
    }
}

fn parse_pattern(lit: &LitStr) -> syn::Result<Pattern> {
    let text = lit.value();
    let (path, query) = match text.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (text.clone(), None),
    };
    let body = path.trim_start_matches('/');
    if body.is_empty() {
        return Err(Error::new(lit.span(), "a route's path needs at least one segment"));
    }
    let segs = body
        .split('/')
        .map(|s| match s.strip_prefix(':') {
            Some(name) if !name.is_empty() => Ok(Seg::Param(syn::Ident::new(name, lit.span()))),
            _ if s.is_empty() || s.contains([':', '#', '%', '&', '=']) => Err(Error::new(lit.span(), format!("`{s}` is not a path segment"))),
            _ => Ok(Seg::Lit(s.to_string())),
        })
        .collect::<syn::Result<_>>()?;
    let query = match query {
        None => vec![],
        Some(q) => q
            .split('&')
            .map(|pair| match pair.split_once("=:") {
                Some((k, f)) if !k.is_empty() && !f.is_empty() && !k.contains(['=', '#', '%', '/']) => {
                    Ok((k.to_string(), syn::Ident::new(f, lit.span())))
                }
                _ => Err(Error::new(lit.span(), format!("`{pair}` is not `name=:field`"))),
            })
            .collect::<syn::Result<_>>()?,
    };
    Ok(Pattern { segs, query })
}

/// Whether a field's type is `Option<…>`.
fn optional(t: &Type) -> bool {
    matches!(t, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

pub fn routes(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let Data::Enum(e) = &input.data else {
        return Err(Error::new(name.span(), "#[derive(Routes)] is for an enum of routes"));
    };
    let mut writes = vec![];
    let mut reads = vec![];
    let mut home = None;
    for v in &e.variants {
        let vi = &v.ident;
        let mut patterns = vec![];
        for a in &v.attrs {
            if a.path().is_ident("route") {
                patterns.push(parse_pattern(&a.parse_args::<LitStr>()?)?);
            } else if a.path().is_ident("home") {
                if !matches!(v.fields, Fields::Unit) {
                    return Err(Error::new(vi.span(), "the #[home] route carries nothing"));
                }
                home = Some(vi.clone());
            }
        }
        let Some(first) = patterns.first() else {
            return Err(Error::new(vi.span(), "every route needs a #[route(\"/…\")]"));
        };
        let fields: Vec<(syn::Ident, bool)> = match &v.fields {
            Fields::Named(f) => f.named.iter().map(|f| (f.ident.clone().unwrap(), optional(&f.ty))).collect(),
            Fields::Unit => vec![],
            Fields::Unnamed(_) => return Err(Error::new(vi.span(), "a route's fields are named, as its path's are")),
        };
        let is_optional = |i: &syn::Ident| fields.iter().any(|(f, o)| f == i && *o);
        for p in &patterns {
            for i in p.fields() {
                if !fields.iter().any(|(f, _)| f == i) {
                    return Err(Error::new(i.span(), format!("`{vi}` has no field `{i}`")));
                }
            }
            for s in &p.segs {
                if let Seg::Param(i) = s {
                    if is_optional(i) {
                        return Err(Error::new(i.span(), "an optional field goes in the query"));
                    }
                }
            }
        }
        // Writing: the first pattern.
        let pushes = first.segs.iter().enumerate().map(|(n, s)| {
            let sep = if n > 0 { quote!(out.push('/');) } else { quote!() };
            match s {
                Seg::Lit(l) => quote! { #sep out.push_str(#l); },
                Seg::Param(i) => quote! { #sep ::ark_router::url::encode(#i, &mut out); },
            }
        });
        let queries = first.query.iter().map(|(k, f)| {
            let key = format!("{k}=");
            match is_optional(f) {
                false => quote! {
                    out.push(sep); sep = '&';
                    out.push_str(#key);
                    ::ark_router::url::encode(#f, &mut out);
                },
                true => quote! {
                    if let ::core::option::Option::Some(value) = #f {
                        out.push(sep); sep = '&';
                        out.push_str(#key);
                        ::ark_router::url::encode(value, &mut out);
                    }
                },
            }
        });
        let binds: Vec<&syn::Ident> = first.fields().collect();
        let pat = match &v.fields {
            Fields::Unit => quote!(#name::#vi),
            _ => quote!(#name::#vi { #(#binds,)* .. }),
        };
        let query_sep = match first.query.is_empty() {
            true => quote!(),
            false => quote!(#[allow(unused_assignments)] let mut sep = '?';),
        };
        writes.push(quote! { #pat => { #(#pushes)* #query_sep #(#queries)* } });
        // Reading: every pattern, in order.
        for p in &patterns {
            let n = p.segs.len();
            let last_param = matches!(p.segs.last(), Some(Seg::Param(_)));
            let count = if last_param {
                quote!(parts.len() >= #n)
            } else {
                quote!(parts.len() == #n)
            };
            let checks = p.segs.iter().enumerate().filter_map(|(i, s)| match s {
                Seg::Lit(l) => Some(quote!(parts[#i] == #l)),
                Seg::Param(_) => None,
            });
            let lets = p.segs.iter().enumerate().filter_map(|(i, s)| match s {
                Seg::Param(f) if i + 1 == n => Some(quote! {
                    let #f = ::ark_router::url::decode(&parts[#i..].join("/"));
                    if #f.is_empty() { return ::core::option::Option::None; }
                }),
                Seg::Param(f) => Some(quote! {
                    let #f = ::ark_router::url::decode(parts[#i]);
                    if #f.is_empty() { return ::core::option::Option::None; }
                }),
                Seg::Lit(_) => None,
            });
            let query_lets = p.query.iter().map(|(k, f)| {
                let found = quote! {
                    query.iter().find(|(key, _)| *key == #k).map(|(_, v)| ::ark_router::url::decode(v)).filter(|v| !v.is_empty())
                };
                match is_optional(f) {
                    true => quote! { let #f = #found; },
                    false => quote! { let #f = #found?; },
                }
            });
            let build = match &v.fields {
                Fields::Unit => quote!(#name::#vi),
                _ => {
                    let inits = fields.iter().map(|(f, _)| match p.fields().any(|i| i == f) {
                        true => quote!(#f),
                        false => quote!(#f: ::core::default::Default::default()),
                    });
                    quote!(#name::#vi { #(#inits,)* })
                }
            };
            // A pattern with no query reads a path with none.
            let no_query = match p.query.is_empty() {
                true => quote!(&&query.is_empty()),
                false => quote!(),
            };
            reads.push(quote! {
                if #count #no_query #(&& #checks)* {
                    let read = (|| -> ::core::option::Option<#name> {
                        #(#lets)*
                        #(#query_lets)*
                        ::core::option::Option::Some(#build)
                    })();
                    if read.is_some() { return read; }
                }
            });
        }
    }
    let Some(home) = home else {
        return Err(Error::new(name.span(), "mark the route a path that names nothing lands on #[home]"));
    };
    Ok(quote! {
        impl #name {
            /// The path, with no `#` or `/` in front: `kind`, `kind/name`, or
            /// `kind?key=value&…`.
            pub fn path(&self) -> ::std::string::String {
                let mut out = ::std::string::String::new();
                match self { #(#writes)* }
                out
            }

            /// The route a path names, or `None` for one that names nothing
            /// here. A leading `#` or `/` is allowed.
            pub fn from_path(path: &str) -> ::core::option::Option<#name> {
                let body = path.trim_start_matches('#').trim_start_matches('/');
                let (body, query) = match body.split_once('?') {
                    ::core::option::Option::Some((b, q)) => (b, q),
                    ::core::option::Option::None => (body, ""),
                };
                let query: ::std::vec::Vec<(&str, &str)> = query
                    .split('&')
                    .filter(|pair| !pair.is_empty())
                    .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
                    .collect();
                let parts: ::std::vec::Vec<&str> = body.split('/').collect();
                #(#reads)*
                ::core::option::Option::None
            }
        }

        /// Two routes are the same when their paths are: a title is what a
        /// route is called, not which it is.
        impl ::core::cmp::PartialEq for #name {
            fn eq(&self, other: &#name) -> bool {
                self.path() == other.path()
            }
        }

        impl ::core::cmp::Eq for #name {}

        impl ::core::hash::Hash for #name {
            fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
                ::core::hash::Hash::hash(&self.path(), state)
            }
        }

        impl ::ark_router::Routable for #name {
            fn path(&self) -> ::std::string::String {
                #name::path(self)
            }
            fn from_path(path: &str) -> ::core::option::Option<#name> {
                #name::from_path(path)
            }
            fn home() -> #name {
                #name::#home
            }
        }
    })
}
