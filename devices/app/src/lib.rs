//! devices as an Ark app (ui/docs/app.md): a phone held to its household's
//! policy as its device owner (ark-dpc). Its host (`host::Devices`) runs the
//! shell, the status screen — which does the work, woken by the OS with no
//! screen as readily as asked from it — and the notices; `policy` turns the
//! domain's rows into ark-dpc's policy and what the phone read into a report.

pub mod host;
pub mod notices;
pub mod policy;
pub mod routes;
pub mod shell;
pub mod status;
pub mod welcome;

pub use routes::Route;

#[cfg(test)]
mod tests;
