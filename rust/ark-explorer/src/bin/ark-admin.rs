//! `ark-admin`: the admin page (`docs/plan-guards.md` D4) — the explorer over
//! a server's authority. Compiled to wasm it is the page a server serves on
//! its admin listener, reaching the listener it was loaded from; run
//! natively it is the same page in a window, reaching the listener named:
//!
//! ```text
//! ark-admin http://127.0.0.1:8788      # ARK_ADMIN_TOKEN=… when it is bound wider
//! ```

use ark_explorer::admin::{remembered, Page};

fn main() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    let base = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    let base = std::env::args().nth(1).unwrap_or_else(|| "http://127.0.0.1:8788".into());
    // The colours are arkui's neutral ones: the admin page is nobody's brand.
    ark_explorer::Theme {
        palette: arkui::theme::Palette::NEUTRAL,
    }
    .install();
    iced::application(move || Page::boot(base.clone(), remembered()), Page::update, Page::view)
        .subscription(Page::subscription)
        .title("admin")
        .window_size((960.0, 640.0))
        .run()
}
