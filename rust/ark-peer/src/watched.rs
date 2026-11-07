//! A maintained view held by its UI (§1.10): its rows kept as the UI's own
//! type and patched as the view moves, so that nothing reads a list again.
//!
//! A [`Watched`] is a view the peer keeps ([`Peer::view`]) of a function on
//! an input, by its typed handle (`ark::domain::Func::on`), and its rows
//! decoded as the UI holds them: the view's own rows, or any type made from
//! one.
//! [`Watched::settle`] takes what the view did and splices it into the rows.
//! What a view does is always its patches: one the peer made again (it could
//! not be kept up, a login came, or a query that refused answers now) says
//! its rows as they were removed and as they are inserted ([`again`]) — a
//! page that is not yours is no rows rather than an error.

use ark::domain::On;
use ark::format::Encode;

use crate::{Patch, Peer};

/// One view and the rows a UI holds of it.
pub struct Watched<T> {
    name: &'static str,
    input: Vec<u8>,
    view: usize,
    read: fn(&[u8]) -> T,
    pub rows: Vec<T>,
}

impl<T> Watched<T> {
    /// The view of a function on its input (`budget::MONTH.on(&month)`),
    /// hydrated once; refused, no rows until it answers.
    pub fn open<O: Encode>(peer: &mut Peer, on: On<O>) -> Watched<T>
    where
        T: From<O>,
    {
        let (view, _) = peer.view(on.name, &on.input);
        let read = |b: &[u8]| T::from(O::from_bytes(b).expect("a view's row is its type's encoding"));
        let rows = peer.rows(view).unwrap_or_default().iter().map(|b| read(b)).collect();
        Watched {
            name: on.name,
            input: on.input,
            view,
            read,
            rows,
        }
    }

    /// Whether it is the view of `on`.
    pub fn is<O>(&self, on: &On<O>) -> bool {
        self.name == on.name && self.input == on.input
    }

    /// The view of `on`: kept when it is that one already, else opened in
    /// its place, the one it was closed. Whether it changed.
    pub fn set<O: Encode>(&mut self, peer: &mut Peer, on: On<O>) -> bool
    where
        T: From<O>,
    {
        let changed = !self.is(&on);
        if changed {
            std::mem::replace(self, Watched::open(peer, on)).close(peer);
        }
        changed
    }

    /// A slot brought to the view of `on` ([`Watched::set`]), or to none,
    /// closing what it held. Whether it changed.
    pub fn want<O: Encode>(slot: &mut Option<Watched<T>>, peer: &mut Peer, on: Option<On<O>>) -> bool
    where
        T: From<O>,
    {
        match (slot.as_mut(), on) {
            (Some(w), Some(on)) => w.set(peer, on),
            (None, Some(on)) => slot.replace(Watched::open(peer, on)).is_none(),
            (_, None) => slot.take().map(|w| w.close(peer)).is_some(),
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    /// Held no more.
    pub fn close(self, peer: &mut Peer) {
        peer.unwatch(self.view);
    }

    /// The view's number.
    pub fn view(&self) -> usize {
        self.view
    }

    /// What the view did since the last settle, spliced into the rows.
    pub fn settle(&mut self, peer: &mut Peer) -> Vec<Patch> {
        let patches = peer.update(self.view);
        splice(&mut self.rows, &patches, self.read);
        patches
    }
}

/// What takes a list of `old` rows to `rows`: each removed, from the back,
/// and each of `rows` inserted — a view made again, as its patches.
pub fn again(old: usize, rows: Vec<Vec<u8>>) -> Vec<Patch> {
    let gone = (0..old).rev().map(|at| Patch::Remove { at });
    gone.chain(rows.into_iter().enumerate().map(|(at, node)| Patch::Insert { at, node }))
        .collect()
}

/// The patches, in order, onto the decoded list: `ark::plan::splice` with
/// the decoding said, because a row here is the UI's type.
pub fn splice<T>(rows: &mut Vec<T>, patches: &[Patch], read: fn(&[u8]) -> T) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => rows.insert((*at).min(rows.len()), read(node)),
            Patch::Remove { at } if *at < rows.len() => drop(rows.remove(*at)),
            Patch::Update { at, node } if *at < rows.len() => rows[*at] = read(node),
            _ => {}
        }
    }
}
