//! Setting up where receipts come from, from the app's Settings: the
//! functions of the server's own a device calls over its link
//! (`money_domain::setup`, ark-peer's `Config::calls`), outside the log.
//! What they are told is kept in the vault ([`crate::vault`]), never a row,
//! and what they answer is [`Stores`]: who, masked, and no secret.
//!
//! - **Amazon**: the login `amazon-orders` signs in with, or none.
//! - **Costco**: costco.com's sign-in opened in the server's browser
//!   ([`crate::costco`]), answered with the browser's page, where a person
//!   signs in; the server keeps nothing of it.
//!
//! A call comes in on a connection its token proved (the sync socket's
//! sign-in), so it is the household's; the importer's own account sets up
//! nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use ark::format::Encode;
use ark::peer::Who;
use money_domain::setup::{self as calls, AmazonState, BrowserPage, CostcoState, Stores};

use crate::costco::Costco;
use crate::importer::ImporterHandle;
use crate::vault::{masked, AmazonLogin, Vault};

/// What the calls share.
#[derive(Clone)]
pub struct Setup {
    pub data: PathBuf,
    pub amazon_program: bool,
    pub costco: Costco,
    /// Filled once the importer has started.
    pub importer: Arc<Mutex<Option<ImporterHandle>>>,
}

/// The calls, by name, for the hub's `Config::calls`.
pub fn calls(setup: Setup) -> BTreeMap<String, ark_peer::Call> {
    let mut out = BTreeMap::new();
    let mut add = |name: &str, f: fn(&Setup, &[u8]) -> Result<Vec<u8>>| {
        let s = setup.clone();
        let call: ark_peer::Call = Arc::new(move |who: &Who, input: &[u8]| {
            if who.actor == crate::importer::ACCOUNT {
                return Err("the importer sets up nothing".into());
            }
            f(&s, input).map_err(|e| format!("{e:#}"))
        });
        out.insert(name.to_string(), call);
    };
    add(calls::STATUS, |s, _| s.status());
    add(calls::AMAZON, Setup::set_amazon);
    add(calls::FORGET_AMAZON, |s, _| s.forget_amazon());
    add(calls::COSTCO_SIGN_IN, |s, _| s.sign_in_costco());
    out
}

fn read<T: Encode>(input: &[u8]) -> Result<T> {
    T::from_bytes(input).map_err(|e| anyhow!("a call this server does not read: {e}"))
}

impl Setup {
    fn nudge(&self) {
        if let Some(i) = self.importer.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            i.nudge();
        }
    }

    fn status(&self) -> Result<Vec<u8>> {
        let v = Vault::load(&self.data)?;
        let last = self.importer.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|i| i.last());
        let last = last
            .map(|r| r.stores.into_iter().map(|(k, o)| (k, outcome(&o))).collect())
            .unwrap_or_default();
        Ok(Stores {
            amazon: AmazonState {
                program: self.amazon_program,
                who: v.amazon.as_ref().map(|a| masked(&a.username)),
                otp: v.amazon.as_ref().is_some_and(|a| a.otp_secret.is_some()),
            },
            costco: CostcoState {
                browser: self.costco.browser.is_some(),
                page: self.costco.page.clone(),
            },
            last,
        }
        .to_bytes())
    }

    fn set_amazon(&self, input: &[u8]) -> Result<Vec<u8>> {
        let f: calls::AmazonLogin = read(input)?;
        let (username, password) = (f.username.trim().to_string(), f.password);
        if username.is_empty() || password.is_empty() {
            bail!("Amazon's email and password, both");
        }
        let otp: String = f.otp_secret.chars().filter(|c| !c.is_whitespace()).collect();
        // An authenticator's secret is base32 (A–Z, 2–7, maybe `=`);
        // amazon-orders fails the sync on anything else.
        let base32 = |c: char| c.is_ascii_alphabetic() || ('2'..='7').contains(&c) || c == '=';
        if !otp.is_empty() && (otp.len() < 16 || !otp.chars().all(base32)) {
            bail!(
                "That isn't an authenticator secret: it's the long key Amazon shows when you add an authenticator app \
                 (\"Can't scan the barcode?\"), letters and the digits 2\u{2013}7, not a 6-digit code"
            );
        }
        Vault::change(&self.data, |v| {
            v.amazon = Some(AmazonLogin {
                username,
                password,
                otp_secret: (!otp.is_empty()).then_some(otp),
            });
            Ok(())
        })?;
        self.nudge();
        self.status()
    }

    fn forget_amazon(&self) -> Result<Vec<u8>> {
        Vault::change(&self.data, |v| {
            v.amazon = None;
            Ok(())
        })?;
        // The session amazon-orders kept goes with the login.
        let _ = std::fs::remove_dir_all(self.data.join("amazon-orders"));
        self.status()
    }

    /// costco.com's sign-in opened in the browser; the browser's page, for
    /// the device to open.
    fn sign_in_costco(&self) -> Result<Vec<u8>> {
        let page = self
            .costco
            .page
            .clone()
            .ok_or_else(|| anyhow!("this server has no browser page for signing in to Costco (services.money.browser)"))?;
        self.costco.show_sign_in()?;
        // Signed in again, maybe as someone else: the next sync pulls from
        // the banks' first day again.
        crate::costco::forget_pulled(&self.data);
        Ok(BrowserPage { url: page }.to_bytes())
    }
}

fn outcome(o: &crate::importer::Outcome) -> String {
    use crate::importer::Outcome::*;
    match o {
        Receipts { receipts } => format!("{receipts} receipt{}", if *receipts == 1 { "" } else { "s" }),
        Charges { charges } => format!("{charges} charge{}", if *charges == 1 { "" } else { "s" }),
        Imported { upserts, removed } => format!("{upserts} changed, {removed} removed"),
        NotReady => "not ready".into(),
        Replaced => "replaced by another login".into(),
        Failed(why) => format!("failed: {why}"),
    }
}
