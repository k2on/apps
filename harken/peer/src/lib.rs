//! harken's peer with no screen, and the adapters it hosts.
//!
//! - [`headless`]: an `ark_peer::Peer` and its link, driven by JSON lines on
//!   stdin — what the process fleet and the VM test run as a device
//!   (`docs/plan-fleet.md` §1), and `harken-peer`'s `main`;
//! - [`house`]: Home Assistant's media players as devices of a server's
//!   listening sessions (`harken-peer --house`), each one a volatile machine
//!   on a socket of its own, reconciled by `harken-player` as a phone is.
//!
//! Neither is the server's: what a peer here knows of a server is its
//! socket, its sign-in, and — for the house — `/house/sign-in`, where the
//! house's key buys a speaker's login in an account's session.

pub mod headless;
pub mod house;

use std::sync::Arc;

use ark::domain::Domain;

/// harken's domain as this build compiled it (§1.16), and the same as a
/// peer's per-space module.
#[derive(Clone)]
pub struct Hosted {
    pub domain: Arc<Domain>,
    pub module: ark_peer::Module,
}

impl Hosted {
    pub fn harken() -> Hosted {
        let domain = harken_domain::shared();
        Hosted {
            module: ark_peer::Module::of(&domain),
            domain,
        }
    }

    /// A registered function's name, as an entry calls it: there when this
    /// build registers it.
    pub fn get(&self, name: &str) -> Option<String> {
        self.domain.get(name).map(|_| name.to_string())
    }
}
