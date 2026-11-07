//! The bank importer: ArkOS's `money-sync`, inside the server, with the
//! household's budget as what it writes to — and the one place a bank is
//! connected, logged in to again, or disconnected.
//!
//! It is a worker of the server's (`Handle::<Authority>::worker`): a peer
//! in this process, signed in as the account `bank` and holding the
//! [`money_domain::BANK`] role, which that account is given and nobody else
//! unless told. Its work is jobs (`ark::effect`), each kind a handler of
//! its, run on a thread of its own:
//!
//! - **a sync** (a `syncing` row, `banks.sync`): every bank in the
//!   server's store ([`crate::store`], `banks.json` in the data directory)
//!   imported, then every store's receipts. For each bank a run asks Plaid
//!   for the item's accounts and for every page of what changed since the
//!   bank's cursor — the one its last import put on its `bank` row, in the
//!   log — makes them one import (`money_domain::plaid::import_of`) and
//!   authors it as `banks.import`, the cursor after it on the row. An
//!   import refused, or lost before the hub kept it, leaves the row's
//!   cursor where it was, so the next run asks from there; an import says
//!   the same thing twice without changing anything (Plaid's id is the
//!   `imported_id`). A bank with no row yet — a connection moved into
//!   `banks.json` by hand — asks from the cursor the store says. An
//!   account the import is the first to see waits for somebody in the
//!   household to say what it feeds (`money_domain::banks`); when somebody
//!   does, the bank's next import is asked from no cursor, its whole
//!   history again. What Plaid says when a bank fails is authored as
//!   `banks.failed`, for the household to see. A sync runs once at start,
//!   whenever it is nudged ([`Importer::nudge`]: `SIGUSR1`, which the NixOS
//!   module's timer sends twice a day), and whenever somebody in the
//!   household asks; asked again while it runs, it runs again after.
//!
//! - **an ask** about a connection (an `asking` row: `banks.connect`,
//!   `banks.reconnect`, `banks.add_accounts`, `banks.disconnect`), several
//!   at once. For a connection it opens a Plaid Hosted Link page and
//!   answers with its address; then it asks Plaid after the page every
//!   [`Banks::poll`] until somebody finishes it there, trades the public
//!   token for the bank's access token, keeps the bank in the store,
//!   answers, and imports it at once. A page nobody finished in
//!   [`Banks::lifetime_s`] is answered expired; a page open when the server
//!   stopped is answered expired at its next start, its link token having
//!   been held in memory only. A disconnection tells Plaid to forget the
//!   login (`/item/remove`) and drops it from the store.
//!   `banks.add_accounts` is the reconnection's page with Plaid's account
//!   picker (`update.account_selection_enabled`); what is picked there is
//!   opened at the import after. A second login to a bank already
//!   connected folds into the first at its first import
//!   (`money_domain::plaid`), and the login it replaced is removed at Plaid
//!   and dropped from the store as a disconnection's is, then said retired
//!   (`banks.retired`).
//!
//! - **a receipt's photo** read ([`crate::photos`]).
//!
//! **Secrets are never said.** The client id and the secret are read from
//! their files whenever they are needed, the tokens from the store; all of
//! them go to Plaid alone. Every line the importer says is in
//! [`Importer::said`] as well as in the server's events, which is how the
//! tests hold it to that.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use ark::format::Encode;
use ark::row::Id;
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::{Authority, Handle, Memory, Peer, Work};
use money_domain::plaid::import_of;
use money_domain::{Answer, AskState, Asking, Bank, BankAsk, BankFailed, BankName, Import, ReceiptImport, Syncing};

use crate::plaid::{LinkMode, Linking, Plaid, PlaidError};
use crate::store::{Connection, Store};
use crate::stores::{put_away, waiting, Stores};

/// The account the importer signs in as: what every row it writes says
/// wrote it.
pub const ACCOUNT: &str = "bank";

/// What the importer's sign-in says it is.
pub const DEVICE: &str = "the bank importer";

