//! The device's policy: what an app that is the phone's device owner (its
//! device policy controller, Android's DPC) holds the phone to. An update
//! asks with [`apply`], an [`Apply`] of one [`Change`] whose output is
//! `()`, or with [`read`], a [`Read`] whose output is the [`State`] the
//! phone is in.
//!
//! What a policy asks is a [`Policy`], and [`plan`] is the changes from the
//! state the phone is in to it: what the policy says nothing of is left as
//! it is, so a policy that forgets the VPN never takes it away ([`Vpn`]).
//! An app reads ([`asked`] says what to), plans, and applies each change;
//! read again, the plan is empty.
//!
//! Only Android has it: an app made the device owner (README.md) and its
//! receiver, `dev.arkui.modules.ArkDpcAdmin`. Every other platform answers
//! `Unavailable`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark_module::{Modules, Op, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The device policy controller: one change at a time, and what the phone
/// is in.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Dpc: Send + Sync {
    /// `change` made; the reply says it was, or why not.
    fn apply(&self, change: Change, reply: Arc<ApplyReply>);
    /// The state the phone is in, of the apps and permissions `asked` names.
    fn read(&self, asked: Asked, reply: Arc<ReadReply>);
}

// * What changes

/// One thing the device owner changes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Change {
    /// The VPN the phone is always on, `app` its package; none is none.
    /// With `lockdown` nothing leaves the phone but through it
    /// (`setAlwaysOnVpnPackage`).
    AlwaysOnVpn { app: Option<String>, lockdown: bool },
    /// A user restriction (`UserManager`'s, `no_config_vpn`) on or off.
    Restriction { name: String, on: bool },
    /// Whether `app` can be uninstalled.
    UninstallBlocked { app: String, blocked: bool },
    /// What `app` is given of `permission`.
    Permission { app: String, permission: String, grant: Grant },
    /// The cameras, every app's.
    CameraDisabled { disabled: bool },
    /// Screenshots and the screen recorded, every app's.
    ScreenCaptureDisabled { disabled: bool },
}

/// What an app is given of a permission: the person's choice, or the
/// owner's either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Grant {
    Default,
    Granted,
    Denied,
}

// * What the phone is in

/// An app's permission, by name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct PermissionOf {
    pub app: String,
    pub permission: String,
}

/// What a read looks at besides what is the device's: the apps whose
/// uninstall it says, the permissions whose grant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Asked {
    pub apps: Vec<String>,
    pub permissions: Vec<PermissionOf>,
}

/// A permission and what its app is given of it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct PermissionState {
    pub app: String,
    pub permission: String,
    pub grant: Grant,
}

/// The state the phone is in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct State {
    /// Whether this app is the device owner. When it is not, nothing else
    /// here was read and every change fails.
    pub owner: bool,
    /// The always-on VPN's package, and whether it is locked down.
    pub always_on_vpn: Option<String>,
    pub lockdown: bool,
    /// The user restrictions the owner has set.
    pub restrictions: Vec<String>,
    /// Of the apps asked, those that cannot be uninstalled.
    pub uninstall_blocked: Vec<String>,
    /// Of the permissions asked, each one's grant.
    pub permissions: Vec<PermissionState>,
    pub camera_disabled: bool,
    pub screen_capture_disabled: bool,
    /// The platform's version (`Build.VERSION.SDK_INT`) and its security
    /// patch (`2026-11-05`), for a server to show.
    pub sdk: i32,
    pub security_patch: String,
}

// * Answering

/// Where the phone says a change was made, once.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct ApplyReply(Reply<()>);

#[cfg_attr(feature = "ffi", uniffi::export)]
impl ApplyReply {
    /// It was made.
    pub fn applied(&self) {
        self.0.done(())
    }

    /// It could not be, in the platform's words.
    pub fn failed(&self, why: String) {
        self.0.failed(why)
    }
}

/// Where the phone says the state it is in, once.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct ReadReply(Reply<State>);

#[cfg_attr(feature = "ffi", uniffi::export)]
impl ReadReply {
    /// What it is in.
    pub fn saw(&self, state: State) {
        self.0.done(state)
    }

