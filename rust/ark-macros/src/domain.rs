//! §1.16 The derives and attributes a domain is written with: `Row`,
//! `Input`, `Describe` and `Schema` read a struct or an enum and write what
//! the kernel needs of it — its table, its key, its typed indexes, its
//! refinements, its type description, its tables' handles — and
//! `#[mutator(..)]` and the rest leave a function as it is, declared by a
//! name that `functions!` reads from the source and registers.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitStr, Token, Type};

pub(crate) fn err<T>(span: Span, msg: &str) -> syn::Result<T> {
    Err(syn::Error::new(span, msg))
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

fn fields_of(input: &DeriveInput) -> syn::Result<&syn::FieldsNamed> {
    match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(n) => Ok(n),
            _ => err(input.ident.span(), "a struct with named fields"),
        },
        _ => err(input.ident.span(), "a struct"),
    }
}

/// `Describe` for a struct (a record of its fields) or an enum (a sum of
/// its variants, each a record of its fields); and its twin, when it asks
/// for one (`crate::ffi`).
fn describe(input: &DeriveInput) -> syn::Result<TokenStream> {
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
    let twin = crate::ffi::twin(input)?;
    Ok(quote! {
        impl ::ark::row::Describe for #name {
            fn describe() -> ::ark::row::Type {
                #body
            }
        }

        #twin
    })
}

