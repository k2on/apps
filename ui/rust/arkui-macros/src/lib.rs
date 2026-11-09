//! ArkUI's macros. UI only: the database's derives and attributes are
//! ark-macros'.

use proc_macro::TokenStream;

mod view;

/// `#[view]`: a function of a screen's record that returns a `View`, made
/// generic over `ark_app::view::Backend` — its `for`, `if`, `if let Some`
/// and field reads over the record rewritten into the backend's calls — and
/// its entry points written beside it (`view.rs`). Used as `ark_app::view`
/// or `arkui::view`.
#[proc_macro_attribute]
pub fn view(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(proc_macro2::Span::call_site(), "#[view] takes nothing")
            .to_compile_error()
            .into();
    }
    let f = syn::parse_macro_input!(item as syn::ItemFn);
    view::expand(f).unwrap_or_else(syn::Error::into_compile_error).into()
}