/// Everything the importer is told. Plaid's keys are files, read whenever
/// they are needed; the banks are the store's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banks {
    /// Where Plaid is: `https://production.plaid.com`, or the sandbox's, or
    /// a mock's.
    pub url: String,
    pub client_id_file: PathBuf,
    pub secret_file: PathBuf,
    /// Days of history asked for the first time a bank is pulled.
    pub history: u32,
    /// How often an open Hosted Link page is asked after.
    pub poll: Duration,
    /// How long a Hosted Link page lasts.
    pub lifetime_s: u64,
}

impl Banks {
    /// Plaid at `url`, as the keys in these files, every other setting its
    /// default.
    pub fn new(url: &str, client_id_file: PathBuf, secret_file: PathBuf) -> Banks {
        Banks {
            url: url.into(),
            client_id_file,
            secret_file,
            history: crate::HISTORY,
            poll: Duration::from_secs(5),
            lifetime_s: 30 * 60,
        }
    }
}

/// A secret's file, trimmed; empty is an error. Its contents are never
/// part of an error.
fn secret(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow!("cannot read {}: {e}", path.display()))?;
    match text.trim() {
        "" => bail!("{} is empty", path.display()),
        s => Ok(s.to_string()),
    }
}

/// What a run did, bank by bank: what each one's run ended in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub banks: BTreeMap<String, Outcome>,
    /// A store's run: `amazon` and `amazon charges` for `amazon-orders`,
    /// an export's file name for one from the folder.
    pub stores: BTreeMap<String, Outcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Imported: authored, its cursor on its row.
    Imported {
        upserts: usize,
        removed: usize,
    },
    /// Plaid has not finished the bank's first pull.
    NotReady,
    /// Another login to the same bank replaced it (`money_domain::plaid`):
    /// not imported, and removed at Plaid as soon as Plaid answers.
    Replaced,
    /// A store's receipts, imported.
    Receipts {
        receipts: usize,
    },
    /// A store's charges, imported.
    Charges {
        charges: usize,
    },
    Failed(String),
}

/// The importer, running: its worker, and what its handlers share.
pub struct Importer {
    worker: Handle<Peer>,
    hub: Handle<Authority>,
    job: Arc<Job>,
}

impl Importer {
    /// The importer for `banks` and `stores`, a worker of `hub` signed in by
    /// `auth`; its store in `data`. It runs once as soon as it is linked.
    pub fn start(banks: Option<Banks>, stores: Stores, hub: &Handle<Authority>, auth: &Arc<Auth>, data: PathBuf) -> Result<Importer> {
        let account = Account {
            id: ACCOUNT.into(),
            name: "Bank".into(),
            roles: vec![money_domain::BANK.into()],
            ..Account::default()
        };
        // In memory: what it holds is the hub's, filled at every start.
        let worker = hub
            .worker(auth, &account, DEVICE, Box::new(Memory::default()))
            .map_err(|e| anyhow!("signing the importer in: {e}"))?;
        let job = Arc::new(Job {
            store: Mutex::new(Store::load(&data)?),
            banks,
            stores,
            data,
            said: Mutex::default(),
            runs: Mutex::default(),
            swept: AtomicBool::new(false),
            started: ark_peer::now_ms(),
            nudged: AtomicBool::new(false),
        });
        let names: Vec<String> = job.store().banks.keys().cloned().collect();
        job.say(match names.len() {
            _ if job.banks.is_none() => "no Plaid keys: importing the stores' receipts alone".into(),
            0 => "no banks connected yet: connect one from the app".into(),
            _ => format!("connected to {}", names.join(", ")),
        });
        {
            let mut p = worker.lock();
            let j = job.clone();
            p.handle::<Syncing>(move |w, _| j.sync(w).map_err(|e| format!("{e:#}")));
            if job.banks.is_some() {
                let j = job.clone();
                p.handle::<Asking>(move |w, a| j.ask(w, a.id).map(|_| 0));
            }
            // Receipt photos read here, from the server's own files.
            if let Some(program) = job.stores.ocr.clone() {
                crate::photos::handle(&mut p, program, job.data.clone());
            }
        }
        let importer = Importer {
            worker,
            hub: hub.clone(),
            job,
        };
        importer.nudge();
        Ok(importer)
    }

    /// Run now (or as soon as the current run ends).
    pub fn nudge(&self) {
        self.handle().nudge();
    }

