//! §1.9 Live state: the volatile space's machine, lighter than the log's.
//!
//! ```text
//! Live = { store over the volatile tables, the functions, pending intents,
//!          me, origin, local, seq, and, as the authority, what it judged }
//! ```
//!
//! It keeps no history. An entry is applied in order — the authority
//! numbers what it takes `seq`, `seq + 1`, … and a replica applies the
//! next one only — and judged by the same function the log's machine
//! judges by (`peer::run`: no function by its name, the function's refusal, a
//! table of the other space); there is no chain, no log, no snapshot of a
//! log, no fold and nothing made durable. What a server writes of it when
//! it closes is its rows (the driver's).
//!
//! **The frames are the log's four** (§1.11), each naming the volatile
//! space, with less in them:
//!
//! - a `Hello` says the replica's `seq` (no hash) and its device; the
//!   authority answers every one with a `Snapshot` of its rows at its
//!   `seq` (no hash) — joining mid-session, coming back after a gap, a
//!   divergence, or an authority that restarted are all the one answer;
//! - a sequenced entry is a `Page` of one, at its `seq`, with its facts and
//!   the state after them, its hash zero; a replica applies it when it is
//!   the next, skips one it has, and says `Hello` at a gap or when the
//!   state it reaches is not the one the entry says (unless the snapshot it
//!   stands on was another schema's, when states are taken as said);
//! - a push is a `Page` of intents, as in the log's space; a `Verdict`
//!   answers each.
//!
//! **A replica** previews its own intents (the optimistic state a UI draws)
//! and rebases them under every entry it is sent: undo newest-first, apply
//! the facts, run the still-pending again — or, when the entry is its own
//! oldest intent as previewed, nothing moves and the state is checked
//! beside the rest by the digests' arithmetic. **The authority** judges a
//! push once: a verdict it remembers (the last [`REMEMBERED`] of each
//! origin) is sent again; an intent at or below its origin's last is
//! refused as overtaken (what was live then is gone); a gap is taken (the
//! ones before it were lost with a restart); one naming a function it does
//! not run is `Held` and nothing waits behind it. `on_join`, `on_leave` and
//! `on_restore` are its own entries, through `Mutate`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use crate::frames::{Frame, Verdict};
use crate::hash::EntryHash;
use crate::log::{Entry, EntryId, Logged, Origin, Seq, Snapshot};
use crate::peer::{intent, redo, run, undo, Functions, Input, Out, Output, Pending, Who};
use crate::schema::{Schema, Space};
use crate::store::Store;

/// How many verdicts of each origin an authority remembers, to answer an
/// intent pushed again.
pub const REMEMBERED: usize = 64;

/// What the authority keeps of what it judged: each origin's last `local`
/// and the actor it is bound to, and its last verdicts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Judged {
    pub last: BTreeMap<Origin, (String, i64)>,
    pub verdicts: BTreeMap<Origin, VecDeque<(EntryId, Verdict)>>,
}

impl Judged {
    fn verdict(&self, e: &Entry) -> Option<Verdict> {
        let vs = self.verdicts.get(&e.origin)?;
        vs.iter().find(|(id, _)| *id == e.id).map(|(_, v)| v.clone())
    }

    fn keep(&mut self, e: &Entry, actor: &str, v: Verdict) {
        let l = self.last.entry(e.origin).or_insert_with(|| (actor.into(), 0));
        l.1 = l.1.max(e.local);
        let vs = self.verdicts.entry(e.origin).or_default();
        vs.push_back((e.id, v));
        if vs.len() > REMEMBERED {
            vs.pop_front();
        }
    }
}

#[derive(Clone, Debug)]
pub struct Live {
    /// The volatile tables, the pending intents' changes applied.
    pub store: Store,
    pub functions: Functions,
    pub pending: Vec<Pending>,
    pub me: Who,
    pub origin: Origin,
    pub local: i64,
    /// The last entry applied: the authority's, or what a replica has of it.
    pub seq: Seq,
    /// The authority's judgments; none on a replica.
    pub authority: Option<Judged>,
    /// What this device says it is, in every `Hello`.
    pub device: Option<Vec<u8>>,
    pub version: String,
    /// The state entries are not checked against: the snapshot this stands
    /// on was another schema's (§1.2).
    pub faith: bool,
    asked: bool,
}

impl Live {
    /// A replica, or with `authority` the authority, over `schema`'s
    /// volatile tables, empty.
    pub fn new(schema: Arc<Schema>, functions: Functions, authority: bool, me: Who) -> Live {
        let origin = Origin::named(&me.session);
        Live {
            store: Store::new_in(schema, Space::Volatile),
            functions,
            pending: vec![],
            me,
            origin,
            local: 0,
            seq: 0,
            authority: authority.then(Judged::default),
            device: None,
            version: String::new(),
            faith: false,
            asked: false,
        }
    }

