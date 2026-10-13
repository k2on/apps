//! `E` — the explorer, over this device's own replica (`docs/plan-guards.md`
//! D4): every table the replica holds, the log as a client knows it, a
//! read-only query console, and every query and mutation of the module,
//! callable from a form. `ark_explorer` draws it and decides what an edit or
//! a call becomes; this file is what it is handed.
//!
//! A client writes only through domain mutations, authored as the signed-in
//! person exactly as a button authors them, and never raw: a client has no
//! raw writer, connected or alone, so what it does here is pushed and judged
//! like any other work. A cell edit goes through the CRUD the domain exposes
//! and harken declares `crud` on no table, so the table browser is read-only
//! here and says so; the raw switch is not drawn. A mutation called from its
//! form is authored like any other, and its standing — pending, in the log,
//! refused — is what [`ark_client::Peer::standing`] says of its id.

use std::collections::BTreeMap;

use ark_client::ark::hash::FnHash;
use ark_client::ark::store::Change;
use ark_client::ark::value::hex;
use ark_client::{Args, Standing};
use ark_explorer::{Connection, CrudVerbs, Explorer, Line, LogView, Source, Ticket, Verdict, Verified, Writer};

use iced::{Point, Size};

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

/// Where an intent the explorer authored stands, by its id: pending until
/// the authority answers, then in the log or refused with the reason every
/// replica reaches.
fn verdict(client: &ark_client::Peer, ticket: &Ticket) -> Verdict {
    let Some(id) = ark_client::ark::value::decode_hex(&ticket.0).and_then(|b| <[u8; 16]>::try_from(b).ok()) else {
        return Verdict::Unknown;
    };
    match client.standing(&id) {
        Standing::Pending => Verdict::Pending,
        Standing::Confirmed => Verdict::Logged(None),
        Standing::Rejected(why) => Verdict::Refused(why),
        Standing::Unknown => Verdict::Unknown,
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
    fn author(&mut self, function: &str, args: Args) -> Result<Ticket, String> {
        let id = self.client.mutate(function, args).map_err(|e| e.to_string())?;
        self.edits.push(Edit {
            id,
            what: format!("{function}, from the explorer"),
        });
        Ok(Ticket(hex(&id)))
    }
    fn verdict(&self, ticket: &Ticket) -> Verdict {
        verdict(self.client, ticket)
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
    fn author(&mut self, _function: &str, _args: Args) -> Result<Ticket, String> {
        Err("drawing writes nothing".into())
    }
    fn verdict(&self, ticket: &Ticket) -> Verdict {
        verdict(self.client, ticket)
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
    /// `E`: open the explorer over this device's replica, docked beside the
    /// app with the keys, or close it and give them back.
    pub fn toggle_explorer(&mut self) {
        match self.explore.is_some() {
            // Closing is a click away: an edit in progress ends first.
            true => {
                self.explore(ark_explorer::Msg::Blur);
                self.explore = None;
            }
            false => self.explore = Some(Explore::open(&self.peer.client)),
        }
        self.dock.keys = true;
        self.dock.dragging = false;
        self.sync_window();
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
            self.sync_window();
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

/// The explorer docked beside the app rather than over it
/// (`docs/plan-guards.md` D4), so the app stays usable while it is open:
/// clicking, scrolling, playing and its own keys. One canvas in a browser
/// makes a docked panel the honest form there, and the desktop docks it the
/// same way — harken is one `iced::application` window, and popping the
/// explorer out to a window of its own would mean running the app as an
/// `iced::daemon` with a view per window, which is a change to its entry and
/// not to this panel.
///
/// **The keyboard follows the last click**, app or explorer, and `` ` ``
/// moves it between them — a key neither grammar binds, chorded with
/// nothing a browser takes for itself. The panel's left edge drags to
/// resize it; the width is kept for the session, so closing and opening it
/// again finds it where it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dock {
    /// How wide the panel is, once it has been opened.
    pub width: f32,
    /// The explorer has the keys, rather than the app beside it.
    pub keys: bool,
    /// The panel's edge is being dragged.
    pub dragging: bool,
    /// Where the pointer is, in the whole window.
    pub pointer: Point,
}

/// The narrowest the panel is, and the narrowest the app beside it is.
pub const DOCK_MIN: f32 = 360.0;
pub const APP_MIN: f32 = 420.0;
/// How far either side of the panel's edge a press starts a drag.
pub const DOCK_GRIP: f32 = 5.0;
/// The rule between the app and the panel.
pub const DOCK_RULE: f32 = 1.0;
/// The key that moves the keyboard between the app and the explorer.
pub const SWITCH: &str = "`";

impl Default for Dock {
    fn default() -> Dock {
        Dock {
            width: 0.0,
            keys: true,
            dragging: false,
            pointer: Point::ORIGIN,
        }
    }
}

impl Dock {
    /// The panel's width in a window `screen` wide: what it was, or a share
    /// of the window the first time, within its bounds and the app's.
    pub fn width_in(&self, screen: Size) -> f32 {
        let want = if self.width > 0.0 { self.width } else { screen.width * 0.46 };
        let least = DOCK_MIN.min(screen.width);
        want.clamp(least, (screen.width - APP_MIN - DOCK_RULE).max(least))
    }

    /// Where the panel starts, from the window's left.
    pub fn edge(&self, screen: Size) -> f32 {
        screen.width - self.width_in(screen) - DOCK_RULE
    }

    /// Whether a press at the pointer is on the panel's edge.
    pub fn on_edge(&self, screen: Size) -> bool {
        (self.pointer.x - self.edge(screen)).abs() <= DOCK_GRIP
    }

    /// Whether the pointer is over the panel.
    pub fn over(&self, screen: Size) -> bool {
        self.pointer.x > self.edge(screen)
    }

    /// Drag the edge to the pointer.
    pub fn drag(&mut self, screen: Size) {
        self.width = screen.width - self.pointer.x - DOCK_RULE;
        self.width = self.width_in(screen);
    }
}

impl App {
    /// The app's own area: the window, less the panel while it is open. What
    /// every geometry of the app's is worked out against.
    pub fn sync_window(&mut self) {
        let docked = match self.explore {
            Some(_) => self.dock.width_in(self.screen) + DOCK_RULE,
            None => 0.0,
        };
        self.window = Size::new((self.screen.width - docked).max(0.0), self.screen.height);
        if let Some(x) = self.explore.as_mut() {
            x.ui.has_keys = self.dock.keys;
        }
    }

    /// Give the keys to the explorer, or back to the app; leaving the
    /// explorer ends an edit in progress as a click away from it does.
    pub fn explorer_keys(&mut self, keys: bool) {
        if self.explore.is_none() || self.dock.keys == keys {
            return;
        }
        self.dock.keys = keys;
        if !keys {
            self.explore(ark_explorer::Msg::Blur);
        }
        self.sync_window();
    }

    /// The pointer moved, anywhere in the window: the edge follows it while
    /// it is being dragged.
    pub fn pointer(&mut self, at: Point) {
        self.dock.pointer = at;
        if self.dock.dragging && self.explore.is_some() {
            self.dock.drag(self.screen);
            self.dock.width = self.dock.width_in(self.screen);
            self.sync_window();
        }
    }

    /// A press, anywhere in the window — seen whatever widget took it, so a
    /// button in either half still moves the keys: on the edge it starts a
    /// drag; otherwise the keys go to whichever half it was in.
    pub fn pressed(&mut self) {
        if self.explore.is_none() {
            return;
        }
        if self.dock.on_edge(self.screen) {
            self.dock.dragging = true;
            return;
        }
        self.explorer_keys(self.dock.over(self.screen));
    }

    /// The panel beside the app: a line saying where the keys are and how to
    /// move them, its close button, and the explorer.
    pub fn view_dock<'a>(&'a self, explorer: iced::Element<'a, Message>) -> iced::Element<'a, Message> {
        use arkui::style;
        use iced::widget::{button, column, container, row, text, Space};
        use iced::Length;
        let keys = match self.dock.keys {
            true => format!("the keys are here \u{b7} {SWITCH} gives them to the app"),
            false => format!("the keys are the app's \u{b7} {SWITCH} or a click brings them here"),
        };
        let head = row![
            text(keys).size(11).style(if self.dock.keys { style::accent } else { style::dim }),
            Space::new().width(Length::Fill),
            button(text("close").size(12))
                .style(button::text)
                .padding([0, 6])
                .on_press(Message::CloseExplorer),
        ]
        .align_y(iced::Alignment::Center)
        .padding([4, 12]);
        container(column![head, explorer])
            .width(Length::Fixed(self.dock.width_in(self.screen)))
            .height(Length::Fill)
            .style(style::page)
            .into()
    }
}
