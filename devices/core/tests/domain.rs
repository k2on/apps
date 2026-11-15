//! devices' rules over one store: only the server's admin writes policies
//! and assignments, a phone writes only its own session's report, and a
//! phone sees the policy of the person it is signed in as.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::sync::OnceLock;

use ark::domain::Domain;
use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::Store;
use devices_core::*;

fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(devices_core::domain)
}

struct Book {
    store: Store,
    n: i64,
}

impl Book {
    fn new() -> Book {
        Book {
            store: Store::new(domain().schema.clone(), ark::schema::Space::Durable),
            n: 0,
        }
    }

    fn ctx(&mut self, user: &str, session: &str, roles: &[&str]) -> Ctx {
        self.n += 1;
        Ctx {
            id: EntryId::of("s", self.n),
            actor: user.into(),
            session: session.into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            now: 1_793_000_000_000 + self.n,
            ..Default::default()
        }
    }

    fn run(&mut self, ctx: &Ctx, name: &str, input: &impl Encode) -> Result<(), String> {
        let f = domain()
            .get(name)
            .and_then(|f| f.proc.clone())
            .unwrap_or_else(|| panic!("{name} writes nothing"));
        f.apply(&mut self.store, ctx, &input.to_bytes()).map(drop).map_err(|r| r.0)
    }

    fn admin(&mut self, name: &str, input: &impl Encode) -> Result<(), String> {
        let ctx = self.ctx("server", "", &[ark::auth::ADMIN]);
        self.run(&ctx, name, input)
    }

    fn view<T: Encode>(&self, user: &str, session: &str, name: &str) -> Vec<T> {
        let ctx = Ctx {
            id: EntryId::of("q", 0),
            actor: user.into(),
            session: session.into(),
            ..Default::default()
        };
        let b = domain()
            .ask(&self.store, &ctx, name, &().to_bytes())
            .unwrap_or_else(|r| panic!("{name}: {r}"));
        Vec::<T>::from_bytes(&b).unwrap()
    }
}

fn tailscale(name: &str) -> SetPolicy {
    SetPolicy {
        policy: Policy {
            name: name.into(),
            vpn: VpnMode::AlwaysOn,
            vpn_app: "com.tailscale.ipn".into(),
            vpn_lockdown: true,
            restrictions: vec![Setting {
                name: "no_config_vpn".into(),
                on: true,
            }],
            ..Default::default()
        },
    }
}

fn assign(user: &str, policy: &str) -> Assign {
    Assign {
        user: user.into(),
        policy: policy.into(),
    }
}

#[test]
fn only_the_server_writes_policies_and_assignments() {
    let mut b = Book::new();
    let ann = b.ctx("ann", "phone", &[]);
    assert!(b.run(&ann, "policies.set", &tailscale("base")).unwrap_err().contains("server"));
    b.admin("policies.set", &tailscale("base")).unwrap();
    assert!(b.run(&ann, "policies.assign", &assign("ann", "")).unwrap_err().contains("server"));
    // An always-on VPN names its app.
    let mut no_app = tailscale("bare");
    no_app.policy.vpn_app = " ".into();
    assert!(b.admin("policies.set", &no_app).unwrap_err().contains("app"));
    assert!(b.admin("policies.assign", &assign("ann", "nope")).unwrap_err().contains("no such policy"));
}

#[test]
fn a_phone_sees_the_policy_of_its_person_and_no_other() {
    let mut b = Book::new();
    b.admin("policies.set", &tailscale("kids")).unwrap();
    b.admin("policies.set", &tailscale("adults")).unwrap();
    b.admin("policies.assign", &assign("ann", "kids")).unwrap();
    let ann: Vec<Policy> = b.view("ann", "phone", "policies.assigned");
    assert_eq!(ann.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["kids"]);
    assert!(
        b.view::<Policy>("bob", "other", "policies.assigned").is_empty(),
        "nobody assigned bob one"
    );
    // Reassigned, and then the policy taken away with its assignments.
    b.admin("policies.assign", &assign("ann", "adults")).unwrap();
    assert_eq!(b.view::<Policy>("ann", "phone", "policies.assigned")[0].name, "adults");
    b.admin("policies.remove", &RemovePolicy { name: "adults".into() }).unwrap();
    assert!(b.view::<Policy>("ann", "phone", "policies.assigned").is_empty());
    assert_eq!(b.view::<Policy>("ann", "phone", "policies.every").len(), 1);
    // Unassigned outright.
    b.admin("policies.assign", &assign("ann", "kids")).unwrap();
    b.admin("policies.assign", &assign("ann", "")).unwrap();
    assert!(b.view::<Policy>("ann", "phone", "policies.assigned").is_empty());
}

#[test]
fn a_phone_reports_only_as_its_own_session() {
    let mut b = Book::new();
    let input = ReportInput {
        policy: "kids".into(),
        owner: true,
        vpn_app: Some("com.tailscale.ipn".into()),
        lockdown: true,
        applied: 3,
        ..Default::default()
    };
    let nobody = b.ctx("", "", &[]);
    assert!(b.run(&nobody, "reports.report", &input).unwrap_err().contains("signed-in"));
    let phone = b.ctx("ann", "phone-1", &[]);
    b.run(&phone, "reports.report", &input).unwrap();
    let tablet = b.ctx("ann", "tablet-1", &[]);
    b.run(
        &tablet,
        "reports.report",
        &ReportInput {
            owner: false,
            ..input.clone()
        },
    )
    .unwrap();
    // Each session's own row, dated by its entry.
    let own: Vec<Report> = b.view("ann", "phone-1", "reports.own");
    assert_eq!(own.len(), 1);
    assert_eq!(
        (own[0].session.as_str(), own[0].user.as_str(), own[0].applied, own[0].at_ms),
        ("phone-1", "ann", 3, phone.now)
    );
    // A second report replaces the first.
    let again = b.ctx("ann", "phone-1", &[]);
    b.run(&again, "reports.report", &ReportInput { applied: 0, ..input }).unwrap();
    let every: Vec<Report> = b.view("ann", "phone-1", "reports.every");
    assert_eq!(every.len(), 2);
    assert_eq!(every[0].session, "phone-1", "newest first");
    assert_eq!(every[0].applied, 0);
}
