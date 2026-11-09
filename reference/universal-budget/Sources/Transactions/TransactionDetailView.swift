import SwiftUI

/// One transaction. Pushed on iPhone (and by double-click / Return on Mac);
/// shown in the inspector beside the table on iPad and Mac. The quick
/// changes people make while reviewing (approve, category, cleared, receipt
/// photo) happen here directly; everything else is in Edit.
struct TransactionDetailView: View {
    var transactionID: Transaction.ID
    var isInInspector = false

    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        if let transaction = store.transaction(transactionID) {
            form(transaction)
                .navigationTitle(isInInspector ? "" : store.payeeDescription(transaction))
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar {
                    if !isInInspector {
                        ToolbarItemGroup(placement: .primaryAction) {
                            Button("Edit") { model.sheet = .editTransaction(transaction.id) }
                            Menu("More", systemImage: "ellipsis.circle") {
                                TransactionActions(ids: [transaction.id])
                            }
                        }
                    }
                }
        } else {
            ContentUnavailableView("Transaction Deleted", systemImage: "trash")
        }
    }

    private func form(_ transaction: Transaction) -> some View {
        Form {
            Section {
                header(transaction)
            }

            Section {
                LabeledContent("Account", value: store.account(transaction.accountID)?.name ?? "")
                if let other = store.account(transaction.transferAccountID) {
                    LabeledContent(transaction.amount < 0 ? "Transfer To" : "Transfer From") {
                        if isInInspector {
                            Text(other.name)
                        } else {
                            NavigationLink(other.name, value: Route.account(other.id))
                        }
                    }
                }
                if !transaction.isSplit && (store.needsCategory(transaction) || transaction.categoryID != nil) {
                    CategoryPicker(selection: binding(transaction, \.categoryID))
                }
                LabeledContent("Date") {
                    Text(transaction.date, format: .dateTime.weekday(.wide).month(.wide).day().year())
                }
                Picker("Status", selection: binding(transaction, \.cleared)) {
                    Text(ClearedState.uncleared.title).tag(ClearedState.uncleared)
                    Text(ClearedState.cleared.title).tag(ClearedState.cleared)
                    if transaction.cleared == .reconciled {
                        Text(ClearedState.reconciled.title).tag(ClearedState.reconciled)
                    }
                }
                .disabled(transaction.cleared == .reconciled)
                if !transaction.memo.isEmpty {
                    LabeledContent("Memo", value: transaction.memo)
                }
            }

            if transaction.isSplit {
                Section {
                    ForEach(transaction.splits) { split in
                        LabeledContent {
                            AmountText(split.amount, style: .signed)
                        } label: {
                            Text(store.categoryName(split.categoryID) ?? "Uncategorized")
                            if !split.memo.isEmpty { Text(split.memo) }
                        }
                    }
                } header: {
                    Text("Split")
                }
            }

            ReceiptSection(transaction: transaction)

            if isInInspector {
                Section {
                    Button("Edit Transaction…", systemImage: "pencil") {
                        model.sheet = .editTransaction(transaction.id)
                    }
                    Button("Delete Transaction", systemImage: "trash", role: .destructive) {
                        store.delete([transaction.id])
                        model.transactionSelection = []
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private func header(_ transaction: Transaction) -> some View {
        VStack(spacing: 6) {
            Text(store.payeeDescription(transaction))
                .font(.title3.weight(.semibold))
                .multilineTextAlignment(.center)
            AmountText(transaction.amount, style: .signed)
                .font(.system(.largeTitle, design: .rounded, weight: .bold))
            if !transaction.isApproved {
                VStack(spacing: 8) {
                    Label(importedFrom(transaction), systemImage: "tray.and.arrow.down")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    Button("Approve", systemImage: "checkmark") {
                        store.approve([transaction.id])
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(store.needsCategory(transaction))
                    if store.needsCategory(transaction) {
                        Text("Choose a category to approve.")
                            .font(.footnote)
                            .foregroundStyle(.orange)
                    }
                }
                .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 6)
        .sensoryFeedback(.success, trigger: transaction.isApproved)
    }

    private func importedFrom(_ transaction: Transaction) -> String {
        let bank = store.bank(store.account(transaction.accountID)?.bankID)?.institution ?? "your bank"
        return "Imported from \(bank)"
    }

    private func binding<Value>(_ transaction: Transaction, _ keyPath: WritableKeyPath<Transaction, Value>) -> Binding<Value> {
        Binding {
            store.transaction(transaction.id)?[keyPath: keyPath] ?? transaction[keyPath: keyPath]
        } set: { newValue in
            guard var updated = store.transaction(transaction.id) else { return }
            updated[keyPath: keyPath] = newValue
            store.update(updated)
        }
    }
}
