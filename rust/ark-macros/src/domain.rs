//! §1.16 The derives and attributes a domain is written with: `Row`,
//! `Input`, `Describe` and `Schema` read a struct or an enum and write what
//! the kernel needs of it — its table, its key, its typed indexes, its
//! refinements, its type description, its tables' handles — and
//! `#[mutator(..)]` and the rest leave a function as it is and add the one
//! line that registers it by name.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Expr, Fields, Ident, ItemFn, LitStr, Token, Type};

fn err(span: Span, msg: &str) -> TokenStream {
    syn::Error::new(span, msg).to_compile_error()
}

/// `snake_case` of a type's name: `PlaylistItem` → `playlist_item`.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// One item of an `#[ark(..)]` attribute.
pub(crate) enum Item {
    Flag(Ident),
    Is(Ident, Expr),
    /// `index(a, b)` / `unique(a, b)`, maybe `as NAME`.
    Cols(Ident, Vec<Ident>, Option<Ident>),
    Refs(Type),
}

impl Parse for Item {
    fn parse(input: ParseStream) -> syn::Result<Item> {
        let name: Ident = input.parse()?;
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            return Ok(Item::Is(name, input.parse()?));
        }
        if input.peek(syn::token::Paren) {
            let inner;
            syn::parenthesized!(inner in input);
            if name == "refs" {
                return Ok(Item::Refs(inner.parse()?));
            }
            let cols = Punctuated::<Ident, Token![,]>::parse_terminated(&inner)?.into_iter().collect();
            let alias = if input.peek(Token![as]) {
                input.parse::<Token![as]>()?;
                Some(input.parse()?)
            } else {
                None
            };
            return Ok(Item::Cols(name, cols, alias));
        }
        Ok(Item::Flag(name))
    }
}

pub(crate) fn items(attrs: &[syn::Attribute]) -> syn::Result<Vec<Vec<Item>>> {
    items_of(attrs, "ark")
}

fn items_of(attrs: &[syn::Attribute], name: &str) -> syn::Result<Vec<Vec<Item>>> {
    let mut out = vec![];
    for a in attrs.iter().filter(|a| a.path().is_ident(name)) {
        let list = a.meta.require_list()?;
        out.push(
            Punctuated::<Item, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())?
                .into_iter()
                .collect(),
        );
    }
    Ok(out)
}

fn fields_of(input: &DeriveInput) -> Result<&syn::FieldsNamed, TokenStream> {
    match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(n) => Ok(n),
            _ => Err(err(input.ident.span(), "a struct with named fields")),
        },
        _ => Err(err(input.ident.span(), "a struct")),
    }
}