    /// It could not be read, in the platform's words.
    pub fn failed(&self, why: String) {
        self.0.failed(why)
    }
}

// * The operations

/// One change made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Apply(pub Change);

impl Op for Apply {
    type Module = dyn Dpc;
    type Output = ();
    const MODULE: &'static str = "ark-dpc";
    fn run(&self, m: &dyn Dpc, reply: Reply<()>) {
        m.apply(self.0.clone(), Arc::new(ApplyReply(reply)))
    }
}

/// The state the phone is in read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read(pub Asked);

impl Op for Read {
    type Module = dyn Dpc;
    type Output = State;
    const MODULE: &'static str = "ark-dpc";
    fn run(&self, m: &dyn Dpc, reply: Reply<State>) {
        m.read(self.0.clone(), Arc::new(ReadReply(reply)))
    }
}

/// `change` made.
pub fn apply(change: Change) -> Apply {
    Apply(change)
}

/// The state the phone is in, of what `asked` names.
pub fn read(asked: Asked) -> Read {
    Read(asked)
}

// * A policy and its plan

/// The VPN a policy asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Vpn {
    /// Left as it is: a policy that says nothing of the VPN.
    #[default]
    Unmanaged,
    /// Always on through `app`, locked down or not.
    AlwaysOn { app: String, lockdown: bool },
    /// None. Only this takes an always-on VPN away.
    Off,
}

/// What a policy holds the phone to. Each map says only what it names:
/// `true` on (blocked, disabled), `false` off; what it leaves out is left
/// as it is, so undoing one is saying `false`, not deleting its line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub vpn: Vpn,
    /// User restrictions by name (`no_config_vpn`).
    pub restrictions: BTreeMap<String, bool>,
    /// Apps by package: whether each cannot be uninstalled.
    pub uninstall_blocked: BTreeMap<String, bool>,
    /// Permissions and what each app is given of them.
    pub permissions: BTreeMap<PermissionOf, Grant>,
    pub camera_disabled: Option<bool>,
    pub screen_capture_disabled: Option<bool>,
}

/// What a read must look at for [`plan`] to see what `policy` names.
pub fn asked(policy: &Policy) -> Asked {
    Asked {
        apps: policy.uninstall_blocked.keys().cloned().collect(),
        permissions: policy.permissions.keys().cloned().collect(),
    }
}

/// The changes from `state` to `policy`, in the order they are made: the
/// VPN first, so that nothing after it is fetched or sent around it.
/// Empty when the phone is already as the policy says, and when this app
/// is not the device owner (nothing could be made).
pub fn plan(policy: &Policy, state: &State) -> Vec<Change> {
    if !state.owner {
        return vec![];
    }
    let mut changes = vec![];
    match &policy.vpn {
        Vpn::Unmanaged => {}
        Vpn::AlwaysOn { app, lockdown } => {
            if state.always_on_vpn.as_deref() != Some(app.as_str()) || state.lockdown != *lockdown {
                changes.push(Change::AlwaysOnVpn {
                    app: Some(app.clone()),
                    lockdown: *lockdown,
                });
            }
        }
        Vpn::Off => {
            if state.always_on_vpn.is_some() {
                changes.push(Change::AlwaysOnVpn { app: None, lockdown: false });
            }
        }
    }
    for (name, on) in &policy.restrictions {
        if state.restrictions.contains(name) != *on {
            changes.push(Change::Restriction { name: name.clone(), on: *on });
        }
    }
    for (app, blocked) in &policy.uninstall_blocked {
        if state.uninstall_blocked.contains(app) != *blocked {
            changes.push(Change::UninstallBlocked {
                app: app.clone(),
                blocked: *blocked,
            });
        }
    }
    for (of, grant) in &policy.permissions {
        let now = state
            .permissions
            .iter()
            .find(|p| p.app == of.app && p.permission == of.permission)
            .map(|p| p.grant);
        if now != Some(*grant) {
            changes.push(Change::Permission {
                app: of.app.clone(),
                permission: of.permission.clone(),
                grant: *grant,
            });
        }
    }
    if let Some(disabled) = policy.camera_disabled {
        if state.camera_disabled != disabled {
            changes.push(Change::CameraDisabled { disabled });
        }
    }
    if let Some(disabled) = policy.screen_capture_disabled {
        if state.screen_capture_disabled != disabled {
            changes.push(Change::ScreenCaptureDisabled { disabled });
        }
    }
    changes
}