    /// A handle Settings' calls ask a run with, and read the last of.
    pub fn handle(&self) -> ImporterHandle {
        ImporterHandle {
            hub: self.hub.clone(),
            job: self.job.clone(),
        }
    }

    /// Every line the importer has said.
    pub fn said(&self) -> Vec<String> {
        held(&self.job.said).clone()
    }

    /// Every run it has finished, oldest first: a run of every bank, or of
    /// one just connected.
    pub fn runs(&self) -> Vec<Run> {
        held(&self.job.runs).clone()
    }

    /// Whether every entry it authored has been answered by the hub.
    pub fn settled(&self) -> bool {
        self.worker.lock().pending_len() == 0
    }
}

fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What of the importer another thread may hold: its nudge, and its runs.
#[derive(Clone)]
pub struct ImporterHandle {
    hub: Handle<Authority>,
    job: Arc<Job>,
}

impl ImporterHandle {
    /// A sync asked, by the server itself: run now, or again once the one
    /// running ends.
    pub fn nudge(&self) {
        self.job.nudged.store(true, Ordering::SeqCst);
        if let Some(Err(why)) = self.hub.mutate("syncing.ask".into(), ().to_bytes()) {
            tracing::warn!("a sync was refused: {why}");
        }
    }

    /// The last run it finished.
    pub fn last(&self) -> Option<Run> {
        held(&self.job.runs).last().cloned()
    }
}

/// What the importer's handlers share.
struct Job {
    /// None: no Plaid keys, and only the stores imported.
    banks: Option<Banks>,
    stores: Stores,
    data: PathBuf,
    store: Mutex<Store>,
    said: Mutex<Vec<String>>,
    runs: Mutex<Vec<Run>>,
    /// Whether the asks open before this start have been looked at, and
    /// when it started; whether the server asked for a sync since the
    /// last began.
    swept: AtomicBool,
    started: i64,
    nudged: AtomicBool,
}

/// What failing at a bank says: Plaid's code, when it gave one, and the
/// sentence. Neither holds a secret.
fn failure(e: &anyhow::Error) -> (String, String) {
    match e.downcast_ref::<PlaidError>() {
        Some(PlaidError::Api { code, message, .. }) => (code.clone(), message.clone()),
        _ => (String::new(), format!("{e:#}")),
    }
}

/// A read of what the worker holds.
fn read<T>(w: &mut Work, f: impl FnOnce(&ark::db::Db<money_domain::Money>) -> T) -> Option<T> {
    w.with(|p| ark::db::read::<money_domain::Money, _>(&p.log.store, f)).ok()
}

/// A mutation authored.
fn author(w: &mut Work, name: &str, input: &impl Encode) -> Result<()> {
    w.mutate(name, input).map(|_| ()).map_err(|why| anyhow!("refused: {why}"))
}

impl Job {
    fn say(&self, line: String) {
        tracing::info!("{line}");
        held(&self.said).push(line);
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        held(&self.store)
    }

    /// The store changed by `f`, and kept.
    fn change<T>(&self, f: impl FnOnce(&mut Store) -> T) -> Result<T> {
        let mut store = self.store();
        let t = f(&mut store);
        store.save(&self.data)?;
        Ok(t)
    }

    /// The day of the first transaction any bank brought; none before one
    /// has.
    fn first_bank_day(&self, w: &mut Work) -> Option<String> {
        read(w, |db| {
            let txns: Vec<money_domain::Txn> = db.txn.scan();
            txns.into_iter().filter(|t| !t.imported_by.is_empty()).map(|t| t.date).min()
        })
        .flatten()
    }

    /// The banks' configuration: only asked on a server with Plaid's keys.
    fn banks(&self) -> &Banks {
        self.banks.as_ref().expect("banks are imported only with Plaid's keys")
    }

    fn plaid(&self) -> Result<Plaid> {
        let id = secret(&self.banks().client_id_file)?;
        let s = secret(&self.banks().secret_file)?;
        Ok(Plaid::new(&self.banks().url, id, s))
    }

    fn record(&self, run: Run) {
        held(&self.runs).push(run);
    }

