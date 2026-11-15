//! devices' part of its host over a peer of its domain in memory, signed in
//! as ann on her phone, with the device modules' fakes: what a phone's host
//! runs when the OS wakes it.

use ark_app::host::{Hosting, Place, Session};
use ark_app::modules::dpc;
use ark_app::{Modules, Woken};
use ark_peer::store::Store;
use ark_peer::{Config, Memory, Peer};
use devices_core::*;

use crate::host::Devices;
use crate::status::{Status, StatusPage};

const TAILSCALE: &str = "com.tailscale.ipn";

/// A phone's place: the replica, its views, the fakes as its modules.
struct Phone {
    peer: Peer,
    store: Store<DevicesView>,
    modules: Modules,
}

impl Place<DevicesView, DevicesChange> for Phone {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<DevicesView>)) {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        1_793_000_000_000
    }
    fn send(&mut self, change: DevicesChange) -> Result<(), String> {
        let r = self.store.sent(self.peer.mutate_bytes(change.name(), change.input()));
        self.store.settle(&mut self.peer);
        r.map(drop)
    }
    fn hold(&mut self, key: DevicesView) {
        self.store.hold(&mut self.peer, key);
        self.store.settle(&mut self.peer);
    }
    fn release(&mut self, key: &DevicesView) {
        self.store.release(&mut self.peer, key)
    }
    fn modules(&self) -> Modules {
        self.modules.clone()
    }
    fn session(&self) -> Session {
        Session {
            user: "ann".into(),
            name: "Ann".into(),
            server: "devices.example.com".into(),
            ..Default::default()
        }
    }
}

struct Fakes {
    dpc: ark_dpc::Fake,
    background: ark_background::Fake,
    notifications: ark_notifications::Fake,
}

/// Ann's phone, her own authority in memory, holding the admin's role so
/// the test can play the server too; `dpc` is the phone's device owner.
fn phone(dpc: ark_dpc::Fake) -> (Phone, Fakes) {
    let mut disk = Memory::default();
    ark_peer::local::preset(&mut disk, |r| (r.actor, r.session) = (Some("ann".into()), "phone".into())).unwrap();
    let mut peer = Peer::open_as(Config::of(devices_core::shared()), Box::new(disk), None);
    peer.set_roles([ark::auth::ADMIN]);
    peer.alone();
    let fakes = Fakes {
        dpc,
        background: ark_background::Fake::new(),
        notifications: ark_notifications::Fake::new(),
    };
    // The distributor on the tailnet (ntfy) hands out an endpoint.
    fakes
        .notifications
        .hand_out(Some(ark_app::modules::notifications::PushEndpoint::UnifiedPush {
            url: "https://ntfy.example.com/up123".into(),
            p256dh: "key".into(),
            auth: "secret".into(),
        }));
    let mut modules = Modules::new();
    fakes.dpc.register(&mut modules);
    fakes.background.register(&mut modules);
    fakes.notifications.register(&mut modules);
    (
        Phone {
            peer,
            store: Store::new(),
            modules,
        },
        fakes,
    )
}

impl Phone {
    /// What the server writes.
    fn server(&mut self, change: DevicesChange) {
        self.peer.mutate_bytes(change.name(), change.input()).unwrap();
        self.store.settle(&mut self.peer);
    }

    /// This phone's last report, as the status screen holds it.
    fn report(&self) -> Option<Report> {
        self.store.rows::<Report>(&DevicesView::Own).first().cloned()
    }
}

fn kids() -> Policy {
    Policy {
        name: "kids".into(),
        vpn: VpnMode::AlwaysOn,
        vpn_app: TAILSCALE.into(),
        vpn_lockdown: true,
        restrictions: vec![Setting {
            name: "no_config_vpn".into(),
            on: true,
        }],
        user_control_disabled: vec![Setting {
            name: TAILSCALE.into(),
            on: true,
        }],
        ..Default::default()
    }
}

fn set(p: Policy) -> DevicesChange {
    DevicesChange::SetPolicy(SetPolicy { policy: p })
}

fn assign(user: &str, policy: &str) -> DevicesChange {
    DevicesChange::Assign(Assign {
        user: user.into(),
        policy: policy.into(),
    })
}

fn open(phone: &mut Phone) -> Hosting<Devices> {
    let session = phone.session();
    Hosting::<Devices>::open(&session, phone)
}