    pub fn is_authority(&self) -> bool {
        self.authority.is_some()
    }

    /// The id this peer's next intent will have (§1.6).
    pub fn next_id(&self) -> EntryId {
        EntryId::of(self.origin, self.local + 1)
    }

    /// Where it stands: its `seq` (no chain, so no hash).
    pub fn head(&self) -> (Seq, EntryHash) {
        (self.seq, EntryHash::default())
    }

    /// The machine's step, over the log's inputs: a frame, an intent, a
    /// connection up, the rows a server kept (`Open`); the rest are the
    /// log's alone.
    pub fn step(&mut self, input: Input) -> Vec<Output> {
        let mut o = Out::default();
        match input {
            Input::Frame { from, bytes } => match Frame::dec(&bytes) {
                Ok(f) if f.space() == Space::Volatile => self.frame(from, f, &mut o),
                _ => {}
            },
            Input::Mutate { func, input, now } => self.mutate(func, input, now, &mut o),
            Input::Connected if !self.is_authority() => {
                self.asked = false;
                self.hello(&mut o);
                self.repush(&mut o);
            }
            Input::Open(d) => {
                if let Ok((store, _)) = Store::from_rows_fit(self.store.schema_arc().clone(), Space::Volatile, &d.rows) {
                    self.become_(store, &mut o);
                }
            }
            _ => {}
        }
        if !o.told.is_empty() {
            o.out.insert(0, Output::Changes(o.told));
        }
        o.out
    }

    fn mutate(&mut self, func: String, input: Vec<u8>, now: i64, o: &mut Out) {
        let e = intent(&self.me, self.origin, self.local + 1, func, input, now);
        let id = e.id;
        if self.is_authority() {
            match self.sequence(e, o) {
                Ok(_) => {
                    self.local += 1;
                    o.out.push(Output::Confirmed(id));
                }
                Err(why) => o.out.push(Output::Refused { id, why }),
            }
            return;
        }
        match run(&self.functions, &mut self.store, &e) {
            Err(why) => o.out.push(Output::Refused { id, why: why.0 }),
            Ok(changes) => {
                o.told.extend(changes.iter().cloned());
                self.local += 1;
                o.send(Frame::push(Space::Volatile, [e.clone()]));
                self.pending.push(Pending::new(e, changes));
            }
        }
    }

    /// The authority's judgment of an intent: run, and, taken, numbered
    /// and sent to every connection.
    fn sequence(&mut self, entry: Entry, o: &mut Out) -> Result<Seq, String> {
        let facts = run(&self.functions, &mut self.store, &entry).map_err(|r| r.0)?;
        o.told.extend(facts.iter().cloned());
        self.seq += 1;
        let l = Logged {
            seq: self.seq,
            entry,
            facts,
            state: self.store.state_hash(),
            hash: EntryHash::default(),
        };
        let page = Frame::Page {
            space: Space::Volatile,
            from: self.seq - 1,
            logged: vec![l],
        };
        o.out.push(Output::Fanout(page.enc()));
        Ok(self.seq)
    }

    fn frame(&mut self, from: Option<Who>, f: Frame, o: &mut Out) {
        match (f, from.filter(|_| self.is_authority())) {
            (Frame::Hello { .. }, Some(_)) => o.send(Frame::Snapshot {
                space: Space::Volatile,
                snapshot: Snapshot {
                    seq: self.seq,
                    hash: EntryHash::default(),
                    state: self.store.state_hash(),
                    rows: self.store.snapshot_rows(),
                },
            }),
            (Frame::Page { logged, .. }, Some(who)) => logged.into_iter().for_each(|l| self.push(&who, l.entry, o)),
            (Frame::Page { logged, .. }, None) if !self.is_authority() => self.confirm(logged, o),
            (Frame::Snapshot { snapshot, .. }, None) if !self.is_authority() => self.reset(snapshot, o),
            (Frame::Verdict { id, verdict, .. }, None) if !self.is_authority() => match verdict {
                // At or below where it stands, a pending intent is in the
                // state it holds (a snapshot carried it).
                Verdict::Ok { seq } if seq <= self.seq && self.settle(&id, o) => o.out.push(Output::Confirmed(id)),
                Verdict::Refused { why } if self.settle(&id, o) => o.out.push(Output::Refused { id, why }),
                _ => {}
            },
            _ => {}
        }
    }

