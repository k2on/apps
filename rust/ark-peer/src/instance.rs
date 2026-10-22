//! An instance (§1.7): one machine and what keeps it — a disk or none, the
//! open from it, every record written in order, the log folded by one rule
//! ([`Fold`]).
//!
//! Two concepts make every kind of instance. A table is durable or
//! volatile (§1.3). An instance is wired — one of a [`Peer`](crate::Peer)'s
//! three, linked to its server or serving one — or it is not: a machine in
//! the authority role never offered a link. So a schema's tables are
//! durable and synced (the app's log), volatile and synced (its live
//! state), durable and **local** ([`Instance::open`]: the log's own format
//! on a disk, `kv` and `log`), or volatile and **ephemeral**
//! ([`Instance::ephemeral`]: no disk). The instance is over the machine, not
//! a schema: a peer's synced machines are built from a domain at run time;
//! an unwired one is read and written through a `Db<S>` of its schema's
//! struct ([`Instance::read`], [`Instance::write`]).
//!
//! Only an unwired instance is written by a closure: what the closure made
//! is the facts of one entry the machine sequences, logs and makes durable
//! (`Input::Write`, an entry naming no function, which nothing runs
//! again). A synced entry names a domain's function.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use ark::db::{Db, Tables, Writes};
use ark::log::Durable;
use ark::peer::{Functions, Input, Output, Peer as Machine, Who};
use ark::schema::Space;
use ark::store::{Change, LOG};
use ark::Refusal;

use crate::{alone_at, draw_origin, now_ms, Disk};

/// When an instance folds its log into its rows, the one rule: once the
/// entries above its snapshot outnumber `keep` and `slack` (and the rows it
/// holds, when `rows`), all but the last `keep` are folded. A server keeps
/// `keep` entries and folds at twice that; a local instance keeps none and
/// folds once its entries outnumber its rows (and 64), so a fold, which
/// writes every row, is paid for by as many writes; a device's live machine
/// keeps none and folds at 64 ([`Fold::REPLICA`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub keep: usize,
    pub slack: usize,
    pub rows: bool,
}

impl Fold {
    /// A device's live machine's, a replica's: none kept, folded once 64
    /// are held — a replica reads no entry below its head again.
    pub const REPLICA: Fold = Fold {
        keep: 0,
        slack: 64,
        rows: false,
    };

    /// A server's: `keep` entries above the horizon (none kept when none).
    pub fn keep(keep: Option<usize>) -> Option<Fold> {
        keep.map(|keep| Fold {
            keep,
            slack: keep,
            rows: false,
        })
    }
}

/// One machine and what keeps it (§1.7): its disk, when it has one —
/// opened from what the disk holds, every `Persist` written in order (and,
/// while a write fails, kept to write again, the failure said), its log
/// folded as its [`Fold`] says. A [`Peer`](crate::Peer) is three of these
/// wired together; one alone is a schema's tables on a machine never
/// linked, read and written through a `Db<S>` of its schema's struct.
pub struct Instance {
    pub machine: Machine,
    pub disk: Option<Box<dyn Disk>>,
    pub fold: Option<Fold>,
    /// Records not yet written.
    pub(crate) unwritten: Vec<Vec<u8>>,
    /// The rows it holds, which a fold by rows is measured against.
    pub(crate) rows: usize,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
}

impl Deref for Instance {
    type Target = Machine;
    fn deref(&self) -> &Machine {
        &self.machine
    }
}

impl DerefMut for Instance {
    fn deref_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }
}

impl Instance {
    pub fn new(machine: Machine, disk: Option<Box<dyn Disk>>) -> Instance {
        Instance {
            machine,
            disk,
            fold: None,
            unwritten: vec![],
            rows: 0,
            failure: None,
        }
    }

    /// `S`'s tables as `disk` last kept them, on an unwired machine; empty,
    /// saying why in [`Instance::failure`], when it does not read.
    pub fn open<S: Tables>(disk: Box<dyn Disk>) -> Instance {
        let mut i = Instance::unwired::<S>(Space::Durable, Some(disk));
        i.fold = Some(Fold {
            keep: 0,
            slack: 64,
            rows: true,
        });
        let stored = i.stored();
        i.step(Input::Open(stored));
        i
    }

