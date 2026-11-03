//! Setting up where receipts come from: functions of the server's own that
//! a device calls over its link (ark-peer's `Config::calls`), never
//! intents. What they carry — Amazon's password, Costco's sign-in — is the
//! server's alone: it is kept in the server's vault and never in a row, so
//! it is in no log, on no device, and in no backup of the budget. Each
//! answers with the stores as the server has them now ([`Stores`]), told
//! who is signed in, masked, and never a secret.
//!
//! - **Amazon** ([`AMAZON`], [`FORGET_AMAZON`]): the login `amazon-orders`
//!   signs in with at each sync.
//! - **Costco** ([`COSTCO_PASSWORD`], [`FORGET_COSTCO`]): the email and
//!   password the server signs in with itself, as Costco's page does; or,
//!   when Costco asks for more than the password ([`COSTCO_SIGN_IN`],
//!   [`COSTCO`]), a sign-in on Costco's own page, begun by the server (its
//!   answer is the page to open) and finished with the address Costco sent
//!   the browser to, whose code the server trades for the sign-in it keeps.

use serde::{Deserialize, Serialize};

/// The stores as the server has them: `()`.
pub const STATUS: &str = "stores.status";
/// Amazon's login kept: an [`AmazonLogin`].
pub const AMAZON: &str = "stores.amazon";
/// Amazon's login forgotten: `()`.
pub const FORGET_AMAZON: &str = "stores.forget_amazon";
/// A Costco sign-in begun: `()`, answered with a [`CostcoPage`] and not a
/// [`Stores`].
pub const COSTCO_SIGN_IN: &str = "stores.costco_sign_in";
/// The Costco sign-in finished: a [`CostcoAddress`].
pub const COSTCO: &str = "stores.costco";
/// Costco signed in to by the server itself: a [`CostcoPassword`].
pub const COSTCO_PASSWORD: &str = "stores.costco_password";
/// Costco's sign-in forgotten: `()`.
pub const FORGET_COSTCO: &str = "stores.forget_costco";

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

/// Costco's email and password, for the server to sign in with as its
/// page does, and again should Costco stop taking its sign-in.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostcoPassword {
    pub email: String,
    pub password: String,
}

impl std::fmt::Debug for CostcoPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "CostcoPassword({})", self.email)
    }
}

/// Where Costco's sign-in sent the browser: `msauth://…?code=…`, or the
/// code alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostcoAddress {
    pub address: String,
}

/// Costco's sign-in page, for the device to open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostcoPage {
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
    /// Whether someone has signed in.
    pub signed_in: bool,
    /// Who, masked, when Costco said.
    pub who: Option<String>,
    /// Whether the server keeps the password, to sign in again by itself.
    pub password: bool,
    /// Whether the server was given a refresh token's file, which it signs
    /// in with in place of the sign-in kept.
    pub file: bool,
}