// * The fake

/// The device owner, faked for tests: a phone's state that each change is
/// made to, and every change asked, in order. A fake that is not the owner
/// ([`Fake::not_owner`]) fails each change as the phone does.
#[derive(Clone)]
pub struct Fake {
    state: Arc<Mutex<State>>,
    asked: Arc<Mutex<Vec<Change>>>,
}

impl Default for Fake {
    fn default() -> Fake {
        Fake::new()
    }
}

impl Fake {
    /// The owner of a phone with nothing set.
    pub fn new() -> Fake {
        Fake::with(State {
            owner: true,
            sdk: 36,
            security_patch: "2026-11-05".into(),
            ..State::default()
        })
    }

    /// An app that is not the owner.
    pub fn not_owner() -> Fake {
        Fake::with(State::default())
    }

    /// A phone in `state`.
    pub fn with(state: State) -> Fake {
        Fake {
            state: Arc::new(Mutex::new(state)),
            asked: Arc::default(),
        }
    }

    /// This fake registered as the device owner.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Dpc>(Arc::new(self.clone()));
    }

    /// The phone's state now.
    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The changes asked, made or not, in order.
    pub fn changes(&self) -> Vec<Change> {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn set(list: &mut Vec<String>, item: &str, on: bool) {
    list.retain(|x| x != item);
    if on {
        list.push(item.to_string());
        list.sort();
    }
}

impl Dpc for Fake {
    fn apply(&self, change: Change, reply: Arc<ApplyReply>) {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).push(change.clone());
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.owner {
            drop(s);
            return reply.failed("not the device owner".into());
        }
        match change {
            Change::AlwaysOnVpn { app, lockdown } => {
                s.lockdown = app.is_some() && lockdown;
                s.always_on_vpn = app;
            }
            Change::Restriction { name, on } => set(&mut s.restrictions, &name, on),
            Change::UninstallBlocked { app, blocked } => set(&mut s.uninstall_blocked, &app, blocked),
            Change::Permission { app, permission, grant } => {
                s.permissions.retain(|p| !(p.app == app && p.permission == permission));
                s.permissions.push(PermissionState { app, permission, grant });
            }
            Change::CameraDisabled { disabled } => s.camera_disabled = disabled,
            Change::ScreenCaptureDisabled { disabled } => s.screen_capture_disabled = disabled,
        }
        drop(s);
        reply.applied()
    }

    fn read(&self, asked: Asked, reply: Arc<ReadReply>) {
        let s = self.state();
        if !s.owner {
            return reply.saw(State {
                owner: false,
                ..State::default()
            });
        }
        let mut seen = s.clone();
        seen.uninstall_blocked.retain(|a| asked.apps.contains(a));
        // A permission asked of that no change has set is the person's.
        seen.permissions = asked
            .permissions
            .iter()
            .map(|of| PermissionState {
                app: of.app.clone(),
                permission: of.permission.clone(),
                grant: s
                    .permissions
                    .iter()
                    .find(|p| p.app == of.app && p.permission == of.permission)
                    .map_or(Grant::Default, |p| p.grant),
            })
            .collect();
        reply.saw(seen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_module::{Answer, ModuleError};

    fn tailscale(lockdown: bool) -> Vpn {
        Vpn::AlwaysOn {
            app: "com.tailscale.ipn".into(),
            lockdown,
        }
    }

    fn base() -> Policy {
        Policy {
            vpn: tailscale(true),
            restrictions: [("no_config_vpn".to_string(), true), ("no_factory_reset".to_string(), true)].into(),
            uninstall_blocked: [("com.tailscale.ipn".to_string(), true)].into(),
            permissions: [(
                PermissionOf {
                    app: "io.heckel.ntfy".into(),
                    permission: "android.permission.POST_NOTIFICATIONS".into(),
                },
                Grant::Granted,
            )]
            .into(),
            camera_disabled: None,
            screen_capture_disabled: Some(false),
        }
    }

    /// Read, plan, apply each change through the registry, as an app does.
    fn reconcile(m: &Modules, policy: &Policy) -> Vec<Answer<()>> {
        let state = Arc::new(Mutex::new(None));
        let s = state.clone();
        m.run(&read(asked(policy)), move |a| *s.lock().unwrap() = Some(a));
        let state = state.lock().unwrap().take().unwrap().unwrap();
        let answers = Arc::new(Mutex::new(vec![]));
        for change in plan(policy, &state) {
            let a = answers.clone();
            m.run(&apply(change), move |r| a.lock().unwrap().push(r));
        }
        let got = answers.lock().unwrap().clone();
        got
    }

    #[test]
    fn a_policy_applied_leaves_nothing_to_plan() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let answers = reconcile(&m, &base());
        assert_eq!(
            answers.len(),
            5,
            "the VPN, two restrictions, an uninstall and a grant; the screen capture is already allowed: {:?}",
            fake.changes()
        );
        assert!(answers.iter().all(Result::is_ok));
        // The VPN is the first change made.
        assert_eq!(
            fake.changes()[0],
            Change::AlwaysOnVpn {
                app: Some("com.tailscale.ipn".into()),
                lockdown: true
            }
        );
        assert!(reconcile(&m, &base()).is_empty());
        let s = fake.state();
        assert_eq!(s.always_on_vpn.as_deref(), Some("com.tailscale.ipn"));
        assert!(s.lockdown);
        assert_eq!(s.restrictions, vec!["no_config_vpn", "no_factory_reset"]);
    }

    #[test]
    fn what_a_policy_leaves_out_is_left_as_it_is() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        reconcile(&m, &base());
        // Forgetting the VPN and a restriction takes neither away.
        let mut forgot = base();
        forgot.vpn = Vpn::Unmanaged;
        forgot.restrictions.remove("no_factory_reset");
        assert!(reconcile(&m, &forgot).is_empty());
        assert!(fake.state().lockdown);
        assert!(fake.state().restrictions.contains(&"no_factory_reset".to_string()));
        // Saying so does.
        let mut undone = forgot.clone();
        undone.vpn = Vpn::Off;
        undone.restrictions.insert("no_factory_reset".into(), false);
        assert_eq!(reconcile(&m, &undone).len(), 2);
        assert_eq!(fake.state().always_on_vpn, None);
        assert!(!fake.state().lockdown);
        assert_eq!(fake.state().restrictions, vec!["no_config_vpn"]);
    }

    #[test]
    fn a_lockdown_turned_off_is_one_change() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        reconcile(&m, &base());
        let mut open = base();
        open.vpn = tailscale(false);
        let state = fake.state();
        assert_eq!(
            plan(&open, &state),
            vec![Change::AlwaysOnVpn {
                app: Some("com.tailscale.ipn".into()),
                lockdown: false
            }]
        );
    }

    #[test]
    fn an_app_that_is_not_the_owner_plans_nothing_and_fails_a_change() {
        let fake = Fake::not_owner();
        let mut m = Modules::new();
        fake.register(&mut m);
        assert!(reconcile(&m, &base()).is_empty());
        let got = Arc::new(Mutex::new(None));
        let g = got.clone();
        m.run(&apply(Change::CameraDisabled { disabled: true }), move |a| *g.lock().unwrap() = Some(a));
        assert_eq!(
            *got.lock().unwrap(),
            Some(Err(ModuleError::Failed {
                why: "not the device owner".into()
            }))
        );
    }

    #[test]
    fn without_a_device_owner_it_is_unavailable() {
        let m = Modules::new();
        let got = Arc::new(Mutex::new(None));
        let g = got.clone();
        m.run(&read(Asked::default()), move |a| *g.lock().unwrap() = Some(a));
        assert_eq!(*got.lock().unwrap(), Some(Err(ModuleError::Unavailable)));
    }
}