    /// `S`'s volatile tables, empty, kept nowhere.
    pub fn ephemeral<S: Tables>() -> Instance {
        Instance::unwired::<S>(Space::Volatile, None)
    }

    fn unwired<S: Tables>(space: Space, disk: Option<Box<dyn Disk>>) -> Instance {
        let schema = Arc::new(S::schema());
        let mut machine = Machine::new(schema, Functions::default(), alone_at(None), Who::default()).in_space(space);
        // Its own ids, drawn afresh at each open: the log it folds keeps none.
        machine.origin = draw_origin();
        Instance::new(machine, disk)
    }

    /// What its disk holds, to open from: empty, the failure said, when it
    /// does not read.
    pub fn stored(&mut self) -> Durable {
        let stored = self.disk.as_mut().map_or(Ok(Default::default()), |d| d.load());
        let s = stored.unwrap_or_else(|e| {
            self.failure = Some(e.to_string());
            Default::default()
        });
        let rows = s.rows.unwrap_or_else(|| self.machine.confirmed_rows());
        Durable {
            rows,
            logged: s.logged,
            ..Durable::default()
        }
    }

    /// One input to its machine, what it made durable written, its log
    /// folded when its rule says; what the machine said.
    pub fn step(&mut self, input: Input) -> Vec<Output> {
        let out = self.machine.step(input);
        self.took(&out);
        let (head, base) = (self.machine.log().head().0, self.machine.log().base_seq());
        if let Some(f) = self
            .fold
            .filter(|f| head - base > (f.keep + f.slack + f.rows as usize * self.rows) as i64)
        {
            let folded = self.machine.step(Input::Compact(head - f.keep as i64));
            self.took(&folded);
        }
        self.flush();
        out
    }

    fn took(&mut self, out: &[Output]) {
        for o in out {
            match o {
                Output::Persist(b) => self.unwritten.push(b.clone()),
                Output::Changes(cs) => {
                    let n = |c: &Change| match c {
                        Change::Add { .. } => 1,
                        Change::Remove { .. } => -1,
                        Change::Edit { .. } => 0,
                    };
                    self.rows = self.rows.saturating_add_signed(cs.iter().filter(|c| c.table() != LOG).map(n).sum());
                }
                _ => {}
            }
        }
    }

    /// What is unwritten, written in order up to the first write that
    /// fails: whether all of it is.
    pub fn flush(&mut self) -> bool {
        let Some(d) = &mut self.disk else {
            self.unwritten.clear();
            return true;
        };
        while let Some(r) = self.unwritten.first() {
            if let Err(e) = d.record(r) {
                self.failure = Some(format!("the log cannot be written: {e}"));
                return false;
            }
            self.unwritten.remove(0);
        }
        self.failure = None;
        true
    }

    /// Its disk, to open it again from.
    pub fn into_disk(self) -> Box<dyn Disk> {
        self.disk.expect("an instance with a disk")
    }

    /// `f` over its tables, as `S`'s.
    pub fn read<S: Tables, T>(&self, f: impl for<'t> FnOnce(&Db<'t, S>) -> T) -> T {
        ark::db::read::<S, T>(&self.machine.store, f)
    }

    /// `f` over its tables as `S`'s, writing `P`'s: what it made is the
    /// facts of one entry (`Input::Write`), all or none, and its answer.
    pub fn write<S: Tables, P: Writes, T>(&mut self, f: impl for<'t> FnOnce(&mut Db<'t, S, P>) -> Result<T, Refusal>) -> Result<T, Refusal> {
        let (t, facts) = ark::db::write_in::<S, P, T>(&mut self.machine.store, f)?;
        if facts.is_empty() {
            return Ok(t);
        }
        self.machine
            .store
            .apply_all(&facts.iter().rev().map(Change::inverse).collect::<Vec<_>>())?;
        let said = self.step(Input::Write { facts, now: now_ms() });
        match said.into_iter().find_map(|o| match o {
            Output::Refused { why, .. } => Some(why),
            _ => None,
        }) {
            Some(why) => Err(Refusal::new(why)),
            None => Ok(t),
        }
    }
}
