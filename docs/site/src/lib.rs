//! The documentation site, an ArkUI app.
//!
//! Its shell is ArkUI's (`ark_app::shell`, drawn by arkui-web): a sidebar
//! of the pages by section, each page's toolbar and the search field.
//! Each page is a route (`/`, `/all/`, `/<component>/`) and a `#[view]`
//! ([`views`]) of long-form writing (`ark_app::writing`) made from the
//! catalogue ([`components`], [`catalogue`], [`pages`]), the component's
//! live arkui-web parts ([`live`]), its desktop pictures drawn by arkui's
//! iced renderer, and the phones' latest captures.
//!
//! It is written once as static HTML (`docs`, feature `native`: [`write`])
//! that is whole without script, and taken over by its wasm module
//! (`docs-web`, feature `hydrate`): ui/docs/app.md, "The Browser's Modes".
pub mod app;
pub mod catalogue;
pub mod components;
pub mod live;
pub mod pages;
pub mod routes;
pub mod views;

#[cfg(feature = "native")]
pub mod examples;
#[cfg(feature = "native")]
pub mod render;
#[cfg(feature = "native")]
pub mod write;

#[cfg(all(test, feature = "native"))]
mod tests;
