//! Between devices' rows and ark-dpc's port: the domain's [`Policy`] as the
//! port's policy, what the phone read ([`ark_app::modules::dpc::State`]) as a
//! record a screen's message carries ([`Seen`]), and that as the report a
//! phone sends. Pure: the tests hold each way round.

use ark_app::modules::dpc;
use devices_core::{Grant, Policy, ReportInput, VpnMode};

/// The domain's policy as ark-dpc's: each list says only what it names,
/// and no policy at all is one that leaves everything as the phone has it
/// (so a phone whose assignment went away loses nothing, its VPN least).
pub fn policy_of(p: Option<&Policy>) -> dpc::Policy {
    let Some(p) = p else { return dpc::Policy::default() };
    let map = |v: &[devices_core::Setting]| v.iter().map(|s| (s.name.clone(), s.on)).collect();
    dpc::Policy {
        vpn: match p.vpn {
            VpnMode::Unmanaged => dpc::Vpn::Unmanaged,
            VpnMode::AlwaysOn => dpc::Vpn::AlwaysOn {
                app: p.vpn_app.clone(),
                lockdown: p.vpn_lockdown,
            },
            VpnMode::Off => dpc::Vpn::Off,
        },
        restrictions: map(&p.restrictions),
        uninstall_blocked: map(&p.uninstall_blocked),
        user_control_disabled: map(&p.user_control_disabled),
        permissions: p
            .permissions
            .iter()
            .map(|r| {
                (
                    dpc::PermissionOf {
                        app: r.app.clone(),
                        permission: r.permission.clone(),
                    },
                    grant_of(r.grant),
                )
            })
            .collect(),
        camera_disabled: p.camera_disabled,
        screen_capture_disabled: p.screen_capture_disabled,
    }
}

fn grant_of(g: Grant) -> dpc::Grant {
    match g {
        Grant::Default => dpc::Grant::Default,
        Grant::Granted => dpc::Grant::Granted,
        Grant::Denied => dpc::Grant::Denied,
    }
}

/// What the phone read of itself, as a screen's message carries it across
/// to its toolkit and back: [`dpc::State`], its permissions each one line
/// (`app permission grant`).
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct Seen {
    pub owner: bool,
    pub vpn_app: Option<String>,
    pub lockdown: bool,
    pub restrictions: Vec<String>,
    pub uninstall_blocked: Vec<String>,
    pub user_control_disabled: Vec<String>,
    pub permissions: Vec<String>,
    pub camera_disabled: bool,
    pub screen_capture_disabled: bool,
    pub sdk: i64,
    pub security_patch: String,
}

fn grant_word(g: dpc::Grant) -> &'static str {
    match g {
        dpc::Grant::Default => "default",
        dpc::Grant::Granted => "granted",
        dpc::Grant::Denied => "denied",
    }
}

/// The port's state as a message carries it.
pub fn seen_of(s: dpc::State) -> Seen {
    Seen {
        owner: s.owner,
        vpn_app: s.always_on_vpn,
        lockdown: s.lockdown,
        restrictions: s.restrictions,
        uninstall_blocked: s.uninstall_blocked,
        user_control_disabled: s.user_control_disabled,
        permissions: s
            .permissions
            .iter()
            .map(|p| format!("{} {} {}", p.app, p.permission, grant_word(p.grant)))
            .collect(),
        camera_disabled: s.camera_disabled,
        screen_capture_disabled: s.screen_capture_disabled,
        sdk: s.sdk.into(),
        security_patch: s.security_patch,
    }
}

/// A message's record as the port's state again, what a plan is made from.
pub fn state_of(s: &Seen) -> dpc::State {
    dpc::State {
        owner: s.owner,
        always_on_vpn: s.vpn_app.clone(),
        lockdown: s.lockdown,
        restrictions: s.restrictions.clone(),
        uninstall_blocked: s.uninstall_blocked.clone(),
        user_control_disabled: s.user_control_disabled.clone(),
        permissions: s
            .permissions
            .iter()
            .filter_map(|line| {
                let mut w = line.split(' ');
                let (app, permission, grant) = (w.next()?, w.next()?, w.next()?);
                Some(dpc::PermissionState {
                    app: app.into(),
                    permission: permission.into(),
                    grant: match grant {
                        "granted" => dpc::Grant::Granted,
                        "denied" => dpc::Grant::Denied,
                        _ => dpc::Grant::Default,
                    },
                })
            })
            .collect(),
        camera_disabled: s.camera_disabled,
        screen_capture_disabled: s.screen_capture_disabled,
        sdk: i32::try_from(s.sdk).unwrap_or_default(),
        security_patch: s.security_patch.clone(),
    }
}

