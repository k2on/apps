//! `E` — the explorer, over this device's own replica (`docs/plan-guards.md`
//! D4): every table the replica holds, the log as a client knows it, and a
//! read-only query console. `ark_explorer` draws it and decides what an
//! edit becomes; this file is what it is handed.
//!
//! A client writes only through the CRUD the domain exposes, authored as
//! the signed-in person like any button, and never raw: a client has no raw
//! writer, connected or alone, so what it does here is pushed and judged
//! like any other work. harken declares `crud` on no table, so here the
//! explorer is read-only, and says so; the raw switch is not drawn.

use std::collections::BTreeMap;

use ark_client::ark::hash::FnHash;
use ark_client::ark::store::Change;
use ark_client::ark::value::hex;
use ark_client::Args;
use ark_explorer::{Connection, CrudVerbs, Explorer, Line, LogView, Source, Verified, Writer};

use crate::{App, Edit, Message};

/// The explorer while it is open, and what it needs that does not move:
/// the names of the functions an entry may name, and the CRUD exposed.
pub struct Explore {
    pub ui: Explorer,
    names: BTreeMap<FnHash, String>,
    exposed: Vec<(String, CrudVerbs)>,
}

impl Explore {
    /// Opened over `client`'s module: every table's CRUD, as the module
    /// declares it, authorable by this device as whoever is signed in.
    pub fn open(client: &ark_client::Peer) -> Explore {
        let module = client.domain().module();
        Explore {
            ui: Explorer::new(),
            names: ark_explorer::function_names(module),
            exposed: ark_explorer::crud_of(module)
                .into_iter()
                .map(|(t, v)| (t, CrudVerbs { may_author: true, ..v }))
                .collect(),
        }
    }
}

/// Who this device writes as, in a word.
fn who(client: &ark_client::Peer) -> String {
    let s = client.status();
    match (s.user.is_empty(), s.alone) {
        (true, _) => "nobody".into(),
        (false, true) => format!("{} (alone)", s.user),
        (false, false) => s.user,
    }
}

/// The client as a writer: the domain's exposed CRUD, authored as the
/// signed-in person and remembered as any change this window makes; no raw
/// write.
struct Writes<'a> {
    client: &'a mut ark_client::Peer,
    edits: &'a mut Vec<Edit>,
    exposed: &'a [(String, CrudVerbs)],
}

impl Writer for Writes<'_> {
    fn exposed(&self) -> Vec<(String, CrudVerbs)> {
        self.exposed.to_vec()
    }
    fn author(&mut self, function: &str, args: Args) -> Result<(), String> {
        let id = self.client.mutate(function, args).map_err(|e| e.to_string())?;
        self.edits.push(Edit {
            id,
            what: format!("{function}, from the explorer"),
        });
        Ok(())
    }
    fn raw(&mut self, _change: Change) -> Result<(), String> {
        Err("not the authority".into())
    }
    fn who(&self) -> String {
        who(self.client)
    }
}

/// The same, to draw with: nothing is written while drawing.
struct Shown<'a> {
    client: &'a ark_client::Peer,
    exposed: &'a [(String, CrudVerbs)],
}

impl Writer for Shown<'_> {
    fn exposed(&self) -> Vec<(String, CrudVerbs)> {
        self.exposed.to_vec()
    }
    fn author(&mut self, _function: &str, _args: Args) -> Result<(), String> {
        Err("drawing writes nothing".into())
    }
    fn raw(&mut self, _change: Change) -> Result<(), String> {
        Err("not the authority".into())
    }
    fn who(&self) -> String {
        who(self.client)
    }
}

/// The log as a client knows it: a client keeps no log, so its own pending
/// intents (none of them in the log yet), where it is, how it is linked,
/// and its `Verify` answers.
struct Seen<'a> {
    client: &'a ark_client::Peer,
    names: &'a BTreeMap<FnHash, String>,
}

