//! A page opened beside the window: Plaid's, to connect a bank.
//!
//! On the desktop it is the system's browser. In a browser it is a new
//! tab, and a browser opens one only soon after a click: so the tab is
//! opened empty when "Connect a bank" is pressed ([`reserve`]), before the
//! server has answered, and pointed at Plaid's page when the answer comes
//! ([`open`]). A tab the browser would not open leaves the page's own link
//! to Plaid, which opens one when it is clicked. Nothing is opened under
//! test.

#[cfg(target_arch = "wasm32")]
thread_local! {
    static RESERVED: std::cell::RefCell<Option<web_sys::Window>> = const { std::cell::RefCell::new(None) };
}

/// A tab to open Plaid's page in, opened now, while the click that asked
/// for it still counts.
pub fn reserve() {
    #[cfg(target_arch = "wasm32")]
    {
        let tab = web_sys::window().and_then(|w| w.open_with_url_and_target("", "_blank").ok().flatten());
        RESERVED.with(|r| *r.borrow_mut() = tab);
    }
}

/// `url` opened: in the tab reserved for it when there is one, else a new
/// tab or the system's browser. Whether it was.
pub fn open(url: &str) -> bool {
    #[cfg(test)]
    {
        let _ = url;
        true
    }
    #[cfg(all(not(test), target_arch = "wasm32"))]
    {
        let reserved = RESERVED.with(|r| r.borrow_mut().take()).filter(|t| !t.closed().unwrap_or(true));
        if let Some(tab) = reserved {
            if tab.location().set_href(url).is_ok() {
                return true;
            }
        }
        web_sys::window()
            .and_then(|w| w.open_with_url_and_target(url, "_blank").ok().flatten())
            .is_some()
    }
    #[cfg(all(not(test), not(target_arch = "wasm32")))]
    {
        ark_auth::client::open_browser(url);
        true
    }
}

/// The tab reserved for a page that is not coming, closed.
pub fn release() {
    #[cfg(target_arch = "wasm32")]
    if let Some(tab) = RESERVED.with(|r| r.borrow_mut().take()) {
        let _ = tab.close();
    }
}
