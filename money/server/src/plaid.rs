//! The slice of Plaid's API the importer asks, over ureq: an item's
//! accounts (`/accounts/get`), its institution's name
//! (`/institutions/get_by_id`), and what changed since a cursor
//! (`/transactions/sync`, every page). ArkOS's `cli/plaid.rs`, the half
//! `money-sync` used, answering in the domain's types
//! (`money_domain::plaid`), which read Plaid's JSON as it comes.
//!
//! **Nothing secret is ever said.** The client id and the secret travel as
//! headers and the access token in the body; none of them is in a URL, an
//! error or this client's `Debug`, so nothing the importer prints can carry
//! one.

use std::fmt;

use money_domain::plaid::{PlaidAccount, PlaidRemoved, PlaidTransaction, Pulled};
use serde::Deserialize;
use serde_json::{json, Value};

/// What Plaid said instead of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaidError {
    /// Plaid answered with an error object: its `error_code`
    /// (`ITEM_LOGIN_REQUIRED`), `error_type` and message.
    Api { code: String, kind: String, message: String },
    /// No answer, or one that could not be read.
    Transport(String),
}

impl PlaidError {
    pub fn code(&self) -> Option<&str> {
        match self {
            PlaidError::Api { code, .. } => Some(code),
            PlaidError::Transport(_) => None,
        }
    }
}

impl fmt::Display for PlaidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaidError::Api { code, kind, message } => write!(f, "plaid: {kind}/{code}: {message}"),
            PlaidError::Transport(why) => write!(f, "plaid: {why}"),
        }
    }
}

impl std::error::Error for PlaidError {}

pub type Res<T> = Result<T, PlaidError>;

/// An item's accounts, and the institution it is at when Plaid said.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub accounts: Vec<PlaidAccount>,
    pub institution_id: Option<String>,
}

/// One page of `/transactions/sync`.
#[derive(Deserialize)]
struct Page {
    added: Vec<PlaidTransaction>,
    modified: Vec<PlaidTransaction>,
    removed: Vec<PlaidRemoved>,
    next_cursor: String,
    has_more: bool,
}

/// Plaid, at `base` (`https://production.plaid.com`; a mock's address in a
/// test), as this server's client id and secret.
pub struct Plaid {
    base: String,
    client_id: String,
    secret: String,
}

impl fmt::Debug for Plaid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plaid").field("base", &self.base).finish_non_exhaustive()
    }
}

/// Where an environment's API is.
pub fn base_of(env: &str) -> String {
    format!("https://{env}.plaid.com")
}

/// How many times a run starts over from its cursor when Plaid says the
/// data moved while it was being paged through.
const RESTARTS: usize = 3;

impl Plaid {
    pub fn new(base: &str, client_id: String, secret: String) -> Plaid {
        Plaid {
            base: base.trim_end_matches('/').to_string(),
            client_id,
            secret,
        }
    }

    fn call<T: serde::de::DeserializeOwned>(&self, endpoint: &str, body: Value) -> Res<T> {
        // ureq's own error names the URL, which holds nothing secret.
        let transport = |e: &dyn fmt::Display| PlaidError::Transport(format!("{endpoint}: {e}"));
        let request = ureq::post(&format!("{}{endpoint}", self.base))
            .timeout(std::time::Duration::from_secs(60))
            .set("Content-Type", "application/json")
            .set("PLAID-CLIENT-ID", &self.client_id)
            .set("PLAID-SECRET", &self.secret)
            .set("Plaid-Version", "2020-09-14");
        match request.send_json(body) {
            Ok(r) => r.into_json().map_err(|e| transport(&e)),
            Err(ureq::Error::Status(status, r)) => {
                let v: Value = r.into_json().unwrap_or(Value::Null);
                let field = |k: &str| v[k].as_str().unwrap_or("?").to_string();
                Err(PlaidError::Api {
                    code: field("error_code"),
                    kind: field("error_type"),
                    message: format!("{endpoint} ({status}): {}", field("error_message")),
                })
            }
            Err(e) => Err(transport(&e)),
        }
    }

    /// An item's accounts, with their balances.
    pub fn accounts(&self, access_token: &str) -> Res<Item> {
        #[derive(Deserialize)]
        struct Reply {
            accounts: Vec<PlaidAccount>,
            #[serde(default)]
            item: Value,
        }
        let r: Reply = self.call("/accounts/get", json!({ "access_token": access_token }))?;
        Ok(Item {
            accounts: r.accounts,
            institution_id: r.item["institution_id"].as_str().map(str::to_string),
        })
    }

    /// An institution's name, as people know it ("Chase").
    pub fn institution(&self, id: &str) -> Res<String> {
        let r: Value = self.call("/institutions/get_by_id", json!({ "institution_id": id, "country_codes": ["US"] }))?;
        r["institution"]["name"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| PlaidError::Transport("/institutions/get_by_id: no name in the answer".into()))
    }

    /// Everything that changed since `cursor` (all of it the first time,
    /// `days` of history asked for), every page of it, and the cursor after
    /// the last; the cursor moves only once every page is read, as Plaid
    /// asks. None while Plaid has not finished an item's first pull: the
    /// next run picks it up.
    pub fn pull(&self, access_token: &str, cursor: Option<&str>, days: u32) -> Res<Option<Pulled>> {
        let start = cursor.filter(|c| !c.is_empty()).map(str::to_string);
        let mut restarts = 0;
        loop {
            let mut pulled = Pulled::default();
            let mut next = start.clone();
            let paged: Res<()> = (|| loop {
                let mut body = json!({
                    "access_token": access_token,
                    "count": 500,
                    "options": { "include_original_description": true },
                });
                match &next {
                    Some(c) => body["cursor"] = json!(c),
                    None => body["options"]["days_requested"] = json!(days),
                }
                let page: Page = self.call("/transactions/sync", body)?;
                pulled.added.extend(page.added);
                pulled.modified.extend(page.modified);
                pulled.removed.extend(page.removed);
                next = Some(page.next_cursor);
                if !page.has_more {
                    return Ok(());
                }
            })();
            match paged {
                Ok(()) => {}
                Err(e) if e.code() == Some("TRANSACTIONS_SYNC_MUTATION_DURING_PAGINATION") && restarts < RESTARTS => {
                    restarts += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
            return Ok(next.filter(|c| !c.is_empty()).map(|c| Pulled { cursor: c, ..pulled }));
        }
    }
}
