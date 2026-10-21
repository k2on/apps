//! ArkDB's derives and attributes (`docs/plan-v5.md` §1.16): what a domain
//! written in plain Rust is read by ([`domain`]) — `Row`, `Input`,
//! `Describe`, `Schema`, and the attributes that register a function by
//! name. A value's encoding is serde's (`ark::json`).

use proc_macro::TokenStream;

mod domain;

/// `#[derive(Row)]`: a struct as a table's row — `#[ark(key)]` on its
/// leading fields, `#[ark(refs(Row))]` on a column naming another row,
/// `#[ark(index(a, b))]`, `#[ark(unique(a, b))]` (each `as NAME`
/// optionally), `#[ark(volatile)]`, `#[ark(table = "..")]` on the struct. A
/// column appended with `#[serde(default)]` takes its type's default in a
/// row written before it.
#[proc_macro_derive(Row, attributes(ark))]
pub fn row(input: TokenStream) -> TokenStream {
    domain::row(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// `#[derive(Input)]`: a struct a function takes, its fields refined by
/// `#[ark(trim, non_empty, len = "1..=80", range = "0..=100", check =
/// "|x| ..", why = "..")]`, each attribute's rules failing with its words.
#[proc_macro_derive(Input, attributes(ark))]
pub fn input(input: TokenStream) -> TokenStream {
    domain::input(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// `#[derive(Describe)]`: what a struct or an enum is, as `ark::row::Type`.
#[proc_macro_derive(Describe)]
pub fn describe(input: TokenStream) -> TokenStream {
    domain::describe_derive(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// `#[derive(Schema)]`: a struct of tables, each field a row type, as the
/// schema and its handles (`ark::db::Tables`).
#[proc_macro_derive(Schema)]
pub fn schema(input: TokenStream) -> TokenStream {
    domain::schema(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// `#[mutator("name")]`: registers `fn(&Ctx, &mut Db<S>, I) -> Result<(),
/// Refusal>` by name.
#[proc_macro_attribute]
pub fn mutator(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain::register("mutator", attr.into(), item.into()).into()
}

/// `#[live("name")]`: a mutator over volatile tables.
#[proc_macro_attribute]
pub fn live(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain::register("live", attr.into(), item.into()).into()
}

/// `#[query("name")]`: registers `fn(&Ctx, &Db<S>, I) -> Result<T, Refusal>`.
#[proc_macro_attribute]
pub fn query(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain::register("query", attr.into(), item.into()).into()
}

/// `#[view("name")]`: registers `fn(&Ctx, &Db<S>, I) -> Result<Plan<S, T>,
/// Refusal>`, a query kept up to date.
#[proc_macro_attribute]
pub fn view(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain::register("view", attr.into(), item.into()).into()
}

/// `#[scope("name")]`: registers `fn() -> Scope<S>`.
#[proc_macro_attribute]
pub fn scope(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain::register("scope", attr.into(), item.into()).into()
}
