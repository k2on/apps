//! Setting up where receipts come from: functions of the server's own that
//! a device calls over its link (ark-peer's `Config::calls`), never
//! intents. What they carry — Amazon's password — is the server's alone:
//! it is kept in the server's vault and never in a row, so it is in no
//! log, on no device, and in no backup of the budget. Each answers with
//! the stores as the server has them now ([`Stores`]), told who is signed
//! in, masked, and never a secret.
//!
//! - **Amazon** ([`AMAZON`], [`FORGET_AMAZON`]): the login `amazon-orders`
//!   signs in with at each sync.
//! - **Costco** ([`COSTCO_SIGN_IN`]): costco.com's sign-in opened in the
//!   server's browser, where a person signs in on its page; the server
//!   reads the receipts there at each sync, and keeps nothing of the
//!   sign-in itself.

use serde::{Deserialize, Serialize};

/// The stores as the server has them: `()`.
pub const STATUS: &str = "stores.status";
/// Amazon's login kept: an [`AmazonLogin`].
pub const AMAZON: &str = "stores.amazon";
/// Amazon's login forgotten: `()`.
pub const FORGET_AMAZON: &str = "stores.forget_amazon";
/// costco.com's sign-in opened in the server's browser: `()`, answered
/// with the browser's page to open ([`BrowserPage`]), not a [`Stores`].
pub const COSTCO_SIGN_IN: &str = "stores.costco_sign_in";

/// The Amazon account `amazon-orders` signs in as.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmazonLogin {
    pub username: String,
    pub password: String,
    /// The authenticator app's secret, for an account with two-step
    /// verification; blank for none.
    pub otp_secret: String,
}

impl std::fmt::Debug for AmazonLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "AmazonLogin({})", self.username)
    }
}

/// The server's browser's page, where a person sees it and signs in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPage {
    pub url: String,
}

/// The stores as the server has them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stores {
    pub amazon: AmazonState,
    pub costco: CostcoState,
    /// What each store's last import did, by store: `3 receipts`,
    /// `failed: …`.
    pub last: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmazonState {
    /// Whether the server has `amazon-orders` to sign in with.
    pub program: bool,
    /// Who it signs in as, masked; none before a login is given.
    pub who: Option<String>,
    /// Whether it has an authenticator secret.
    pub otp: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostcoState {
    /// Whether the server has a browser to read Costco in.
    pub browser: bool,
    /// The browser's page, where costco.com is signed in to.
    pub page: Option<String>,
}
