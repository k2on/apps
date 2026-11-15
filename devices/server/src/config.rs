//! The household's config: the policies and who each applies to, as Nix
//! declares them (a JSON file, `DEVICES_POLICIES`), held by the server.
//! At every start the server signs its own account in — `config`, holding
//! the admin's role — and writes what the file says that the store does not
//! hold: each policy that differs, each assignment that differs, then away
//! with the assignments and the policies the file no longer names. What is
//! declared is the whole truth: a policy removed from Nix is removed here.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use ark::peer::Who;
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::{Handle, Memory, Peer, Standing};
use devices_core::{Assign, Assignment, DevicesChange, Policy, RemovePolicy, SetPolicy};
use serde::Deserialize;

/// The server's own account, which writes the config.
pub const ACCOUNT: &str = "config";
/// Its device, as its session says.
pub const DEVICE: &str = "the household's config";

/// How long a change waits to be confirmed, and the link to come up.
const WITHIN: Duration = Duration::from_secs(30);

/// What the file declares.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    #[serde(default)]
    pub policies: Vec<Policy>,
    /// Who each policy applies to: a person's user name, and the policy's.
    #[serde(default)]
    pub assignments: BTreeMap<String, String>,
}

impl Declared {
    /// The file at `path`.
    pub fn read(path: &Path) -> Result<Declared> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let d: Declared = serde_json::from_str(&text).with_context(|| format!("reading {} as the household's policies", path.display()))?;
        for (user, policy) in &d.assignments {
            if !d.policies.iter().any(|p| &p.name == policy) {
                bail!("{}: {user} is assigned {policy}, which it does not declare", path.display());
            }
        }
        Ok(d)
    }
}

/// The changes from what the store holds to what is declared, in the order
/// they are made: policies written first (an assignment names one), then
/// assignments, then what is no longer declared taken away.
pub fn changes(declared: &Declared, policies: &[Policy], assignments: &[Assignment]) -> Vec<DevicesChange> {
    let mut out = vec![];
    for p in &declared.policies {
        if policies.iter().find(|h| h.name == p.name) != Some(p) {
            out.push(DevicesChange::SetPolicy(SetPolicy { policy: p.clone() }));
        }
    }
    for (user, policy) in &declared.assignments {
        if !assignments.iter().any(|a| &a.user == user && &a.policy == policy) {
            out.push(DevicesChange::Assign(Assign {
                user: user.clone(),
                policy: policy.clone(),
            }));
        }
    }
    for a in assignments {
        if !declared.assignments.contains_key(&a.user) {
            out.push(DevicesChange::Assign(Assign {
                user: a.user.clone(),
                policy: String::new(),
            }));
        }
    }
    for p in policies {
        if !declared.policies.iter().any(|d| d.name == p.name) {
            out.push(DevicesChange::RemovePolicy(RemovePolicy { name: p.name.clone() }));
        }
    }
    out
}

/// `declared` held by the hub: the config's account signed in (`auth`), its
/// own peer dialling `hub` in this process, the store read, each change
/// written and confirmed. How many it made.
pub fn hold(declared: &Declared, auth: &Auth, hub: &Handle) -> Result<usize> {
    let account = Account {
        id: ACCOUNT.into(),
        name: "Config".into(),
        roles: vec![ark::auth::ADMIN.into()],
        ..Account::default()
    };
    let login = auth.own(&account, DEVICE).map_err(|e| anyhow!("signing the config in: {e}"))?;
    let who = Who {
        actor: login.user.id.clone(),
        session: login.session.clone(),
        roles: vec![ark::auth::ADMIN.into()],
    };
    let mut peer = Peer::open(
        ark_peer::Config::of(devices_core::shared()),
        who.clone(),
        Box::new(Memory::default()),
        false,
    );
    peer.sign_in(who.clone());
    let token = login.token.clone();
    let dial = hub.dialer(Arc::new(move |t: &str| (t == token).then(|| who.clone())));
    peer.set_token(Some(login.token));
    let (wake, rx) = ark_peer::link::bell();
    peer.set_waker(wake);
    // What the hub holds now: the config reads the store only once its own
    // replica has caught up to it (linked alone is said at the handshake,
    // before the log arrives).
    let head = hub
        .with(|p| p.machine(ark::schema::Space::Durable).map_or(0, |m| m.log().head().0))
        .unwrap_or(0);
    peer.connect_with("", dial);
    let caught_up = ark_peer::link::until(WITHIN, Duration::from_millis(0), || {
        ark_peer::link::wait(&peer, &rx, Duration::from_millis(50));
        drop(peer.pump());
        let st = peer.status();
        st.linked && st.pending == 0 && peer.cursor() >= head
    });
    if !caught_up {
        bail!("the config did not catch up with the hub within {}s", WITHIN.as_secs());
    }
    let policies = peer
        .answer(devices_core::every_policy, ())
        .map_err(|e| anyhow!("reading the policies: {e}"))?;
    let assignments = peer
        .answer(devices_core::every_assignment, ())
        .map_err(|e| anyhow!("reading the assignments: {e}"))?;
    let todo = changes(declared, &policies, &assignments);
    for c in &todo {
        author(&mut peer, &rx, c)?;
    }
    Ok(todo.len())
}

