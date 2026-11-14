//! The slice of Plaid's API the importer asks, over ureq: an item's
//! accounts (`/accounts/get`), its institution's name
//! (`/institutions/get_by_id`), and what changed since a cursor
//! (`/transactions/sync`, every page); and connecting a bank through a
//! Hosted Link page (`/link/token/create`, `/link/token/get`,
//! `/item/public_token/exchange`) and forgetting one (`/item/remove`).
//! ArkOS's `cli/plaid.rs`, answering in the domain's types
//! (`money_core::plaid`), which read Plaid's JSON as it comes. It is the
//! importer's [`BankFeed`] adapter (`crate::feed`): the importer asks the
//! port, never this client by name.
//!
//! **Nothing secret is ever said.** The client id and the secret travel as
//! headers and the access token in the body; none of them is in a URL, an
//! error or this client's `Debug`, so nothing the importer prints can carry
//! one.

use std::fmt;

use money_core::plaid::{PlaidAccount, PlaidRemoved, PlaidTransaction, Pulled};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::feed::BankFeed;
pub use crate::feed::{FeedError as PlaidError, Item, LinkMode, LinkSession, Linking, Res};

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
}

impl BankFeed for Plaid {
    /// An item's accounts, with their balances.
    fn accounts(&self, access_token: &str) -> Res<Item> {
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
    /// where its login is given again, or its accounts picked again.
    fn link_create(&self, mode: &LinkMode, lifetime_s: u64) -> Res<LinkSession> {
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
            LinkMode::AddAccounts(access_token) => {
                body["access_token"] = json!(access_token);
                body["update"] = json!({ "account_selection_enabled": true });
            }
        }
        #[derive(Deserialize)]
        struct Reply {
            link_token: String,
            hosted_link_url: String,
        }
        let r: Reply = self.call("/link/token/create", body)?;
        Ok(LinkSession::new(r.hosted_link_url, r.link_token))
    }

    /// Where a Hosted Link session stands.
    fn link_get(&self, session: &LinkSession) -> Res<Linking> {
        let r: Value = self.call("/link/token/get", json!({ "link_token": session.token() }))?;
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
    fn exchange(&self, public_token: &str) -> Res<(String, String)> {
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
    fn remove(&self, access_token: &str) -> Res<()> {
        let _: Value = self.call("/item/remove", json!({ "access_token": access_token }))?;
        Ok(())
    }

    /// An institution's name, as people know it ("Chase").
    fn institution(&self, id: &str) -> Res<String> {
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
    fn pull(&self, access_token: &str, cursor: Option<&str>, days: u32) -> Res<Option<Pulled>> {
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