/// `Effect` for a row whose `state` field is a `Job<T>` — a job — and
/// what `#[effect(..)]` says: the lease and the retry (a minute each by
/// default), the role a claim needs, the field naming its owner. Nothing
/// for any other row.
fn effect(input: &DeriveInput, fields: &syn::FieldsNamed, keys: &[&Ident], key_tys: &[&Type]) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let state = fields.named.iter().find(|f| f.ident.as_ref().is_some_and(|i| i == "state"));
    let outcome = state.and_then(|f| match &f.ty {
        Type::Path(p) => match &p.path.segments.last()?.arguments {
            syn::PathArguments::AngleBracketed(a) if p.path.segments.last()?.ident == "Job" => a.args.first().cloned(),
            _ => None,
        },
        _ => None,
    });
    let Some(outcome) = outcome else {
        return match items_of(&input.attrs, "effect")?.is_empty() {
            true => Ok(quote!()),
            false => err(name.span(), "a job is a row with a `state: Job<T>` field"),
        };
    };
    let ([key], [key_ty]) = (keys, key_tys) else {
        return err(name.span(), "a job is keyed by one column");
    };
    let (mut lease, mut retry, mut role, mut owner) = (quote!(60_000), quote!(60_000), quote!(None), quote!());
    for it in items_of(&input.attrs, "effect")?.into_iter().flatten() {
        match it {
            Item::Is(n, e) if n == "lease_ms" => lease = e.to_token_stream(),
            Item::Is(n, e) if n == "retry_ms" => retry = e.to_token_stream(),
            Item::Is(n, e) if n == "role" => role = quote!(Some(#e)),
            Item::Is(n, Expr::Path(f)) if n == "owner" => owner = quote!(fn owner(&self) -> Option<&str> { Some(&self.#f) }),
            _ => return err(name.span(), "#[effect(..)]: lease_ms = .., retry_ms = .., role = \"..\", owner = field"),
        }
    }
    Ok(quote! {
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
    })
}

pub fn describe_derive(input: DeriveInput) -> syn::Result<TokenStream> {
    describe(&input)
}

pub fn row(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let fields = fields_of(&input)?;
    let (mut table, mut volatile, mut indexes, mut fed) = (snake(&name.to_string()), false, vec![], quote!());
    let ctx = quote!(::ark::peer::Ctx);
    for it in items(&input.attrs)?.into_iter().flatten() {
        match it {
            Item::Flag(f) if f == "volatile" => volatile = true,
            Item::Flag(f) if f == "unfed" => fed = quote!(fn fed(_: &#ctx, _: &[u8]) -> Result<bool, String> { Ok(false) }),
            Item::Is(n, Expr::Path(f)) if n == "owner" => {
                fed = quote! {
                    fn fed(ctx: &#ctx, row: &[u8]) -> Result<bool, String> {
                        Ok(<Self as ::ark::row::Value>::decode(row)?.#f == ctx.user())
                    }
                }
            }
            Item::Flag(f) if f == "ffi" || f == "ffi_view" => {}
            Item::Is(n, Expr::Lit(l)) if n == "table" => match &l.lit {
                syn::Lit::Str(s) => table = s.value(),
                _ => return err(n.span(), "table = \"name\""),
            },
            Item::Cols(k, cols, alias) if k == "index" || k == "unique" => indexes.push((k == "unique", cols, alias)),
            _ => {
                return err(
                    name.span(),
                    "#[ark(..)] on a row: table = \"..\", volatile, owner = field, unfed, index(..), unique(..), ffi, ffi_view",
                )
            }
        }
    }
    let names: Vec<&Ident> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let tys: Vec<&Type> = fields.named.iter().map(|f| &f.ty).collect();
    let (mut keys, mut refs) = (vec![], vec![]);
    for (i, f) in fields.named.iter().enumerate() {
        for it in items(&f.attrs)?.into_iter().flatten() {
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
    let describe = describe(&input)?;
    let effect = effect(&input, fields, key_names, key_tys)?;
    Ok(quote! {
        #describe
        #effect

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
            #fed
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
    })
}

/// One field's refinements, attribute by attribute: each rule's check —
/// the words for its failing the attribute's `why`, or its own — and how a
/// form says it beside the field.
fn rules(f: &syn::Field) -> syn::Result<Vec<TokenStream>> {
    let name = f.ident.as_ref().unwrap();
    let label = name.to_string();
    let mut out = vec![];
    let lit = |k: &Ident, e: &Expr, form: &str| match e {
        Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Ok(s.value()),
        _ => err(k.span(), &format!("{k} = \"{form}\"")),
    };
    for group in items(&f.attrs)? {
        let why = group.iter().find_map(|i| match i {
            Item::Is(n, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) if n == "why" => Some(s.value()),
            _ => None,
        });
        let words = |default: TokenStream| match &why {
            Some(w) => quote!(::std::string::String::from(#w)),
            None => default,
        };
        // A test's failing, said in its words — the attribute's `why`, or its
        // own — once per field: a field fails at most once.
        let w = words(quote!(why));
        let fail = |test: TokenStream| {
            quote! {
                if ok {
                    if let Some(why) = #test {
                        fails.push((#label.into(), #w));
                        ok = false;
                    }
                }
            }
        };
        let invalid = |t: TokenStream| quote!((!#t).then(|| ::std::string::String::from("is not valid")));
        for it in &group {
            let r = match it {
                Item::Flag(k) if k == "trim" => quote!(self.#name = ::ark::row::Trim::trimmed(self.#name);),
                Item::Flag(k) if k == "non_empty" => fail(quote!(::ark::row::non_empty(&self.#name))),
                Item::Is(k, e) if k == "len" => {
                    let Some((lo, hi)) = bounds(&lit(k, e, "lo..=hi")?) else {
                        return err(k.span(), "len = \"lo..=hi\", \"lo..hi\" or \"lo..\"");
                    };
                    fail(quote!(::ark::row::within(&self.#name, #lo, #hi)))
                }
                Item::Is(k, e) if k == "range" => {
                    let r: Expr = syn::parse_str(&lit(k, e, "lo..=hi")?)?;
                    fail(invalid(quote!((#r).contains(&self.#name))))
                }
                Item::Is(k, e) if k == "check" => {
                    let c: Expr = syn::parse_str(&lit(k, e, "|x| …")?)?;
                    fail(invalid(quote!((#c)(&self.#name))))
                }
                Item::Is(k, _) if k == "why" => continue,
                _ => {
                    return err(
                        name.span(),
                        "#[ark(..)] on an input's field: trim, non_empty, len = \"..\", range = \"..\", check = \"..\", why = \"..\"",
                    )
                }
            };
            out.push(r);
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

pub fn input(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let fields = fields_of(&input)?;
    let mut per = vec![];
    for f in &fields.named {
        let r = rules(f)?;
        per.push(quote! {
            #[allow(unused_mut, unused_assignments)]
            let mut ok = true;
            #(#r)*
        });
    }
    let describe = describe(&input)?;
    Ok(quote! {
        #describe

        impl ::ark::row::Input for #name {
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
    })
}

pub fn schema(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let fields = fields_of(&input)?;
    let db = format_ident!("{}Db", name);
    let names: Vec<&Ident> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let tys: Vec<&Type> = fields.named.iter().map(|f| &f.ty).collect();
    let vis = &input.vis;
    let doc = format!("The tables of [`{name}`], each a handle (§1.16).");
    Ok(quote! {
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
            fn fed(ctx: &::ark::peer::Ctx, table: &str, row: &[u8]) -> Result<bool, String> {
                #(if table == <#tys as ::ark::row::Row>::TABLE {
                    return <#tys as ::ark::row::Row>::fed(ctx, row);
                })*
                Ok(false)
            }
        }
    })
}

/// `#[mutator("name")]` and the rest: the function as it is. What declares
/// it is the attribute itself, which [`functions`] reads.
pub fn function(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    syn::parse2::<LitStr>(attr)?;
    Ok(item)
}

/// `functions!(domain)`: `domain` with every function of the file it is
/// written in, and of the modules that file declares, registered by the
/// name its `#[mutator(..)]`, `#[view(..)]` or `#[scope(..)]` gives it. A
/// module or a function under `#[cfg(..)]`, and a module at a `#[path]`, are
/// not read.
pub fn functions(domain: TokenStream) -> syn::Result<TokenStream> {
    let Some(file) = proc_macro::Span::call_site().local_file() else {
        return err(Span::call_site(), "functions! reads the file it is written in, which is not on disk");
    };
    let mut found = vec![];
    read(&file, &mut vec![], &mut found)?;
    let adds = found.iter().map(|(kind, name, path)| match kind.as_str() {
        "scope" => quote!(.scope(#name, self::#(#path)::*())),
        _ => {
            let method = format_ident!("{}", kind);
            quote!(.#method(#name, self::#(#path)::*))
        }
    });
    Ok(quote!((#domain) #(#adds)*))
}

/// A function found: its kind, its name, its path from the file
/// [`functions`] is written in.
type Found = (String, LitStr, Vec<Ident>);

fn read(file: &std::path::Path, at: &mut Vec<Ident>, found: &mut Vec<Found>) -> syn::Result<()> {
    let text = std::fs::read_to_string(file).or_else(|e| err(Span::call_site(), &format!("{}: {e}", file.display())))?;
    // A file declares its modules in the directory of its name; a crate's
    // root, or a `mod.rs`, beside it.
    let dir = match file.with_extension("") {
        d if d.is_dir() && !["lib", "main", "mod"].iter().any(|r| d.ends_with(r)) => d,
        _ => file.with_file_name(""),
    };
    scan(&syn::parse_file(&text)?.items, &dir, at, found)
}

fn scan(items: &[syn::Item], dir: &std::path::Path, at: &mut Vec<Ident>, found: &mut Vec<Found>) -> syn::Result<()> {
    let has = |attrs: &[syn::Attribute], name: &str| attrs.iter().any(|a| a.path().is_ident(name));
    for item in items {
        match item {
            syn::Item::Fn(f) if !has(&f.attrs, "cfg") => {
                for a in &f.attrs {
                    let kind = a.path().segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                    if ["mutator", "view", "scope"].contains(&kind.as_str()) {
                        let path = at.iter().chain([&f.sig.ident]).cloned().collect();
                        found.push((kind, a.parse_args()?, path));
                    }
                }
            }
            syn::Item::Mod(m) if !has(&m.attrs, "cfg") && !has(&m.attrs, "path") => {
                at.push(m.ident.clone());
                match &m.content {
                    Some((_, inner)) => scan(inner, &dir.join(m.ident.to_string()), at, found)?,
                    None => {
                        let one = dir.join(format!("{}.rs", m.ident));
                        let many = dir.join(m.ident.to_string()).join("mod.rs");
                        read(if one.exists() { &one } else { &many }, at, found)?;
                    }
                }
                at.pop();
            }
            _ => {}
        }
    }
    Ok(())
}
