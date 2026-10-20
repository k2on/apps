//! `#[ark::live("path")]`: a live transition (§1.9), a mutator over
//! volatile tables keyed by instance. That it touches no durable table is
//! judged when its module is assembled (`ark::author::assemble`).

use proc_macro2::TokenStream;

pub fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    crate::mutator::expand(crate::mutator::Kind::Live, args, item)
}
