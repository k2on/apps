//! The router's derive, kept apart from ArkDB's (ark-macros): an app's
//! `Route` enum as paths. Used through `ark_router::Routes`.

use proc_macro::TokenStream;

mod routes;

/// `#[derive(Routes)]`: an app's `Route` enum as paths — `#[route("/album/:key")]`
/// on each variant (more than one `#[route]`: the first is written, all are read),
/// `#[home]` on the one a path that names nothing lands on (`routes.rs`).
#[proc_macro_derive(Routes, attributes(route, home))]
pub fn routes(input: TokenStream) -> TokenStream {
    routes::routes(syn::parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
