//! money's host on the phones: its part of its host (`money_app::host`,
//! written once for every platform) under the app host every Ark app has
//! (`ark_ffi::ArkHost`, which runs the first run and the session's link),
//! through ark-ffi's `Hosted`. What is money's here is only how its
//! replica and its demo open; the objects the phone holds are each
//! screen's runtime over it ([`crate::app`]), and what only the phone's
//! shell does ([`NativeAsk`]).

use std::sync::Arc;

use ark_ffi::hosted::{Hosted, PhoneApp};
use ark_ffi::{ArkHost, Launch, Login, Peer, Refusal, Twin};
use money_app::host::{Money, Native};
use money_app::routes::Route;
use money_app::MoneyRequest;
use money_core::MoneyChange;

use crate::{AccountsApp, BudgetApp, InsightsApp, RegisterApp, SheetsApp, ShellApp, TransactionsApp, WelcomeApp};

type Tag = crate::UniFfiTag;

/// What only the phone's shell does, asked of it in order: taken when it
/// is told something moved ([`MoneyHost::take_asks`]).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum NativeAsk {
    /// A page on the section's stack: an account's register, All Accounts,
    /// Connected Banks, a bank.
    Open { route: <Route as Twin<Tag>>::Ffi },
    /// Settings, on the pane of this title (empty: as it was).
    Settings { pane: String },
    /// Add a Device's page.
    AddDevice,
    /// A link taken as a launch: what the shell shows of its own put away
    /// (a section's pages, Settings, a sheet of its own).
    Reset,
}

/// money on the phones: how its replica and its demo open.
pub struct MoneyPhone;

impl PhoneApp for MoneyPhone {
    type App = Money;

    fn info(&self) -> ark_ffi::ark_auth::onboarding::AppInfo {
        ark_ffi::ark_auth::onboarding::AppInfo {
            name: "Money".into(),
            scheme: "money".into(),
            demo: true,
            what: "your budget".into(),
        }
    }
    fn copy(&self, scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy {
        money_app::welcome::copy(scan, demo)
    }
    fn domain(&self) -> String {
        crate::money_domain()
    }
    fn open(&self, dir: &str, login: Login) -> Result<Arc<Peer>, Refusal> {
        crate::money_process_open(dir.into(), Some(login))
    }
    fn demo(&self, today: Option<String>) -> Result<Arc<Peer>, Refusal> {
        crate::money_demo(today)
    }
    fn today(&self) -> String {
        crate::money_today()
    }
    fn encode(change: &MoneyChange) -> (&'static str, Vec<u8>) {
        (change.name(), change.input())
    }
    fn signs_out(native: &Native) -> bool {
        matches!(native, Native::SignOut)
    }
}

/// money's host, as the phone holds it: the app host every Ark app has
/// ([`ArkHost`], [`MoneyHost::host`]), the first run's page
/// ([`MoneyHost::welcome`]), and money's screens' runtimes while a session
/// is open.
#[derive(uniffi::Object)]
pub struct MoneyHost {
    host: Arc<ArkHost>,
    money: Arc<Hosted<MoneyPhone>>,
}

#[uniffi::export]
impl MoneyHost {
    /// money's host as the phone launched it, its login kept in `store`
    /// (ark-secure-store's `PhoneSecureStore`; none: for this run alone).
    #[uniffi::constructor]
    pub fn new(launch: Launch, store: Option<Arc<dyn ark_secure_store::phone::SecureStore>>) -> Arc<MoneyHost> {
        let money = Hosted::new(MoneyPhone);
        let host = ArkHost::new(money.clone(), launch, store);
        Arc::new(MoneyHost { host, money })
    }

    /// The app host every Ark app has: its first run, its link, the shell's
    /// notices.
    pub fn host(&self) -> Arc<ArkHost> {
        self.host.clone()
    }

    /// The first run's pages, as the generated first run reads them.
    pub fn welcome(&self) -> Arc<WelcomeApp> {
        WelcomeApp::new(self.host.clone())
    }

    /// money's shell: its sections, toolbar, inspector, search, Settings and
    /// commands; none until a session is open.
    pub fn shell(&self) -> Option<Arc<ShellApp>> {
        self.money.is_open().then(|| Arc::new(ShellApp(self.money.clone())))
    }

    pub fn accounts(&self) -> Option<Arc<AccountsApp>> {
        self.money.is_open().then(|| Arc::new(AccountsApp(self.money.clone())))
    }

    pub fn budget(&self) -> Option<Arc<BudgetApp>> {
        self.money.is_open().then(|| Arc::new(BudgetApp(self.money.clone())))
    }

    pub fn transactions(&self) -> Option<Arc<TransactionsApp>> {
        self.money.is_open().then(|| Arc::new(TransactionsApp(self.money.clone())))
    }

    pub fn insights(&self) -> Option<Arc<InsightsApp>> {
        self.money.is_open().then(|| Arc::new(InsightsApp(self.money.clone())))
    }

    pub fn sheets(&self) -> Option<Arc<SheetsApp>> {
        self.money.is_open().then(|| Arc::new(SheetsApp(self.money.clone())))
    }

    /// A register's runtime, for as long as the phone shows it: the account
    /// of this hex id's, or every account's for none.
    pub fn register(&self, account: Option<String>) -> Option<Arc<RegisterApp>> {
        RegisterApp::over(self.money.clone(), account)
    }

    /// The session the screens' runtimes are over: grows with every session
    /// opened, so the phone makes its holders again; none open is zero.
    pub fn generation(&self) -> u64 {
        self.money.generation()
    }

    /// What the phone's shell is asked to do, in order, taken.
    pub fn take_asks(&self) -> Vec<NativeAsk> {
        self.money
            .take_natives()
            .into_iter()
            .filter_map(|n| match n {
                Native::Open { route } => Some(NativeAsk::Open { route: route.ffi() }),
                Native::Settings { pane } => Some(NativeAsk::Settings { pane }),
                Native::AddDevice => Some(NativeAsk::AddDevice),
                Native::Reset => Some(NativeAsk::Reset),
                Native::SignOut => None,
            })
            .collect()
    }

    /// Who is using the budget and where: the account sheet's.
    pub fn household(&self) -> <money_app::host::Household as Twin<Tag>>::Ffi {
        self.money
            .with(|o| o.hosting.with(&mut o.place, |m, cx| m.household(cx)))
            .unwrap_or_default()
            .ffi()
    }

    /// The session's day (`2026-10-26`): the demo's in the demo.
    pub fn today(&self) -> String {
        self.money.today()
    }

    /// The month the budget shows (`2026-10`).
    pub fn month(&self) -> String {
        self.money.read(|m, _| m.month().to_string()).unwrap_or_default()
    }

    /// Ask the bank for anything new (pulled down): what came of it said.
    pub fn sync_bank(&self) {
        self.money
            .with(|o| o.hosting.with(&mut o.place, |_, cx| cx.request(MoneyRequest::Refresh)));
    }
}
