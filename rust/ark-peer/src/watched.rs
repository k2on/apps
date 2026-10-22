//! A maintained view held by its UI (§1.10): its rows kept as the UI's own
//! type and patched as the view moves, so that nothing reads a list again.
//!
//! A [`Watched`] is a view of a function by name and input's bytes, opened
//! for whoever is signed in, and its rows decoded by the reader it was given.
//! [`Watched::settle`] takes what the view did and splices it into the rows;
//! the rows are read whole when the view was made again (a reset), when it
//! was opened for another login (it is opened again for this one), or when
//! a query that refused at open answers now — a page that is not yours is
//! no rows rather than an error, and is asked again when anything moved.

use ark::peer::Who;

use crate::{Patch, Peer, Update};

/// One view and the rows a UI holds of it.
pub struct Watched<T> {
    name: String,
    input: Vec<u8>,
    who: Who,
    /// The view, or — while its query refuses — none, which is no rows.
    view: Option<usize>,
    read: fn(&[u8]) -> T,
    pub rows: Vec<T>,
}

impl<T> Watched<T> {
    /// A view of `name` on `input`, its rows read by `read`, hydrated once;
    /// refused, no rows until it answers.
    pub fn open(peer: &mut Peer, name: &str, input: Vec<u8>, read: fn(&[u8]) -> T) -> Watched<T> {
        let view = peer.view(name, &input).ok();
        Watched::made(peer, name, input, read, view)
    }

    /// [`Watched::open`], or the query's refusal.
    pub fn try_open(peer: &mut Peer, name: &str, input: Vec<u8>, read: fn(&[u8]) -> T) -> Result<Watched<T>, String> {
        let view = peer.view(name, &input)?;
        Ok(Watched::made(peer, name, input, read, Some(view)))
    }

    fn made(peer: &Peer, name: &str, input: Vec<u8>, read: fn(&[u8]) -> T, view: Option<usize>) -> Watched<T> {
        let mut w = Watched {
            name: name.into(),
            input,
            who: peer.me().clone(),
            view,
            read,
            rows: vec![],
        };
        w.fill(peer);
        w
    }

    /// Whether it is the view of `name` on `input`.
    pub fn is(&self, name: &str, input: &[u8]) -> bool {
        self.name == name && self.input == input
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Held no more.
    pub fn close(self, peer: &mut Peer) {
        if let Some(v) = self.view {
            peer.unwatch(v);
        }
    }

    /// The view's number while its query answers.
    pub fn view(&self) -> Option<usize> {
        self.view
    }

    /// What the view did since the last settle, spliced into the rows, or
    /// the rows read whole (`Reset`). `moved` is whether anything moved:
    /// what a refused query is asked again on.
    pub fn settle(&mut self, peer: &mut Peer, moved: bool) -> Update {
        let update = self.take(peer, moved);
        self.apply(peer, &update);
        update
    }

    /// What the view did since the last settle, the rows not yet told: a
    /// view opened for another login, or refused, opened again (`Reset`).
    pub fn take(&mut self, peer: &mut Peer, moved: bool) -> Update {
        match self.view {
            Some(v) if self.who == *peer.me() => peer.update(v),
            None if !moved && self.who == *peer.me() => Update::Unchanged,
            was => {
                if let Some(v) = was {
                    peer.unwatch(v);
                }
                self.who = peer.me().clone();
                self.view = peer.view(&self.name, &self.input).ok();
                match (was, self.view) {
                    (None, None) => Update::Unchanged,
                    _ => Update::Reset,
                }
            }
        }
    }

    /// What [`Watched::take`] said, onto the rows.
    pub fn apply(&mut self, peer: &Peer, update: &Update) {
        match update {
            Update::Unchanged => {}
            Update::Patched(ps) => splice(&mut self.rows, ps, self.read),
            Update::Reset => self.fill(peer),
        }
    }

    fn fill(&mut self, peer: &Peer) {
        let rows = self.view.map(|v| peer.rows(v)).unwrap_or_default();
        self.rows = rows.iter().map(|b| (self.read)(b)).collect();
    }
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
