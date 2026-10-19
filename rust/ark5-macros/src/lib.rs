//! ArkDB v5's macros (`docs/plan-v5.md` §1.4, §1.12). `#[derive(Node)]`
//! is [`derive`]'s; the authoring attributes are A1's.

use proc_macro::TokenStream;

mod derive;

/// `#[derive(Node)]`: the type of a Rust struct or enum as a §1.1 `Ty`, and
/// its encoder and decoder by the codec (`ark5::ir::Node`).
#[proc_macro_derive(Node)]
pub fn node(input: TokenStream) -> TokenStream {
    derive::node(syn::parse_macro_input!(input as syn::DeriveInput)).into()
}
