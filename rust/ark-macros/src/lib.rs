//! ArkDB v5's derive (`docs/plan-v5.md` §1.4): `#[derive(Node)]`, a Rust
//! type as a value of §1.1, by the codec ([`derive`]) — what the kernel's
//! own types are, and what the bindings `ark::gen` writes derive.

use proc_macro::TokenStream;

mod derive;

/// `#[derive(Node)]`: the type of a Rust struct or enum as a §1.1 `Ty`, and
/// its encoder and decoder by the codec (`ark::ir::Node`).
#[proc_macro_derive(Node)]
pub fn node(input: TokenStream) -> TokenStream {
    derive::node(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}
