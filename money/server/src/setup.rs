//! Setting up where receipts come from, from the app's Settings: the
//! functions of the server's own a device calls over its link
//! (`money_domain::setup`, ark-peer's `Config::calls`), outside the log.
//! What they are told is kept in the vault ([`crate::vault`]), never a row,
//! and what they answer is [`Stores`]: who, masked, and no secret.
//!
//! - **Amazon**: the login `amazon-orders` signs in with, or none.
//! - **Costco**: the email and password the server signs in with itself,
//!   as Costco's page does, and keeps to sign in again; or, when Costco asks
//!   for more, a sign-in begun, which answers with Costco's page for the
//!   device to open, and finished with the address Costco sent the browser
//!   to; either traded for a refresh token ([`crate::costco`]); or none.
//!
//! A call comes in on a connection its token proved (the sync socket's
//! sign-in), so it is the household's; the importer's own account sets up
//! nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use ark::format::Encode;
use ark::peer::Who;
use money_domain::setup::{self as calls, AmazonState, CostcoAddress, CostcoPage, CostcoPassword, CostcoState, Stores};

use crate::costco::{Costco, SignIn};
use crate::importer::ImporterHandle;
use crate::vault::{masked, AmazonLogin, Vault};

/// How long a Costco sign-in may take from its page opening to its address
/// given back: Costco's code lasts minutes.
const SIGN_IN_TIME: Duration = Duration::from_secs(15 * 60);

/// What the calls share.
#[derive(Clone)]
pub struct Setup {
    pub data: PathBuf,
    pub amazon_program: bool,
    pub costco: Costco,
    /// Filled once the importer has started.
    pub importer: Arc<Mutex<Option<ImporterHandle>>>,
    /// The Costco sign-in under way, the latest begun.
    pub signing_in: Arc<Mutex<Option<(SignIn, Instant)>>>,
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
    add(calls::COSTCO, Setup::set_costco);
    add(calls::COSTCO_PASSWORD, Setup::sign_in_costco_with_password);
    add(calls::FORGET_COSTCO, |s, _| s.forget_costco());
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
                signed_in: v.costco.is_some(),
                who: v.costco.as_ref().filter(|c| !c.who.is_empty()).map(|c| masked(&c.who)),
                password: v.costco.as_ref().is_some_and(|c| c.password.is_some()),
                file: self.costco.token_file.is_some(),
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

    fn sign_in_costco(&self) -> Result<Vec<u8>> {
        let started = self.costco.sign_in()?;
        let url = started.url.clone();
        *self.signing_in.lock().unwrap_or_else(|e| e.into_inner()) = Some((started, Instant::now()));
        Ok(CostcoPage { url }.to_bytes())
    }

    fn set_costco(&self, input: &[u8]) -> Result<Vec<u8>> {
        let f: CostcoAddress = read(input)?;
        let started = match self.signing_in.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some((started, at)) if at.elapsed() < SIGN_IN_TIME => started,
            _ => bail!("no Costco sign-in under way: sign in to Costco again"),
        };
        let login = self.costco.finish(&started, &f.address)?;
        Vault::change(&self.data, |v| {
            v.costco = Some(login);
            Ok(())
        })?;
        // A code is good once.
        *self.signing_in.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.nudge();
        self.status()
    }

    fn sign_in_costco_with_password(&self, input: &[u8]) -> Result<Vec<u8>> {
        let f: CostcoPassword = read(input)?;
        let email = f.email.trim();
        if email.is_empty() || f.password.is_empty() {
            bail!("Costco's email and password, both");
        }
        let login = self.costco.sign_in_with_password(email, &f.password)?;
        Vault::change(&self.data, |v| {
            v.costco = Some(login);
            Ok(())
        })?;
        self.nudge();
        self.status()
    }

    fn forget_costco(&self) -> Result<Vec<u8>> {
        Vault::change(&self.data, |v| {
            v.costco = None;
            Ok(())
        })?;
        self.status()
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
