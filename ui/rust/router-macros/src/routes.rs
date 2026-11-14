//! `#[derive(Routes)]`: an app's `Route` enum spelled as paths, one pattern
//! per variant — `#[route("/album/:key")]`, `#[route("/join?server=:server&code=:code")]`
//! — the way a web router names its routes. What it writes: `address()`
//! and `from_address()` (`ark_router::Address`: each `:name` a path segment,
//! the variant's field of that name; each `key=:name` a query key), `url()`
//! and `from_url()` (the address as a URL, `/album/water-music/`), `key()`,
//! the route's identity (`PartialEq`, `Eq` and `Hash` by it), and
//! `ark_router::Routable`, whose `home` is the variant marked `#[home]`.
//! `#[route("/")]` is the root, with no segment.
//!
//! **Ids, and a slug.** `:id(hex)` is a row's id (`ark_router::url::id`): it
//! reads only 32 hex digits, so an old link's name in its place falls
//! through to the next pattern. A last segment `~name` is the field `name` as
//! a readable slug (`account/<id>/checking`): written when there is one, and
//! on reading allowed and ignored, so the field reads back as its default.
//! A slug is not in the `key`, so a renamed row is the same route.
//!
//! A variant may have more than one `#[route]`. It is written by the first
//! whose path fields (the slug aside) are all non-empty, and read by every
//! one, in order: `#[route("/account/:id(hex)/~name")] #[route("/account/:name")]`
//! writes the id, still reads an old link's name, and an old link read back
//! (no id, a name) is written as it was. A field that is in no pattern — a
//! title — is not part of the path, and a route read back has it as its
//! type's default. A pattern's last `:name` in the path takes the rest of it,
//! so a slash an old link left unescaped in it survives.
//!
//! A query field is a `String`, which an address must carry, or an
//! `Option<String>`, which is left out when it is `None` and read as `None`
//! when it is missing or empty. A query is read in any order, and a key a
//! pattern does not name is the UI's (`?inspector=1`), not the route's: it
//! is passed over.
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Type};

enum Seg {
    Lit(String),
    /// A field, and whether it is a row's id (`:id(hex)`).
    Param(syn::Ident, bool),
    /// A field as a readable slug, last: `~name`.
    Slug(syn::Ident),
}

struct Pattern {
    segs: Vec<Seg>,
    query: Vec<(String, syn::Ident)>,
}

impl Pattern {
    /// The fields the pattern reads: its path's and its query's, not a slug's.
    fn fields(&self) -> impl Iterator<Item = &syn::Ident> {
        self.segs
            .iter()
            .filter_map(|s| match s {
                Seg::Param(i, _) => Some(i),
                Seg::Lit(_) | Seg::Slug(_) => None,
            })
            .chain(self.query.iter().map(|(_, f)| f))
    }

    fn slug(&self) -> Option<&syn::Ident> {
        match self.segs.last() {
            Some(Seg::Slug(i)) => Some(i),
            _ => None,
        }
    }
}

fn field(name: &str, lit: &LitStr) -> syn::Result<syn::Ident> {
    match syn::parse_str::<syn::Ident>(name) {
        Ok(_) => Ok(syn::Ident::new(name, lit.span())),
        Err(_) => Err(Error::new(lit.span(), format!("`{name}` is not a field's name"))),
    }
}