    /// A sync: every bank, then every store; run again after when it was
    /// asked for again meanwhile.
    fn sync(&self, w: &mut Work) -> Result<i64> {
        let began = ark_peer::now_ms();
        self.nudged.store(false, Ordering::SeqCst);
        if self.banks.is_some() && !self.swept.swap(true, Ordering::SeqCst) {
            self.sweep(w);
        }
        let mut run = match self.banks.is_some() {
            true => self.import_all(w),
            false => Run::default(),
        };
        run.stores = self.import_stores(w);
        self.record(run);
        let asked = read(w, money_domain::plaid::last_asked).flatten().map_or(0, |a| a.asked_ms);
        if asked > began || self.nudged.load(Ordering::SeqCst) {
            w.again(ark_peer::now_ms());
        }
        Ok(0)
    }

    // * Asks

    /// The asks open from before this start: a page open then was opened
    /// by a process that is gone, with its link token, and one asked
    /// before asks were jobs is nobody's: each answered expired.
    fn sweep(&self, w: &mut Work) {
        let open = read(w, |db| {
            let open = money_domain::banks::open_asks(db).into_iter();
            let before = |a: &BankAsk| a.state == AskState::Waiting && a.answered_ms < self.started;
            open.filter(|a| before(a) || db.asking.get(a.id).is_none()).collect::<Vec<BankAsk>>()
        });
        for a in open.unwrap_or_default() {
            let a: BankAsk = a;
            self.answer(w, a.id, AskState::Expired, "the server restarted while Plaid's page was open: ask again");
        }
    }

    /// An ask answered: a page opened for a connection and asked after
    /// until it is finished, a disconnection done. One answered already —
    /// expired at a restart — is nothing to do.
    fn ask(&self, w: &mut Work, id: Id<BankAsk>) -> Result<(), String> {
        let Some(a) = read(w, |db| db.bank_ask.get(id)).flatten() else {
            return Ok(());
        };
        if a.state != AskState::Asked {
            return Ok(());
        }
        let token = self.store().banks.get(&a.bank).map(|c| c.access_token.clone());
        let answered = match (a.what.as_str(), token) {
            ("connect", _) => self.page(w, &a, LinkMode::New { days: self.banks().history }),
            ("reconnect", Some(token)) => self.page(w, &a, LinkMode::Update(token)),
            // Plaid's account picker, for accounts not shared before.
            ("add_accounts", Some(token)) => self.page(w, &a, LinkMode::AddAccounts(token)),
            ("reconnect" | "add_accounts", None) => Err(anyhow!("this server holds no login for {}", a.bank)),
            ("disconnect", _) => self.disconnect(w, &a),
            (what, _) => Err(anyhow!("nothing here does {what:?}")),
        };
        if let Err(e) = answered {
            let (code, why) = failure(&e);
            self.say(format!("{} {}: failed: {why}", a.what, a.bank));
            let said = match code.is_empty() {
                true => why,
                false => format!("Plaid said {code}: {why}"),
            };
            self.answer(w, a.id, AskState::Failed, &said);
        }
        Ok(())
    }

    /// `banks.answer` authored with a sentence, said when it is refused.
    fn answer(&self, w: &mut Work, id: Id<BankAsk>, state: AskState, message: &str) {
        let message = message.into();
        self.answer_with(
            w,
            Answer {
                id,
                state,
                message,
                ..Answer::default()
            },
        );
    }

    fn answer_with(&self, w: &mut Work, answer: Answer) {
        if let Err(e) = author(w, "banks.answer", &answer) {
            self.say(format!("could not answer an ask: {e:#}"));
        }
    }

