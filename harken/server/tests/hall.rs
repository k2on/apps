//! A hall: the hub `harken-server` runs (`hub_config`'s, `config::hub`) in
//! the process, over memory, and two devices of one account that dial it
//! without a socket, each a phone's peer (`config::peer`, saying what it is
//! in its volatile `Hello`). Both stand in the account's listening session,
//! and one's press is the other's state once carried. The devices are
//! pumped by hand ([`Hall::carry`]), so the test says when what was said has
//! been answered. (Moved here from the hand-written phones' library, which
//! the generated one replaced.)

use std::sync::Arc;

use ark::peer::Who;
use ark_ffi::Login;
use ark_ffi::Peer;
use ark_peer::{now_ms, Handle, Memory};
use harken_domain::host::Listening;
use harken_server::config;

/// Who a hall's token, `user/session`, is.
fn token(t: &str) -> Option<Who> {
    let (actor, session) = t.split_once('/')?;
    Some(Who {
        actor: actor.into(),
        session: session.into(),
        roles: vec![],
    })
}

/// The hub and the devices that dial it.
struct Hall {
    hub: Handle,
    devices: Vec<Arc<Peer>>,
}

impl Hall {
    fn new() -> Hall {
        let c = config::hub(&harken_domain::shared(), None);
        Hall {
            hub: Handle::spawn(ark_peer::Peer::serve(
                c,
                Box::new(Memory::default()),
                now_ms(),
            )),
            devices: vec![],
        }
    }

    /// A phone on the disk at `dir`, signed in as `user` on the session
    /// `session` (its id in the listening session), called `name`.
    fn device(
        &mut self,
        dir: &std::path::Path,
        user: &str,
        session: &str,
        name: &str,
    ) -> Arc<Peer> {
        let login = Login {
            token: format!("{user}/{session}"),
            session: session.into(),
            user: user.into(),
            name: String::new(),
            email: String::new(),
            roles: vec![],
            expires_ms: 0,
            picture: String::new(),
        };
        let said = config::device(true, "phone", name);
        let c = config::peer(&harken_domain::shared(), said, config::THROTTLE_MS);
        let p = Peer::open(c, dir.to_str().unwrap(), Some(login)).unwrap();
        p.by_hand();
        p.connect_with("hall", self.hub.dialer(Arc::new(token)));
        self.devices.push(p.clone());
        self.carry();
        p
    }

    /// Everything every device said answered and taken in, until nobody
    /// has anything more to say.
    fn carry(&self) {
        let mut still = 0;
        for _ in 0..64 {
            // Whatever the hub had, answered: its thread runs this after.
            let _ = self.hub.with(|_| ());
            let moved = self.devices.iter().fold(false, |m, p| p.pump() | m);
            still = if moved { 0 } else { still + 1 };
            if still == 2 {
                return;
            }
        }
    }

    /// `user`'s listening session as the hall holds it.
    fn listening(&self, user: &str) -> Listening {
        let user = user.to_string();
        self.hub
            .with(move |p| p.live.as_ref().map(|l| Listening::of(&l.store, &user)))
            .flatten()
            .unwrap_or_default()
    }
}

/// The account's listening session as `p` holds it.
fn listening(p: &Peer) -> Listening {
    p.with(|p| match &p.live {
        Some(live) => Listening::of(&live.store, &p.me().actor),
        None => Listening::default(),
    })
}

/// Falsified by `carry` returning before the hub had answered (no
/// barrier): the second device never saw the first standing in the session.
#[test]
fn two_devices_share_a_session() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut hall = Hall::new();
    let phone = hall.device(a.path(), "alice", "phone", "Phone");
    let laptop = hall.device(b.path(), "alice", "laptop", "Laptop");
    hall.carry();
    let ids: Vec<String> = listening(&laptop)
        .devices
        .iter()
        .map(|d| d.id.clone())
        .collect();
    assert_eq!(ids, ["laptop", "phone"]);
    let track = r#"{"album":"","creator":"","duration_ms":1000,"file":"a.flac","id":"00000000000000000000000000000001","title":"A"}"#;
    phone
        .call(
            "start".into(),
            format!(r#"{{"at":0,"from":"","position":0,"queue":[{track}]}}"#),
        )
        .unwrap();
    hall.carry();
    let s = listening(&laptop).session;
    let output = serde_json::to_value(&s).unwrap()["output"].clone();
    assert_eq!(output["Device"]["id"], "phone", "{output}");
    assert_eq!(phone.status().pending, 0);
    assert_eq!(
        hall.listening("alice").session,
        s,
        "the hall holds what the laptop does"
    );
}