/// `Describe` for a struct (a record of its fields) or an enum (a sum of
/// its variants, each a record of its fields).
fn describe(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    let label = name.to_string();
    let record = |fields: &Fields| {
        let fs = fields.iter().enumerate().map(|(i, f)| {
            let n = f.ident.as_ref().map_or(i.to_string(), |n| n.to_string());
            let t = &f.ty;
            quote!((#n.to_string(), <#t as ::ark::row::Describe>::describe()))
        });
        quote!(vec![#(#fs),*])
    };
    let body = match &input.data {
        Data::Struct(s) => {
            let fs = record(&s.fields);
            quote!(::ark::row::Type::Record(#label.into(), #fs))
        }
        Data::Enum(e) => {
            let vs = e.variants.iter().map(|v| {
                let n = v.ident.to_string();
                let fs = record(&v.fields);
                quote!((#n.to_string(), #fs))
            });
            quote!(::ark::row::Type::Sum(#label.into(), vec![#(#vs),*]))
        }
        Data::Union(_) => return err(name.span(), "a union describes nothing"),
    };
    quote! {
        impl ::ark::row::Describe for #name {
            fn describe() -> ::ark::row::Type {
                #body
            }
        }
    }
}

/// `Effect` for a row: its `state: Job<T>` field, and what `#[effect(..)]`
/// says — the lease and the retry (a minute each by default), the role a
/// claim needs, the field naming its owner.
pub fn effect(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let fields = match fields_of(&input) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let state = fields.named.iter().find(|f| f.ident.as_ref().is_some_and(|i| i == "state"));
    let outcome = state.and_then(|f| match &f.ty {
        Type::Path(p) => match &p.path.segments.last()?.arguments {
            syn::PathArguments::AngleBracketed(a) if p.path.segments.last()?.ident == "Job" => a.args.first().cloned(),
            _ => None,
        },
        _ => None,
    });
    let Some(outcome) = outcome else {
        return err(name.span(), "a job is a row with a `state: Job<T>` field");
    };
    let keyed = |f: &&syn::Field| matches!(items(&f.attrs), Ok(i) if i.iter().flatten().any(|i| matches!(i, Item::Flag(k) if k == "key")));
    let keys: Vec<&syn::Field> = fields.named.iter().filter(keyed).collect();
    let [key] = keys[..] else {
        return err(name.span(), "a job is keyed by one column");
    };
    let (key, key_ty) = (key.ident.as_ref().unwrap(), &key.ty);
    let (mut lease, mut retry, mut role, mut owner) = (quote!(60_000), quote!(60_000), quote!(None), quote!());
    let its = match items_of(&input.attrs, "effect") {
        Ok(i) => i,
        Err(e) => return e.to_compile_error(),
    };
    for it in its.into_iter().flatten() {
        match it {
            Item::Is(n, e) if n == "lease_ms" => lease = e.to_token_stream(),
            Item::Is(n, e) if n == "retry_ms" => retry = e.to_token_stream(),
            Item::Is(n, e) if n == "role" => role = quote!(Some(#e)),
            Item::Is(n, Expr::Path(f)) if n == "owner" => owner = quote!(fn owner(&self) -> Option<&str> { Some(&self.#f) }),
            _ => return err(name.span(), "#[effect(..)]: lease_ms = .., retry_ms = .., role = \"..\", owner = field"),
        }
    }
    let table = items(&input.attrs)
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .find_map(|i| match i {
            Item::Is(n, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) if n == "table" => Some(s.value()),
            _ => None,
        })
        .unwrap_or_else(|| snake(&name.to_string()));
    let job = match &outcome {
        syn::GenericArgument::Type(t) => crate::ffi::job(&input, t, key_ty, &table),
        _ => err(name.span(), "a job's outcome is a type"),
    };
    quote! {
        #job

        impl ::ark::effect::Effect for #name {
            type Outcome = #outcome;
            const LEASE_MS: i64 = #lease;
            const RETRY_MS: i64 = #retry;
            const ROLE: Option<&'static str> = #role;
            fn job(&self) -> &::ark::effect::Job<#outcome> {
                &self.state
            }
            fn job_mut(&mut self) -> &mut ::ark::effect::Job<#outcome> {
                &mut self.state
            }
            #owner
            type Id = #key_ty;
            fn key_of(id: #key_ty) -> Self::Key {
                (id,)
            }
            fn id(&self) -> #key_ty {
                self.#key.clone()
            }
        }
    }
}

pub fn describe_derive(input: DeriveInput) -> TokenStream {
    let (describe, twin) = (describe(&input), crate::ffi::twin(&input));
    quote!(#describe #twin)
}

pub fn row(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let fields = match fields_of(&input) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let (mut table, mut volatile, mut indexes) = (snake(&name.to_string()), false, vec![]);
    let top = match items(&input.attrs) {
        Ok(i) => i,
        Err(e) => return e.to_compile_error(),
    };
    for it in top.into_iter().flatten() {
        match it {
            Item::Flag(f) if f == "volatile" => volatile = true,
            Item::Flag(f) if f == "ffi" || f == "ffi_view" => {}
            Item::Is(n, Expr::Lit(l)) if n == "table" => match &l.lit {
                syn::Lit::Str(s) => table = s.value(),
                _ => return err(n.span(), "table = \"name\""),
            },
            Item::Cols(k, cols, alias) if k == "index" || k == "unique" => indexes.push((k == "unique", cols, alias)),
            _ => return err(name.span(), "#[ark(..)] on a row: table = \"..\", volatile, index(..), unique(..)"),
        }
    }
    let names: Vec<&Ident> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let tys: Vec<&Type> = fields.named.iter().map(|f| &f.ty).collect();
    let (mut keys, mut refs) = (vec![], vec![]);
    for (i, f) in fields.named.iter().enumerate() {
        let its = match items(&f.attrs) {
            Ok(i) => i,
            Err(e) => return e.to_compile_error(),
        };
        for it in its.into_iter().flatten() {
            match it {
                Item::Flag(k) if k == "key" => keys.push(i),
                Item::Refs(t) => refs.push((i, t)),
                _ => return err(f.ident.as_ref().unwrap().span(), "#[ark(..)] on a column: key, refs(Row)"),
            }
        }
    }
    if keys.is_empty() || keys.iter().enumerate().any(|(n, k)| n != *k) {
        return err(name.span(), "a row's key is its leading fields, each #[ark(key)], at least one");
    }
    let nkey = keys.len();
    let (key_names, key_tys) = (&names[..nkey], &tys[..nkey]);
    let columns = fields.named.iter().map(|f| {
        let (n, t) = (f.ident.as_ref().unwrap().to_string(), &f.ty);
        let defaulted = f
            .attrs
            .iter()
            .any(|a| a.path().is_ident("serde") && a.to_token_stream().to_string().contains("default"));
        match defaulted {
            true => quote!(::ark::row::column_or_default::<#t>(#n)),
            false => quote!(::ark::row::column::<#t>(#n)),
        }
    });
    let (space, marker) = if volatile {
        (quote!(::ark::schema::Space::Volatile), quote!(::ark::db::Volatile))
    } else {
        (quote!(::ark::schema::Space::Durable), quote!(::ark::db::Durable))
    };
    let mut ix_defs = vec![];
    let mut ix_consts = vec![];
    for (n, (unique, cols, alias)) in indexes.iter().enumerate() {
        let mut at = vec![];
        let mut ctys = vec![];
        for c in cols {
            match names.iter().position(|x| *x == c) {
                Some(p) => {
                    at.push(p);
                    ctys.push(tys[p]);
                }
                None => return err(c.span(), "an index over a column of this row"),
            }
        }
        ix_defs.push(quote!(::ark::schema::Index { columns: vec![#(#at),*], unique: #unique }));
        let cname = alias.clone().unwrap_or_else(|| {
            let s: Vec<String> = cols.iter().map(|c| c.to_string().to_uppercase()).collect();
            format_ident!("{}", s.join("_"))
        });
        ix_consts.push(quote! {
            pub const #cname: ::ark::row::Index<#name, (#(#ctys,)*)> = ::ark::row::Index::nth(#n);
        });
    }
    let ref_defs = refs
        .iter()
        .map(|(i, t)| quote!(::ark::schema::Ref { column: #i, table: <#t as ::ark::row::Row>::TABLE.into() }));
    let (describe, twin) = (describe(&input), crate::ffi::twin(&input));
    quote! {
        #describe
        #twin

        impl ::ark::row::Row for #name {
            const TABLE: &'static str = #table;
            type Key = (#(#key_tys,)*);
            type Space = #marker;
            fn key(&self) -> Self::Key {
                (#(::std::clone::Clone::clone(&self.#key_names),)*)
            }
            fn table() -> ::ark::schema::Table {
                ::ark::schema::Table {
                    name: #table.into(),
                    columns: vec![#(#columns),*],
                    key: #nkey,
                    indexes: vec![#(#ix_defs),*],
                    refs: vec![#(#ref_defs),*],
                    space: #space,
                }
            }
        }

        /// A row is an input as it is, refined by nothing: what a table's
        /// CRUD takes.
        impl ::ark::row::Input for #name {
            fn check(self) -> ::std::result::Result<Self, ::std::vec::Vec<(::std::string::String, ::std::string::String)>> {
                Ok(self)
            }
        }

        impl #name {
            /// The rows by their key.
            pub const KEY: ::ark::row::Index<#name, (#(#key_tys,)*)> = ::ark::row::Index::key();
            #(#ix_consts)*
        }
    }
}

/// One field's refinements, attribute by attribute: each a list of rules
/// and the words for any of them failing.
fn rules(f: &syn::Field) -> Result<Vec<TokenStream>, TokenStream> {
    let name = f.ident.as_ref().unwrap();
    let label = name.to_string();
    let mut out = vec![];
    let its = items(&f.attrs).map_err(|e| e.to_compile_error())?;
    for group in its {
        let why = group.iter().find_map(|i| match i {
            Item::Is(n, Expr::Lit(l)) if n == "why" => Some(l.lit.clone()),
            _ => None,
        });
        let words = |default: TokenStream| match &why {
            Some(w) => quote!(::std::string::String::from(#w)),
            None => default,
        };
        for it in &group {
            let r = match it {
                Item::Flag(k) if k == "trim" => quote! {
                    self.#name = ::ark::row::Trim::trimmed(self.#name);
                },
                Item::Flag(k) if k == "non_empty" => {
                    let w = words(quote!(why));
                    quote! {
                        if ok {
                            if let Some(why) = ::ark::row::non_empty(&self.#name) {
                                fails.push((#label.into(), #w));
                                ok = false;
                            }
                        }
                    }
                }
                Item::Is(k, Expr::Lit(l)) if k == "len" => {
                    let s = match &l.lit {
                        syn::Lit::Str(s) => s.value(),
                        _ => return Err(err(k.span(), "len = \"lo..=hi\"")),
                    };
                    let (lo, hi) = match bounds(&s) {
                        Some(b) => b,
                        None => return Err(err(k.span(), "len = \"lo..=hi\", \"lo..hi\" or \"lo..\"")),
                    };
                    let w = words(quote!(why));
                    quote! {
                        if ok {
                            if let Some(why) = ::ark::row::within(&self.#name, #lo, #hi) {
                                fails.push((#label.into(), #w));
                                ok = false;
                            }
                        }
                    }
                }
                Item::Is(k, Expr::Lit(l)) if k == "range" => {
                    let r: Expr = match &l.lit {
                        syn::Lit::Str(s) => s.parse().map_err(|e| e.to_compile_error())?,
                        _ => return Err(err(k.span(), "range = \"lo..=hi\"")),
                    };
                    let w = words(quote!(::std::string::String::from("is not valid")));
                    quote! {
                        if ok && !(#r).contains(&self.#name) {
                            fails.push((#label.into(), #w));
                            ok = false;
                        }
                    }
                }
                Item::Is(k, Expr::Lit(l)) if k == "check" => {
                    let c: Expr = match &l.lit {
                        syn::Lit::Str(s) => s.parse().map_err(|e| e.to_compile_error())?,
                        _ => return Err(err(k.span(), "check = \"|x| …\"")),
                    };
                    let w = words(quote!(::std::string::String::from("is not valid")));
                    quote! {
                        if ok && !(#c)(&self.#name) {
                            fails.push((#label.into(), #w));
                            ok = false;
                        }
                    }
                }
                Item::Is(k, _) if k == "why" => continue,
                _ => {
                    return Err(err(
                        name.span(),
                        "#[ark(..)] on an input's field: trim, non_empty, len = \"..\", range = \"..\", check = \"..\", why = \"..\"",
                    ))
                }
            };
            out.push(r);
        }
    }
    Ok(out)
}

/// What a field's refinements are, as a form says them beside it.
fn said(f: &syn::Field) -> syn::Result<Vec<String>> {
    let mut out = vec![];
    for group in items(&f.attrs)? {
        let why = group.iter().find_map(|i| match i {
            Item::Is(n, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) if n == "why" => Some(s.value()),
            _ => None,
        });
        let quoted = why.as_ref().map_or(String::new(), |w| format!(" (\u{201c}{w}\u{201d})"));
        for it in &group {
            let lit = |e: &Expr| match e {
                Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => s.value(),
                _ => String::new(),
            };
            out.push(match it {
                Item::Flag(k) if k == "trim" => "trimmed".into(),
                Item::Flag(k) if k == "non_empty" => format!("non-empty{quoted}"),
                Item::Is(k, e) if k == "len" => {
                    let chars = |n: usize| if n == 1 { "char" } else { "chars" };
                    match bounds(&lit(e)) {
                        Some((lo, usize::MAX)) => format!("min {lo} {}{quoted}", chars(lo)),
                        Some((0, hi)) => format!("max {hi} {}{quoted}", chars(hi)),
                        Some((lo, hi)) => format!("{lo} to {hi} {}{quoted}", chars(hi)),
                        None => continue,
                    }
                }
                Item::Is(k, e) if k == "range" => why.clone().unwrap_or_else(|| format!("in {}", lit(e))),
                Item::Is(k, _) if k == "check" => why.clone().unwrap_or_else(|| "refined".into()),
                _ => continue,
            });
        }
    }
    Ok(out)
}

/// `"1..=80"`, `"0..120"`, `"1.."` as inclusive bounds.
fn bounds(s: &str) -> Option<(usize, usize)> {
    let (lo, hi) = s.split_once("..")?;
    let lo: usize = lo.trim().parse().ok()?;
    let hi = hi.trim();
    let hi = if hi.is_empty() {
        usize::MAX
    } else if let Some(h) = hi.strip_prefix('=') {
        h.trim().parse().ok()?
    } else {
        hi.parse::<usize>().ok()?.checked_sub(1)?
    };
    Some((lo, hi))
}

pub fn input(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let fields = match fields_of(&input) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let mut per = vec![];
    let mut checks = vec![];
    for f in &fields.named {
        let s = match said(f) {
            Ok(s) => s,
            Err(e) => return e.to_compile_error(),
        };
        if !s.is_empty() {
            let n = f.ident.as_ref().unwrap().to_string();
            checks.push(quote!((#n.to_string(), vec![#(#s.to_string()),*])));
        }
        match rules(f) {
            Ok(r) => per.push(quote! {
                #[allow(unused_mut, unused_assignments)]
                let mut ok = true;
                #(#r)*
            }),
            Err(e) => return e,
        }
    }
    let (describe, twin) = (describe(&input), crate::ffi::twin(&input));
    quote! {
        #describe
        #twin

        impl ::ark::row::Input for #name {
            fn checks() -> ::std::vec::Vec<(::std::string::String, ::std::vec::Vec<::std::string::String>)> {
                vec![#(#checks),*]
            }

            #[allow(unused_mut)]
            fn check(mut self) -> ::std::result::Result<Self, ::std::vec::Vec<(::std::string::String, ::std::string::String)>> {
                let mut fails: ::std::vec::Vec<(::std::string::String, ::std::string::String)> = vec![];
                #({ #per })*
                if fails.is_empty() {
                    Ok(self)
                } else {
                    Err(fails)
                }
            }
        }
    }
}

pub fn schema(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let fields = match fields_of(&input) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let db = format_ident!("{}Db", name);
    let names: Vec<&Ident> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let tys: Vec<&Type> = fields.named.iter().map(|f| &f.ty).collect();
    let vis = &input.vis;
    let doc = format!("The tables of [`{name}`], each a handle (§1.16).");
    quote! {
        #[doc = #doc]
        #vis struct #db<'t, P = ::ark::db::Durable> {
            #(pub #names: ::ark::db::Tab<'t, #tys, P>,)*
        }

        impl ::ark::db::Tables for #name {
            type Db<'t, P: ::ark::db::Writes> = #db<'t, P>;
            fn handles<'t, P: ::ark::db::Writes>(txn: &'t ::std::cell::RefCell<::ark::db::Txn<'t>>) -> #db<'t, P> {
                #db { #(#names: ::ark::db::Tab::new(txn),)* }
            }
            fn schema() -> ::ark::schema::Schema {
                ::ark::schema::Schema { tables: vec![#(<#tys as ::ark::row::Row>::table()),*] }
            }
        }
    }
}

/// `#[mutator("name")]` and the rest: the function as it is, and beside
/// it a module of its name whose `register` adds it to a domain.
pub fn register(kind: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    let name: LitStr = match syn::parse2(attr) {
        Ok(n) => n,
        Err(e) => return e.to_compile_error(),
    };
    let f: ItemFn = match syn::parse2(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    let (ident, vis) = (&f.sig.ident, &f.vis);
    let method = format_ident!("{}", kind);
    let add = if kind == "scope" {
        quote!(d.scope(#name, super::#ident()))
    } else {
        quote!(d.#method(#name, super::#ident))
    };
    let doc = format!("Registers `{}` as `{}`.", ident, name.value());
    quote! {
        #f

        #[doc(hidden)]
        #[allow(non_snake_case)]
        #vis mod #ident {
            #[doc = #doc]
            pub fn register(d: ::ark::domain::Domain) -> ::ark::domain::Domain {
                #add
            }
        }
    }
}
