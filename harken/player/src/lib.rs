//! harken's players' adapters (§1.9): what drives the domain's reconciler
//! (`harken_domain::player`) over something — [`Player`], over a device's
//! own peer (its presses, its plays authored in the log, the one view a
//! phone draws, alone or joined); [`MediaSound`], the media player module
//! as a `Sound`; and the peers' configuration ([`config`]). The rules are
//! the domain's.

pub mod config;
pub mod media;
pub mod player;

pub use media::{remote_press, Knob, MediaSound, Repeat};
pub use player::{Player, Stats};
