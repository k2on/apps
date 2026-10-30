//! money's server as a whole: signing in through its routes and syncing the
//! household's budget on its socket; a phone's sign-in, its code sent back
//! to `money://`; a token nobody issued turned away; the budget surviving
//! a restart; and the importer's role, which nobody signing in holds.

mod common;

use common::*;
use money_domain::*;

fn open_account(name: &str) -> OpenAccount {
    OpenAccount {
        name: name.into(),
        kind: "checking".into(),
        on_budget: true,
        balance: 10_000,
        date: "2026-10-01".into(),
    }
}

fn names(server: &money_server::Server) -> Vec<String> {
    let mut n: Vec<String> = hub_rows::<Account>(server, "account").into_iter().map(|a| a.name).collect();
    n.sort();
    n
}

/// The routes and the socket agree about who somebody is: a login from
/// `/auth/login` and `/auth/exchange` is a token the socket takes, and the
/// household's other member sees what was written, entered by whom. A token
/// nobody issued is closed and keeps what it authored. Falsified by `start`
/// not handing the builder the sign-in (`.auth`): no `/auth` routes, and
/// the login failed.
#[test]
fn one_household_signs_in_and_shares_one_budget() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let login = ark_auth::client::login(&url, Some("alice"), |u| panic!("dev auth opens no browser: {u}")).unwrap();
    let mut laptop = open(ark_peer::who(&login));
    laptop.connect(&url, &login.token);
    laptop.mutate("accounts.open_account", &open_account("Checking"));
    pump_until(&mut [&mut laptop], 10, |ps| ps[0].pending() == 0);
    assert_eq!(names(&server), ["Checking"]);

    let mut sam = signed_in(&server, "sam");
    pump_until(&mut [&mut sam], 10, |ps| ps[0].cursor() == head(&server));
    let accounts: Vec<AccountLine> = sam.query("accounts.accounts", &()).unwrap();
    assert_eq!(accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Checking"]);
    let txns: Vec<Txn> = hub_rows(&server, "txn");
    assert!(txns.iter().all(|t| t.user_id == "alice"), "entered by alice");

    let mut mallory = open(ark::peer::Who {
        actor: "mallory".into(),
        session: "nope".into(),
        roles: vec![],
    });
    mallory.connect(&url, "forged");
    mallory.mutate("accounts.open_account", &open_account("Mine now"));
    pump_until(&mut [&mut mallory], 10, |ps| ps[0].closed());
    assert_eq!(mallory.pending(), 1);
    assert_eq!(names(&server), ["Checking"]);
    rt.block_on(server.stop());
}

/// A phone signs in as the browser does, through the server — the only
/// OpenID Connect client — with its own scheme as the redirect: the code
/// comes back to `money://…` and is traded for a login at
/// `/auth/exchange`. A redirect anywhere else unconfigured is refused.
/// Falsified by not allowing `PHONE_SCHEME`: the login was a 400.
#[test]
fn a_phone_signs_in_through_its_scheme() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let back = agent
        .get(&format!("{url}/auth/login"))
        .query("redirect", "money://signed-in")
        .query("user", "alice")
        .call()
        .unwrap();
    assert!((300..400).contains(&back.status()), "{}", back.status());
    let to = back.header("location").unwrap().to_string();
    assert!(to.starts_with("money://signed-in?code="), "{to}");
    let code = ark_auth::query_value(to.split_once('?').unwrap().1, "code").unwrap();
    let login = ark_auth::client::exchange(&url, &code).unwrap();
    assert_eq!(login.user.id, "alice");
    let mut phone = open(ark_peer::who(&login));
    phone.connect(&url, &login.token);
    pump_until(&mut [&mut phone], 10, |ps| ps[0].client.linked);

    let refused = agent
        .get(&format!("{url}/auth/login"))
        .query("redirect", "https://elsewhere.example/")
        .query("user", "alice")
        .call();
    assert!(matches!(refused, Err(ureq::Error::Status(400, _))), "{refused:?}");
    rt.block_on(server.stop());
}

/// Nobody who signs in holds `bank`: an import from a household member is
/// refused on the device as the hub would, and nothing lands. Falsified by
/// granting the role to everybody: the import landed.
#[test]
fn only_the_importer_imports() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| ps[0].client.linked);
    let import = Import {
        bank: "chase".into(),
        cursor: "c1".into(),
        ..Import::default()
    };
    alice.mutate("banks.import", &import);
    assert_eq!(alice.refused, ["only the bank importer imports"]);
    assert!(hub_rows::<Bank>(&server, "bank").is_empty());
    rt.block_on(server.stop());
}

/// The budget is in the data directory: a restart holds it, and a login
/// from before the restart still signs in.
#[test]
fn the_budget_and_the_logins_survive_a_restart() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let l = login(&server, "alice");
    let mut alice = open(ark_peer::who(&l));
    alice.connect(&server.url(), &l.token);
    alice.mutate("accounts.open_account", &open_account("Savings"));
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    rt.block_on(server.stop());

    let server = serve(&rt, data.path(), |_| {});
    assert_eq!(names(&server), ["Savings"]);
    let mut again = open(ark_peer::who(&l));
    again.connect(&server.url(), &l.token);
    pump_until(&mut [&mut again], 10, |ps| ps[0].client.linked && ps[0].cursor() == head(&server));
    rt.block_on(server.stop());
}

/// `/healthz` says what domain the server runs — its schema's hash and
/// version, as `domain <hash> <version> current` — which is what a phone
/// holds its own build to, and how many are connected.
#[test]
fn health_says_which_domain() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| ps[0].client.linked);
    let text = ureq::get(&format!("{}/healthz", server.url())).call().unwrap().into_string().unwrap();
    let d = money_domain::shared();
    assert!(text.starts_with("ok\n"), "{text}");
    assert!(
        text.contains(&format!("domain {} {} current\n", d.schema_hash(), money_domain::VERSION)),
        "{text}"
    );
    assert!(text.contains("connections 1\n"), "{text}");
    rt.block_on(server.stop());
}
