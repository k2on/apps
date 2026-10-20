//! ArkDB v5's macros (`docs/plan-v5.md` §1.4, §1.12). `#[derive(Node)]`
//! is [`derive`]'s. The authoring attributes read a function's restricted
//! Rust and write Rust that builds its `Expr` through `ark::author`
//! ([`mutator`] is the common entry, [`lower`] the one lowering); the
//! derives say what a row, a record and a sum are, by name ([`shape`]);
//! `module!` assembles a schema and functions into a module. What is
//! accepted is `spec/AUTHORING.md`'s table; anything else is a compile
//! error naming the construct.

use proc_macro::TokenStream;

mod derive;
mod live;
mod lower;
mod mutator;
mod query;
mod scope;
mod shape;

/// `#[derive(Node)]`: the type of a Rust struct or enum as a §1.1 `Ty`, and
/// its encoder and decoder by the codec (`ark::ir::Node`).
#[proc_macro_derive(Node)]
pub fn node(input: TokenStream) -> TokenStream {
    derive::node(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// `#[ark::mutator("path.name")]`: `(Input) → ()` at `Write`.
#[proc_macro_attribute]
pub fn mutator(args: TokenStream, item: TokenStream) -> TokenStream {
    mutator::expand(mutator::Kind::Mutator, args.into(), item.into()).into()
}

/// `#[ark::helper]`: a function others call, at the least effect its body has.
#[proc_macro_attribute]
pub fn helper(args: TokenStream, item: TokenStream) -> TokenStream {
    mutator::expand(mutator::Kind::Helper, args.into(), item.into()).into()
}

/// `#[ark::guard]`: a helper a mutator calls first, which refuses.
#[proc_macro_attribute]
pub fn guard(args: TokenStream, item: TokenStream) -> TokenStream {
    mutator::expand(mutator::Kind::Helper, args.into(), item.into()).into()
}

#[proc_macro_attribute]
pub fn query(args: TokenStream, item: TokenStream) -> TokenStream {
    query::expand(args.into(), item.into()).into()
}

#[proc_macro_attribute]
pub fn scope(args: TokenStream, item: TokenStream) -> TokenStream {
    scope::expand(args.into(), item.into()).into()
}

#[proc_macro_attribute]
pub fn live(args: TokenStream, item: TokenStream) -> TokenStream {
    live::expand(args.into(), item.into()).into()
}

/// `module!{ schema: S, functions: [f, g] }`: `ark::author::assemble`.
#[proc_macro]
pub fn module(input: TokenStream) -> TokenStream {
    mutator::module(input.into()).into()
}

/// A table's row: its columns by name, its key, indexes and references.
#[proc_macro_derive(Row, attributes(ark))]
pub fn row(input: TokenStream) -> TokenStream {
    shape::row(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// A record (an input, a result) and its fields' refinements, or a
/// refinement of one type (a newtype).
#[proc_macro_derive(Input, attributes(ark))]
pub fn input(input: TokenStream) -> TokenStream {
    shape::input(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// A sum: its variants, by name, each a record or a tuple.
#[proc_macro_derive(Sum, attributes(ark))]
pub fn sum(input: TokenStream) -> TokenStream {
    shape::sum(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}

/// The table set: a struct of `Table<Row>`s, in schema order, and the sum
/// of its rows, `<Name>Row`, which a scope takes.
#[proc_macro_derive(Schema)]
pub fn schema(input: TokenStream) -> TokenStream {
    shape::schema(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}
