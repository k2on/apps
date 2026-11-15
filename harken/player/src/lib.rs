//! harken's players' adapters (§1.9): what drives the domain's reconciler
//! (`harken_domain::player`) over a device's own peer — [`Player`]: its
//! presses, its plays authored in the log, the one view a phone draws,
//! alone or joined — and the peers' configuration ([`config`]). The rules
//! are the domain's; this device's own sound is harken-app's
//! (`harken_app::sound`).

pub mod config;
pub mod player;

pub use player::{Player, Stats};