    /// A Hosted Link page — for a new bank, a bank's login, or its
    /// accounts — opened, answered with, and asked after until somebody
    /// finishes it, closes it, or it expires.
    fn page(&self, w: &mut Work, a: &BankAsk, mode: LinkMode) -> Result<()> {
        let (plaid, lifetime_s) = (self.plaid()?, self.banks().lifetime_s);
        let session = plaid.link_create(&mode, lifetime_s)?;
        let lifetime = Duration::from_secs(lifetime_s);
        self.say(match a.bank.as_str() {
            "" => format!("{}: Plaid's page is open", a.what),
            bank => format!("{} {bank}: Plaid's page is open", a.what),
        });
        let (url, expires_ms) = (session.url.clone(), ark_peer::now_ms() + lifetime.as_millis() as i64);
        self.answer_with(
            w,
            Answer {
                id: a.id,
                state: AskState::Waiting,
                url,
                expires_ms,
                ..Answer::default()
            },
        );
        let until = Instant::now() + lifetime;
        let linking = loop {
            std::thread::sleep(self.banks().poll);
            match plaid.link_get(&session) {
                Ok(Linking::Open) if Instant::now() < until => {}
                Ok(l) => break l,
                // Plaid out of reach for a moment is not the page's end.
                Err(PlaidError::Transport(why)) => self.say(format!("asking after Plaid's page: {why}")),
                Err(e) => break Linking::Exited(Some(e.to_string())),
            }
        };
        let (id, bank) = (a.id, a.bank.clone());
        match linking {
            Linking::Open => self.answer(w, id, AskState::Expired, "nobody finished Plaid's page in time: ask again"),
            Linking::Connected(Some(public)) if a.what == "connect" => {
                if let Err(e) = self.connected(w, id, &plaid, &public) {
                    let (_, why) = failure(&e);
                    self.say(format!("connecting a bank failed: {why}"));
                    self.answer(w, id, AskState::Failed, &why);
                }
            }
            // Update mode: the login given again, or accounts picked;
            // nothing to exchange.
            Linking::Connected(_) | Linking::Exited(None) if a.what == "reconnect" || a.what == "add_accounts" => {
                self.say(match a.what.as_str() {
                    "reconnect" => format!("{bank}: logged in again"),
                    _ => format!("{bank}: its accounts picked again"),
                });
                self.answer(w, id, AskState::Done, "");
                self.import_now(w, &bank);
            }
            Linking::Connected(_) | Linking::Exited(None) => {
                self.answer(w, id, AskState::Failed, "Plaid's page was closed before a bank was connected");
            }
            Linking::Exited(Some(why)) => self.answer(w, id, AskState::Failed, &why),
        }
        Ok(())
    }

    /// A new bank's public token traded for its login, the bank kept under
    /// a name of its own, the ask answered, and the bank imported at once.
    fn connected(&self, w: &mut Work, id: Id<BankAsk>, plaid: &Plaid, public: &str) -> Result<()> {
        let (token, item_id) = plaid.exchange(public)?;
        let item = plaid.accounts(&token).ok();
        let institution_id = item.as_ref().and_then(|i| i.institution_id.clone()).unwrap_or_default();
        let institution = match institution_id.as_str() {
            "" => String::new(),
            i => plaid.institution(i).unwrap_or_default(),
        };
        let known: BTreeSet<String> = read(w, |db| db.bank.scan().into_iter().map(|b: Bank| b.name).collect()).unwrap_or_default();
        let connection = Connection {
            access_token: token,
            cursor: None,
            item_id,
            institution_id,
            institution: institution.clone(),
            added_ms: ark_peer::now_ms(),
            rest: BTreeMap::new(),
        };
        let name = self.change(|s| {
            let name = s.name_for(&institution, |n| known.contains(n));
            s.banks.insert(name.clone(), connection);
            name
        })?;
        self.say(format!("{name}: connected"));
        let bank = name.clone();
        self.answer_with(
            w,
            Answer {
                id,
                state: AskState::Done,
                bank,
                institution,
                ..Answer::default()
            },
        );
        self.import_now(w, &name);
        Ok(())
    }

    /// Plaid told to forget a bank's login, the login dropped, and the ask
    /// answered: the domain forgets the bank.
    fn disconnect(&self, w: &mut Work, a: &BankAsk) -> Result<()> {
        let held = self.store().banks.get(&a.bank).cloned();
        if let Some(c) = held {
            match self.plaid()?.remove(&c.access_token) {
                Ok(()) => {}
                // Plaid has forgotten it already.
                Err(e) if matches!(e.code(), Some("ITEM_NOT_FOUND" | "INVALID_ACCESS_TOKEN")) => {}
                Err(e) => return Err(e.into()),
            }
            self.change(|s| s.banks.remove(&a.bank))?;
        }
        self.say(format!("{}: disconnected", a.bank));
        self.answer(w, a.id, AskState::Done, "");
        Ok(())
    }

    // * Imports

