//! Where the importer's banks come from: the [`BankFeed`] port. The
//! importer (`crate::importer`) asks it for an item's accounts, what
//! changed since a cursor, and a Hosted Link page's progress; it never
//! names the service behind it. The adapter is Plaid's (`crate::plaid`,
//! over HTTP), and [`Memory`] is a feed in memory the importer's tests
//! script: banks by access token, each its accounts and its pages by
//! cursor.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use money_core::plaid::{PlaidAccount, Pulled};

/// What the feed said instead of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// It answered with an error: its code (`ITEM_LOGIN_REQUIRED`), its
    /// kind and its message.
    Api { code: String, kind: String, message: String },
    /// No answer, or one that could not be read.
    Transport(String),
}

impl FeedError {
    /// The code it gave, when it gave one.
    pub fn code(&self) -> Option<&str> {
        match self {
            FeedError::Api { code, .. } => Some(code),
            FeedError::Transport(_) => None,
        }
    }

    /// An error with a code, as the feed says one.
    pub fn api(code: &str, message: impl Into<String>) -> FeedError {
        FeedError::Api {
            code: code.into(),
            kind: "API_ERROR".into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedError::Api { code, kind, message } => write!(f, "plaid: {kind}/{code}: {message}"),
            FeedError::Transport(why) => write!(f, "plaid: {why}"),
        }
    }
}

impl std::error::Error for FeedError {}

pub type Res<T> = Result<T, FeedError>;

/// An item's accounts, and the institution it is at when the feed said.
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
    /// The item behind this access token, with the account picker:
    /// accounts it did not share before shared now.
    AddAccounts(String),
}

/// A Hosted Link page: its address for the person, and the token the
/// importer asks after it by (held in memory only, never printed).
pub struct LinkSession {
    pub url: String,
    token: String,
}

impl LinkSession {
    pub fn new(url: String, token: String) -> LinkSession {
        LinkSession { url, token }
    }

    /// The token the feed knows the page by.
    pub fn token(&self) -> &str {
        &self.token
    }
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
    /// Ended without connecting anything, and why when the feed said.
    Exited(Option<String>),
}

/// The banks' feed: what the importer asks of the service that has them.
pub trait BankFeed: Send + Sync {
    /// An item's accounts, with their balances.
    fn accounts(&self, access_token: &str) -> Res<Item>;
    /// A Hosted Link page opened for `mode`, lasting `lifetime_s`.
    fn link_create(&self, mode: &LinkMode, lifetime_s: u64) -> Res<LinkSession>;
    /// Where a page stands.
    fn link_get(&self, session: &LinkSession) -> Res<Linking>;
    /// A public token traded for the item's access token, and its id.
    fn exchange(&self, public_token: &str) -> Res<(String, String)>;
    /// The item forgotten: its token works no more.
    fn remove(&self, access_token: &str) -> Res<()>;
    /// An institution's name, as people know it.
    fn institution(&self, id: &str) -> Res<String>;
    /// Everything that changed since `cursor` (all of it the first time,
    /// `days` of history), and the cursor after; none while the item's
    /// first pull is not finished.
    fn pull(&self, access_token: &str, cursor: Option<&str>, days: u32) -> Res<Option<Pulled>>;
}

/// A feed handed to the importer in place of the one its keys make
/// (`Banks::feed`): one feed, compared by which it is.
#[derive(Clone)]
pub struct Feed(pub Arc<dyn BankFeed>);

impl fmt::Debug for Feed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Feed")
    }
}

impl PartialEq for Feed {
    fn eq(&self, other: &Feed) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Feed {}

/// One bank in a [`Memory`] feed: its accounts, and what a pull from each
/// cursor answers (`None` is from the start).
#[derive(Clone, Debug, Default)]
pub struct MemoryBank {
    pub item: Item,
    pub pages: BTreeMap<Option<String>, Pulled>,
}

/// A feed in memory: the importer's tests' fake. Banks by access token;
/// every pull's cursor is kept to look at, and an unknown token is turned
/// away as the real feed turns one away.
#[derive(Clone, Default)]
pub struct Memory {
    inner: Arc<Mutex<Held>>,
}

#[derive(Default)]
struct Held {
    banks: BTreeMap<String, MemoryBank>,
    cursors: Vec<Option<String>>,
    removed: Vec<String>,
}

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `bank` behind `access_token`, in place of any before.
    pub fn bank(&self, access_token: &str, bank: MemoryBank) {
        self.held().banks.insert(access_token.into(), bank);
    }

    /// Every pull's cursor, in order.
    pub fn cursors(&self) -> Vec<Option<String>> {
        self.held().cursors.clone()
    }

    /// The tokens forgotten.
    pub fn removed(&self) -> Vec<String> {
        self.held().removed.clone()
    }

    /// This feed, as the importer takes one.
    pub fn feed(&self) -> Feed {
        Feed(Arc::new(self.clone()))
    }

    fn with<T>(&self, token: &str, f: impl FnOnce(&MemoryBank) -> T) -> Res<T> {
        let held = self.held();
        match held.banks.get(token).filter(|_| !held.removed.iter().any(|r| r == token)) {
            Some(b) => Ok(f(b)),
            None => Err(FeedError::api("INVALID_ACCESS_TOKEN", "no such token")),
        }
    }
}

impl BankFeed for Memory {
    fn accounts(&self, access_token: &str) -> Res<Item> {
        self.with(access_token, |b| b.item.clone())
    }
    fn link_create(&self, _: &LinkMode, _: u64) -> Res<LinkSession> {
        Err(FeedError::api("NOT_FOUND", "the memory feed opens no page"))
    }
    fn link_get(&self, _: &LinkSession) -> Res<Linking> {
        Ok(Linking::Exited(None))
    }
    fn exchange(&self, _: &str) -> Res<(String, String)> {
        Err(FeedError::api("INVALID_PUBLIC_TOKEN", "the memory feed opens no page"))
    }
    fn remove(&self, access_token: &str) -> Res<()> {
        self.with(access_token, |_| ())?;
        self.held().removed.push(access_token.into());
        Ok(())
    }
    fn institution(&self, id: &str) -> Res<String> {
        Ok(id.into())
    }
    fn pull(&self, access_token: &str, cursor: Option<&str>, _: u32) -> Res<Option<Pulled>> {
        let cursor = cursor.filter(|c| !c.is_empty()).map(str::to_string);
        let got = self.with(access_token, |b| b.pages.get(&cursor).cloned())?;
        self.held().cursors.push(cursor.clone());
        match got {
            Some(p) => Ok(Some(p)),
            None => Err(FeedError::api("INVALID_FIELD", format!("unexpected cursor {cursor:?}"))),
        }
    }
}
