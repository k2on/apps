//! The documentation site, an Ark app (docs/site/app.toml).
//!
//! Its shell is ArkUI's (`ark_app::shell`): a sidebar of the pages by
//! section, each page's toolbar and the search field. Each page is a route
//! (`/`, `/all/`, `/<component>/`) and a `#[view]` ([`views`]) of
//! long-form writing (`ark_app::writing`) made from the catalogue
//! ([`components`], [`catalogue`], [`pages`]), the toolkit's own live
//! specimen of the component (`ark_app::shell::specimen`), its desktop
//! pictures drawn by arkui's iced renderer, and the phones' latest
//! captures. Its part of its host is [`app::Docs`]: it has no data, no
//! server and no first run.
//!
//! Like every app, it never names a toolkit: its export (ui/docs/app.md,
//! "Export"; `nix run .#docs-export-web`) writes each page as static HTML
//! that is whole without script, taken over by its module, and runs
//! [`export::prepare`] first (feature `native`), which draws the pictures.
pub mod app;
pub mod catalogue;
pub mod components;
pub mod pages;
pub mod routes;
pub mod views;

#[cfg(feature = "native")]
pub mod examples;
#[cfg(feature = "native")]
pub mod export;
#[cfg(feature = "native")]
pub mod render;

#[cfg(all(test, feature = "native"))]
mod tests;

// The docs' declaration (ui/docs/app.md, "The App's Declaration"): their
// part of their host, which runs no screen of its own.
ark_app::app! {
    /// The docs as their page runs them: their part of their host.
    pub type App = app::Docs {}
}