    /// One bank, now, as a run of its own.
    fn import_now(&self, w: &mut Work, name: &str) {
        let outcome = match self.plaid() {
            Ok(plaid) => self.import_or_say(w, &plaid, name),
            Err(e) => Outcome::Failed(e.to_string()),
        };
        self.record(Run {
            banks: [(name.to_string(), outcome)].into(),
            ..Run::default()
        });
    }

    /// Every bank, each on its own — one failing is said and the rest go
    /// on.
    fn import_all(&self, w: &mut Work) -> Run {
        let mut run = Run::default();
        let names: Vec<String> = self.store().banks.keys().cloned().collect();
        if names.is_empty() {
            self.say("no banks to import: connect one from the app".into());
            return run;
        }
        let plaid = match self.plaid() {
            Ok(p) => p,
            Err(e) => {
                self.say(format!("no Plaid keys: {e}"));
                for item in names {
                    run.banks.insert(item, Outcome::Failed(e.to_string()));
                }
                return run;
            }
        };
        self.retire(w, &plaid);
        for name in names {
            // Retired by the import of the login that replaced it.
            if !self.store().banks.contains_key(&name) {
                continue;
            }
            let outcome = self.import_or_say(w, &plaid, &name);
            run.banks.insert(name, outcome);
        }
        run
    }

    /// One bank imported, or what went wrong said — in the server's events,
    /// and to the household as `banks.failed` when it is news.
    fn import_or_say(&self, w: &mut Work, plaid: &Plaid, name: &str) -> Outcome {
        let replaced = read(w, |db| db.bank.get(name)).flatten().is_some_and(|b| !b.replaced_by.is_empty());
        if replaced {
            self.retire(w, plaid);
            return Outcome::Replaced;
        }
        let e = match self.import_one(w, plaid, name) {
            Ok(o) => {
                // Its first import may have folded it into another login
                // to the same bank: that one goes.
                self.retire(w, plaid);
                return o;
            }
            Err(e) => e,
        };
        let (code, why) = failure(&e);
        match money_domain::plaid::LOGIN_CODES.contains(&code.as_str()) {
            true => self.say(format!("{name}: the bank wants a fresh login; Reconnect it from the app")),
            false => self.say(format!("{name}: failed: {e:#}")),
        }
        let said = read(w, |db| db.bank.get(name)).flatten();
        if said.as_ref().is_none_or(|b| b.error_code != code || b.error != why) {
            let failed = BankFailed {
                bank: name.to_string(),
                institution: self.store().banks.get(name).map(|c| c.institution.clone()).unwrap_or_default(),
                error_code: code,
                error: why,
            };
            if let Err(e) = author(w, "banks.failed", &failed) {
                self.say(format!("{name}: could not say so: {e:#}"));
            }
        }
        Outcome::Failed(format!("{e:#}"))
    }

    /// Every login another login to the same bank replaced
    /// (`Bank::replaced_by`, which a fold sets): removed at Plaid, dropped
    /// from the store, and said retired, which has the budget forget it as
    /// it does a disconnected bank. One Plaid will not remove now is tried
    /// again at the next run, and imported by nobody meanwhile.
    fn retire(&self, w: &mut Work, plaid: &Plaid) {
        let replaced: Vec<Bank> = read(w, |db| db.bank.scan().into_iter().filter(|b: &Bank| !b.replaced_by.is_empty()).collect()).unwrap_or_default();
        for b in replaced {
            let held = self.store().banks.get(&b.name).cloned();
            if let Some(c) = held {
                match plaid.remove(&c.access_token) {
                    Ok(()) => {}
                    // Plaid has forgotten it already.
                    Err(e) if matches!(e.code(), Some("ITEM_NOT_FOUND" | "INVALID_ACCESS_TOKEN")) => {}
                    Err(e) => {
                        self.say(format!("{}: replaced by {}; Plaid would not remove it yet: {e}", b.name, b.replaced_by));
                        continue;
                    }
                }
                if let Err(e) = self.change(|s| s.banks.remove(&b.name)) {
                    self.say(format!("{}: removed at Plaid, and could not be dropped here: {e:#}", b.name));
                    continue;
                }
            }
            self.say(format!("{}: replaced by {}, removed", b.name, b.replaced_by));
            if let Err(e) = author(w, "banks.retired", &BankName { bank: b.name.clone() }) {
                self.say(format!("{}: could not say it is retired: {e:#}", b.name));
            }
        }
    }

