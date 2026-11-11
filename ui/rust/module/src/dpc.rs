//! The device policy's port: what an app that is the phone's device owner
//! (its device policy controller, Android's DPC) holds the phone to. An
//! update asks with [`apply`], an [`Apply`] of one [`Change`] whose output
//! is `()`, or with [`read`], a [`Read`] whose output is the [`State`] the
//! phone is in. Its implementation (Android's `DevicePolicyManager`) and
//! its fake are the module's (modules/ark-dpc, `ark-dpc`); every other
//! platform registers none, and is answered
//! [`ModuleError::Unavailable`](crate::ModuleError::Unavailable).
//!
//! What a policy asks is a [`Policy`], and [`plan`] is the changes from the
//! state the phone is in to it: what the policy says nothing of is left as
//! it is, so a policy that forgets the VPN never takes it away ([`Vpn`]).
//! An update reads ([`asked`] says what to), plans, and applies each
//! change; read again, the plan is empty.

use std::collections::BTreeMap;

use crate::{Answer, Op, Reply};

/// The device policy controller: one change at a time, and the state the
/// phone is in. Each call is answered at once.
pub trait Dpc: Send + Sync {
    /// `change` made, or why not.
    fn apply(&self, change: Change) -> Answer<()>;
    /// The state the phone is in, of the apps and permissions `asked` names.
    fn read(&self, asked: Asked) -> Answer<State>;
}

// * What changes

/// One thing the device owner changes.
#[derive(Clone, Debug, PartialEq, Eq)]
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
pub enum Grant {
    Default,
    Granted,
    Denied,
}

// * What the phone is in

/// An app's permission, by name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PermissionOf {
    pub app: String,
    pub permission: String,
}

/// What a read looks at besides what is the device's: the apps whose
/// uninstall it says, the permissions whose grant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub apps: Vec<String>,
    pub permissions: Vec<PermissionOf>,
}

/// A permission and what its app is given of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionState {
    pub app: String,
    pub permission: String,
    pub grant: Grant,
}

/// The state the phone is in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

// * The operations

/// One change made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Apply(pub Change);

impl Op for Apply {
    type Module = dyn Dpc;
    type Output = ();
    const MODULE: &'static str = "ark-dpc";
    fn run(&self, m: &dyn Dpc, reply: Reply<()>) {
        reply.answer(m.apply(self.0.clone()))
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
        reply.answer(m.read(self.0.clone()))
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