    fn push(&mut self, who: &Who, mut entry: Entry, o: &mut Out) {
        let id = entry.id;
        let j = self.authority.as_mut().expect("an authority's");
        let verdict = match j.verdict(&entry) {
            Some(v) => v,
            None if !entry.id_checks() || j.last.get(&entry.origin).is_some_and(|l| l.0 != who.actor) => Verdict::Refused {
                why: "an id that is not its own, or from an origin of another actor".into(),
            },
            None if entry.local <= j.last.get(&entry.origin).map_or(0, |l| l.1) => Verdict::Refused {
                why: "overtaken by a later one of its origin".into(),
            },
            None if self.functions.get(&entry.func).is_none() => Verdict::Held { func: entry.func.clone() },
            None => {
                (entry.actor, entry.session, entry.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
                let v = match self.sequence(entry.clone(), o) {
                    Ok(seq) => Verdict::Ok { seq },
                    Err(why) => Verdict::Refused { why },
                };
                self.authority.as_mut().unwrap().keep(&entry, &who.actor, v.clone());
                v
            }
        };
        o.send(Frame::Verdict {
            space: Space::Volatile,
            id,
            verdict,
        });
    }

    /// Takes an intent out of the pending ones, the rest rebased.
    fn settle(&mut self, id: &EntryId, o: &mut Out) -> bool {
        let Some(i) = self.pending.iter().position(|p| p.entry.id == *id) else {
            return false;
        };
        undo(&mut self.store, &self.pending, &mut o.told);
        self.pending.remove(i);
        redo(&self.functions, &mut self.store, &mut self.pending, &mut o.told);
        true
    }

    /// Entries in order, applied by their facts: the pending intents undone
    /// at the first that is not the oldest of them as previewed, and run
    /// again once, after the page.
    fn confirm(&mut self, ls: Vec<Logged>, o: &mut Out) {
        let mut undone = false;
        for l in ls {
            if l.seq <= self.seq {
                continue;
            }
            let previewed = !undone && self.pending.first().is_some_and(|p| p.entry.id == l.entry.id && p.changes == l.facts);
            let ok = if l.seq > self.seq + 1 {
                false
            } else if previewed {
                let store = &self.store;
                let rest = self.pending[1..].iter().map(|p| p.undo.0.get_or_init(|| store.undo_of(&p.changes)));
                let ok = self.faith || store.hash_beside(rest) == l.state;
                if ok {
                    self.pending.remove(0);
                }
                ok
            } else {
                if !undone {
                    undo(&mut self.store, &self.pending, &mut o.told);
                    undone = true;
                }
                let applied = self.store.apply_all(&l.facts).is_ok();
                if applied {
                    o.told.extend(l.facts.iter().cloned());
                    self.pending.retain(|p| p.entry.id != l.entry.id);
                }
                applied && (self.faith || self.store.state_hash() == l.state)
            };
            if !ok {
                if undone {
                    redo(&self.functions, &mut self.store, &mut self.pending, &mut o.told);
                }
                return self.hello(o);
            }
            self.seq = l.seq;
            self.asked = false;
            if l.entry.origin == self.origin {
                o.out.push(Output::Confirmed(l.entry.id));
            }
        }
        if undone {
            redo(&self.functions, &mut self.store, &mut self.pending, &mut o.told);
        }
    }

    /// The authority's rows: the store becomes them, from any `seq` (an
    /// authority that restarted is behind), the pending intents run again
    /// on top and pushed again.
    fn reset(&mut self, s: Snapshot, o: &mut Out) {
        let Ok((new, other)) = Store::from_rows_fit(self.store.schema_arc().clone(), Space::Volatile, &s.rows) else {
            return;
        };
        let same = new.state_hash() == s.state;
        if !same && !other {
            return;
        }
        self.faith = !same;
        self.become_(new, o);
        self.seq = s.seq;
        self.asked = false;
        self.repush(o);
    }

    /// The store replaced, as changes, the pending intents run again on top.
    fn become_(&mut self, new: Store, o: &mut Out) {
        undo(&mut self.store, &self.pending, &mut o.told);
        o.told.extend(self.store.diff(&new));
        self.store = new;
        redo(&self.functions, &mut self.store, &mut self.pending, &mut o.told);
    }

    /// Where it stands, said once until answered.
    fn hello(&mut self, o: &mut Out) {
        if std::mem::replace(&mut self.asked, true) {
            return;
        }
        o.send(Frame::Hello {
            space: Space::Volatile,
            cursor: self.seq,
            hash: None,
            device: self.device.clone(),
            schema: self.store.schema_hash(),
            version: self.version.clone(),
        });
    }

    fn repush(&mut self, o: &mut Out) {
        for page in self.pending.chunks(crate::peer::PAGE) {
            o.send(Frame::push(Space::Volatile, page.iter().map(|p| p.entry.clone())));
        }
    }
}