/// `change` written and confirmed by the hub, or why not.
fn author(peer: &mut Peer, rx: &std::sync::mpsc::Receiver<()>, change: &DevicesChange) -> Result<()> {
    let id = peer
        .mutate_bytes(change.name(), change.input())
        .map_err(|why| anyhow!("{}: refused here: {why}", change.name()))?;
    let mut why = None;
    ark_peer::link::until(WITHIN, Duration::from_millis(0), || {
        ark_peer::link::wait(peer, rx, Duration::from_millis(50));
        drop(peer.pump());
        for (r, w) in peer.take_rejections() {
            if r == id {
                why = Some(w);
            }
        }
        !matches!(peer.standing(&id), Standing::Pending)
    });
    match (peer.standing(&id), why) {
        (Standing::Confirmed, _) => Ok(()),
        (Standing::Rejected(w), _) | (_, Some(w)) => bail!("{}: the hub refused it: {w}", change.name()),
        _ => bail!("{}: the hub did not confirm it within {}s", change.name(), WITHIN.as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devices_core::VpnMode;

    fn policy(name: &str) -> Policy {
        Policy {
            name: name.into(),
            vpn: VpnMode::AlwaysOn,
            vpn_app: "com.tailscale.ipn".into(),
            vpn_lockdown: true,
            ..Default::default()
        }
    }

    fn assignment(user: &str, policy: &str) -> Assignment {
        Assignment {
            user: user.into(),
            policy: policy.into(),
        }
    }

    /// What is declared is written, what is held already is left, and what
    /// is no longer declared goes — assignments before the policies they
    /// name. Falsified by a held policy written again: a restart wrote
    /// every policy anew.
    #[test]
    fn the_store_is_brought_to_what_is_declared() {
        let mut changed = policy("kids");
        changed.vpn_lockdown = false;
        let declared = Declared {
            policies: vec![policy("adults"), policy("kids")],
            assignments: [("ann".to_string(), "kids".to_string()), ("bob".to_string(), "adults".to_string())].into(),
        };
        let held_policies = vec![policy("adults"), changed, policy("old")];
        let held = vec![assignment("ann", "kids"), assignment("cy", "old")];
        let names: Vec<(&str, String)> = changes(&declared, &held_policies, &held)
            .iter()
            .map(|c| match c {
                DevicesChange::SetPolicy(s) => ("set", s.policy.name.clone()),
                DevicesChange::Assign(a) => ("assign", format!("{}={}", a.user, a.policy)),
                DevicesChange::RemovePolicy(r) => ("remove", r.name.clone()),
                DevicesChange::Report(_) => ("report", String::new()),
            })
            .collect();
        assert_eq!(
            names,
            vec![
                ("set", "kids".into()),
                ("assign", "bob=adults".into()),
                ("assign", "cy=".into()),
                ("remove", "old".into()),
            ]
        );
        assert!(changes(&declared, &declared.policies, &[assignment("ann", "kids"), assignment("bob", "adults")]).is_empty());
    }

    /// The file as Nix writes it (`builtins.toJSON`), and an assignment to a
    /// policy it does not declare refused before anything is written.
    #[test]
    fn the_file_reads_as_nix_writes_it() {
        let dir = std::env::temp_dir().join(format!("devices-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("policies.json");
        std::fs::write(
            &file,
            r#"{"assignments":{"ann":"kids"},"policies":[{"name":"kids","vpn":"AlwaysOn","vpn_app":"com.tailscale.ipn","vpn_lockdown":true,
               "restrictions":[{"name":"no_config_vpn","on":true}],"uninstall_blocked":[],"user_control_disabled":[],
               "permissions":[{"app":"io.heckel.ntfy","permission":"android.permission.POST_NOTIFICATIONS","grant":"Granted"}],
               "camera_disabled":null,"screen_capture_disabled":false}]}"#,
        )
        .unwrap();
        let d = Declared::read(&file).unwrap();
        assert_eq!(d.policies[0].restrictions[0].name, "no_config_vpn");
        assert_eq!(d.assignments.get("ann").map(String::as_str), Some("kids"));
        std::fs::write(&file, r#"{"assignments":{"ann":"nope"},"policies":[]}"#).unwrap();
        assert!(Declared::read(&file).unwrap_err().to_string().contains("does not declare"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
