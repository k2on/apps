//! `#[ark::query("path")]`: the relational subset, `(Input) → List<T>` at
//! `Read` (§1.4); its body is lowered as any other ([`crate::lower`]).

use proc_macro2::TokenStream;

pub fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    crate::mutator::expand(crate::mutator::Kind::Query, args, item)
}
