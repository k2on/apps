import SwiftUI

/// Adds a budget or tracking account, entered by hand or linked to a bank.
struct AddAccountView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model
    @Environment(\.currencyCode) private var currencyCode
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var kind: AccountKind = .checking
    @State private var balance: Decimal = 0
    @State private var isLinked = false
    @State private var bankID: BankConnection.ID?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name, prompt: Text("e.g. Joint Checking"))
                    Picker("Type", selection: $kind) {
                        Section("Budget") {
                            ForEach(AccountKind.budgetKinds) { Label($0.title, systemImage: $0.systemImage).tag($0) }
                        }
                        Section("Tracking") {
                            ForEach(AccountKind.trackingKinds) { Label($0.title, systemImage: $0.systemImage).tag($0) }
                        }
                    }
                } footer: {
                    Text(kind.isBudget
                         ? "The balance goes to Ready to Assign, so you can give it a job."
                         : "Tracking accounts count towards net worth but stay out of the budget.")
                }

                Section {
                    LabeledContent(kind.isLiability ? "Amount Owed" : "Current Balance") {
                        TextField("Balance", value: $balance, format: .currency(code: currencyCode))
                            .multilineTextAlignment(.trailing)
                            .monospacedDigit()
                            .decimalKeyboard()
                    }
                }

                Section {
                    Toggle("Sync with a Bank", isOn: $isLinked.animation())
                    if isLinked {
                        Picker("Bank", selection: $bankID) {
                            Text("Choose…").tag(BankConnection.ID?.none)
                            ForEach(store.banks) { bank in
                                Text(bank.institution).tag(BankConnection.ID?.some(bank.id))
                            }
                        }
                    }
                } footer: {
                    Text("Synced accounts import new transactions for you to review. This sample app doesn't connect to real banks.")
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Add Account")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        let amount = kind.isLiability ? -abs(balance) : balance
                        let id = store.addAccount(
                            name: name.trimmingCharacters(in: .whitespaces), kind: kind,
                            balance: amount, bankID: isLinked ? bankID : nil
                        )
                        model.paths[model.tab, default: []].append(.account(id))
                        dismiss()
                    }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || (isLinked && bankID == nil))
                }
            }
        }
        .sheetFrame(width: 460, height: 520)
    }
}

/// Reconciling: confirm the cleared balance matches the bank statement,
/// then lock those transactions. A mismatch can be recorded as an
/// adjustment.
struct ReconcileView: View {
    var accountID: Account.ID

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @Environment(\.dismiss) private var dismiss

    @State private var matches: Bool?
    @State private var statementBalance: Decimal = 0

    var body: some View {
        let cleared = store.clearedBalance(of: accountID)
        NavigationStack {
            Form {
                Section {
                    VStack(spacing: 8) {
                        Text("Is your cleared balance")
                            .foregroundStyle(.secondary)
                        AmountText(cleared)
                            .font(.system(.largeTitle, design: .rounded, weight: .bold))
                        Text("Compare it with the balance in your banking app or on your statement.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 6)

                    Picker("Matches", selection: $matches) {
                        Text("Yes").tag(Bool?.some(true))
                        Text("No").tag(Bool?.some(false))
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }

                if matches == false {
                    Section {
                        LabeledContent("Statement Balance") {
                            TextField("Statement Balance", value: $statementBalance, format: .currency(code: currencyCode))
                                .multilineTextAlignment(.trailing)
                                .monospacedDigit()
                                .decimalKeyboard()
                        }
                        LabeledContent("Difference") {
                            AmountText(statementBalance - cleared, style: .signed)
                        }
                    } footer: {
                        Text("Check for missing or duplicate transactions first. Finishing records the difference as an adjustment.")
                    }
                }

                if let last = store.account(accountID)?.lastReconciled {
                    Section {
                        LabeledContent("Last Reconciled", value: last.formatted(date: .long, time: .omitted))
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Reconcile \(store.account(accountID)?.name ?? "")")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Finish") {
                        store.reconcile(accountID, statementBalance: matches == true ? cleared : statementBalance)
                        dismiss()
                    }
                    .disabled(matches == nil)
                }
            }
            .onAppear { statementBalance = cleared }
        }
        .sheetFrame(width: 460, height: 460)
    }
}
