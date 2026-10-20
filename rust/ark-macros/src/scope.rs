//! `#[ark::scope("path")]`: partial sync's predicate (§1.8), `(Ctx,
//! Row<t>) → Bool` at `Pure`.

use proc_macro2::TokenStream;

pub fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    crate::mutator::expand(crate::mutator::Kind::Scope, args, item)
}
