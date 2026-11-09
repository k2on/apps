import PhotosUI
import SwiftUI

/// Adds or edits a transaction: an expense, income, or a transfer between
/// accounts, optionally split across categories and with a receipt.
/// Presented as a sheet everywhere: a form with Cancel and Save.
struct TransactionEditor: View {
    enum Kind: String, CaseIterable, Identifiable {
        case expense, income, transfer
        var id: Self { self }
        var title: String {
            switch self {
            case .expense: "Expense"
            case .income: "Income"
            case .transfer: "Transfer"
            }
        }
    }

    private enum Field: Hashable { case amount, payee, memo }

    let isNew: Bool
    @State private var draft: Transaction
    @State private var kind: Kind
    @State private var magnitude: Decimal
    /// Transfers are edited from either side; keep the direction.
    @State private var isOutgoingTransfer: Bool
    @State private var isSplit: Bool
    /// Split amounts as positive numbers while editing.
    @State private var splitLines: [Split]
    @State private var receiptItems: [ReceiptItem]
    @State private var receiptImage: Data?
    @State private var pickedPhoto: PhotosPickerItem?
    @FocusState private var focusedField: Field?

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @Environment(\.dismiss) private var dismiss

    init(draft: Transaction, isNew: Bool) {
        self.isNew = isNew
        _draft = State(initialValue: draft)
        _kind = State(initialValue: draft.isTransfer ? .transfer : draft.amount > 0 ? .income : .expense)
        _magnitude = State(initialValue: abs(draft.amount))
        _isOutgoingTransfer = State(initialValue: draft.amount <= 0)
        _isSplit = State(initialValue: draft.isSplit)
        _splitLines = State(initialValue: draft.splits.map { split in
            var line = split
            line.amount = abs(split.amount)
            return line
        })
        _receiptItems = State(initialValue: draft.receipt?.items ?? [])
        _receiptImage = State(initialValue: draft.receipt?.imageData)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Type", selection: $kind) {
                        ForEach(Kind.allCases) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()

                    TextField("Amount", value: $magnitude, format: .currency(code: currencyCode))
                        .font(.title2.weight(.semibold))
                        .monospacedDigit()
                        .decimalKeyboard()
                        .focused($focusedField, equals: .amount)
                        .accessibilityLabel(kind == .income ? "Amount received" : "Amount spent")
                }

                Section {
                    accountPickers
                    if kind != .transfer {
                        payeeField
                    }
                    if showsCategory && !isSplit {
                        CategoryPicker(selection: $draft.categoryID, includesReadyToAssign: kind == .income)
                    }
                    DatePicker("Date", selection: $draft.date, displayedComponents: .date)
                    Toggle("Cleared", isOn: Binding(
                        get: { draft.cleared != .uncleared },
                        set: { draft.cleared = $0 ? .cleared : .uncleared }
                    ))
                    .disabled(draft.cleared == .reconciled)
                    TextField("Memo", text: $draft.memo, axis: .vertical)
                        .focused($focusedField, equals: .memo)
                }

                if kind == .expense {
                    splitSection
                }