impl LogView for Seen<'_> {
    fn head(&self) -> i64 {
        self.client.cursor()
    }
    fn lines(&self) -> Vec<Line> {
        let r = self.client.replica();
        r.pending
            .iter()
            .map(|e| Line {
                seq: None,
                entry: e.clone(),
                function: self.names.get(&e.fn_hash).cloned().unwrap_or_else(|| hex(&e.fn_hash)),
                facts: r.recorded.get(&e.id).cloned(),
                standing: "pending".into(),
            })
            .collect()
    }
    fn connections(&self) -> Vec<Connection> {
        let s = self.client.status();
        let refused = self.client.replica().rejections.len();
        vec![Connection {
            who: format!("this device \u{b7} {}", who(self.client)),
            cursor: s.cursor,
            pending: Some(s.pending),
            note: format!("{} \u{b7} {refused} refused", s.link),
        }]
    }
    fn verifies(&self) -> Vec<Verified> {
        let me = "this device".to_string();
        self.client
            .agreed()
            .iter()
            .map(|(n, ok)| Verified {
                who: me.clone(),
                seq: *n,
                answer: Some(*ok),
            })
            .chain(self.client.unknown().iter().map(|n| Verified {
                who: me.clone(),
                seq: *n,
                answer: None,
            }))
            .collect()
    }
}

impl App {
    /// `E`: open the explorer over this device's replica, or close it.
    pub fn toggle_explorer(&mut self) {
        self.explore = match self.explore.take() {
            Some(_) => None,
            None => Some(Explore::open(&self.peer.client)),
        };
    }

    /// A message for the explorer: what it reads is the replica as it stands
    /// — copied for the call, since the writer borrows the client to author
    /// through it — and what it writes goes through the client.
    pub fn explore(&mut self, msg: ark_explorer::Msg) {
        let Some(x) = self.explore.as_mut() else { return };
        let client = &mut self.peer.client;
        let store = client.store().clone();
        let schema = client.schema().clone();
        let domain = client.domain().clone();
        let ctx = client.ctx().clone();
        let log = Owned::of(client, &x.names);
        let src = Source {
            store: &store,
            schema: &schema,
            module: domain.module(),
            log: &log,
            ctx,
        };
        let mut w = Writes {
            client,
            edits: &mut self.edits,
            exposed: &x.exposed,
        };
        if x.ui.update(msg, &src, &mut w) == ark_explorer::Outcome::Close {
            self.explore = None;
        }
    }

    /// What `view` draws in place of the page while the explorer is open.
    pub fn view_explorer(&self) -> Option<iced::Element<'_, Message>> {
        let x = self.explore.as_ref()?;
        let client = &self.peer.client;
        let log = Seen { client, names: &x.names };
        let src = Source {
            store: client.store(),
            schema: client.schema(),
            module: client.domain().module(),
            log: &log,
            ctx: client.ctx().clone(),
        };
        let shown = Shown { client, exposed: &x.exposed };
        // The source is the client's own, borrowed for the frame: what the
        // explorer draws is laid out now, so nothing it keeps outlives it.
        Some(x.ui.view(&src, &shown).map(Message::Explore))
    }
}

/// [`Seen`]'s answers taken while the client is borrowed for writing.
struct Owned {
    head: i64,
    lines: Vec<Line>,
    connections: Vec<Connection>,
    verifies: Vec<Verified>,
}

impl Owned {
    fn of(client: &ark_client::Peer, names: &BTreeMap<FnHash, String>) -> Owned {
        let seen = Seen { client, names };
        Owned {
            head: seen.head(),
            lines: seen.lines(),
            connections: seen.connections(),
            verifies: seen.verifies(),
        }
    }
}

impl LogView for Owned {
    fn head(&self) -> i64 {
        self.head
    }
    fn lines(&self) -> Vec<Line> {
        self.lines.clone()
    }
    fn connections(&self) -> Vec<Connection> {
        self.connections.clone()
    }
    fn verifies(&self) -> Vec<Verified> {
        self.verifies.clone()
    }
}
