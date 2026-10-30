//! The bank importer: ArkOS's `money-sync`, inside the server, with the
//! household's budget as what it writes to.
//!
//! It is a peer like any other: an `ark_peer::Peer` in this process,
//! dialling the hub through its handle (no socket), signed in as the
//! account `bank` and holding the [`money_domain::BANK`] role, which the
//! server grants that account and nobody else unless told. For each bank
//! it holds a token for (an "item" to Plaid) a run reads the token from
//! its file, asks Plaid for the item's accounts and for every page of
//! what changed since the bank's cursor, makes them one import
//! (`money_domain::plaid::import_of`) and authors it as `banks.import`.
//! The cursor after it is kept in the data directory (`importer.json`)
//! only once the hub has confirmed the entry, which it does only once the
//! entry is on its disk; a run that stops anywhere before that is asked
//! again from the old cursor next time, and an import says the same thing
//! twice without changing anything (Plaid's id is the `imported_id`).
//!
//! **When it runs:** once at start, whenever it is nudged ([`Importer::nudge`]:
//! `SIGUSR1`, which the NixOS module's timer sends twice a day), and
//! whenever somebody in the household asks (`banks.sync`, a row of the
//! `asked` table it holds like everything else): an ask newer than the last
//! one it answered is answered by a run. What it answered is kept beside
//! the cursors, so a restart does not answer an old ask again.
//!
//! **Naming and following:** what the server is told (`MONEY_ACCOUNTS`,
//! the NixOS module's `accounts`) names a bank's accounts the way the
//! household had them in Actual — by mask, or Plaid's account id when two
//! share one — and says which are followed. A bank with any account named
//! follows those alone: the rest are known, ignored, and opened nowhere
//! until somebody links one (`banks.link`). A bank with none named follows
//! every account under the bank's name for it, as the domain does. Either
//! way it is the first import's say only; after that, which account feeds
//! which is the household's.
//!
//! **Secrets are never said.** Tokens, the client id and the secret are read
//! from their files at each run and go to Plaid alone; every line the
//! importer says is in [`Importer::said`] as well as on stderr, which is
//! how the tests hold it to that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use ark::format::Encode;
use ark::peer::Who;
use ark_auth::Login;
use ark_peer::{Handle, Memory, Peer, Standing};
use money_domain::plaid::import_of;
use money_domain::{Asked, Import};
use serde::{Deserialize, Serialize};

use crate::plaid::{Plaid, PlaidError};

/// The account the importer signs in as: what every row it writes says
/// wrote it.
pub const ACCOUNT: &str = "bank";

/// What the importer's sign-in says it is.
pub const DEVICE: &str = "the bank importer";

/// How long a run waits for the hub to confirm an import.
const CONFIRM: Duration = Duration::from_secs(60);

/// One of the household's accounts as Actual had it: on which bank, and
/// which of the bank's accounts — by the last digits Plaid shows, or by
/// Plaid's id when two share them.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub item: String,
    #[serde(default)]
    pub mask: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
}

impl Mapping {
    /// Whether this is `item`'s account `account_id`, whose mask is `mask`.
    pub fn is(&self, item: &str, account_id: &str, mask: &str) -> bool {
        self.item == item
            && match &self.account_id {
                Some(id) => id == account_id,
                None => self.mask.as_deref() == Some(mask),
            }
    }
}

/// Everything the importer is told. Secrets are files, read at each run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banks {
    /// Where Plaid is: `https://production.plaid.com`, or the sandbox's, or
    /// a mock's.
    pub url: String,
    pub client_id_file: PathBuf,
    pub secret_file: PathBuf,
    /// Each bank by the short name it was linked under, and the file
    /// holding its access token.
    pub items: BTreeMap<String, PathBuf>,
    /// The household's name for an account → the bank's account it is.
    pub accounts: BTreeMap<String, Mapping>,
    /// Days of history asked for the first time a bank is pulled.
    pub history: u32,
}

impl Banks {
    /// What is wrong with what the importer was told, before it starts.
    pub fn check(&self) -> Result<()> {
        for (name, m) in &self.accounts {
            if m.mask.is_none() && m.account_id.is_none() {
                bail!("account {name:?}: say its mask or its accountId");
            }
            if !self.items.contains_key(&m.item) {
                bail!("account {name:?} is on {:?}, which is not a bank the importer has a token for", m.item);
            }
        }
        Ok(())
    }
}

/// What the importer keeps in the data directory: each bank's cursor, and
/// the last ask it answered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub cursors: BTreeMap<String, String>,
    pub answered_ms: i64,
}

/// Where the importer's state is kept.
pub fn state_file(data: &Path) -> PathBuf {
    data.join("importer.json")
}