    fn import_one(&self, w: &mut Work, plaid: &Plaid, name: &str) -> Result<Outcome> {
        let Some(conn) = self.store().banks.get(name).cloned() else {
            bail!("no longer connected");
        };
        let token = conn.access_token.clone();
        let got = plaid.accounts(&token)?;
        // What a connection written by hand lacks, filled in from Plaid.
        let mut filled = conn.clone();
        if filled.item_id.is_empty() {
            filled.item_id = got.item_id.clone().unwrap_or_default();
        }
        if filled.institution_id.is_empty() {
            filled.institution_id = got.institution_id.clone().unwrap_or_default();
        }
        if filled.institution.is_empty() && !filled.institution_id.is_empty() {
            filled.institution = plaid.institution(&filled.institution_id).unwrap_or_default();
        }
        if filled != conn {
            if let Err(e) = self.change(|s| s.banks.insert(name.to_string(), filled.clone())) {
                self.say(format!("{name}: could not keep what Plaid said of it: {e:#}"));
            }
        }
        let bank = read(w, |db| db.bank.get(name)).flatten();
        let institution = match (filled.institution.as_str(), &bank) {
            ("", Some(b)) if !b.institution.is_empty() => b.institution.clone(),
            ("", _) => name.to_string(),
            (i, _) => i.to_string(),
        };
        // Where its last import got to, as its row says; a bank no import
        // has reached yet, as the store does.
        let cursor = match &bank {
            Some(b) if b.repull => None,
            Some(b) if !b.cursor.is_empty() => Some(b.cursor.clone()),
            _ => conn.cursor.clone(),
        };
        let cursor = cursor.filter(|c| !c.is_empty());
        let Some(pulled) = plaid.pull(&token, cursor.as_deref(), self.banks().history)? else {
            self.say(format!("{name}: Plaid has not finished its first pull; the next run picks it up"));
            // Its accounts known meanwhile, so the household can choose
            // what each feeds before its transactions arrive.
            let known = read(w, |db| got.accounts.iter().all(|a| db.linked.get(&a.account_id).is_some()));
            if known == Some(false) {
                let import = Import {
                    cursor: bank.map(|b| b.cursor).unwrap_or_default(),
                    choose: true,
                    institution_id: Some(filled.institution_id.clone()).filter(|i| !i.is_empty()),
                    ..import_of(name, &institution, &got.accounts, &Default::default())
                };
                author(w, "banks.import", &import)?;
            }
            return Ok(Outcome::NotReady);
        };
        drop(token);
        self.say(format!(
            "{name}: {} added, {} modified, {} removed at the bank{}",
            pulled.added.len(),
            pulled.modified.len(),
            pulled.removed.len(),
            if cursor.is_none() { ", from the start" } else { "" }
        ));
        let import = Import {
            choose: true,
            from_start: cursor.is_none(),
            institution_id: Some(filled.institution_id.clone()).filter(|i| !i.is_empty()),
            ..import_of(name, &institution, &got.accounts, &pulled)
        };
        let (upserts, removed) = (import.upserts.len(), import.removed.len());
        author(w, "banks.import", &import)?;
        Ok(Outcome::Imported { upserts, removed })
    }

