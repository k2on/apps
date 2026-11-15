//! The status screen, and the work it does: what holds this phone to its
//! policy. Woken by the OS with no screen (a periodic sync, a push, the
//! device started, the app just made the device's owner) or asked from the
//! screen, it
//!
//! 1. syncs (`Command::Sync`), so the policy is the server's latest;
//! 2. reads what the phone is in (ark-dpc's `read`, of what the policy names);
//! 3. makes each change the plan from that to the policy says
//!    (`dpc::plan`: the VPN first), one at a time, each failure kept in words;
//! 4. reads the phone again and reports what it found (`reports.report`).
//!
//! What a policy leaves out is left as the phone has it, and no policy at
//! all changes nothing (crate::policy), so a mistake on the server never
//! takes a phone's VPN away. The first sync also asks the OS to wake the app
//! every fifteen minutes and registers it for push: a device owner's
//! background is the point of it, not the person's choice.

pub mod view;

use ark_app::modules::{background, dpc, notifications};
use ark_app::{Answer, App, Command, Data, Woken};
use devices_core::{DevicesChange, DevicesView, Policy, Report};

use crate::policy::{policy_of, report_of, said, seen_of, state_of, Seen};
use crate::Route;

/// How often the OS is asked to wake the app to sync (Android's shortest).
pub const EVERY_S: u32 = 15 * 60;

pub struct Status;

/// Where the work stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Syncing,
    Reading,
    Applying,
    Checking,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusState {
    pub phase: Phase,
    /// The policy as it was when this run read the phone.
    wanted: dpc::Policy,
    policy: String,
    /// The changes still to make, and the one on its way.
    plan: Vec<dpc::Change>,
    making: Option<dpc::Change>,
    applied: i64,
    failed: Vec<String>,
    /// What the phone was last read to be.
    pub seen: Option<Seen>,
    /// Why the last run stopped short (no device owner here, the platform's
    /// words), while it did.
    problem: Option<String>,
    /// The OS asked to wake the app, and push registered for, once.
    scheduled: bool,
}

/// Everything the status screen can be told.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum StatusMsg {
    /// The OS woke the process for it (`App::woken`).
    Woken,
    /// Apply Now.
    Refresh,
    /// The replica caught up with the server.
    Synced,
    /// What the phone is in, read; or why it could not be.
    Read { seen: Option<Seen>, failed: Option<String> },
    /// The change on its way made, or why not.
    Applied { failed: Option<String> },
}

/// What the status screen shows.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct StatusPage {
    /// The policy that applies to this phone's person, by name; empty for
    /// none.
    pub policy: String,
    /// Whether the app is the phone's device owner: "Yes" or "No".
    pub owner: String,
    /// The always-on VPN as the phone has it: its app, or "None".
    pub vpn: String,
    /// "On" or "Off".
    pub lockdown: String,
    /// The user restrictions set, and the apps kept installed and running.
    pub restrictions: String,
    pub kept: String,
    /// The platform's security patch.
    pub patch: String,
    /// What the last run did, in a sentence.
    pub last: String,
    pub busy: bool,
    /// Apply Now can be pressed: nothing is on its way.
    pub idle: bool,
    /// What is on its way, in a sentence, while something is.
    pub doing: Option<String>,
    /// Why the last run stopped short, or what it could not make.
    pub problem: Option<String>,
}

impl StatusState {
    /// The policy the last run held the phone to.
    pub fn wanted(&self) -> &dpc::Policy {
        &self.wanted
    }

    /// Whether a run has read the phone since the app started.
    pub fn ran(&self) -> bool {
        self.seen.is_some()
    }
}

fn read_answer(a: Answer<dpc::State>) -> StatusMsg {
    match a {
        Ok(s) => StatusMsg::Read {
            seen: Some(seen_of(s)),
            failed: None,
        },
        Err(e) => StatusMsg::Read {
            seen: None,
            failed: Some(e.to_string()),
        },
    }
}

fn applied_answer(a: Answer<()>) -> StatusMsg {
    StatusMsg::Applied {
        failed: a.err().map(|e| e.to_string()),
    }
}

/// The policy assigned to this phone's person, as the replica holds it.
fn assigned<'a>(data: &Data<'a, DevicesView>) -> Option<&'a Policy> {
    data.rows::<Policy>(&DevicesView::Assigned).first()
}

/// The next change asked, or, with none left, the phone read again for the
/// report.
fn next(s: &mut StatusState) -> Vec<Command<Status>> {
    if s.plan.is_empty() {
        s.making = None;
        s.phase = Phase::Checking;
        return vec![Command::ask_then(dpc::read(dpc::asked(&s.wanted)), read_answer)];
    }
    let c = s.plan.remove(0);
    s.making = Some(c.clone());
    vec![Command::ask_then(dpc::apply(c), applied_answer)]
}

