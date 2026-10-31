//! The slice of Plaid's API the importer asks, over ureq: an item's
//! accounts (`/accounts/get`), its institution's name
//! (`/institutions/get_by_id`), and what changed since a cursor
//! (`/transactions/sync`, every page); and connecting a bank through a
//! Hosted Link page (`/link/token/create`, `/link/token/get`,
//! `/item/public_token/exchange`) and forgetting one (`/item/remove`).
//! ArkOS's `cli/plaid.rs`, answering in the domain's types
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
    pub item_id: Option<String>,
}

/// What a Hosted Link page is for.
pub enum LinkMode {
    /// A new bank, and this many days of its history.
    New { days: u32 },
    /// The item behind this access token, logged in to again.
    Update(String),
}

/// A Hosted Link page: its address for the person, and the link token the
/// importer asks after it by (held in memory only).
pub struct LinkSession {
    pub url: String,
    link_token: String,
}

impl fmt::Debug for LinkSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkSession").field("url", &self.url).finish_non_exhaustive()
    }
}

/// Where a Hosted Link page stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linking {
    /// Nobody has finished it yet.
    Open,
    /// Finished, with a public token to exchange for a new bank's login
    /// (none in update mode).
    Connected(Option<String>),
    /// Ended without connecting anything, and why when Plaid said.
    Exited(Option<String>),
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
            item_id: r.item["item_id"].as_str().map(str::to_string),
        })
    }

    /// A Hosted Link session: Plaid hosts the page, the importer asks after
    /// it ([`Plaid::link_get`]), so nothing here has to be reachable from
    /// the person's browser and there is no redirect to register. `days`
    /// of history for a new bank; an item's token for Plaid's update mode,
    /// where its login is given again.
    pub fn link_create(&self, mode: &LinkMode, lifetime_s: u64) -> Res<LinkSession> {
        let mut body = json!({
            "client_name": "money",
            "language": "en",
            "country_codes": ["US"],
            "user": { "client_user_id": "money-household" },
            "hosted_link": { "url_lifetime_seconds": lifetime_s },
        });
        match mode {
            LinkMode::New { days } => {
                body["products"] = json!(["transactions"]);
                body["transactions"] = json!({ "days_requested": days });
            }
            LinkMode::Update(access_token) => body["access_token"] = json!(access_token),
        }
        #[derive(Deserialize)]
        struct Reply {
            link_token: String,
            hosted_link_url: String,
        }
        let r: Reply = self.call("/link/token/create", body)?;
        Ok(LinkSession {
            url: r.hosted_link_url,
            link_token: r.link_token,
        })
    }

    /// Where a Hosted Link session stands.
    pub fn link_get(&self, session: &LinkSession) -> Res<Linking> {
        let r: Value = self.call("/link/token/get", json!({ "link_token": session.link_token }))?;
        let sessions = r["link_sessions"].as_array().cloned().unwrap_or_default();
        let Some(done) = sessions.iter().find(|s| !s["finished_at"].is_null()) else {
            return Ok(Linking::Open);
        };
        let token = done["results"]["item_add_results"][0]["public_token"]
            .as_str()
            .or_else(|| done["on_success"]["public_token"].as_str());
        if let Some(t) = token {
            return Ok(Linking::Connected(Some(t.to_string())));
        }
        let error = &done["on_exit"]["error"];
        match error["error_code"].as_str() {
            Some(code) => {
                let said = error["display_message"].as_str().or(error["error_message"].as_str());
                Ok(Linking::Exited(Some(format!("{code}: {}", said.unwrap_or("Plaid's page ended")))))
            }
            // Update mode ends with nothing to exchange, as a new bank's page
            // closed before a bank was connected does: the caller tells
            // them apart.
            None => Ok(Linking::Exited(None)),
        }
    }

    /// A public token from Link traded for the item's access token, and
    /// the item's id.
    pub fn exchange(&self, public_token: &str) -> Res<(String, String)> {
        #[derive(Deserialize)]
        struct Reply {
            access_token: String,
            #[serde(default)]
            item_id: String,
        }
        let r: Reply = self.call("/item/public_token/exchange", json!({ "public_token": public_token }))?;
        Ok((r.access_token, r.item_id))
    }

    /// Plaid told to forget an item: its token works no more.
    pub fn remove(&self, access_token: &str) -> Res<()> {
        let _: Value = self.call("/item/remove", json!({ "access_token": access_token }))?;
        Ok(())
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