    /// Every store's receipts, from `amazon-orders` and the folder of
    /// exports, and Costco's page in the browser: each on its own, one failing said and
    /// the rest going on.
    fn import_stores(&self, w: &mut Work) -> BTreeMap<String, Outcome> {
        let mut out = BTreeMap::new();
        if let Some(amazon) = self.stores.amazon.clone() {
            // A login's first pull reaches back to the banks' first day, so
            // every order the household's transactions show is there; each
            // sync after, the last days alone.
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let since = (!amazon.pulled(&self.data)).then(|| self.first_bank_day(w)).flatten();
            let got = match &since {
                Some(day) => amazon.orders_since(&self.data, day, &today),
                None => amazon.run(&self.data),
            }
            .and_then(|text| money_domain::stores::amazon(&text).map_err(|e| anyhow!(e)))
            .and_then(|import| self.author_receipts(w, &import));
            let ordered = got.is_ok();
            out.insert("amazon".to_string(), self.outcome("amazon", got));
            // How each order was charged, once its orders are in.
            if ordered {
                let got = match &since {
                    Some(day) => amazon.charges_since(&self.data, day, &today),
                    None => amazon.charges(&self.data),
                }
                .and_then(|text| money_domain::stores::amazon_charges(&text).map_err(|e| anyhow!(e)))
                .and_then(|import| self.author_charges(w, &import));
                let charged = got.is_ok();
                out.insert("amazon charges".to_string(), self.outcome_charges("amazon charges", got));
                if charged && since.is_some() {
                    if let Err(e) = amazon.mark_pulled(&self.data) {
                        self.say(format!("amazon: the first pull is in, and could not be marked so: {e:#}"));
                    }
                }
            }
        }
        // Nothing from Costco, said or not, on a server with no browser. Its
        // first pull reaches back to the banks' first day, as Amazon's does.
        let since = (!crate::costco::pulled(&self.data)).then(|| self.first_bank_day(w)).flatten();
        if let Some(got) = self.stores.costco.receipts(since.as_deref()).transpose() {
            let got = got
                .and_then(|text| money_domain::stores::costco(&text).map_err(|e| anyhow!(e)))
                .and_then(|import| self.author_receipts(w, &import));
            if got.is_ok() && since.is_some() {
                if let Err(e) = crate::costco::mark_pulled(&self.data) {
                    self.say(format!("costco: the first pull is in, and could not be marked so: {e:#}"));
                }
            }
            out.insert("costco".to_string(), self.outcome("costco", got));
        }
        let Some(dir) = self.stores.dir.clone() else { return out };
        for w8 in waiting(&dir) {
            let name = format!("{}/{}", w8.store, w8.file.file_name().and_then(|n| n.to_str()).unwrap_or("?"));
            let read = std::fs::read_to_string(&w8.file)
                .map_err(|e| anyhow!("cannot read it: {e}"))
                .and_then(|text| money_domain::stores::read(&w8.store, &text).map_err(|e| anyhow!(e)));
            let (got, under) = match read {
                Err(e) => (Err(e), "failed"),
                Ok(import) => match self.author_receipts(w, &import) {
                    Ok(n) => (Ok(n), "imported"),
                    // Not the file's fault: it is tried again next run.
                    Err(e) => {
                        out.insert(name.clone(), self.outcome(&name, Err(e)));
                        continue;
                    }
                },
            };
            if let Err(e) = put_away(&w8, under) {
                self.say(format!("{name}: could not put it away: {e:#}"));
            }
            out.insert(name.clone(), self.outcome(&name, got));
        }
        out
    }

    fn outcome(&self, what: &str, got: Result<usize>) -> Outcome {
        match got {
            Ok(n) => {
                self.say(format!("{what}: {n} receipt{}", if n == 1 { "" } else { "s" }));
                Outcome::Receipts { receipts: n }
            }
            Err(e) => {
                self.say(format!("{what}: failed: {e:#}"));
                Outcome::Failed(format!("{e:#}"))
            }
        }
    }

    fn outcome_charges(&self, what: &str, got: Result<usize>) -> Outcome {
        match got {
            Ok(n) => {
                self.say(format!("{what}: {n} charge{}", if n == 1 { "" } else { "s" }));
                Outcome::Charges { charges: n }
            }
            Err(e) => {
                self.say(format!("{what}: failed: {e:#}"));
                Outcome::Failed(format!("{e:#}"))
            }
        }
    }

    /// A store's charges authored as `receipts.add_charges`: how many.
    fn author_charges(&self, w: &mut Work, import: &money_domain::ChargeImport) -> Result<usize> {
        if import.charges.is_empty() {
            return Ok(0);
        }
        author(w, "receipts.add_charges", import)?;
        Ok(import.charges.len())
    }

    /// A store's receipts authored as `receipts.add_receipts`: how many.
    fn author_receipts(&self, w: &mut Work, import: &ReceiptImport) -> Result<usize> {
        if import.receipts.is_empty() {
            return Ok(0);
        }
        author(w, "receipts.add_receipts", import)?;
        Ok(import.receipts.len())
    }
}