                receiptSection
            }
            .formStyle(.grouped)
            .navigationTitle(isNew ? "New Transaction" : "Edit Transaction")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(isNew ? "Add" : "Save") { save() }
                        .disabled(!isValid)
                }
            }
            .defaultFocus($focusedField, isNew ? .amount : nil)
            .onChange(of: pickedPhoto) { _, item in
                guard let item else { return }
                Task {
                    receiptImage = try? await item.loadTransferable(type: Data.self)
                    pickedPhoto = nil
                }
            }
        }
        .sheetFrame(width: 520, height: 700)
    }

    // MARK: - Sections

    @ViewBuilder
    private var accountPickers: some View {
        Picker(kind == .transfer && !isOutgoingTransfer ? "To Account" : "Account", selection: $draft.accountID) {
            ForEach(store.accounts) { account in
                Text(account.name).tag(account.id)
            }
        }
        if kind == .transfer {
            Picker(isOutgoingTransfer ? "To" : "From", selection: $draft.transferAccountID) {
                Text("Choose an Account").tag(Account.ID?.none)
                ForEach(store.accounts.filter { $0.id != draft.accountID }) { account in
                    Text(account.name).tag(Account.ID?.some(account.id))
                }
            }
        }
    }

    private var payeeField: some View {
        HStack {
            TextField("Payee", text: $draft.payee)
                .focused($focusedField, equals: .payee)
                .onSubmit(prefillCategory)
            Menu {
                ForEach(store.payees.prefix(10), id: \.self) { payee in
                    Button(payee) {
                        draft.payee = payee
                        prefillCategory()
                    }
                }
            } label: {
                Label("Recent Payees", systemImage: "clock.arrow.circlepath")
                    .labelStyle(.iconOnly)
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .fixedSize()
        }
    }

    private var splitSection: some View {
        Section {
            Toggle("Split Across Categories", isOn: $isSplit.animation())
                .onChange(of: isSplit) { _, isOn in
                    if isOn && splitLines.isEmpty {
                        splitLines = [
                            Split(categoryID: draft.categoryID, amount: magnitude),
                            Split(categoryID: nil, amount: 0),
                        ]
                    }
                }
            if isSplit {
                ForEach($splitLines) { $line in
                    HStack {
                        CategoryPicker(title: "Category", selection: $line.categoryID, includesReadyToAssign: false)
                            .labelsHidden()
                        TextField("Amount", value: $line.amount, format: .currency(code: currencyCode))
                            .multilineTextAlignment(.trailing)
                            .monospacedDigit()
                            .decimalKeyboard()
                    }
                }
                .onDelete { splitLines.remove(atOffsets: $0) }
                Button("Add Split", systemImage: "plus") {
                    splitLines.append(Split(categoryID: nil, amount: max(0, splitRemainder)))
                }
            }
        } footer: {
            if isSplit {
                if splitRemainder == 0 {
                    Text("The split adds up to the total.")
                } else {
                    Text("\(splitRemainder.formatted(currency: currencyCode)) left to split.")
                        .foregroundStyle(.orange)
                }
            }
        }
    }

    private var receiptSection: some View {
        Section {
            if let receiptImage, let image = Image(receiptData: receiptImage) {
                image
                    .resizable()
                    .scaledToFit()
                    .frame(maxHeight: 200)
                    .clipShape(.rect(cornerRadius: 8))
                    .accessibilityLabel("Receipt photo")
            }
            PhotosPicker(selection: $pickedPhoto, matching: .images) {
                Label(receiptImage == nil ? "Attach Photo" : "Replace Photo", systemImage: "photo.badge.plus")
            }
            ForEach($receiptItems) { $item in
                HStack {
                    TextField("Item", text: $item.name)
                    Stepper("×\(item.quantity)", value: $item.quantity, in: 1...99)
                        .fixedSize()
                        .monospacedDigit()
                    TextField("Price", value: $item.price, format: .currency(code: currencyCode))
                        .multilineTextAlignment(.trailing)
                        .monospacedDigit()
                        .decimalKeyboard()
                        .frame(maxWidth: 110)
                }
            }
            .onDelete { receiptItems.remove(atOffsets: $0) }
            Button("Add Item", systemImage: "plus") {
                receiptItems.append(ReceiptItem(name: "", price: 0))
            }
        } header: {
            Text("Receipt")
        } footer: {
            if !receiptItems.isEmpty {
                Text("Items total \(receiptItems.reduce(Decimal(0)) { $0 + $1.total }.formatted(currency: currencyCode)).")
            }
        }
    }

    // MARK: - Logic

    /// Categories matter when money enters or leaves the budget: not for a
    /// transfer between two budget accounts, or within tracking accounts.
    private var showsCategory: Bool {
        guard store.account(draft.accountID)?.isBudget == true else { return false }
        guard kind == .transfer else { return true }
        return store.account(draft.transferAccountID).map { !$0.isBudget } ?? false
    }

    private var splitRemainder: Decimal {
        magnitude - splitLines.reduce(0) { $0 + $1.amount }
    }

    private var isValid: Bool {
        guard magnitude > 0 else { return false }
        switch kind {
        case .transfer:
            return draft.transferAccountID != nil && draft.transferAccountID != draft.accountID
        case .expense, .income:
            guard !draft.payee.trimmingCharacters(in: .whitespaces).isEmpty else { return false }
            if isSplit && kind == .expense {
                return splitLines.count >= 2 && splitRemainder == 0
            }
            return true
        }
    }

    private func prefillCategory() {
        if draft.categoryID == nil, let last = store.lastCategory(for: draft.payee) {
            draft.categoryID = last
        }
    }

    private func save() {
        var transaction = draft
        switch kind {
        case .expense:
            transaction.amount = -magnitude
            transaction.transferAccountID = nil
        case .income:
            transaction.amount = magnitude
            transaction.transferAccountID = nil
            if transaction.categoryID == nil { transaction.categoryID = Category.readyToAssign }
        case .transfer:
            transaction.amount = isOutgoingTransfer ? -magnitude : magnitude
            transaction.payee = "Transfer"
        }
        if !showsCategory { transaction.categoryID = nil }
        if isSplit && kind == .expense {
            transaction.splits = splitLines.map { line in
                var split = line
                split.amount = -line.amount
                return split
            }
            transaction.categoryID = nil
        } else {
            transaction.splits = []
        }
        let receipt = Receipt(
            imageData: receiptImage,
            items: receiptItems.filter { !$0.name.isEmpty || $0.price != 0 }
        )
        transaction.receipt = receipt.isEmpty ? nil : receipt
        // Saving is reviewing.
        transaction.isApproved = true

        if isNew {
            store.add(transaction)
        } else {
            store.update(transaction)
        }
        dismiss()
    }
}
