//! money for its phones (`docs/plan-v5.md` §1.16, N3): the bridge
//! (`ark-ffi`) with money's domain and its screens' runtime, exported
//! through UniFFI — an adapter (layers.toml), so money-core and money-app
//! know nothing of UniFFI. Every record and message that crosses is a
//! twin written here from the core's and the app's types (`ark::twins!`
//! below; ui/docs/app.md, Layers), with the rules the phones call as its
//! methods ([`core`]). The phones' library is no crate of money's: nix
//! generates it (nix/ark-app.nix, `money-ffi`), a shim that links this
//! crate as a static library for the iPhone and the Mac and a shared one
//! for Android. The apps open their replica with
//! [`money_open`] — or the demo's household with [`money_demo`] — and
//! send every change of the domain's through it as a `MoneyChange`
//! ([`Session::send`]). Every view hands over UniFFI's records, and the
//! records carry the domain's rules as methods ([`core`]): nothing
//! crosses as JSON.
//!
//! Signing in is `ark_auth::onboarding`'s machine, through the bridge: the server's page
//! (`Effect::Browse`, redirecting to `money://auth`), the code exchanged
//! (`exchange`), the peer told (`Peer::login`) and dialled (`socket_url`,
//! `Peer::connect`). A refusal is a value wherever the app can ask before
//! it acts (`Peer::check`) or after (`Peer::standing`, `take_rejections`);
//! [`money_send`] is a mutation whose own refusal is one too. A [`Hall`] is
//! a money server in the same process, over memory, that devices dial
//! without a socket: what the apps' tests sync through.
//!
//! The session's views ([`MoneyView`], [`Budget`], [`Holding`]) are what
//! the screens still hand-written in money/native read; they go as those
//! screens move into this crate's views.

use std::sync::Arc;

use ark_ffi::{Login, Peer, Refusal};
use ark_peer::Config;

uniffi::setup_scaffolding!();

// What crosses, written here from the core's and the app's types.
ark::twins!(
    money_core::{
        accounts::{AccountKind, AccountShelf, Shelving},
        banks::Follow,
        changes::MoneyChange,
        schema::{AskState, Direction, Filter, Term},
        transactions::Draft,
        types::{
            AccountLine, AddCategory, AddGroup, AddPhoto, AddSplit, AddSubcategory, AddTransaction, Answer, AskLine,
            Assign, Auto, AutoAssign, BankAccount, BankFailed, BankLine, BankName, BankRemoved, BankTxn, BudgetLine,
            Categorize, ChangeFilter, ChargeImport, ClassifyItems, ClearFormula, Cleared, CloseAccount,
            DeleteCategory, DeleteGroup, EditTransaction, FilterId, GroupLine, HideCategory, HideGroup,
            IgnoreLinked, Import, ItemLine, ItemRef, LinkAccount, LinkedLine, MatchReceipt, MoveCategory,
            MoveFilter, MoveGroup, MoveMoney, OpenAccount, OpenLinked, PayCharge, PayeeLine, PhotoLine,
            ReceiptImport, ReceiptLine, Reconcile, RenameAccount, RenameCategory, RenameFilter, RenameGroup,
            SaveFilter, SetCleared, SetFormula, SetRule, SetTarget, SpendingLine, SplitPart, SplitTxn,
            StoreCharge, StoreItem, StoreReceipt, Summary, TxnLine, Txns, WriteReceipt,
        },
        words::{Meter, Tone},
    },
    money_app::{
        MoneyRequest,
        accounts::{AccountItem, AccountsMsg, AccountsPage, AccountsShelf, AddForm, KindChoice, RenameForm},
        insights::{InsightsMsg, InsightsPage, PlanRow, SpentGroup},
        budget::{AmountForm, BudgetChoice, BudgetMsg, BudgetPage, CategoryForm, CategoryItem, GroupItem, MoveForm, NameForm},
        receipts::{FormItem, MatchRow, MatchSheet, PhotoSheet, ReadingPhoto, ReceiptForm, ReceiptSection},
        register::{RegisterDay, RegisterMsg, RegisterPage, RegisterRow},
        sheets::{CategorizeSheet, PayeeChoice, ReconcileSheet, SheetChoice, SheetsMsg, SheetsPage, TxnSheet},
        transactions::{CategorizeForm, CategoryChoice, DeleteQuestion, TransactionsMsg, TransactionsPage, TxnDay, TxnItem},
        routes::Route,
        shell::{AccountPlace, Appearance, ShellMsg, ShellPage},
        shell::settings::{AskItem, BankItem, BankPage, FeedChoice, FeedItem, FeedTarget, SessionFacts, SettingsPage},
        shell::inspect::{CategoryInspector, DetailLine, TransactionInspector},
        words::{BankStatus, Captions},
    },
    ark_app::welcome::{
        AddDevicePage, AddDeviceMsg, ConfirmPage, IntroPage, OpeningPage, ServerPage, SignInPage, WelcomeMsg, WelcomePage,
    },
);

