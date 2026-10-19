//! §1.12 What a type is, said by the derives, every field by name.
//!
//! Each writes `ark5::author::Shape` (the type, the declarations it names,
//! the Rust type its expressions are built as) and what the lowering reads
//! by name: a record's view and setters, a sum's variant indices, arities,
//! field positions, setters and arm witnesses. `#[ark(..)]` says the rest:
//!
//! - a refinement, in order: `trim`, `utf8`, `non_empty`, `len = 16`, `len
//!   = "1..=80"` (also `"1.."`, `"..=80"`); of an `Int`, `min = n`, `max =
//!   n`, `ne = n`, `range = "lo..=hi"` — each followed by `why = ".."` to
//!   give its words — on a record's field (a declaration `S.f`), or on a
//!   one-field tuple struct, which is then a refinement of that field's type;
//! - on a row: `table = "t"` (else the struct's name in snake case),
//!   `volatile`, `index(a, b)`, `unique(a, b)`, and on its fields `key`
//!   (the leading ones) and `refs = Row`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, Data, DeriveInput, Fields, Ident, Lit, Type};

use crate::lower::no;

/// One `#[ark(..)]` item.
enum Ark {
    Rule(TokenStream, Option<String>),
    Flag(String, Ident),
    Value(String, syn::Expr),
    Cols(bool, Vec<Ident>),
}

