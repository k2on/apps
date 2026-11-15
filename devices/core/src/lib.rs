//! devices' domain (docs/plan-v5.md §1.16): a household's managed phones.
//!
//! - **Policies** are what a phone is held to — its always-on VPN, user
//!   restrictions, apps that cannot be removed or stopped, permissions —
//!   written only by the server (an admin: the household's Nix config, through
//!   `devices-server`), each under a name.
//! - **Assignments** say which policy applies to whom: a phone signed in as a
//!   person holds that person's policy, so a phone needs no name of its own
//!   before it is enrolled.
//! - **Reports** are what each phone last read of itself and did, one row
//!   per signed-in session, written by that session alone.
//!
//! A phone reads [`Policy`] through the `policies.assigned` view and turns it
//! into ark-dpc's policy in its app (devices-app); this crate knows nothing
//! of a device. Like every core it is pure (clippy.toml).

use std::sync::{Arc, OnceLock};

use ark::format::Encode;
use ark::prelude::*;
use ark::views::{Opening, ViewKey};

/// This domain's version, which a `Hello` carries beside its schema's hash.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// * Rows

/// What a policy asks of the always-on VPN.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
pub enum VpnMode {
    /// Left as the phone has it.
    #[default]
    Unmanaged,
    /// Always on through the policy's `vpn_app`, locked down or not.
    AlwaysOn,
    /// None: the only way a policy takes an always-on VPN away.
    Off,
}

/// A thing a policy turns on (`true`) or off (`false`) by name: a user
/// restriction (`no_config_vpn`) or an app's package. What a policy does not
/// name, it leaves as the phone has it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
pub struct Setting {
    pub name: String,
    pub on: bool,
}

/// What an app is given of a permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
pub enum Grant {
    /// The person's choice.
    #[default]
    Default,
    Granted,
    Denied,
}

/// One app's permission, and what the policy gives it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
pub struct PermissionRule {
    pub app: String,
    pub permission: String,
    pub grant: Grant,
}

/// What a phone is held to, under its name.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Policy {
    #[ark(key)]
    pub name: String,
    pub vpn: VpnMode,
    /// The VPN's package when `vpn` is [`VpnMode::AlwaysOn`]
    /// (`com.tailscale.ipn`).
    pub vpn_app: String,
    /// With it, nothing leaves the phone but through the VPN.
    pub vpn_lockdown: bool,
    /// User restrictions (`no_config_vpn`, `no_factory_reset`).
    pub restrictions: Vec<Setting>,
    /// Apps by package: whether each cannot be uninstalled.
    pub uninstall_blocked: Vec<Setting>,
    /// Apps by package: whether the person cannot force-stop each or clear
    /// its data.
    pub user_control_disabled: Vec<Setting>,
    pub permissions: Vec<PermissionRule>,
    /// The cameras and screen capture, every app's; none leaves them.
    pub camera_disabled: Option<bool>,
    pub screen_capture_disabled: Option<bool>,
}

/// Which policy applies to a person's phones.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Assignment {
    #[ark(key)]
    pub user: String,
    pub policy: String,
}

/// What a phone last read of itself and did, as its own session reported it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Report {
    #[ark(key)]
    pub session: String,
    pub user: String,
    /// The policy it applied, by name; empty when none was assigned.
    pub policy: String,
    /// Whether the app is the phone's device owner. When it is not, the rest
    /// of what it read is nothing.
    pub owner: bool,
    pub vpn_app: Option<String>,
    pub lockdown: bool,
    pub restrictions: Vec<String>,
    pub uninstall_blocked: Vec<String>,
    pub user_control_disabled: Vec<String>,
    pub camera_disabled: bool,
    pub screen_capture_disabled: bool,
    /// The platform's version and its security patch (`2026-11-05`).
    pub sdk: i64,
    pub security_patch: String,
    /// How many changes it made this time, and those it could not, in the
    /// platform's words.
    pub applied: i64,
    pub failed: Vec<String>,
    pub at_ms: i64,
}

/// devices' tables, in the schema's order. A schema only grows: a table is
/// appended, a column added at the end as an `Option` or a default.
#[derive(Schema)]
pub struct Devices {
    pub policy: Policy,
    pub assignment: Assignment,
    pub report: Report,
    pub sign_in: ark::auth::SignIn,
    pub role: ark::auth::Role,
    pub push: ark::auth::Push,
}

ark::auth_tables!(Devices);

// * Inputs

