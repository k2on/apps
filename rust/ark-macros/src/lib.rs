//! ArkDB v5's derives (`docs/plan-v5.md` §1.4, §1.15). `#[derive(Node)]`
//! is [`derive`]'s: a Rust type as a value of §1.1, by the codec. The
//! others say what a row, a record, a sum and a table set are, by name,
//! for the Rust builders in `ark::author` ([`shape`]).

use proc_macro::TokenStream;

mod derive;
mod shape;

/// `#[derive(Node)]`: the type of a Rust struct or enum as a §1.1 `Ty`, and
/// its encoder and decoder by the codec (`ark::ir::Node`).
#[proc_macro_derive(Node)]
pub fn node(input: TokenStream) -> TokenStream {
    derive::node(syn::parse_macro_input!(input as syn::DeriveInput)).into()
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

/// The table set: a struct of `Table<Row>`s, in schema order, the sum of
/// its rows, `<Name>Row`, which a scope takes, and `db()`, the tables a
/// builder reads.
#[proc_macro_derive(Schema)]
pub fn schema(input: TokenStream) -> TokenStream {
    shape::schema(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}
