//! Every change money takes, as one type: what a client sends rather than
//! a function's name and its input's JSON. A [`MoneyChange`] names its
//! function ([`MoneyChange::name`]) and carries its input, which crosses
//! to the peer as its bytes ([`MoneyChange::input`]); its refinements are
//! asked of it before it is sent ([`MoneyChange::problem`]), in the words
//! a form says under its field. The phones have it as UniFFI's enum
//! (`money/ffi`'s `Session::send`), the window builds it in Rust.

use ark::format::Encode;
use ark::prelude::*;
use ark::row::Input;

use crate::types::*;

macro_rules! changes {
    ($( $(#[$doc:meta])* $v:ident $(($t:ty))? = $name:literal, )*) => {
        /// A change of money's, by the function that makes it.
        #[derive(Clone, Debug, PartialEq, Eq, Describe)]
        #[cfg_attr(feature = "ffi", ark(ffi))]
        pub enum MoneyChange {
            $( $(#[$doc])* $v $(($t))?, )*
        }

        impl MoneyChange {
            /// The function it is, by its name (`accounts.open_account`).
            pub fn name(&self) -> &'static str {
                match self {
                    $( changes!(@pat $v $(($t))?) => $name, )*
                }
            }

            /// Its input, as the peer takes it: the value format's bytes.
            pub fn input(&self) -> Vec<u8> {
                match self {
                    $( changes!(@bind $v i $(($t))?) => changes!(@bytes i $(($t))?), )*
                }
            }

            /// Why it would be refused before it is sent: the first of its
            /// input's refinements it fails, in the domain's words ("an
            /// account needs a name"); none when it passes them.
            pub fn problem(&self) -> Option<String> {
                match self {
                    $( changes!(@bind $v i $(($t))?) => changes!(@check i $(($t))?), )*
                }
            }

            /// One of each, every input its default.
            pub fn every() -> Vec<MoneyChange> {
                vec![$( changes!(@default $v $(($t))?), )*]
            }
        }
    };
    (@pat $v:ident ($t:ty)) => { MoneyChange::$v(_) };
    (@pat $v:ident) => { MoneyChange::$v };
    (@bind $v:ident $i:ident ($t:ty)) => { MoneyChange::$v($i) };
    (@bind $v:ident $i:ident) => { MoneyChange::$v };
    (@bytes $i:ident ($t:ty)) => { $i.to_bytes() };
    (@bytes $i:ident) => { ().to_bytes() };
    (@check $i:ident ($t:ty)) => { first(<$t as Input>::check($i.clone())) };
    (@check $i:ident) => { None };
    (@default $v:ident ($t:ty)) => { MoneyChange::$v(<$t>::default()) };
    (@default $v:ident) => { MoneyChange::$v };
}

fn first<T>(r: Result<T, Vec<(String, String)>>) -> Option<String> {
    r.err().and_then(|mut e| (!e.is_empty()).then(|| e.remove(0).1))
}

changes! {
    // The budget.
    /// The budget's own categories made, once (any client's first change does it).
    Setup = "budget.setup",
    Assign(Assign) = "budget.assign",
    MoveMoney(MoveMoney) = "budget.move_money",
    AutoAssign(AutoAssign) = "budget.auto_assign",
    // Groups and categories.
    AddGroup(AddGroup) = "categories.add_group",
    RenameGroup(RenameGroup) = "categories.rename_group",
    HideGroup(HideGroup) = "categories.hide_group",
    DeleteGroup(DeleteGroup) = "categories.delete_group",
    AddCategory(AddCategory) = "categories.add_category",
    AddSubcategory(AddSubcategory) = "categories.add_subcategory",
    RenameCategory(RenameCategory) = "categories.rename_category",
    HideCategory(HideCategory) = "categories.hide_category",
    MoveCategory(MoveCategory) = "categories.move_category",
    MoveGroup(MoveGroup) = "categories.move_group",
    SetTarget(SetTarget) = "categories.set_target",
    DeleteCategory(DeleteCategory) = "categories.delete_category",
    SetFormula(SetFormula) = "categories.set_formula",
    ClearFormula(ClearFormula) = "categories.clear_formula",
    SetRule(SetRule) = "categories.set_rule",
    // Filters.
    SaveFilter(SaveFilter) = "filters.save_filter",
    RenameFilter(RenameFilter) = "filters.rename_filter",
    ChangeFilter(ChangeFilter) = "filters.change_filter",
    DeleteFilter(FilterId) = "filters.delete_filter",
    MoveFilter(MoveFilter) = "filters.move_filter",
    // Accounts.
    OpenAccount(OpenAccount) = "accounts.open_account",
    RenameAccount(RenameAccount) = "accounts.rename_account",
    CloseAccount(CloseAccount) = "accounts.close_account",
    // Transactions.
    AddTransaction(AddTransaction) = "transactions.add_transaction",
    AddSplit(AddSplit) = "transactions.add_split",
    Split(SplitTxn) = "transactions.split",
    EditTransaction(EditTransaction) = "transactions.edit_transaction",
    DeleteTransactions(Txns) = "transactions.delete_transactions",
    Approve(Txns) = "transactions.approve",
    Categorize(Categorize) = "transactions.categorize",
    SetCleared(SetCleared) = "transactions.set_cleared",
    Reconcile(Reconcile) = "transactions.reconcile",
    // Receipts.
    AddPhoto(AddPhoto) = "receipts.add_photo",
    WriteReceipt(WriteReceipt) = "receipts.write_receipt",
    Pay(MatchReceipt) = "receipts.pay",
    PayCharge(PayCharge) = "receipts.pay_charge",
    FileItems(ClassifyItems) = "receipts.file_items",
    /// The server's: a store's export read.
    AddReceipts(ReceiptImport) = "receipts.add_receipts",
    /// The server's: a store's charges read.
    AddCharges(ChargeImport) = "receipts.add_charges",
    // Banks, asked of the server's importer.
    Connect = "banks.connect",
    Reconnect(BankName) = "banks.reconnect",
    AddAccounts(BankName) = "banks.add_accounts",
    Disconnect(BankName) = "banks.disconnect",
    /// The importer asked to ask the banks now.
    Sync = "banks.sync",
    Link(LinkAccount) = "banks.link",
    OpenLinked(OpenLinked) = "banks.open",
    Ignore(IgnoreLinked) = "banks.ignore",
    /// The importer's (the `bank` role's): what a bank said.
    ImportBank(Import) = "banks.import",
    /// The importer's: an ask answered.
    Answer(Answer) = "banks.answer",
    /// The importer's: a bank that could not be imported.
    Failed(BankFailed) = "banks.failed",
    /// The importer's: a login another replaced.
    Retired(BankName) = "banks.retired",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::AccountKind;

    /// Falsified by a variant named for a function the domain does not
    /// have (`budget.assing`), and by a mutator added with no change here.
    #[test]
    fn every_mutator_is_a_change() {
        let d = crate::domain();
        let catalog = ark::domain::Catalog::of(&d);
        let ours = ["budget.", "categories.", "filters.", "accounts.", "transactions.", "receipts.", "banks."];
        let mut mutators: Vec<&str> = catalog
            .functions
            .iter()
            .filter(|f| f.kind == ark::domain::Kind::Mutator && ours.iter().any(|p| f.name.starts_with(p)))
            .map(|f| f.name.as_str())
            .collect();
        let mut changes: Vec<&str> = MoneyChange::every().iter().map(|c| c.name()).collect();
        mutators.sort();
        changes.sort();
        assert_eq!(changes, mutators);
    }

    #[test]
    fn a_change_says_its_first_problem_in_the_domains_words() {
        let open = |name: &str| {
            MoneyChange::OpenAccount(OpenAccount {
                name: name.into(),
                kind: AccountKind::Savings,
                on_budget: true,
                balance: 0,
                date: "2026-10-26".into(),
            })
        };
        assert_eq!(open("  ").problem().as_deref(), Some("an account needs a name"));
        assert_eq!(open("Savings").problem(), None);
        assert_eq!(MoneyChange::Connect.problem(), None);
        let bad_day = MoneyChange::Reconcile(Reconcile {
            date: "26 Oct".into(),
            ..Reconcile::default()
        });
        assert_eq!(bad_day.problem().as_deref(), Some("a day, as 2026-10-26"));
    }

    /// The kind crosses as its key, as the text it was before it was a
    /// kind: the bytes and the JSON a client sent before still read.
    #[test]
    fn an_accounts_kind_is_its_key_on_the_wire() {
        let o = OpenAccount {
            name: "Card".into(),
            kind: AccountKind::Credit,
            ..OpenAccount::default()
        };
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["kind"], "credit");
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Before {
            name: String,
            kind: String,
            on_budget: bool,
            balance: i64,
            date: String,
        }
        let before = Before {
            name: "Card".into(),
            kind: "credit".into(),
            on_budget: false,
            balance: 0,
            date: String::new(),
        };
        assert_eq!(ark::format::Encode::to_bytes(&before), o.to_bytes());
        assert!(serde_json::from_value::<OpenAccount>(
            serde_json::json!({"name": "x", "kind": "boat", "on_budget": true, "balance": 0, "date": "2026-10-26"})
        )
        .is_err());
    }
}