fn rule(r: TokenStream) -> Ark {
    Ark::Rule(quote!(::ark5::ir::Rule::#r), None)
}

fn ark(attrs: &[Attribute]) -> syn::Result<Vec<Ark>> {
    let mut out = vec![];
    for a in attrs.iter().filter(|a| a.path().is_ident("ark")) {
        a.parse_nested_meta(|m| {
            let id = m.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
            match id.as_str() {
                "trim" => out.push(rule(quote!(Trim))),
                "utf8" => out.push(rule(quote!(Utf8))),
                "non_empty" => out.push(rule(quote!(NonEmpty))),
                "len" => out.push(match m.value()?.parse::<Lit>()? {
                    Lit::Int(n) => rule(quote!(Len(#n))),
                    Lit::Str(s) => {
                        let v = s.value();
                        let (lo, hi) = v.split_once("..").ok_or_else(|| m.error("len is n or a range"))?;
                        let int = |t: &str, d: i64| {
                            if t.is_empty() {
                                Ok(d)
                            } else {
                                t.parse::<i64>().map_err(|_| m.error("a bound is an int"))
                            }
                        };
                        let (lo, hi) = (int(lo, 0)?, int(hi.trim_start_matches('='), i64::MAX)?);
                        rule(quote!(LenRange(#lo, #hi)))
                    }
                    l => return Err(syn::Error::new_spanned(l, "len is n or a range")),
                }),
                "range" => {
                    let v = m.value()?.parse::<syn::LitStr>()?.value();
                    let (lo, hi) = v.split_once("..=").ok_or_else(|| m.error("range is \"lo..=hi\""))?;
                    let int = |t: &str| t.trim().parse::<i64>().map_err(|_| m.error("a bound is an int"));
                    let (lo, hi) = (int(lo)?, int(hi)?);
                    out.push(Ark::Rule(quote!(::ark5::author::within(#lo, #hi)), None));
                }
                "min" | "max" | "ne" => {
                    let n: syn::Expr = m.value()?.parse()?;
                    let op = format_ident!(
                        "{}",
                        match id.as_str() {
                            "min" => "Ge",
                            "max" => "Le",
                            _ => "Ne",
                        }
                    );
                    out.push(Ark::Rule(quote!(::ark5::author::bound(::ark5::ir::Op::#op, #n)), None));
                }
                "why" => match out.last_mut() {
                    Some(Ark::Rule(_, why)) => *why = Some(m.value()?.parse::<syn::LitStr>()?.value()),
                    _ => return Err(m.error("why follows the rule it gives words to")),
                },
                "key" | "volatile" => out.push(Ark::Flag(id, m.path.get_ident().unwrap().clone())),
                "table" | "refs" => out.push(Ark::Value(id, m.value()?.parse()?)),
                "index" | "unique" => {
                    let mut cols = vec![];
                    m.parse_nested_meta(|c| {
                        cols.push(c.path.get_ident().cloned().ok_or_else(|| c.error("a column"))?);
                        Ok(())
                    })?;
                    out.push(Ark::Cols(id == "unique", cols));
                }
                _ => return Err(m.error(format!("`{id}` is no ark attribute"))),
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// The refinements among the items, as `(Rule, Option<&str>)`s.
fn rules(items: &[Ark]) -> Vec<TokenStream> {
    let quoted = |w: &Option<String>| match w {
        Some(w) => quote!(Some(#w)),
        None => quote!(None),
    };
    items
        .iter()
        .filter_map(|a| {
            if let Ark::Rule(r, w) = a {
                let w = quoted(w);
                Some(quote!((#r, #w)))
            } else {
                None
            }
        })
        .collect()
}

fn named(input: &DeriveInput) -> syn::Result<Vec<(Ident, Type, Vec<Ark>)>> {
    match &input.data {
        Data::Struct(s) if matches!(s.fields, Fields::Named(_)) => s
            .fields
            .iter()
            .map(|f| Ok((f.ident.clone().unwrap(), f.ty.clone(), ark(&f.attrs)?)))
            .collect(),
        _ => Err(no(&input.ident, "a row or record that is no struct with named fields")),
    }
}

fn shape(name: &Ident, base: TokenStream, ty: TokenStream, decls: TokenStream, label: String) -> TokenStream {
    quote! {
        impl ::ark5::author::Shape for #name {
            type Base = #base;
            fn ty() -> ::ark5::ty::Ty { #ty }
            fn decls(d: &mut ::ark5::author::Decls) { #decls }
            fn name() -> String { #label.into() }
        }
    }
}

/// A record's view and setters: what `x.f` and `S { f: .. }` lower to.
fn record(input: &DeriveInput, fields: &[(Ident, Type, Vec<Ark>)]) -> TokenStream {
    let (name, vis) = (&input.ident, &input.vis);
    let view = format_ident!("__ArkView_{}", name);
    let (ids, tys): (Vec<_>, Vec<_>) = fields.iter().map(|f| (&f.0, &f.1)).unzip();
    let idx: Vec<u32> = (0..fields.len() as u32).collect();
    let n = fields.len() as u32;
    let set = ids.iter().map(|f| format_ident!("__ark_set_{}", f));
    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #vis struct #view { #(pub #ids: ::ark5::author::Ex<<#tys as ::ark5::author::Shape>::Base>),* }
        impl ::ark5::author::Record for #name {
            type View = #view;
            fn view(e: ::ark5::ir::Expr) -> #view {
                #view { #(#ids: ::ark5::author::Ex::of(::ark5::ir::Expr::Field(Box::new(e.clone()), #idx))),* }
            }
        }
        #[allow(non_upper_case_globals, non_snake_case)]
        impl #name {
            #[doc(hidden)]
            pub const __ark_n: u32 = #n;
            #(#[doc(hidden)] pub fn #set(e: ::ark5::author::Ex<<#tys as ::ark5::author::Shape>::Base>) -> (u32, ::ark5::ir::Expr) {
                ::ark5::author::set(#idx, e)
            })*
        }
    }
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

pub fn row(input: DeriveInput) -> TokenStream {
    row_(&input).unwrap_or_else(|e| e.to_compile_error())
}

fn row_(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = named(input)?;
    let name = &input.ident;
    let (mut table, mut more) = (snake(&name.to_string()), vec![]);
    for a in ark(&input.attrs)? {
        match a {
            Ark::Value(k, syn::Expr::Lit(l)) if k == "table" => match &l.lit {
                Lit::Str(s) => table = s.value(),
                l => return Err(no(l, "a table name that is no string")),
            },
            Ark::Flag(k, _) if k == "volatile" => more.push(quote!(t = t.volatile();)),
            Ark::Cols(unique, cols) => {
                if let Some(c) = cols.iter().find(|c| !fields.iter().any(|f| f.0 == **c)) {
                    return Err(no(c, &format!("an index on `{c}`, which is no column,")));
                }
                let cols = cols.iter().map(|c| c.to_string());
                more.push(quote!(t = t.index(&[#(#cols),*], #unique);));
            }
            _ => return Err(no(name, "this row attribute")),
        }
    }
    let (mut key, mut cols) = (0, vec![]);
    for (i, (f, ty, items)) in fields.iter().enumerate() {
        let fname = f.to_string();
        for a in items {
            match a {
                Ark::Flag(k, at) if k == "key" => match key == i {
                    true => key += 1,
                    false => return Err(no(at, "a key column after a column that is not one")),
                },
                Ark::Value(k, r) if k == "refs" => more.push(quote!(t = t.refs(#fname, <#r as ::ark5::author::Row>::TABLE);)),
                _ => return Err(no(f, "this column attribute")),
            }
        }
        cols.push(quote! {
            let (decl, ty, _) = ::ark5::author::column_ty::<#ty>();
            t.columns.push(::ark5::schema::Column { name: #fname.into(), decl, ty, default: None });
        });
    }
    let key = key.max(1);
    let tys: Vec<_> = fields.iter().map(|f| &f.1).collect();
    let shape = shape(
        name,
        quote!(#name),
        quote!(::ark5::ty::Ty::Tuple(vec![#(::ark5::author::column_ty::<#tys>().2),*])),
        quote!(#(<#tys as ::ark5::author::Shape>::decls(d);)*),
        name.to_string(),
    );
    let record = record(input, &fields);
    Ok(quote! {
        #shape
        #record
        impl ::ark5::author::Row for #name {
            const TABLE: &'static str = #table;
            fn table() -> ::ark5::schema::Table {
                let mut t = ::ark5::schema::Table::new(#table).keyed(#key);
                #(#cols)*
                #(#more)*
                t
            }
        }
    })
}

pub fn input(input: DeriveInput) -> TokenStream {
    input_(&input).unwrap_or_else(|e| e.to_compile_error())
}

fn input_(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let label = name.to_string();
    let guard = quote!(if d.contains_key(#label) { return; });
    if let Data::Struct(s) = &input.data {
        if let Fields::Unnamed(u) = &s.fields {
            let [f] = &u.unnamed.iter().collect::<Vec<_>>()[..] else {
                return Err(no(name, "a tuple struct of other than one field"));
            };
            let (ty, rules) = (&f.ty, rules(&ark(&input.attrs)?));
            let decls = quote! {
                #guard
                ::ark5::author::decl(#label, <#ty as ::ark5::author::Shape>::ty(), vec![#(#rules),*], d);
                <#ty as ::ark5::author::Shape>::decls(d);
            };
            let named = quote!(::ark5::ty::Ty::Named(#label.into()));
            return Ok(shape(name, quote!(<#ty as ::ark5::author::Shape>::Base), named, decls, label));
        }
    }
    let fields = named(input)?;
    let (mut tys, mut decls) = (vec![], vec![]);
    for (f, ty, items) in &fields {
        let r = rules(items);
        if items.iter().any(|a| !matches!(a, Ark::Rule(..))) {
            return Err(no(f, "a field attribute other than a refinement"));
        }
        decls.push(quote!(<#ty as ::ark5::author::Shape>::decls(d);));
        match r.is_empty() {
            true => tys.push(quote!(<#ty as ::ark5::author::Shape>::ty())),
            false => {
                let n = format!("{label}.{f}");
                decls.push(quote!(::ark5::author::decl(#n, <#ty as ::ark5::author::Shape>::ty(), vec![#(#r),*], d);));
                tys.push(quote!(::ark5::ty::Ty::Named(#n.into())));
            }
        }
    }
    let decls = quote! {
        #guard
        ::ark5::author::decl(#label, ::ark5::ty::Ty::Tuple(vec![#(#tys),*]), vec![], d);
        #(#decls)*
    };
    let shape = shape(name, quote!(#name), quote!(::ark5::ty::Ty::Named(#label.into())), decls, label);
    let record = record(input, &fields);
    Ok(quote!(#shape #record))
}

pub fn sum(input: DeriveInput) -> TokenStream {
    sum_(&input).unwrap_or_else(|e| e.to_compile_error())
}

fn sum_(input: &DeriveInput) -> syn::Result<TokenStream> {
    let (name, vis) = (&input.ident, &input.vis);
    let Data::Enum(e) = &input.data else {
        return Err(no(name, "a sum that is no enum"));
    };
    let label = name.to_string();
    let (mut items, mut defs, mut vtys, mut arities, mut decls) = (vec![], vec![], vec![], vec![], vec![]);
    for (i, v) in e.variants.iter().enumerate() {
        let (vn, i) = (&v.ident, i as u8);
        let n = v.fields.len() as u32;
        let arm = format_ident!("__ArkArm_{}_{}", name, vn);
        let (ci, cn, carm) = (
            format_ident!("__ark_i_{vn}"),
            format_ident!("__ark_n_{vn}"),
            format_ident!("__ark_arm_{vn}"),
        );
        let mut ftys = vec![];
        let mut wits = vec![];
        for (k, f) in v.fields.iter().enumerate() {
            let (ty, k32) = (&f.ty, k as u32);
            let base = quote!(::ark5::author::Ex<<#ty as ::ark5::author::Shape>::Base>);
            let fname = f.ident.as_ref().map_or(k.to_string(), |x| x.to_string());
            let set = format_ident!("__ark_set_{vn}_{fname}");
            items.push(quote!(#[doc(hidden)] pub fn #set(e: #base) -> (u32, ::ark5::ir::Expr) { ::ark5::author::set(#k32, e) }));
            if let Some(id) = &f.ident {
                let p = format_ident!("__ark_p_{vn}_{id}");
                items.push(quote!(#[doc(hidden)] pub const #p: u32 = #k32;));
            }
            ftys.push(quote!(<#ty as ::ark5::author::Shape>::ty()));
            decls.push(quote!(<#ty as ::ark5::author::Shape>::decls(d);));
            wits.push((f.ident.clone(), base));
        }
        let make = wits.iter().map(|(id, _)| {
            let w = quote!(::ark5::author::Ex::of(::ark5::ir::Expr::Var(u32::MAX)));
            id.as_ref().map_or(w.clone(), |id| quote!(#id: #w))
        });
        let (def, build) = match &v.fields {
            Fields::Named(_) => {
                let f = wits.iter().map(|(id, b)| quote!(pub #id: #b));
                (quote!(#vis struct #arm { #(#f),* }), quote!(#arm { #(#make),* }))
            }
            _ => {
                let f = wits.iter().map(|(_, b)| quote!(pub #b));
                (quote!(#vis struct #arm(#(#f),*);), quote!(#arm(#(#make),*)))
            }
        };
        items.push(quote!(#[doc(hidden)] pub const #ci: u8 = #i; #[doc(hidden)] pub const #cn: u32 = #n;));
        items.push(quote!(#[doc(hidden)] pub fn #carm() -> #arm { #build }));
        arities.push(n);
        vtys.push(quote!(vec![#(#ftys),*]));
        defs.push(quote!(#[doc(hidden)] #[allow(non_camel_case_types)] #def));
    }
    let decls = quote! {
        if d.contains_key(#label) { return; }
        ::ark5::author::decl(#label, ::ark5::ty::Ty::Sum(vec![#(#vtys),*]), vec![], d);
        #(#decls)*
    };
    let shape = shape(name, quote!(#name), quote!(::ark5::ty::Ty::Named(#label.into())), decls, label);
    Ok(quote! {
        #shape
        #(#defs)*
        #[allow(non_upper_case_globals, non_snake_case)]
        impl #name {
            #(#items)*
            #[doc(hidden)]
            pub fn __ark_arities() -> Vec<u32> { vec![#(#arities),*] }
        }
    })
}

pub fn schema(input: DeriveInput) -> TokenStream {
    let fields = match named(&input) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    let name = &input.ident;
    let mut rows = vec![];
    for (f, ty, _) in &fields {
        let row = match ty {
            Type::Path(p) => match &p.path.segments.last().unwrap().arguments {
                syn::PathArguments::AngleBracketed(a) => a.args.first().cloned(),
                _ => None,
            },
            _ => None,
        };
        match row {
            Some(r) => rows.push(r),
            None => return no(f, "a table set's field that is no `Table<Row>`").to_compile_error(),
        }
    }
    let ids = fields.iter().map(|f| &f.0);
    let (vis, sum) = (&input.vis, format_ident!("{}Row", name));
    let variants = rows.iter().map(|r| match r {
        syn::GenericArgument::Type(Type::Path(p)) => p.path.segments.last().unwrap().ident.clone(),
        _ => format_ident!("Row"),
    });
    quote! {
        /// A row of any of the tables, as its table's variant, in schema
        /// order: what a scope is a function of (§1.8).
        #[derive(::ark5::author::Sum)]
        #vis enum #sum { #(#variants(#rows)),* }
        impl #name {
            #[doc(hidden)]
            pub fn __ark_open() -> #name { #name { #(#ids: ::std::default::Default::default()),* } }
            /// The schema: these tables, in this order.
            pub fn schema() -> ::ark5::schema::Schema {
                ::ark5::schema::Schema { tables: vec![#(<#rows as ::ark5::author::Row>::table()),*] }
            }
        }
    }
}
