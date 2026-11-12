//! A hall: an app's server in this process — the hub a server runs over
//! its domain — over memory, which devices dial without a socket. Its
//! devices are pumped by hand ([`Hall::carry`]), so a test says when what
//! was said has been answered. What an app's tests sync two phones
//! through.

use std::sync::{Arc, Mutex, Weak};

use ark::peer::Who;
use ark_peer::{now_ms, Config, Handle, Memory};

use crate::{Login, Peer, Refusal};

/// Who a hall's token, `user/session` or `user/session/role,role`, is.
fn token(t: &str) -> Option<Who> {
    let mut parts = t.splitn(3, '/');
    let (actor, session) = (parts.next()?, parts.next()?);
    let roles = parts
        .next()
        .map_or(vec![], |r| r.split(',').filter(|r| !r.is_empty()).map(String::from).collect());
    Some(Who {
        actor: actor.into(),
        session: session.into(),
        roles,
    })
}

/// An app's server in the process, and the devices that dial it.
pub struct Hall {
    config: Config,
    hub: Handle,
    devices: Mutex<Vec<Weak<Peer>>>,
}

impl Hall {
    /// A server of the domain `config` names, over memory.
    pub fn new(config: Config) -> Arc<Hall> {
        let hub = Handle::spawn(ark_peer::Peer::serve(config.clone(), Box::new(Memory::default()), now_ms()));
        Arc::new(Hall {
            config,
            hub,
            devices: Mutex::default(),
        })
    }

    /// A device on the disk at `dir`, signed in as `user` on the session
    /// `session` holding `roles` (`bank` is the importer's), dialling this
    /// hall.
    pub fn device(&self, dir: String, user: String, session: String, roles: Vec<String>) -> Result<Arc<Peer>, Refusal> {
        let login = Login {
            token: format!("{user}/{session}/{}", roles.join(",")),
            session,
            user,
            name: String::new(),
            email: String::new(),
            roles,
            expires_ms: 0,
            picture: String::new(),
        };
        let p = Peer::open(self.config.clone(), &dir, Some(login))?;
        p.by_hand();
        p.connect_with("hall", self.hub.dialer(Arc::new(token)));
        self.devices.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::downgrade(&p));
        self.carry();
        Ok(p)
    }

    /// Everything every device said answered and taken in, until nobody
    /// has anything more to say.
    pub fn carry(&self) {
        let devices: Vec<Arc<Peer>> = self
            .devices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        let mut still = 0;
        for _ in 0..64 {
            // Whatever the hub had, answered: its thread runs this after.
            let _ = self.hub.with(|_| ());
            let moved = devices.iter().fold(false, |m, p| p.pump() | m);
            still = if moved { 0 } else { still + 1 };
            if still == 2 {
                return;
            }
        }
    }
}
