//! The device's policy: what an app that is the phone's device owner (its
//! device policy controller, Android's DPC) holds the phone to. Its port —
//! [`Change`], [`State`], the [`Dpc`] trait, [`apply`] and [`read`], a
//! [`Policy`] and its [`plan`] — is the runtime's (`ark_module::dpc`),
//! which an update asks through; here is what implements it: the fake, and
//! (`ffi`) the phones' foreign trait, which Android's `PhoneDpc` implements
//! (kotlin/) as the admin `dev.arkui.modules.ArkDpcAdmin`. Every other
//! platform registers none and is answered `Unavailable`.

pub use ark_module::dpc::*;

use std::sync::{Arc, Mutex};

use ark_module::{Answer, ModuleError, Modules};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The phones' device owner, through UniFFI (`ffi`): the foreign trait
/// Kotlin's `PhoneDpc` implements, and it registered as the port.
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    use ark_module::{Answer, ModuleError};

    use super::{Asked, Change, Grant, PermissionOf, PermissionState, State};

    #[uniffi::remote(Enum)]
    pub enum Change {
        AlwaysOnVpn { app: Option<String>, lockdown: bool },
        Restriction { name: String, on: bool },
        UninstallBlocked { app: String, blocked: bool },
        Permission { app: String, permission: String, grant: Grant },
        CameraDisabled { disabled: bool },
        ScreenCaptureDisabled { disabled: bool },
    }

    #[uniffi::remote(Enum)]
    pub enum Grant {
        Default,
        Granted,
        Denied,
    }

    #[uniffi::remote(Record)]
    pub struct PermissionOf {
        pub app: String,
        pub permission: String,
    }

    #[uniffi::remote(Record)]
    pub struct Asked {
        pub apps: Vec<String>,
        pub permissions: Vec<PermissionOf>,
    }

    #[uniffi::remote(Record)]
    pub struct PermissionState {
        pub app: String,
        pub permission: String,
        pub grant: Grant,
    }

    #[uniffi::remote(Record)]
    pub struct State {
        pub owner: bool,
        pub always_on_vpn: Option<String>,
        pub lockdown: bool,
        pub restrictions: Vec<String>,
        pub uninstall_blocked: Vec<String>,
        pub permissions: Vec<PermissionState>,
        pub camera_disabled: bool,
        pub screen_capture_disabled: bool,
        pub sdk: i32,
        pub security_patch: String,
    }

    /// Why the phone made no change, or read nothing.
    #[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
    pub enum DpcError {
        /// It failed, in the platform's words: not the device owner, a VPN
        /// app that is not installed, a restriction the platform refuses.
        Failed { why: String },
    }

    impl std::fmt::Display for DpcError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self {
                DpcError::Failed { why } => f.write_str(why),
            }
        }
    }

    impl std::error::Error for DpcError {}

    impl From<uniffi::UnexpectedUniFFICallbackError> for DpcError {
        fn from(e: uniffi::UnexpectedUniFFICallbackError) -> DpcError {
            DpcError::Failed { why: e.reason }
        }
    }

    impl From<DpcError> for ModuleError {
        fn from(e: DpcError) -> ModuleError {
            match e {
                DpcError::Failed { why } => ModuleError::Failed { why },
            }
        }
    }

    /// The phone's device owner, as Kotlin implements it.
    #[uniffi::export(with_foreign)]
    pub trait Dpc: Send + Sync {
        fn apply(&self, change: Change) -> Result<(), DpcError>;
        fn read(&self, asked: Asked) -> Result<State, DpcError>;
    }

    struct Phone(Arc<dyn Dpc>);

    impl super::Dpc for Phone {
        fn apply(&self, change: Change) -> Answer<()> {
            Ok(self.0.apply(change)?)
        }
        fn read(&self, asked: Asked) -> Answer<State> {
            Ok(self.0.read(asked)?)
        }
    }

    /// The phone's device owner as the port an update asks through.
    pub fn port(d: Arc<dyn Dpc>) -> Arc<dyn super::Dpc> {
        Arc::new(Phone(d))
    }
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
    fn apply(&self, change: Change) -> Answer<()> {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).push(change.clone());
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.owner {
            return Err(ModuleError::Failed {
                why: "not the device owner".into(),
            });
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
        Ok(())
    }

    fn read(&self, asked: Asked) -> Answer<State> {
        let s = self.state();
        if !s.owner {
            return Ok(State {
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
        Ok(seen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
            permissions: BTreeMap::from([(
                PermissionOf {
                    app: "io.heckel.ntfy".into(),
                    permission: "android.permission.POST_NOTIFICATIONS".into(),
                },
                Grant::Granted,
            )]),
            camera_disabled: None,
            screen_capture_disabled: Some(false),
        }
    }

    /// Read, plan, apply each change through the registry, as an update does.
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
        assert_eq!(
            plan(&open, &fake.state()),
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
