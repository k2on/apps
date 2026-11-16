//! The router's macros, kept apart from ArkDB's (ark-macros): an app's
//! `Route` enum as paths, and its routes read from a folder. Used through
//! `ark_router`.

use proc_macro::TokenStream;

mod folder;
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

/// `routes! { pub enum Route in "src/app"; pub type App = host::Money; }`:
/// the routes, the screens and what a route does to them, read from the
/// folder (`folder.rs`; ark_router's docs say the conventions).
#[proc_macro]
pub fn folder(input: TokenStream) -> TokenStream {
    folder::routes(syn::parse_macro_input!(input as folder::Input))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(Route)]` on a route file's struct: what `routes!` reads it by.
/// Its attributes are read there: `#[shows(screen)]`, `#[tab = "…"]`, and
/// on its fields `#[hex]`, `#[slug]`, `#[query]` (`#[query = "key"]`). It
/// writes nothing itself.
#[proc_macro_derive(Route, attributes(shows, tab, hex, slug, query))]
pub fn route(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

/// `#[screen(view = view::budget)]` on a layout's runtime: the screen that
/// shows the routes in its folder, as `routes!` reads it (`name = "…"`,
/// `beside`, and its further views, `settings = view::settings`). It
/// leaves the item as it is.
#[proc_macro_attribute]
pub fn screen(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}
