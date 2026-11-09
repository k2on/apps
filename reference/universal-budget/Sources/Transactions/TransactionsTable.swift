import SwiftUI

extension BudgetStore {
    /// "Transfer to Savings" rather than the stored payee, for transfers.
    func payeeDescription(_ transaction: Transaction) -> String {
        guard let other = account(transaction.transferAccountID) else { return transaction.payee }
        return transaction.amount < 0 ? "Transfer to \(other.name)" : "Transfer from \(other.name)"
    }

    func categoryDescription(_ transaction: Transaction) -> String {
        if transaction.isSplit {
            return "Split (\(transaction.splits.count) categories)"
        }
        if needsCategory(transaction) { return "Needs Category" }
        if let name = categoryName(transaction.categoryID) { return name }
        if transaction.isTransfer { return "Transfer" }
        return account(transaction.accountID)?.isBudget == true ? "Uncategorized" : "Tracking"
    }
}

/// Transactions as a sortable, multi-select table: columns on Mac and on
/// iPad in regular width; a single column of two-line rows in compact width
/// (SwiftUI shows only a table's first column there, so that column draws
/// the compact row). Used for all transactions and for an account register.
struct TransactionsTable: View {
    var transactions: [Transaction]
    var showsAccount = true

    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model
    @IsCompact private var isCompact

    @State private var sortOrder = [KeyPathComparator(\Transaction.date, order: .reverse)]

    var body: some View {
        @Bindable var model = model
        Table(transactions.sorted(using: sortOrder), selection: $model.transactionSelection, sortOrder: $sortOrder) {
            TableColumn("Date", value: \.date) { transaction in
                if isCompact {
                    CompactTransactionRow(transaction: transaction, showsAccount: showsAccount)
                } else {
                    Text(transaction.date, format: .dateTime.month(.abbreviated).day().year())
                        .monospacedDigit()
                }
            }
            .width(min: isCompact ? nil : 96, ideal: isCompact ? nil : 104, max: isCompact ? nil : 130)

            TableColumn("Payee", value: \.payee) { transaction in
                HStack(spacing: 6) {
                    ReviewDot(isVisible: !transaction.isApproved)
                    Text(store.payeeDescription(transaction))
                    if transaction.receipt.map({ !$0.isEmpty }) ?? false {
                        Image(systemName: "doc.text")
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Has receipt")
                    }
                }
            }
            .width(min: 140, ideal: 200)

            TableColumn("Category") { transaction in
                CategoryCell(transaction: transaction)
            }
            .width(min: 120, ideal: 170)

            if showsAccount {
                TableColumn("Account") { transaction in
                    Text(store.account(transaction.accountID)?.name ?? "")
                        .foregroundStyle(.secondary)
                }
                .width(min: 100, ideal: 140)
            }

            TableColumn("Memo") { transaction in
                Text(transaction.memo)
                    .foregroundStyle(.secondary)
            }
            .width(min: 80, ideal: 160)

            TableColumn("Amount", value: \.amount) { transaction in
                AmountText(transaction.amount, style: .signed)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 90, ideal: 110)

            TableColumn("Cleared", value: \.cleared.rawValue) { transaction in
                ClearedButton(transaction: transaction)
                    .frame(maxWidth: .infinity)
            }
            .width(44)
        }
        .contextMenu(forSelectionType: Transaction.ID.self) { ids in
            TransactionActions(ids: ids)
        } primaryAction: { ids in
            if ids.count == 1, let id = ids.first { model.open(.transaction(id)) }
        }
    }
}

/// The two-line row used in compact width.
struct CompactTransactionRow: View {
    var transaction: Transaction
    var showsAccount: Bool

    @Environment(BudgetStore.self) private var store

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    ReviewDot(isVisible: !transaction.isApproved)
                    Text(store.payeeDescription(transaction))
                        .fontWeight(.medium)
                        .lineLimit(1)
                }
                Text(secondaryLine)
                    .font(.subheadline)
                    .foregroundStyle(store.needsCategory(transaction) ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                    .lineLimit(1)
            }
            Spacer(minLength: 8)
            VStack(alignment: .trailing, spacing: 2) {
                AmountText(transaction.amount, style: .signed)
                    .fontWeight(.medium)
                Text(transaction.date, format: .dateTime.month(.abbreviated).day())
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var secondaryLine: String {
        let category = store.categoryDescription(transaction)
        guard showsAccount, let account = store.account(transaction.accountID) else { return category }
        return "\(category) · \(account.name)"
    }
}

/// A blue dot for transactions imported from the bank and not yet reviewed,
/// like unread mail. Keeps its space when hidden so payees line up.
struct ReviewDot: View {
    var isVisible: Bool

    var body: some View {
        Circle()
            .fill(.tint)
            .frame(width: 8, height: 8)
            .opacity(isVisible ? 1 : 0)
            .accessibilityLabel("Needs review")
            .accessibilityHidden(!isVisible)
    }
}

private struct CategoryCell: View {
    var transaction: Transaction
    @Environment(BudgetStore.self) private var store

    var body: some View {
        if store.needsCategory(transaction) && !transaction.isSplit {
            // Categorise straight from the table: the common task when
            // reviewing imported transactions.
            CategoryMenu("Needs Category", groups: store.groups) { id in
                store.categorize([transaction.id], as: id)
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .foregroundStyle(.orange)
            .fixedSize()
        } else {
            Text(store.categoryDescription(transaction))
        }
    }
}

/// Toggles a transaction between cleared and uncleared. Reconciled
/// transactions are locked.
struct ClearedButton: View {
    var transaction: Transaction
    @Environment(BudgetStore.self) private var store

    var body: some View {
        Button {
            store.setCleared(transaction.cleared == .uncleared ? .cleared : .uncleared, for: [transaction.id])
        } label: {
            Image(systemName: transaction.cleared.systemImage)
                .foregroundStyle(transaction.cleared == .uncleared ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.green))
        }
        .buttonStyle(.borderless)
        .disabled(transaction.cleared == .reconciled)
        .help(transaction.cleared.title)
        .accessibilityLabel(transaction.cleared.title)
    }
}

/// Actions for one or more transactions: the context menu, and the same
/// set appears in the menu bar's Transaction menu.
struct TransactionActions: View {
    var ids: Set<Transaction.ID>

    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        let selected = ids.compactMap { store.transaction($0) }
        if selected.contains(where: { !$0.isApproved }) {
            Button("Approve", systemImage: "checkmark") { store.approve(ids) }
        }
        if ids.count == 1, let id = ids.first {
            Button("Edit…", systemImage: "pencil") { model.sheet = .editTransaction(id) }
        }
        CategoryMenu("Categorize", groups: store.groups) { store.categorize(ids, as: $0) }
        if selected.allSatisfy({ $0.cleared != .uncleared }) {
            Button("Mark as Uncleared", systemImage: "circle") { store.setCleared(.uncleared, for: ids) }
        } else {
            Button("Mark as Cleared", systemImage: "checkmark.circle") { store.setCleared(.cleared, for: ids) }
        }
        Button("Duplicate", systemImage: "plus.square.on.square") { store.duplicate(ids) }
        Divider()
        Button("Delete", systemImage: "trash", role: .destructive) {
            store.delete(ids)
            model.transactionSelection.subtract(ids)
        }
    }
}