fn words(n: usize, one: &str, many: &str) -> String {
    match n {
        0 => "None".into(),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

impl App for Status {
    type Key = DevicesView;
    type State = StatusState;
    type Msg = StatusMsg;
    type Change = DevicesChange;
    type Route = Route;
    type Request = ();
    type Screen = StatusPage;

    fn holds(_: &StatusState) -> Vec<DevicesView> {
        vec![DevicesView::Assigned, DevicesView::Own]
    }

    fn update(s: &mut StatusState, msg: StatusMsg, data: &Data<'_, DevicesView>) -> Vec<Command<Self>> {
        match msg {
            StatusMsg::Woken | StatusMsg::Refresh => {
                if s.phase != Phase::Idle {
                    return vec![];
                }
                s.phase = Phase::Syncing;
                s.problem = None;
                vec![Command::Sync {
                    then: Some(StatusMsg::Synced),
                }]
            }
            StatusMsg::Synced => {
                let policy = assigned(data);
                s.policy = policy.map(|p| p.name.clone()).unwrap_or_default();
                s.wanted = policy_of(policy);
                s.phase = Phase::Reading;
                let mut out = vec![Command::ask_then(dpc::read(dpc::asked(&s.wanted)), read_answer)];
                if !s.scheduled {
                    s.scheduled = true;
                    out.push(Command::ask(background::set(Some(EVERY_S))));
                    out.push(Command::ask(notifications::register_push()));
                }
                out
            }
            StatusMsg::Read { seen: None, failed } => {
                // No device owner here (a platform without one, or the
                // module not registered): nothing to make or report.
                s.phase = Phase::Idle;
                s.problem = failed;
                vec![]
            }
            StatusMsg::Read { seen: Some(seen), .. } => match s.phase {
                Phase::Reading => {
                    s.plan = dpc::plan(&s.wanted, &state_of(&seen));
                    s.applied = 0;
                    s.failed.clear();
                    s.seen = Some(seen);
                    s.phase = Phase::Applying;
                    next(s)
                }
                Phase::Checking => {
                    s.phase = Phase::Idle;
                    let report = report_of(&s.policy, &seen, s.applied, s.failed.clone());
                    s.seen = Some(seen);
                    vec![Command::send(DevicesChange::Report(report))]
                }
                _ => vec![],
            },
            StatusMsg::Applied { failed } => {
                if s.phase != Phase::Applying {
                    return vec![];
                }
                match failed {
                    None => s.applied += 1,
                    Some(why) => {
                        let what = s.making.as_ref().map(said).unwrap_or_default();
                        s.failed.push(format!("{what}: {why}"));
                    }
                }
                next(s)
            }
        }
    }

    fn screen(s: &StatusState, data: &Data<'_, DevicesView>) -> StatusPage {
        let report = data.rows::<Report>(&DevicesView::Own).first();
        let seen = s.seen.clone().or_else(|| {
            report.map(|r| Seen {
                owner: r.owner,
                vpn_app: r.vpn_app.clone(),
                lockdown: r.lockdown,
                restrictions: r.restrictions.clone(),
                uninstall_blocked: r.uninstall_blocked.clone(),
                user_control_disabled: r.user_control_disabled.clone(),
                security_patch: r.security_patch.clone(),
                sdk: r.sdk,
                ..Seen::default()
            })
        });
        let seen = seen.unwrap_or_default();
        let failed = if s.phase == Phase::Idle { s.failed.clone() } else { vec![] };
        let failed = if failed.is_empty() {
            report.map(|r| r.failed.clone()).unwrap_or_default()
        } else {
            failed
        };
        StatusPage {
            policy: assigned(data).map(|p| p.name.clone()).unwrap_or_default(),
            owner: if seen.owner { "Yes".into() } else { "No".into() },
            vpn: seen.vpn_app.clone().unwrap_or_else(|| "None".into()),
            lockdown: if seen.lockdown { "On".into() } else { "Off".into() },
            restrictions: words(seen.restrictions.len(), "restriction", "restrictions"),
            kept: words(seen.uninstall_blocked.len().max(seen.user_control_disabled.len()), "app", "apps"),
            patch: seen.security_patch.clone(),
            last: match report {
                Some(r) if r.failed.is_empty() => format!("{} made", words(r.applied as usize, "change", "changes")),
                Some(r) => format!("{} made, {} not", words(r.applied as usize, "change", "changes"), r.failed.len()),
                None => "Not yet".into(),
            },
            busy: s.phase != Phase::Idle,
            idle: s.phase == Phase::Idle,
            doing: match s.phase {
                Phase::Idle => None,
                Phase::Syncing => Some("Syncing with the server".into()),
                Phase::Reading | Phase::Checking => Some("Reading this phone".into()),
                Phase::Applying => Some("Applying the policy".into()),
            },
            problem: s.problem.clone().or_else(|| failed.first().cloned()),
        }
    }

    fn woken(reason: Woken) -> Option<StatusMsg> {
        match reason {
            Woken::Sync | Woken::Push | Woken::Boot | Woken::Enrolled => Some(StatusMsg::Woken),
            Woken::Action { .. } => None,
        }
    }
}
