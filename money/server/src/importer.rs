//! The bank importer: ArkOS's `money-sync`, inside the server, with the
//! household's budget as what it writes to — and the one place a bank is
//! connected, logged in to again, or disconnected.
//!
//! It is a peer like any other: an `ark_peer::Peer` in this process,
//! dialling the hub through its handle (no socket), signed in as the
//! account `bank` and holding the [`money_domain::BANK`] role, which the
//! server grants that account and nobody else unless told.
//!
//! **What it imports** is every bank in the server's store
//! ([`crate::store`], `banks.json` in the data directory): for each, a run
//! asks Plaid for the item's accounts and for every page of what changed
//! since the bank's cursor, makes them one import
//! (`money_domain::plaid::import_of`) and authors it as `banks.import`. The
//! cursor after it is kept in the store only once the hub has confirmed the
//! entry, which it does only once the entry is on its disk; a run that
//! stops anywhere before that is asked again from the old cursor next
//! time, and an import says the same thing twice without changing anything
//! (Plaid's id is the `imported_id`). An account the import is the first to
//! see waits for somebody in the household to say what it feeds
//! (`money_domain::banks`); when somebody does, the bank's next import is
//! asked from no cursor, its whole history again. What Plaid says when a
//! bank fails is authored as `banks.failed`, for the household to see.
//!
//! **When it runs:** once at start, whenever it is nudged ([`Importer::nudge`]:
//! `SIGUSR1`, which the NixOS module's timer sends twice a day), whenever
//! somebody in the household asks (`banks.sync`, a row of the `asked`
//! table it holds like everything else: an ask newer than the last one it
//! answered is answered by a run), and for one bank at once when it has
//! just been connected or logged in to again.
//!
//! **Connecting** is an ask too (`banks.connect`, `banks.reconnect`,
//! `banks.disconnect`, rows of `bank_ask`). For a connection it opens a
//! Plaid Hosted Link page and answers with its address; then it asks Plaid
//! after the page every [`Banks::poll`] until somebody finishes it there,
//! trades the public token for the bank's access token, keeps the bank in
//! the store, answers, and imports it at once. A page nobody finished in
//! [`Banks::lifetime_s`] is answered expired; a page open when the server
//! stopped is answered expired at its next start, its link token having
//! been held in memory only. A disconnection tells Plaid to forget the
//! login (`/item/remove`) and drops it from the store.
//!
//! **Secrets are never said.** The client id and the secret are read from
//! their files whenever they are needed, the tokens from the store; all of
//! them go to Plaid alone. Every line the importer says is in
//! [`Importer::said`] as well as on stderr, which is how the tests hold it
//! to that.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use ark::format::Encode;
use ark::peer::Who;
use ark::row::Id;
use ark_auth::Login;
use ark_peer::{Handle, Memory, Peer, Standing};
use money_domain::plaid::import_of;
use money_domain::{Answer, AskState, Asked, Bank, BankAsk, BankFailed, Import};

use crate::plaid::{LinkMode, LinkSession, Linking, Plaid, PlaidError};
use crate::store::{Connection, Store};

/// The account the importer signs in as: what every row it writes says
/// wrote it.
pub const ACCOUNT: &str = "bank";

/// What the importer's sign-in says it is.
pub const DEVICE: &str = "the bank importer";

/// How long a run waits for the hub to confirm an entry.
const CONFIRM: Duration = Duration::from_secs(60);

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Imported, confirmed, its cursor kept.
    Imported {
        upserts: usize,
        removed: usize,
    },
    /// Plaid has not finished the bank's first pull.
    NotReady,
    Failed(String),
}