// money's first run: the shared pages, money's words (money/app/src/welcome.rs).
ark_ffi::first_run_app!(money_app::welcome::copy);

/// Numbers, days and lengths as every client writes them: in this library,
/// so its bindings (`uniffi.ark_format`; `formatAmount`, `formatLength` …)
/// come with the app's, and the phones print what the desktop prints.
pub use ark_format_ffi;

mod app;
pub mod core;
mod demo;
mod hall;
mod session;
pub use app::{register_devices, AccountsApp, AppShell, BudgetApp, InsightsApp, RegisterApp, SheetsApp, ShellApp, TransactionsApp};
pub use demo::{money_demo, DemoBank, DemoSync};
pub use hall::Hall;
pub use session::{Budget, Changed, Holding, MoneyView, Session};

mod routes;
pub use routes::route_from_url;

/// money's peer as a phone is made of it: the domain for the log, and no
/// live space — a budget has nothing but what was said.
pub fn config() -> Config {
    Config::of(money_core::shared())
}

/// money's peer on the disk at `dir`, as whoever last signed in there or
/// as `login`.
#[uniffi::export]
pub fn money_open(dir: String, login: Option<Login>) -> Result<Arc<Peer>, Refusal> {
    Peer::open(config(), &dir, login)
}

/// money's peer on the disk at `dir`, as the process's one runtime
/// (`ark_ffi::ArkProcess::shared`): opened the first time, the same peer
/// for every Activity, Scene or window after, and running while none
/// shows; another `dir` (another server) lets the old one go. Signing out
/// is `ArkProcess::shared().sign_out()`.
#[uniffi::export]
pub fn money_process_open(dir: String, login: Option<Login>) -> Result<Arc<Peer>, Refusal> {
    ark_ffi::ArkProcess::shared().open(&dir.clone(), || money_open(dir, login))
}

/// The domain this library carries as a server's `/healthz` names it: its
/// schema's hash and its version (the server's `domain` line).
#[uniffi::export]
pub fn money_domain() -> String {
    let d = money_core::shared();
    format!("{} {}", d.schema_hash(), d.version)
}

/// What became of a mutation as the device sent it: its intent, whose
/// standing the app then follows, or the domain's own sentence for why not.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Sent {
    Intent { id: String },
    Refused { why: String },
}

/// A change sent over `peer` with no session, its refusal a value: the
/// tests', and anything that sends before it holds a view.
#[uniffi::export]
pub fn money_send(peer: Arc<Peer>, change: <money_core::MoneyChange as ark_ffi::Twin<crate::UniFfiTag>>::Ffi) -> Sent {
    let c = <money_core::MoneyChange as ark_ffi::Twin<crate::UniFfiTag>>::from_ffi(change);
    match peer.mutate_encoded(c.name(), c.input()) {
        Ok(id) => Sent::Intent { id },
        Err(Refusal::Refused { why }) => Sent::Refused { why },
    }
}

/// Today, as the budget writes a day (`2026-10-26`), in UTC as the domain
/// reads one.
#[uniffi::export]
pub fn money_today() -> String {
    money_core::dates::day_of_ms(ark_peer::now_ms())
}

/// The emoji the phones draw before a category's name, by the words in
/// it ([`money_app::look::category_emoji`]); empty when none says.
#[uniffi::export]
pub fn money_category_emoji(name: String) -> String {
    money_app::look::category_emoji(&name).into()
}

/// The month `n` months from `month` (`2026-10`), either way.
#[uniffi::export]
pub fn money_add_months(month: String, n: i64) -> String {
    money_core::dates::add_months(&month, n)
}
