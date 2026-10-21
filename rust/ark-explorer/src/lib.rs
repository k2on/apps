//! `ark-explorer`: a generic browser for any ArkDB module, as one iced
//! component (`docs/plan-guards.md` D4).
//!
//! It knows nothing about any domain: everything it draws is driven by the
//! module and its types (`docs/plan-v5.md` §1.1, [`value`]). A host hands
//! it, on every call, a [`Source`] — the store it holds, the module and the
//! library of its checked functions, a [`LogView`] and who a query reads
//! as — and a [`Writer`]; and it shows ([`model`]):
//!
//! - **tables as grids**: the columns from the schema, the rows from the
//!   store, sortable; a cell edit and a row delete routed by [`edit`] to the
//!   CRUD the module exposes or the authority's raw write;
//! - **functions as a call screen by name**: a form whose fields are the
//!   input type's, checked before calling with `parse`'s own messages
//!   ([`form`]), a mutation authored through the host's writer as a button
//!   would author it, a query's answer as a grid;
//! - **the log**: its entries with their facts and verdicts, the host's
//!   pending intents among them, and the connections.
//!
//! Hosted twice: by an app's client over its own replica (writing only
//! through the domain's mutations, as the signed-in person), and by a
//! server's admin page ([`admin`], the `ark-admin` binary), compiled to wasm
//! and speaking `ark-server`'s `/admin/api` ([`wire`]).
//!
//! The keyboard is `arkui`'s vim grammar: `j`/`k` and `h`/`l`, counts,
//! `gg`/`G`; `<Enter>` opens a table or edits a cell, `e` edits, `x`
//! deletes a row, `s` sorts by the cursor's column, `R` flips the raw
//! switch, `<Tab>` goes between the tables, the log and the functions (`f`
//! goes straight to them), and `<Esc>` goes back — out of the explorer from
//! the top. On a call screen `j`/`k` walk its fields, `<Enter>` toggles,
//! chooses, adds or opens an id's picker (where `/` filters), `r` calls.
//!
//! The colours are the host's: it installs its palette with
//! `arkui::theme::install` (or hands one to [`Theme::install`]).
//!
//! Without the `ui` feature only what needs no window is built — [`data`],
//! [`edit`], [`form`], [`value`], [`wire`] — which is what a server links
//! to answer its admin page.

pub mod data;
pub mod edit;
pub mod form;
pub mod value;
pub mod wire;

#[cfg(feature = "ui")]
pub mod admin;
#[cfg(feature = "ui")]
pub mod model;
#[cfg(feature = "ui")]
pub mod view;

pub use data::{crud_of, function_names, Ask, Catalog, Connection, CrudVerbs, Described, Line, LogView, NoLog, Source, Ticket, Verdict, Writer};
pub use edit::Route;
#[cfg(feature = "ui")]
pub use model::{Explorer, Msg, Outcome, Screen};

/// The host's palette, for a host that has not installed one — the admin
/// page, which is only an explorer.
#[cfg(feature = "ui")]
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub palette: arkui::theme::Palette,
}

#[cfg(feature = "ui")]
impl Theme {
    /// Make it the palette every style asks, once, before the first frame.
    /// A host that installed its own already keeps it.
    pub fn install(self) -> bool {
        arkui::theme::install(self.palette)
    }
}

#[cfg(all(test, feature = "ui"))]
mod tests;
