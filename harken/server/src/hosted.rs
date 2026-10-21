//! The domain the hub runs (§1.16): harken's, in plain Rust, as this build
//! compiled it — its functions registered by name, its schema derived from
//! its structs. What a release changes is this build: there is no module to
//! load at run time, and an intent naming a function this build does not
//! register is held, as any unknown name is.

use std::sync::Arc;

use ark::domain::Domain;

/// The domain assembled, and the same as a peer's per-space module.
#[derive(Clone)]
pub struct Hosted {
    pub domain: Arc<Domain>,
    pub module: ark_peer::Module,
}

impl Hosted {
    /// harken's own domain, as this build compiled it.
    pub fn harken() -> Hosted {
        Hosted::of(harken_domain::shared())
    }

    pub fn of(domain: Arc<Domain>) -> Hosted {
        Hosted {
            module: ark_peer::Module::of(&domain),
            domain,
        }
    }

    /// A registered function's name, `library.add_song`, as an entry
    /// calls it: there when this build registers it.
    pub fn get(&self, name: &str) -> Option<String> {
        self.domain.get(name).map(|_| name.to_string())
    }

    /// [`Hosted::get`] of a function that must be there.
    pub fn func(&self, name: &str) -> String {
        self.get(name)
            .unwrap_or_else(|| panic!("no function {name}"))
    }

    /// What this build runs, in a word: its domain's schema hash and
    /// version.
    pub fn version(&self) -> String {
        format!("{} {}", self.domain.schema_hash(), self.domain.version)
    }
}