/// A policy written whole, in place of any of its name.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct SetPolicy {
    pub policy: Policy,
}

/// A policy taken away, and every assignment to it with it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RemovePolicy {
    #[ark(trim, non_empty, why = "which policy?")]
    pub name: String,
}

/// A person's phones held to `policy`; an empty policy, to none.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Assign {
    #[ark(trim, non_empty, why = "whose phones?")]
    pub user: String,
    pub policy: String,
}

/// What a phone read of itself and did: everything of a [`Report`] but whose
/// it is and when, which are the entry's.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct ReportInput {
    pub policy: String,
    pub owner: bool,
    pub vpn_app: Option<String>,
    pub lockdown: bool,
    pub restrictions: Vec<String>,
    pub uninstall_blocked: Vec<String>,
    pub user_control_disabled: Vec<String>,
    pub camera_disabled: bool,
    pub screen_capture_disabled: bool,
    pub sdk: i64,
    pub security_patch: String,
    pub applied: i64,
    pub failed: Vec<String>,
}

// * Functions

fn admin(ctx: &Ctx) -> Result<(), Refusal> {
    match ctx.has_role(ark::auth::ADMIN) {
        true => Ok(()),
        false => Err(Refusal::new("only the household's server changes policies")),
    }
}

/// A policy written whole (the server, from its config).
#[mutator("policies.set")]
pub fn set_policy(ctx: &Ctx, db: &mut Db<Devices>, input: SetPolicy) -> Result<(), Refusal> {
    admin(ctx)?;
    let mut p = input.policy;
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        return Err(Refusal::new("a policy needs a name"));
    }
    if p.vpn == VpnMode::AlwaysOn && p.vpn_app.trim().is_empty() {
        return Err(Refusal::new("an always-on VPN needs its app"));
    }
    db.policy.put(p)
}

/// A policy taken away, and the assignments to it.
#[mutator("policies.remove")]
pub fn remove_policy(ctx: &Ctx, db: &mut Db<Devices>, input: RemovePolicy) -> Result<(), Refusal> {
    admin(ctx)?;
    let name = input.name.trim().to_string();
    let held: Vec<Assignment> = db.assignment.scan().into_iter().filter(|a| a.policy == name).collect();
    for a in held {
        db.assignment.remove(&a)?;
    }
    match db.policy.has(name.clone()) {
        true => db.policy.delete(name),
        false => Ok(()),
    }
}

/// A person's phones held to a policy, or to none.
#[mutator("policies.assign")]
pub fn assign(ctx: &Ctx, db: &mut Db<Devices>, input: Assign) -> Result<(), Refusal> {
    admin(ctx)?;
    let user = input.user.trim().to_string();
    let policy = input.policy.trim().to_string();
    if policy.is_empty() {
        return match db.assignment.has(user.clone()) {
            true => db.assignment.delete(user),
            false => Ok(()),
        };
    }
    if !db.policy.has(policy.clone()) {
        return Err(Refusal::new("no such policy"));
    }
    db.assignment.put(Assignment { user, policy })
}

/// What the calling phone read of itself and did: its own session's row,
/// and only a live session of the caller's (`ark::auth::set_push`'s rule).
#[mutator("reports.report")]
pub fn report(ctx: &Ctx, db: &mut Db<Devices>, input: ReportInput) -> Result<(), Refusal> {
    if ctx.session.is_empty() || ctx.user().is_empty() {
        return Err(Refusal::new("only a signed-in phone reports"));
    }
    if let Some(s) = <Devices as ark::auth::AuthTables>::sign_in(db).get(ctx.session.clone()) {
        if s.user != ctx.user() || !ark::auth::live(&s, ctx.now) {
            return Err(Refusal::new("that session has ended; sign in again"));
        }
    }
    db.report.put(Report {
        session: ctx.session.clone(),
        user: ctx.user().to_string(),
        policy: input.policy,
        owner: input.owner,
        vpn_app: input.vpn_app,
        lockdown: input.lockdown,
        restrictions: input.restrictions,
        uninstall_blocked: input.uninstall_blocked,
        user_control_disabled: input.user_control_disabled,
        camera_disabled: input.camera_disabled,
        screen_capture_disabled: input.screen_capture_disabled,
        sdk: input.sdk,
        security_patch: input.security_patch,
        applied: input.applied,
        failed: input.failed,
        at_ms: ctx.now,
    })
}