/// What the phone sends of what it found and did, under the policy's name.
pub fn report_of(policy: &str, s: &Seen, applied: i64, failed: Vec<String>) -> ReportInput {
    ReportInput {
        policy: policy.into(),
        owner: s.owner,
        vpn_app: s.vpn_app.clone(),
        lockdown: s.lockdown,
        restrictions: s.restrictions.clone(),
        uninstall_blocked: s.uninstall_blocked.clone(),
        user_control_disabled: s.user_control_disabled.clone(),
        camera_disabled: s.camera_disabled,
        screen_capture_disabled: s.screen_capture_disabled,
        sdk: s.sdk,
        security_patch: s.security_patch.clone(),
        applied,
        failed,
    }
}

/// A change in a sentence, for a failure the report and the screen say.
pub fn said(c: &dpc::Change) -> String {
    use dpc::Change as C;
    let onoff = |b: bool| if b { "on" } else { "off" };
    match c {
        C::AlwaysOnVpn { app: Some(app), lockdown } => format!("always-on VPN {app}{}", if *lockdown { ", locked down" } else { "" }),
        C::AlwaysOnVpn { app: None, .. } => "always-on VPN off".into(),
        C::Restriction { name, on } => format!("{name} {}", onoff(*on)),
        C::UninstallBlocked { app, blocked } => format!("{app} {}", if *blocked { "kept installed" } else { "removable" }),
        C::UserControlDisabled { app, disabled } => format!("{app} {}", if *disabled { "kept running" } else { "stoppable" }),
        C::Permission { app, permission, grant } => format!("{app} {permission} {}", grant_word(*grant)),
        C::CameraDisabled { disabled } => format!("cameras {}", onoff(!*disabled)),
        C::ScreenCaptureDisabled { disabled } => format!("screen capture {}", onoff(!*disabled)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devices_core::{PermissionRule, Setting};

    fn kids() -> Policy {
        Policy {
            name: "kids".into(),
            vpn: VpnMode::AlwaysOn,
            vpn_app: "com.tailscale.ipn".into(),
            vpn_lockdown: true,
            restrictions: vec![
                Setting {
                    name: "no_config_vpn".into(),
                    on: true,
                },
                Setting {
                    name: "no_factory_reset".into(),
                    on: false,
                },
            ],
            permissions: vec![PermissionRule {
                app: "io.heckel.ntfy".into(),
                permission: "android.permission.POST_NOTIFICATIONS".into(),
                grant: Grant::Granted,
            }],
            screen_capture_disabled: Some(true),
            ..Default::default()
        }
    }

    /// The domain's policy as the port's, every list and the VPN; and no
    /// policy is one that changes nothing. Falsified by a missing policy as
    /// `Vpn::Off`: a phone whose assignment went away lost its VPN.
    #[test]
    fn a_policy_reads_as_the_ports_and_none_changes_nothing() {
        let p = policy_of(Some(&kids()));
        assert_eq!(
            p.vpn,
            dpc::Vpn::AlwaysOn {
                app: "com.tailscale.ipn".into(),
                lockdown: true
            }
        );
        assert_eq!(p.restrictions.get("no_config_vpn"), Some(&true));
        assert_eq!(p.restrictions.get("no_factory_reset"), Some(&false));
        assert_eq!(p.permissions.values().copied().collect::<Vec<_>>(), vec![dpc::Grant::Granted]);
        assert_eq!((p.camera_disabled, p.screen_capture_disabled), (None, Some(true)));
        let none = policy_of(None);
        assert_eq!(none, dpc::Policy::default());
        let anything = dpc::State {
            owner: true,
            always_on_vpn: Some("com.tailscale.ipn".into()),
            lockdown: true,
            ..Default::default()
        };
        assert!(dpc::plan(&none, &anything).is_empty());
    }

    /// What the phone read crosses as a record and comes back the same.
    #[test]
    fn what_was_seen_comes_back_as_it_was() {
        let s = dpc::State {
            owner: true,
            always_on_vpn: Some("com.tailscale.ipn".into()),
            lockdown: true,
            restrictions: vec!["no_config_vpn".into()],
            uninstall_blocked: vec!["com.tailscale.ipn".into()],
            user_control_disabled: vec!["us.koon.devices".into()],
            permissions: vec![dpc::PermissionState {
                app: "io.heckel.ntfy".into(),
                permission: "android.permission.POST_NOTIFICATIONS".into(),
                grant: dpc::Grant::Denied,
            }],
            camera_disabled: false,
            screen_capture_disabled: true,
            sdk: 36,
            security_patch: "2026-11-05".into(),
        };
        let seen = seen_of(s.clone());
        assert_eq!(state_of(&seen), s);
        let r = report_of("kids", &seen, 2, vec!["x".into()]);
        assert_eq!((r.policy.as_str(), r.applied, r.sdk, r.failed.len()), ("kids", 2, 36, 1));
    }

    #[test]
    fn a_change_in_a_sentence() {
        assert_eq!(
            said(&dpc::Change::AlwaysOnVpn {
                app: Some("com.tailscale.ipn".into()),
                lockdown: true
            }),
            "always-on VPN com.tailscale.ipn, locked down"
        );
        assert_eq!(said(&dpc::Change::CameraDisabled { disabled: true }), "cameras off");
    }
}
