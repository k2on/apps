//! A page opened beside the window: Plaid's, to connect a bank, in the
//! system's browser. Nothing is opened under test.

/// `url` opened in the system's browser. Whether it was.
pub fn open(url: &str) -> bool {
    #[cfg(test)]
    {
        let _ = url;
        true
    }
    #[cfg(not(test))]
    {
        ark_auth::client::open_browser(url);
        true
    }
}