/// The policy that applies to the caller's phones: one row, or none when no
/// policy is assigned to them.
#[view("policies.assigned")]
pub fn assigned(ctx: &Ctx, _: &Db<Devices>, _: ()) -> Result<Plan<Devices, Policy>, Refusal> {
    let user = ctx.user().to_string();
    Ok(scan::<Devices, Policy>().filter_with(move |db, p| db.assignment.get(user.clone()).is_some_and(|a| a.policy == p.name)))
}

/// Every policy, by name.
#[view("policies.every")]
pub fn every_policy(_: &Ctx, _: &Db<Devices>, _: ()) -> Result<Plan<Devices, Policy>, Refusal> {
    Ok(scan::<Devices, Policy>())
}

/// Every assignment the connection holds (an admin's: everyone's): what the
/// server's config holds its own against.
#[view("policies.assignments")]
pub fn every_assignment(_: &Ctx, _: &Db<Devices>, _: ()) -> Result<Plan<Devices, Assignment>, Refusal> {
    Ok(scan::<Devices, Assignment>())
}

/// What every phone the connection holds last reported.
#[view("reports.every")]
pub fn every_report(_: &Ctx, _: &Db<Devices>, _: ()) -> Result<Plan<Devices, Report>, Refusal> {
    Ok(scan::<Devices, Report>().sort(|r| Desc(r.at_ms)))
}

/// What this phone last reported of itself: one row, or none yet.
#[view("reports.own")]
pub fn own_report(ctx: &Ctx, _: &Db<Devices>, _: ()) -> Result<Plan<Devices, Report>, Refusal> {
    let session = ctx.session.clone();
    Ok(scan::<Devices, Report>().filter(move |r| r.session == session))
}

/// What a connection fed by scope holds (§1.8): every policy (they are the
/// household's), and of the assignments and reports a person's own — an
/// admin's, everyone's — and of the sign-ins, the roles and the push
/// endpoints what `ark::auth::feed` gives.
#[scope("session.feed")]
pub fn feed() -> Scope<Devices> {
    ark::auth::feed(
        Scope::new()
            .on::<Policy>(|_, _| true)
            .on::<Assignment>(|ctx, a| a.user == ctx.user() || ctx.has_role(ark::auth::ADMIN))
            .on::<Report>(|ctx, r| r.user == ctx.user() || ctx.has_role(ark::auth::ADMIN)),
    )
}

/// Every function, registered by its name.
pub fn domain() -> Domain {
    ark::register!(Domain::new::<Devices>("devices", VERSION);
        set_policy, remove_policy, assign, report, assigned, every_policy, every_assignment, every_report, own_report, feed)
    .with(ark::auth::register::<Devices>)
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}

// * What an app holds and sends

/// A view of devices', as an app holds it (`ark::views`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DevicesView {
    /// The policy that applies to this phone's person (`Policy`, one or none).
    Assigned,
    /// Every policy (`Policy`).
    Policies,
    /// What every phone the connection holds last reported (`Report`).
    Reports,
    /// What this phone last reported (`Report`, one or none).
    Own,
}

impl ViewKey for DevicesView {
    fn open(&self) -> Option<Opening> {
        let none = ().to_bytes();
        Some(match self {
            // By the views' full names: `every` is two views' last name.
            DevicesView::Assigned => Opening::of::<Policy>("policies.assigned", none),
            DevicesView::Policies => Opening::of::<Policy>("policies.every", none),
            DevicesView::Reports => Opening::of::<Report>("reports.every", none),
            DevicesView::Own => Opening::of::<Report>("reports.own", none),
        })
    }
}

/// A change of devices', by the function that makes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicesChange {
    SetPolicy(SetPolicy),
    RemovePolicy(RemovePolicy),
    Assign(Assign),
    Report(ReportInput),
}

impl DevicesChange {
    /// The function it is, by its name.
    pub fn name(&self) -> &'static str {
        match self {
            DevicesChange::SetPolicy(_) => "policies.set",
            DevicesChange::RemovePolicy(_) => "policies.remove",
            DevicesChange::Assign(_) => "policies.assign",
            DevicesChange::Report(_) => "reports.report",
        }
    }

    /// Its input, as the peer takes it: the value format's bytes.
    pub fn input(&self) -> Vec<u8> {
        match self {
            DevicesChange::SetPolicy(i) => i.to_bytes(),
            DevicesChange::RemovePolicy(i) => i.to_bytes(),
            DevicesChange::Assign(i) => i.to_bytes(),
            DevicesChange::Report(i) => i.to_bytes(),
        }
    }
}