/// The OS wakes the phone: the status screen syncs, reads the phone, makes
/// what ann's policy asks — the VPN first — reads it again and reports, and
/// the first run asks the OS to wake it every fifteen minutes and registers
/// for push. Falsified by `App::woken` giving the status screen no message:
/// nothing was made and no report sent.
#[test]
fn a_wake_holds_the_phone_to_its_persons_policy_and_reports() {
    let (mut phone, fakes) = phone(ark_dpc::Fake::new());
    phone.server(set(kids()));
    phone.server(assign("ann", "kids"));
    let mut h = open(&mut phone);
    assert!(h.woken(Woken::Sync, &mut phone), "the status screen took it");
    assert!(h.idle(&phone), "done: the process may go");
    let s = fakes.dpc.state();
    assert_eq!((s.always_on_vpn.as_deref(), s.lockdown), (Some(TAILSCALE), true));
    assert_eq!(s.restrictions, vec!["no_config_vpn"]);
    assert_eq!(s.user_control_disabled, vec![TAILSCALE]);
    assert!(matches!(fakes.dpc.changes()[0], dpc::Change::AlwaysOnVpn { .. }), "the VPN is made first");
    let r = phone.report().expect("a report sent");
    assert_eq!((r.policy.as_str(), r.owner, r.applied, r.lockdown), ("kids", true, 3, true));
    assert!(r.failed.is_empty());
    assert_eq!(fakes.background.every(), Some(crate::status::EVERY_S));
    assert!(fakes.notifications.registered());
    let page: StatusPage = h.page::<Status>(&phone);
    assert_eq!((page.policy.as_str(), page.owner.as_str(), page.vpn.as_str()), ("kids", "Yes", TAILSCALE));
    assert!(!page.busy);
    // Woken again with the phone as the policy says: nothing to make.
    let before = fakes.dpc.changes().len();
    assert!(h.woken(Woken::Push, &mut phone));
    assert_eq!(fakes.dpc.changes().len(), before);
    assert_eq!(phone.report().unwrap().applied, 0);
}

/// A phone whose person has no policy is left as it is — its VPN least —
/// and still reports what it found. Falsified by no policy reading as one
/// that turns the VPN off.
#[test]
fn no_policy_changes_nothing() {
    let dpc = ark_dpc::Fake::with(dpc::State {
        owner: true,
        always_on_vpn: Some(TAILSCALE.into()),
        lockdown: true,
        ..Default::default()
    });
    let (mut phone, fakes) = phone(dpc);
    phone.server(set(kids()));
    let mut h = open(&mut phone);
    assert!(h.woken(Woken::Enrolled, &mut phone));
    assert!(fakes.dpc.changes().is_empty(), "nothing made");
    assert!(fakes.dpc.state().lockdown);
    let r = phone.report().unwrap();
    assert_eq!((r.policy.as_str(), r.vpn_app.as_deref()), ("", Some(TAILSCALE)));
}

/// The app is not the device owner: nothing can be made, and the report
/// says so.
#[test]
fn not_the_owner_reports_as_much() {
    let (mut phone, fakes) = phone(ark_dpc::Fake::not_owner());
    phone.server(set(kids()));
    phone.server(assign("ann", "kids"));
    let mut h = open(&mut phone);
    assert!(h.woken(Woken::Boot, &mut phone));
    assert!(fakes.dpc.changes().is_empty());
    assert!(!phone.report().unwrap().owner);
    assert_eq!(h.page::<Status>(&phone).owner, "No");
}

/// The server changes the policy while the app runs: it is applied at once,
/// with no wake. Falsified by `HostedApp::moved` doing nothing: the cameras
/// stayed on.
#[test]
fn a_policy_that_moves_after_a_run_is_applied_at_once() {
    let (mut phone, fakes) = phone(ark_dpc::Fake::new());
    phone.server(set(kids()));
    phone.server(assign("ann", "kids"));
    let mut h = open(&mut phone);
    h.woken(Woken::Sync, &mut phone);
    assert!(!fakes.dpc.state().camera_disabled);
    phone.server(set(Policy {
        camera_disabled: Some(true),
        ..kids()
    }));
    h.moved(&mut phone);
    assert!(h.idle(&phone));
    assert!(fakes.dpc.state().camera_disabled);
    assert_eq!(phone.report().unwrap().applied, 1);
}

/// What the phone reads for: nothing on a desktop or in a browser, where
/// there is no device owner.
#[test]
fn with_no_device_owner_the_run_stops_and_says_why() {
    let mut disk = Memory::default();
    ark_peer::local::preset(&mut disk, |r| (r.actor, r.session) = (Some("ann".into()), "phone".into())).unwrap();
    let mut peer = Peer::open_as(Config::of(devices_core::shared()), Box::new(disk), None);
    peer.alone();
    let mut phone = Phone {
        peer,
        store: Store::new(),
        modules: Modules::new(),
    };
    let mut h = open(&mut phone);
    assert!(h.woken(Woken::Sync, &mut phone));
    assert!(h.idle(&phone));
    let page = h.page::<Status>(&phone);
    assert!(page.problem.is_some(), "it says why");
    assert!(phone.report().is_none());
}
