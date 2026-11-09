import SwiftUI

/// One account's register: its transactions, with cleared, uncleared and
/// working balances above, and reconcile and sync in the toolbar.
struct AccountRegisterView: View {
    var accountID: Account.ID

    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model
    @State private var searchText = ""

    var body: some View {
        @Bindable var model = model
        if let account = store.account(accountID) {
            let transactions = store.transactions(matching: [.account(accountID)], text: searchText)
            TransactionsTable(transactions: transactions, showsAccount: false)
                .overlay {
                    if transactions.isEmpty {
                        if searchText.isEmpty {
                            ContentUnavailableView {
                                Label("No Transactions", systemImage: "list.bullet.rectangle")
                            } description: {
                                Text(account.bankID == nil
                                     ? "Add this account's transactions as you make them."
                                     : "Transactions will appear here when the bank syncs.")
                            } actions: {
                                Button("New Transaction") { model.newTransaction(in: store, account: accountID) }
                            }
                        } else {
                            ContentUnavailableView.search(text: searchText)
                        }
                    }
                }
                .safeAreaInset(edge: .top, spacing: 0) {
                    RegisterSummary(account: account)
                }
                .navigationTitle(account.name)
                .searchable(text: $searchText, prompt: Text("Search \(account.name)"))
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        if account.bankID != nil {
                            Button("Sync", systemImage: "arrow.triangle.2.circlepath") {
                                if let bank = account.bankID { store.sync(bank) }
                            }
                            .help("Sync with \(store.bank(account.bankID)?.institution ?? "bank") (⌘R)")
                        }
                        Button("Reconcile", systemImage: "checkmark.seal") {
                            model.sheet = .reconcile(accountID)
                        }
                        .help("Reconcile with a statement (⌥⌘R)")
                        Button("New Transaction", systemImage: "plus") {
                            model.newTransaction(in: store, account: accountID)
                        }
                        .help("New Transaction (⌘N)")
                        Button("Transaction Details", systemImage: "sidebar.trailing") {
                            model.showsInspector.toggle()
                        }
                        .help("Show or hide transaction details (⌥⌘I)")
                    }
                }
                .inspector(isPresented: $model.showsInspector) {
                    TransactionInspector()
                        .inspectorColumnWidth(min: 300, ideal: 340, max: 440)
                }
        } else {
            ContentUnavailableView("Account Not Found", systemImage: "building.columns")
        }
    }
}

/// Cleared + Uncleared = Working balance, and the account's bank status.
private struct RegisterSummary: View {
    var account: Account

    @Environment(BudgetStore.self) private var store

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 20) { balances }
                VStack(alignment: .leading, spacing: 6) { balances }
            }
            .labeledContentStyle(.vertical)

            if let bank = store.bank(account.bankID) {
                BankStatusLine(bank: bank, reviewCount: store.unapproved(in: account.id).count)
            } else if let date = account.lastReconciled {
                Text("Reconciled \(date.formatted(date: .abbreviated, time: .omitted))")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal)
        .padding(.vertical, 10)
        .background(.bar)
    }

    @ViewBuilder
    private var balances: some View {
        LabeledContent("Cleared") { AmountText(store.clearedBalance(of: account.id)) }
        LabeledContent("Uncleared") { AmountText(store.unclearedBalance(of: account.id)) }
        LabeledContent("Working Balance") { AmountText(store.balance(of: account.id)) }
    }
}

struct BankStatusLine: View {
    var bank: BankConnection
    var reviewCount: Int

    @Environment(SceneModel.self) private var model

    var body: some View {
        HStack(spacing: 6) {
            switch bank.status {
            case .connected:
                Image(systemName: "checkmark.icloud")
                    .foregroundStyle(.secondary)
                Text("\(bank.institution) · Synced \(bank.lastSynced.formatted(date: .abbreviated, time: .shortened))")
                    .foregroundStyle(.secondary)
            case .needsAttention(let message):
                Image(systemName: "exclamationmark.icloud")
                    .foregroundStyle(.orange)
                Text("\(bank.institution): \(message)")
                    .foregroundStyle(.orange)
            }
            if reviewCount > 0 {
                Button("^[\(reviewCount) to review](inflect: true)") {
                    model.showTransactions(matching: [.needsReview])
                }
                .buttonStyle(.borderless)
            }
        }
        .font(.footnote)
        .accessibilityElement(children: .combine)
    }
}
