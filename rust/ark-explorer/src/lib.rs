//! `ark-explorer`: a database explorer for any ArkDB module, as one iced
//! component (`docs/plan-guards.md` D4).
//!
//! It knows nothing about any domain. A host hands it, on every call, a
//! [`Source`] — a `&dyn Store` with dynamic rows, the schema, the module,
//! a [`LogView`] and who the console reads as — and a [`Writer`]; it draws
//! the tables, a table's rows a page at a time with every column the store
//! has, a cell editor and a row delete, a read-only query console, and the
//! log, and it decides what each edit becomes ([`edit`]). Hosted twice:
//!
//! - **by an app's client** over its own replica, writing only through the
//!   CRUD mutations the domain exposes (`r.crud::<T>()`), authored as the
//!   signed-in person like any button — a client has no raw writer, connected
//!   or alone, so what it does is pushed and judged like any offline work;
//! - **by a server's admin page**, compiled to wasm as a browser peer is,
//!   over the authority's store, writing through the domain's CRUD by default
//!   and raw — `ark.put_row`, `ark.delete_row`, as the authority — when the
//!   visible switch says so ([`wire`] is what the page and the server say).
//!
//! The keyboard is `arkui`'s vim grammar: `j`/`k` and `h`/`l`, counts,
//! `gg`/`G`; `<Enter>` opens a table or edits a cell, `e` edits, `x`
//! deletes a row, `R` flips the raw switch, `<Tab>` (or `1`, `2`, `3`) goes
//! between the tables, the log and the console, and `<Esc>` goes back —
//! out of the explorer from the top. The colours are the host's: it
//! installs its palette with `arkui::theme::install` (or hands one to
//! [`Theme::install`]), and nothing here writes one down.
//!
//! Without the `ui` feature only what needs no window is built — [`data`],
//! [`edit`], [`wire`] — which is what a server links to answer its admin
//! page.

pub mod data;
pub mod edit;
pub mod wire;

#[cfg(feature = "ui")]
pub mod admin;
#[cfg(feature = "ui")]
pub mod model;
#[cfg(feature = "ui")]
pub mod view;

pub use data::{crud_of, function_names, Connection, CrudVerbs, Line, LogView, NoLog, Source, Verified, Writer};
pub use edit::Route;
#[cfg(feature = "ui")]
pub use model::{Explorer, Msg, Outcome, Screen};

/// The host's palette, for a host that has not installed one: the explorer
/// draws with whatever `arkui::theme` holds, and this is how a host that is
/// only an explorer — the admin page — says what that is.
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