fn parse_pattern(lit: &LitStr) -> syn::Result<Pattern> {
    let text = lit.value();
    let (path, query) = match text.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (text.clone(), None),
    };
    let body = path.trim_matches('/');
    let parts: Vec<&str> = match body.is_empty() {
        true => vec![],
        false => body.split('/').collect(),
    };
    let mut segs = vec![];
    for (n, s) in parts.iter().enumerate() {
        let seg = if let Some(name) = s.strip_prefix(':') {
            match name.strip_suffix("(hex)") {
                Some(name) => Seg::Param(field(name, lit)?, true),
                None => Seg::Param(field(name, lit)?, false),
            }
        } else if let Some(name) = s.strip_prefix('~') {
            if n + 1 != parts.len() || n == 0 {
                return Err(Error::new(lit.span(), "a slug (`~name`) is the last segment, after another"));
            }
            Seg::Slug(field(name, lit)?)
        } else if s.is_empty()
            || s.contains([':', '#', '%', '&', '=', '~', '(', ')'])
            || s.bytes().any(|b| b.is_ascii_uppercase() || b == b'_' || b == b' ')
        {
            return Err(Error::new(
                lit.span(),
                format!("`{s}` is not a path segment: lowercase kebab, `expanding-card`"),
            ));
        } else {
            Seg::Lit(s.to_string())
        };
        segs.push(seg);
    }
    let query = match query {
        None => vec![],
        Some(q) => q
            .split('&')
            .map(|pair| match pair.split_once("=:") {
                Some((k, f)) if !k.is_empty() && !f.is_empty() && !k.contains(['=', '#', '%', '/']) => Ok((k.to_string(), field(f, lit)?)),
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
        if patterns.is_empty() {
            return Err(Error::new(vi.span(), "every route needs a #[route(\"/…\")]"));
        }
        let fields: Vec<(syn::Ident, bool)> = match &v.fields {
            Fields::Named(f) => f.named.iter().map(|f| (f.ident.clone().unwrap(), optional(&f.ty))).collect(),
            Fields::Unit => vec![],
            Fields::Unnamed(_) => return Err(Error::new(vi.span(), "a route's fields are named, as its path's are")),
        };
        let is_optional = |i: &syn::Ident| fields.iter().any(|(f, o)| f == i && *o);
        for p in &patterns {
            for i in p.fields().chain(p.slug()) {
                if !fields.iter().any(|(f, _)| f == i) {
                    return Err(Error::new(i.span(), format!("`{vi}` has no field `{i}`")));
                }
            }
            for s in &p.segs {
                if let Seg::Param(i, _) | Seg::Slug(i) = s {
                    if is_optional(i) {
                        return Err(Error::new(i.span(), "an optional field goes in the query"));
                    }
                }
            }
        }
        // Writing: the first pattern whose path fields are all there. Only
        // the first pattern with a given set of them can ever be chosen.
        let mut bound: Vec<syn::Ident> = vec![];
        let mut choices: Vec<(Vec<String>, Vec<syn::Ident>, TokenStream)> = vec![];
        for p in &patterns {
            let needs: Vec<syn::Ident> = p.fields().filter(|i| !is_optional(i)).cloned().collect();
            let mut set: Vec<String> = needs.iter().map(|i| i.to_string()).collect();
            set.sort();
            if choices.iter().any(|(s, ..)| *s == set) {
                continue;
            }
            for i in p.fields().chain(p.slug()) {
                if !bound.contains(i) {
                    bound.push(i.clone());
                }
            }
            let pushes = p.segs.iter().map(|s| match s {
                Seg::Lit(l) => quote! { out.push(::std::string::String::from(#l)); },
                Seg::Param(i, _) => quote! { out.push(::std::clone::Clone::clone(#i)); },
                Seg::Slug(i) => quote! {
                    if slugs {
                        let slug = ::ark_router::url::slug(#i);
                        if !slug.is_empty() {
                            out.push(slug);
                        }
                    }
                },
            });
            let queries = p.query.iter().map(|(k, f)| match is_optional(f) {
                false => quote! { query.push((::std::string::String::from(#k), ::std::clone::Clone::clone(#f))); },
                true => quote! {
                    if let ::core::option::Option::Some(value) = #f {
                        if !value.is_empty() {
                            query.push((::std::string::String::from(#k), ::std::clone::Clone::clone(value)));
                        }
                    }
                },
            });
            let query_sep = quote!();
            choices.push((set, needs, quote! { #(#pushes)* #query_sep #(#queries)* }));
        }
        let pat = match &v.fields {
            Fields::Unit => quote!(#name::#vi),
            _ => quote!(#name::#vi { #(#bound,)* .. }),
        };
        let write = match choices.len() {
            1 => choices[0].2.clone(),
            _ => {
                let fallback = choices[0].2.clone();
                let arms = choices.iter().map(|(_, needs, body)| {
                    let conds = needs.iter().map(|i| quote!(!#i.is_empty()));
                    quote! { if true #(&& #conds)* { #body } else }
                });
                quote! { #(#arms)* { #fallback } }
            }
        };
        writes.push(quote! { #pat => { #write } });
        // Reading: every pattern, in order.
        for p in &patterns {
            let slugged = p.slug().is_some();
            let n = p.segs.len() - usize::from(slugged);
            let last_param = !slugged && matches!(p.segs.last(), Some(Seg::Param(_, false)));
            let count = if slugged {
                quote!((parts.len() == #n || parts.len() == #n + 1))
            } else if last_param {
                quote!(parts.len() >= #n)
            } else {
                quote!(parts.len() == #n)
            };
            let checks = p.segs.iter().enumerate().filter_map(|(i, s)| match s {
                Seg::Lit(l) => Some(quote!(parts[#i] == #l)),
                _ => None,
            });
            let lets = p.segs.iter().enumerate().filter_map(|(i, s)| match s {
                Seg::Param(f, true) => Some(quote! {
                    let #f = ::ark_router::url::id(parts[#i])?;
                }),
                Seg::Param(f, false) if last_param && i + 1 == n => Some(quote! {
                    let #f = parts[#i..].join("/");
                    if #f.is_empty() { return ::core::option::Option::None; }
                }),
                Seg::Param(f, false) => Some(quote! {
                    let #f = ::std::string::String::from(parts[#i]);
                    if #f.is_empty() { return ::core::option::Option::None; }
                }),
                Seg::Lit(_) | Seg::Slug(_) => None,
            });
            let query_lets = p.query.iter().map(|(k, f)| {
                let found = quote! {
                    address.get(#k).filter(|v| !v.is_empty()).map(::std::string::String::from)
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
            reads.push(quote! {
                if #count #(&& #checks)* {
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
            /// The address (`ark_router::Address`): the path that names it,
            /// a readable slug after a row's id, and its own query (a
            /// sign-in's server). Its URL is [`Self::url`].
            pub fn address(&self) -> ::ark_router::Address {
                self.write(true)
            }

            /// `/album/water-music/`, `/join/?server=…`: the address as a
            /// URL, at the root (`Address::url_under` puts it under a base).
            pub fn url(&self) -> ::std::string::String {
                self.address().url()
            }

            /// Which route this is: its URL without a slug. Two routes are
            /// the same when their keys are.
            pub fn key(&self) -> ::std::string::String {
                self.write(false).url()
            }

            #[allow(unused_variables, unused_mut, clippy::nonminimal_bool, clippy::overly_complex_bool_expr)]
            fn write(&self, slugs: bool) -> ::ark_router::Address {
                let mut out: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
                let mut query: ::std::vec::Vec<(::std::string::String, ::std::string::String)> = ::std::vec::Vec::new();
                match self { #(#writes)* }
                ::ark_router::Address { path: out, query }
            }

            /// The route an address names, or `None` for one that names
            /// nothing here. A query key no pattern names is the UI's, and
            /// passed over.
            pub fn from_address(address: &::ark_router::Address) -> ::core::option::Option<#name> {
                let parts: ::std::vec::Vec<&str> = address.path.iter().map(::std::string::String::as_str).collect();
                #(#reads)*
                ::core::option::Option::None
            }

            /// The route a URL names (`Address::of_url` at the root: a
            /// phone's `scheme://…` and a fragment read too), or `None`.
            pub fn from_url(url: &str) -> ::core::option::Option<#name> {
                #name::from_address(&::ark_router::Address::of_url(url, "/"))
            }
        }

        /// Two routes are the same when their keys are: a title is what a
        /// route is called, not which it is.
        impl ::core::cmp::PartialEq for #name {
            fn eq(&self, other: &#name) -> bool {
                self.key() == other.key()
            }
        }

        impl ::core::cmp::Eq for #name {}

        impl ::core::hash::Hash for #name {
            fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
                ::core::hash::Hash::hash(&self.key(), state)
            }
        }

        impl ::ark_router::Routable for #name {
            fn address(&self) -> ::ark_router::Address {
                #name::address(self)
            }
            fn from_address(address: &::ark_router::Address) -> ::core::option::Option<#name> {
                #name::from_address(address)
            }
            fn key(&self) -> ::std::string::String {
                #name::key(self)
            }
            fn home() -> #name {
                #name::#home
            }
        }
    })
}