/// The importer, running on a thread of its own.
pub struct Importer {
    wake: ark_peer::Wake,
    nudged: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    said: Arc<Mutex<Vec<String>>>,
    runs: Arc<Mutex<Vec<Run>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Importer {
    /// The importer for `banks`, signed in as `login`, dialling `hub`; its
    /// store in `data`. It runs once as soon as it is linked.
    pub fn start(banks: Banks, hub: Handle, data: PathBuf, login: Login) -> Result<Importer> {
        let mut roles = login.user.roles.clone();
        roles.push(money_domain::BANK.to_string());
        roles.sort();
        roles.dedup();
        let who = Who {
            actor: login.user.id.clone(),
            session: login.session.clone(),
            roles,
        };
        // In memory: what it holds is the hub's, filled at every start, and
        // an import it had not seen confirmed is pulled again from the
        // cursor kept, rather than pushed again from a disk.
        let config = ark_peer::Config::of(money_domain::shared());
        let mut peer = Peer::open(config, who.clone(), Box::new(Memory::default()), false);
        peer.sign_in(who.clone());
        let token = login.token.clone();
        let dial = hub.dialer(Arc::new(move |t: &str| (t == token).then(|| who.clone())));
        peer.set_token(Some(login.token));
        let (wake, rx) = ark_peer::link::bell();
        peer.set_waker(wake.clone());
        peer.connect_with("", dial);

        let store = Store::load(&data)?;
        let (nudged, stop) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)));
        let (said, runs) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
        let job = Job {
            peer,
            rx,
            banks,
            data,
            store,
            live: BTreeMap::new(),
            said: said.clone(),
            runs: runs.clone(),
        };
        let names: Vec<&str> = job.store.banks.keys().map(String::as_str).collect();
        job.say(match names.len() {
            0 => "no banks connected yet: connect one from the app".into(),
            _ => format!("connected to {}", names.join(", ")),
        });
        let thread = {
            let (nudged, stop) = (nudged.clone(), stop.clone());
            let mut job = job;
            std::thread::Builder::new()
                .name("money-importer".into())
                .spawn(move || job.run(&nudged, &stop))
                .map_err(|e| anyhow!("starting the importer: {e}"))?
        };
        Ok(Importer {
            wake,
            nudged,
            stop,
            said,
            runs,
            thread: Some(thread),
        })
    }

    /// Run now (or as soon as the current run ends).
    pub fn nudge(&self) {
        self.nudged.store(true, Ordering::SeqCst);
        (self.wake)();
    }

    /// Every line the importer has said.
    pub fn said(&self) -> Vec<String> {
        self.said.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Every run it has finished, oldest first: a run of every bank, or of
    /// one just connected.
    pub fn runs(&self) -> Vec<Run> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Drop for Importer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        (self.wake)();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A Hosted Link page open for an ask.
struct Live {
    what: String,
    bank: String,
    session: LinkSession,
    until: Instant,
    next: Instant,
}

/// The importer's thread: its peer, and what it keeps.
struct Job {
    peer: Peer,
    rx: mpsc::Receiver<()>,
    banks: Banks,
    data: PathBuf,
    store: Store,
    /// The pages open, by the ask they answer.
    live: BTreeMap<Id<BankAsk>, Live>,
    said: Arc<Mutex<Vec<String>>>,
    runs: Arc<Mutex<Vec<Run>>>,
}

/// What failing at a bank says: Plaid's code, when it gave one, and the
/// sentence. Neither holds a secret.
fn failure(e: &anyhow::Error) -> (String, String) {
    match e.downcast_ref::<PlaidError>() {
        Some(PlaidError::Api { code, message, .. }) => (code.clone(), message.clone()),
        _ => (String::new(), format!("{e:#}")),
    }
}

impl Job {
    fn say(&self, line: String) {
        eprintln!("money-server: importer: {line}");
        self.said.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }

    /// The peer pumped, and whatever the hub refused said.
    fn pump(&mut self) {
        let p = self.peer.pump();
        if let Some(why) = p.denied {
            self.say(format!("the hub turned the importer's sign-in away: {why}"));
        }
        for (_, why) in self.peer.take_rejections() {
            self.say(format!("refused: {why}"));
        }
    }

    /// A read of what the importer holds.
    fn read<T>(&self, f: impl FnOnce(&ark::db::Db<money_domain::Money>) -> T) -> Option<T> {
        ark::db::read::<money_domain::Money, _>(&self.peer.log.store, |db| Ok::<_, ark::Refusal>(f(db))).ok()
    }

    /// The newest ask the importer holds.
    fn asked(&self) -> i64 {
        self.read(money_domain::plaid::last_asked).flatten().map_or(0, |a: Asked| a.asked_ms)
    }

    fn plaid(&self) -> Result<Plaid> {
        let id = secret(&self.banks.client_id_file)?;
        let s = secret(&self.banks.secret_file)?;
        Ok(Plaid::new(&self.banks.url, id, s))
    }

    fn save(&self) -> Result<()> {
        self.store.save(&self.data)
    }

    fn record(&self, run: Run) {
        self.runs.lock().unwrap_or_else(|e| e.into_inner()).push(run);
    }

    fn run(&mut self, nudged: &AtomicBool, stop: &AtomicBool) {
        loop {
            let wait = match self.live.is_empty() {
                true => Duration::from_secs(1),
                false => self.banks.poll.min(Duration::from_secs(1)),
            };
            ark_peer::link::wait(&self.peer, &self.rx, wait);
            if stop.load(Ordering::SeqCst) {
                return;
            }
            self.pump();
            // Linked and filled: the asks it holds are the hub's.
            if !self.peer.linked || self.peer.log.fill.is_some() {
                continue;
            }
            self.answer_asks();
            self.poll_pages();
            let asked = self.asked();
            let due = nudged.swap(false, Ordering::SeqCst) || asked > self.store.answered_ms;
            if !due {
                continue;
            }
            let run = self.import_all(stop);
            if asked > self.store.answered_ms {
                self.store.answered_ms = asked;
                if let Err(e) = self.save() {
                    self.say(format!("could not keep what it answered: {e:#}"));
                }
            }
            self.record(run);
        }
    }

    // * Asks

    /// Every ask nobody has answered, answered: a page opened for a
    /// connection, a disconnection done. A page the store says is open but
    /// this process never opened was opened before a restart: expired.
    fn answer_asks(&mut self) {
        let open = self.read(money_domain::banks::open_asks).unwrap_or_default();
        for a in open {
            if self.live.contains_key(&a.id) {
                continue;
            }
            let answered = match (a.state, a.what.as_str()) {
                (AskState::Waiting, _) => {
                    self.answer(a.id, AskState::Expired, "the server restarted while Plaid's page was open: ask again");
                    Ok(())
                }
                (_, "connect") => self.open_page(&a, None),
                (_, "reconnect") => match self.store.banks.get(&a.bank).map(|c| c.access_token.clone()) {
                    Some(token) => self.open_page(&a, Some(token)),
                    None => Err(anyhow!("this server holds no login for {}", a.bank)),
                },
                (_, "disconnect") => self.disconnect(&a),
                (_, what) => Err(anyhow!("nothing here does {what:?}")),
            };
            if let Err(e) = answered {
                let (code, why) = failure(&e);
                self.say(format!("{} {}: failed: {why}", a.what, a.bank));
                let said = match code.is_empty() {
                    true => why,
                    false => format!("Plaid said {code}: {why}"),
                };
                self.answer(a.id, AskState::Failed, &said);
            }
        }
    }

    /// `banks.answer` authored with a sentence, said when it is refused.
    fn answer(&mut self, id: Id<BankAsk>, state: AskState, message: &str) {
        self.answer_with(Answer {
            id,
            state,
            message: message.into(),
            ..Answer::default()
        });
    }

    fn answer_with(&mut self, answer: Answer) {
        if let Err(e) = self.author("answer", &answer) {
            self.say(format!("could not answer an ask: {e:#}"));
        }
    }

    /// A Hosted Link page for a new bank, or for `token`'s bank's login.
    fn open_page(&mut self, a: &BankAsk, token: Option<String>) -> Result<()> {
        let plaid = self.plaid()?;
        let mode = match token {
            Some(t) => LinkMode::Update(t),
            None => LinkMode::New { days: self.banks.history },
        };
        let session = plaid.link_create(&mode, self.banks.lifetime_s)?;
        let lifetime = Duration::from_secs(self.banks.lifetime_s);
        self.say(match a.bank.as_str() {
            "" => format!("{}: Plaid's page is open", a.what),
            bank => format!("{} {bank}: Plaid's page is open", a.what),
        });
        self.answer_with(Answer {
            id: a.id,
            state: AskState::Waiting,
            url: session.url.clone(),
            expires_ms: ark_peer::now_ms() + lifetime.as_millis() as i64,
            ..Answer::default()
        });
        self.live.insert(
            a.id,
            Live {
                what: a.what.clone(),
                bank: a.bank.clone(),
                session,
                until: Instant::now() + lifetime,
                next: Instant::now() + self.banks.poll,
            },
        );
        Ok(())
    }

    /// Every open page whose turn it is, asked after.
    fn poll_pages(&mut self) {
        let now = Instant::now();
        let due: Vec<Id<BankAsk>> = self.live.iter().filter(|(_, l)| l.next <= now).map(|(id, _)| *id).collect();
        for id in due {
            let Some(mut live) = self.live.remove(&id) else { continue };
            let plaid = match self.plaid() {
                Ok(p) => p,
                Err(e) => {
                    self.answer(id, AskState::Failed, &format!("{e:#}"));
                    continue;
                }
            };
            let linking = match plaid.link_get(&live.session) {
                Ok(l) => l,
                // Plaid out of reach for a moment is not the page's end.
                Err(PlaidError::Transport(why)) => {
                    self.say(format!("asking after Plaid's page: {why}"));
                    Linking::Open
                }
                Err(e) => Linking::Exited(Some(e.to_string())),
            };
            match linking {
                Linking::Open if Instant::now() >= live.until => {
                    self.answer(id, AskState::Expired, "nobody finished Plaid's page in time: ask again");
                }
                Linking::Open => {
                    live.next = Instant::now() + self.banks.poll;
                    self.live.insert(id, live);
                }
                Linking::Connected(Some(public)) if live.what == "connect" => {
                    if let Err(e) = self.connected(id, &plaid, &public) {
                        let (_, why) = failure(&e);
                        self.say(format!("connecting a bank failed: {why}"));
                        self.answer(id, AskState::Failed, &why);
                    }
                }
                // Update mode: the login given again, nothing to exchange.
                Linking::Connected(_) | Linking::Exited(None) if live.what == "reconnect" => {
                    let bank = live.bank.clone();
                    self.say(format!("{bank}: logged in again"));
                    self.answer(id, AskState::Done, "");
                    self.import_now(&bank);
                }
                Linking::Connected(_) | Linking::Exited(None) => {
                    self.answer(id, AskState::Failed, "Plaid's page was closed before a bank was connected");
                }
                Linking::Exited(Some(why)) => self.answer(id, AskState::Failed, &why),
            }
        }
    }

    /// A new bank's public token traded for its login, the bank kept under
    /// a name of its own, the ask answered, and the bank imported at once.
    fn connected(&mut self, id: Id<BankAsk>, plaid: &Plaid, public: &str) -> Result<()> {
        let (token, item_id) = plaid.exchange(public)?;
        let item = plaid.accounts(&token).ok();
        let institution_id = item.as_ref().and_then(|i| i.institution_id.clone()).unwrap_or_default();
        let institution = match institution_id.as_str() {
            "" => String::new(),
            i => plaid.institution(i).unwrap_or_default(),
        };
        let known: BTreeSet<String> = self.read(|db| db.bank.scan().into_iter().map(|b: Bank| b.name).collect()).unwrap_or_default();
        let name = self.store.name_for(&institution, |n| known.contains(n));
        self.store.banks.insert(
            name.clone(),
            Connection {
                access_token: token,
                cursor: None,
                item_id,
                institution_id,
                institution: institution.clone(),
                added_ms: ark_peer::now_ms(),
                rest: BTreeMap::new(),
            },
        );
        self.save()?;
        self.say(format!("{name}: connected"));
        self.answer_with(Answer {
            id,
            state: AskState::Done,
            bank: name.clone(),
            institution,
            ..Answer::default()
        });
        self.import_now(&name);
        Ok(())
    }

    /// Plaid told to forget a bank's login, the login dropped, and the ask
    /// answered: the domain forgets the bank.
    fn disconnect(&mut self, a: &BankAsk) -> Result<()> {
        if let Some(c) = self.store.banks.get(&a.bank).cloned() {
            match self.plaid()?.remove(&c.access_token) {
                Ok(()) => {}
                // Plaid has forgotten it already.
                Err(e) if matches!(e.code(), Some("ITEM_NOT_FOUND" | "INVALID_ACCESS_TOKEN")) => {}
                Err(e) => return Err(e.into()),
            }
            self.store.banks.remove(&a.bank);
            self.save()?;
        }
        self.say(format!("{}: disconnected", a.bank));
        self.answer(a.id, AskState::Done, "");
        Ok(())
    }

    // * Imports

    /// One bank, now, as a run of its own.
    fn import_now(&mut self, name: &str) {
        let outcome = match self.plaid() {
            Ok(plaid) => self.import_or_say(&plaid, name),
            Err(e) => Outcome::Failed(e.to_string()),
        };
        self.record(Run {
            banks: [(name.to_string(), outcome)].into(),
        });
    }

    /// One run: every bank, each on its own — one failing is said and the
    /// rest go on.
    fn import_all(&mut self, stop: &AtomicBool) -> Run {
        let mut run = Run::default();
        if self.store.banks.is_empty() {
            self.say("no banks to import: connect one from the app".into());
            return run;
        }
        let plaid = match self.plaid() {
            Ok(p) => p,
            Err(e) => {
                self.say(format!("no Plaid keys: {e}"));
                for item in self.store.banks.keys() {
                    run.banks.insert(item.clone(), Outcome::Failed(e.to_string()));
                }
                return run;
            }
        };
        let names: Vec<String> = self.store.banks.keys().cloned().collect();
        for name in names {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let outcome = self.import_or_say(&plaid, &name);
            run.banks.insert(name, outcome);
        }
        run
    }

    /// One bank imported, or what went wrong said — on stderr, and to the
    /// household as `banks.failed` when it is news.
    fn import_or_say(&mut self, plaid: &Plaid, name: &str) -> Outcome {
        let e = match self.import_one(plaid, name) {
            Ok(o) => return o,
            Err(e) => e,
        };
        let (code, why) = failure(&e);
        match money_domain::plaid::LOGIN_CODES.contains(&code.as_str()) {
            true => self.say(format!("{name}: the bank wants a fresh login; Reconnect it from the app")),
            false => self.say(format!("{name}: failed: {e:#}")),
        }
        let said = self.read(|db| db.bank.get(name)).flatten();
        if said.as_ref().is_none_or(|b| b.error_code != code || b.error != why) {
            let failed = BankFailed {
                bank: name.to_string(),
                institution: self.store.banks.get(name).map(|c| c.institution.clone()).unwrap_or_default(),
                error_code: code,
                error: why,
            };
            if let Err(e) = self.author("failed", &failed) {
                self.say(format!("{name}: could not say so: {e:#}"));
            }
        }
        Outcome::Failed(format!("{e:#}"))
    }

    fn import_one(&mut self, plaid: &Plaid, name: &str) -> Result<Outcome> {
        let Some(conn) = self.store.banks.get(name).cloned() else {
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
            self.store.banks.insert(name.to_string(), filled.clone());
            if let Err(e) = self.save() {
                self.say(format!("{name}: could not keep what Plaid said of it: {e:#}"));
            }
        }
        let bank = self.read(|db| db.bank.get(name)).flatten();
        let institution = match (filled.institution.as_str(), &bank) {
            ("", Some(b)) if !b.institution.is_empty() => b.institution.clone(),
            ("", _) => name.to_string(),
            (i, _) => i.to_string(),
        };
        let repull = bank.as_ref().is_some_and(|b| b.repull);
        let cursor = match repull {
            true => None,
            false => conn.cursor.clone().filter(|c| !c.is_empty()),
        };
        let Some(pulled) = plaid.pull(&token, cursor.as_deref(), self.banks.history)? else {
            self.say(format!("{name}: Plaid has not finished its first pull; the next run picks it up"));
            // Its accounts known meanwhile, so the household can choose
            // what each feeds before its transactions arrive.
            let known = self.read(|db| got.accounts.iter().all(|a| db.linked.get(&a.account_id).is_some()));
            if known == Some(false) {
                let import = Import {
                    cursor: bank.map(|b| b.cursor).unwrap_or_default(),
                    choose: true,
                    ..import_of(name, &institution, &got.accounts, &Default::default())
                };
                self.author("import", &import)?;
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
            ..import_of(name, &institution, &got.accounts, &pulled)
        };
        let (upserts, removed) = (import.upserts.len(), import.removed.len());
        self.author("import", &import)?;
        if let Some(c) = self.store.banks.get_mut(name) {
            c.cursor = Some(pulled.cursor.clone());
        }
        self.save()
            .map_err(|e| anyhow!("the import is in, and its cursor could not be kept: {e:#}"))?;
        Ok(Outcome::Imported { upserts, removed })
    }

    /// A mutation authored, and waited for until the hub confirms it — on
    /// its disk — or refuses it.
    fn author(&mut self, name: &str, input: &impl Encode) -> Result<()> {
        let id = self
            .peer
            .mutate_bytes(name, input.to_bytes())
            .map_err(|why| anyhow!("refused here: {why}"))?;
        let mut why = None;
        let held = ark_peer::link::until(CONFIRM, Duration::from_millis(0), || {
            ark_peer::link::wait(&self.peer, &self.rx, Duration::from_millis(50));
            let p = self.peer.pump();
            for (r, w) in self.peer.take_rejections() {
                if r == id {
                    why = Some(w);
                }
            }
            drop(p);
            !matches!(self.peer.standing(&id), Standing::Pending)
        });
        match (held, self.peer.standing(&id), why) {
            (_, Standing::Confirmed, _) => Ok(()),
            (_, Standing::Rejected(w), _) | (_, _, Some(w)) => bail!("the hub refused it: {w}"),
            (false, ..) => bail!("the hub did not confirm it within {}s", CONFIRM.as_secs()),
            (true, s, None) => bail!("it stands {s:?}"),
        }
    }
}