impl State {
    pub fn load(path: &Path) -> Result<State> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("reading {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Written whole beside and moved in, each step on the disk before the
    /// next: a crash leaves the old state or the new, never half of one.
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let tmp = path.with_extension("json.tmp");
        let mut f = std::fs::File::create(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
        if let Some(dir) = path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
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
    /// state in `data`. It runs once as soon as it is linked.
    pub fn start(banks: Banks, hub: Handle, data: PathBuf, login: Login) -> Result<Importer> {
        banks.check()?;
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

        let state = State::load(&state_file(&data))?;
        let (nudged, stop) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)));
        let (said, runs) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
        let mut job = Job {
            peer,
            rx,
            banks,
            file: state_file(&data),
            state,
            said: said.clone(),
            runs: runs.clone(),
        };
        let thread = {
            let (nudged, stop) = (nudged.clone(), stop.clone());
            std::thread::Builder::new()
                .name("money-importer".into())
                .spawn(move || job.run(&nudged, &stop))
                .context("starting the importer")?
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

    /// Every run it has finished, oldest first.
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

/// The importer's thread: its peer, and what it keeps.
struct Job {
    peer: Peer,
    rx: mpsc::Receiver<()>,
    banks: Banks,
    file: PathBuf,
    state: State,
    said: Arc<Mutex<Vec<String>>>,
    runs: Arc<Mutex<Vec<Run>>>,
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

    /// The newest ask the importer holds.
    fn asked(&self) -> i64 {
        let asked = ark::db::read::<money_domain::Money, _>(&self.peer.log.store, |db| Ok::<_, ark::Refusal>(money_domain::plaid::last_asked(db)));
        asked.ok().flatten().map_or(0, |a: Asked| a.asked_ms)
    }

    fn run(&mut self, nudged: &AtomicBool, stop: &AtomicBool) {
        loop {
            ark_peer::link::wait(&self.peer, &self.rx, Duration::from_secs(1));
            if stop.load(Ordering::SeqCst) {
                return;
            }
            self.pump();
            // Linked and filled: the asks it holds are the hub's.
            if !self.peer.linked || self.peer.log.fill.is_some() {
                continue;
            }
            let asked = self.asked();
            let due = nudged.swap(false, Ordering::SeqCst) || asked > self.state.answered_ms;
            if !due {
                continue;
            }
            let run = self.import_all(stop);
            if asked > self.state.answered_ms {
                self.state.answered_ms = asked;
                if let Err(e) = self.state.save(&self.file) {
                    self.say(format!("could not keep what it answered: {e:#}"));
                }
            }
            self.runs.lock().unwrap_or_else(|e| e.into_inner()).push(run);
        }
    }

    /// One run: every bank, each on its own — one failing is said and the
    /// rest go on.
    fn import_all(&mut self, stop: &AtomicBool) -> Run {
        let mut run = Run::default();
        if self.banks.items.is_empty() {
            self.say("no banks to import: link one and give the server its token".into());
            return run;
        }
        let plaid = match (secret(&self.banks.client_id_file), secret(&self.banks.secret_file)) {
            (Ok(id), Ok(s)) => Plaid::new(&self.banks.url, id, s),
            (Err(e), _) | (_, Err(e)) => {
                self.say(format!("no Plaid keys: {e}"));
                for item in self.banks.items.keys() {
                    run.banks.insert(item.clone(), Outcome::Failed(e.to_string()));
                }
                return run;
            }
        };
        let items: Vec<(String, PathBuf)> = self.banks.items.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (item, token_file) in items {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let outcome = match self.import_one(&plaid, &item, &token_file) {
                Ok(o) => o,
                Err(e) => {
                    let why = format!("{e:#}");
                    match e.downcast_ref::<PlaidError>().and_then(PlaidError::code) {
                        Some("ITEM_LOGIN_REQUIRED") => self.say(format!(
                            "{item}: the bank wants a fresh login; run `ark service money link {item} --update` from a laptop"
                        )),
                        _ => self.say(format!("{item}: failed: {why}")),
                    }
                    Outcome::Failed(why)
                }
            };
            run.banks.insert(item, outcome);
        }
        run
    }

    fn import_one(&mut self, plaid: &Plaid, item: &str, token_file: &Path) -> Result<Outcome> {
        let token = secret(token_file)?;
        let got = plaid.accounts(&token)?;
        let institution = match &got.institution_id {
            Some(id) => plaid.institution(id).unwrap_or_else(|_| item.to_string()),
            None => item.to_string(),
        };
        let cursor = self.state.cursors.get(item).cloned();
        let Some(pulled) = plaid.pull(&token, cursor.as_deref(), self.banks.history)? else {
            self.say(format!("{item}: Plaid has not finished its first pull; the next run picks it up"));
            return Ok(Outcome::NotReady);
        };
        drop(token);
        self.say(format!(
            "{item}: {} added, {} modified, {} removed at the bank",
            pulled.added.len(),
            pulled.modified.len(),
            pulled.removed.len()
        ));
        let import = self.named(item, import_of(item, &institution, &got.accounts, &pulled));
        let (upserts, removed) = (import.upserts.len(), import.removed.len());
        self.author(&import)?;
        self.state.cursors.insert(item.to_string(), pulled.cursor.clone());
        self.state
            .save(&self.file)
            .context("the import is in, and its cursor could not be kept")?;
        Ok(Outcome::Imported { upserts, removed })
    }

    /// The import as the server was told to name and follow the bank's
    /// accounts.
    fn named(&self, item: &str, mut import: Import) -> Import {
        let told: Vec<(&String, &Mapping)> = self.banks.accounts.iter().filter(|(_, m)| m.item == item).collect();
        if told.is_empty() {
            return import;
        }
        for a in &mut import.accounts {
            match told.iter().find(|(_, m)| m.is(item, &a.plaid_account_id, &a.mask)) {
                Some((name, _)) => a.called = name.to_string(),
                None => a.ignored = true,
            }
        }
        import
    }

    /// `banks.import` authored, and waited for until the hub confirms it —
    /// on its disk — or refuses it.
    fn author(&mut self, import: &Import) -> Result<()> {
        let id = self
            .peer
            .mutate_bytes("import", import.to_bytes())
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
            (_, Standing::Rejected(w), _) | (_, _, Some(w)) => bail!("the hub refused the import: {w}"),
            (false, ..) => bail!("the hub did not confirm the import within {}s", CONFIRM.as_secs()),
            (true, s, None) => bail!("the import stands {s:?}"),
        }
    }
}
