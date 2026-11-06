//! `#[derive(Routes)]`: an app's `Route` enum spelled as paths, one pattern
//! per variant — `#[route("/album/:key")]`, `#[route("/join?server=:server&code=:code")]`
//! — the way a web router names its routes. What it writes: `path()` and
//! `from_path()` (each `:name` the variant's field of that name,
//! percent-encoded by `ark_router::url`), `key()`, the route's identity
//! (`PartialEq`, `Eq` and `Hash` by it), and `ark_router::Routable`, whose
//! `home` is the variant marked `#[home]`.
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
//! A query field is a `String`, which a path must carry, or an
//! `Option<String>`, which is left out of the path when it is `None` and
//! read as `None` when it is missing or empty. A query is read in any order.
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
    let body = path.trim_start_matches('/');
    if body.is_empty() {
        return Err(Error::new(lit.span(), "a route's path needs at least one segment"));
    }
    let parts: Vec<&str> = body.split('/').collect();
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
        } else if s.is_empty() || s.contains([':', '#', '%', '&', '=', '~', '(', ')']) {
            return Err(Error::new(lit.span(), format!("`{s}` is not a path segment")));
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
            let pushes = p.segs.iter().enumerate().map(|(n, s)| {
                let sep = if n > 0 { quote!(out.push('/');) } else { quote!() };
                match s {
                    Seg::Lit(l) => quote! { #sep out.push_str(#l); },
                    Seg::Param(i, _) => quote! { #sep ::ark_router::url::encode(#i, &mut out); },
                    Seg::Slug(i) => quote! {
                        if slugs {
                            let slug = ::ark_router::url::slug(#i);
                            if !slug.is_empty() {
                                out.push('/');
                                out.push_str(&slug);
                            }
                        }
                    },
                }
            });
            let queries = p.query.iter().map(|(k, f)| {
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
            let query_sep = match p.query.is_empty() {
                true => quote!(),
                false => quote!(#[allow(unused_assignments)] let mut sep = '?';),
            };
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
                    let #f = ::ark_router::url::decode(&parts[#i..].join("/"));
                    if #f.is_empty() { return ::core::option::Option::None; }
                }),
                Seg::Param(f, false) => Some(quote! {
                    let #f = ::ark_router::url::decode(parts[#i]);
                    if #f.is_empty() { return ::core::option::Option::None; }
                }),
                Seg::Lit(_) | Seg::Slug(_) => None,
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
            /// The path, with no `#` or `/` in front: `kind`, `kind/name`,
            /// `kind/<id>/slug`, or `kind?key=value&…`.
            pub fn path(&self) -> ::std::string::String {
                self.write_path(true)
            }

            /// Which route this is: its path without a slug. Two routes are
            /// the same when their keys are.
            pub fn key(&self) -> ::std::string::String {
                self.write_path(false)
            }

            #[allow(unused_variables, clippy::nonminimal_bool, clippy::overly_complex_bool_expr)]
            fn write_path(&self, slugs: bool) -> ::std::string::String {
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
            fn path(&self) -> ::std::string::String {
                #name::path(self)
            }
            fn from_path(path: &str) -> ::core::option::Option<#name> {
                #name::from_path(path)
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
