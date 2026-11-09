import SwiftUI

/// Everything about one category in the month being viewed: what's
/// available, how it got there, its goal and its recent transactions.
/// Pushed on iPhone; shown in the inspector beside the budget on iPad and Mac.
struct CategoryDetailView: View {
    var categoryID: Category.ID
    /// Rows link to transactions when pushed; not from the inspector, which
    /// has no navigation stack of its own.
    var linksTransactions = true

    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model
    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        if let category = store.category(categoryID) {
            content(category, summary: store.summary(for: category, in: model.month))
                .navigationTitle(category.name)
        } else {
            ContentUnavailableView("Category Not Found", systemImage: "questionmark.folder")
        }
    }

    private func content(_ category: Category, summary: CategorySummary) -> some View {
        Form {
            Section {
                VStack(spacing: 8) {
                    Text(category.emoji ?? "")
                        .font(.largeTitle)
                        .accessibilityHidden(true)
                    AvailableBadge(amount: summary.available, status: summary.status)
                        .font(.title2)
                    if let message = summary.status.message(currency: currencyCode) {
                        Text(message)
                            .font(.subheadline)
                            .foregroundStyle(summary.status.isAttentionNeeded ? summary.status.tint : .secondary)
                    }
                    Text(store.group(containing: category.id)?.name ?? "")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 4)

                if summary.neededThisMonth > 0 {
                    Button("Assign \(summary.neededThisMonth.formatted(currency: currencyCode))", systemImage: "target") {
                        store.assign(summary.assigned + summary.neededThisMonth, to: category.id, in: model.month)
                    }
                }
                if summary.available < 0 {
                    CoverOverspendingMenu(categoryID: category.id, amount: -summary.available, month: model.month)
                }
            }

            Section(model.month.title) {
                LabeledContent("Carried Over") { AmountText(summary.carriedOver) }
                LabeledContent("Assigned") {
                    AssignedField(categoryID: category.id, month: model.month)
                }
                LabeledContent("Activity") { AmountText(summary.activity) }
                LabeledContent("Available") {
                    AmountText(summary.available).fontWeight(.semibold)
                }
            }

            GoalSection(category: category, summary: summary)

            Section("Activity") {
                let transactions = recentTransactions(category)
                if transactions.isEmpty {
                    Text("No transactions this month.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(transactions) { transaction in
                        if linksTransactions {
                            NavigationLink(value: Route.transaction(transaction.id)) {
                                ActivityRow(transaction: transaction, categoryID: category.id)
                            }
                        } else {
                            ActivityRow(transaction: transaction, categoryID: category.id)
                        }
                    }
                }
                Button("Show All in Transactions") {
                    model.showTransactions(matching: [.category(category.id), .period(.thisMonth)])
                }
            }
        }
        .formStyle(.grouped)
    }

    private func recentTransactions(_ category: Category) -> [Transaction] {
        store.transactions
            .filter { model.month.contains($0.date) && $0.categoryLines.contains { $0.categoryID == category.id } }
            .sorted { $0.date > $1.date }
    }
}

private struct ActivityRow: View {
    var transaction: Transaction
    var categoryID: Category.ID

    var body: some View {
        let amount = transaction.categoryLines
            .filter { $0.categoryID == categoryID }
            .reduce(Decimal(0)) { $0 + $1.amount }
        LabeledContent {
            AmountText(amount, style: .signed)
        } label: {
            Text(transaction.payee)
            Text(transaction.date, format: .dateTime.month(.abbreviated).day())
        }
    }
}

/// Shows and edits a category's goal.
private struct GoalSection: View {
    var category: Category
    var summary: CategorySummary

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        Section {
            Picker("Goal", selection: kind) {
                Text("None").tag(Goal.Kind?.none)
                ForEach(Goal.Kind.allCases) { kind in
                    Text(kind.title).tag(Goal.Kind?.some(kind))
                }
            }
            if let goal = category.goal {
                LabeledContent("Target") {
                    TextField("Target", value: amount(goal), format: .currency(code: currencyCode))
                        .multilineTextAlignment(.trailing)
                        .monospacedDigit()
                        .decimalKeyboard()
                }
                if case .targetBalance(_, let deadline?) = goal {
                    DatePicker("By", selection: deadlineBinding(goal, deadline: deadline), displayedComponents: .date)
                }
                if let progress = summary.goalProgress {
                    VStack(alignment: .leading, spacing: 6) {
                        GoalProgressBar(progress: progress, status: summary.status)
                        Text(description(goal, progress: progress))
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        } header: {
            Text("Goal")
        }
    }

    private var kind: Binding<Goal.Kind?> {
        Binding {
            category.goal?.kind
        } set: { newKind in
            let amount = category.goal?.amount ?? 100
            let goal: Goal? = switch newKind {
            case nil: nil
            case .monthly: .monthly(amount)
            case .targetBalance: .targetBalance(amount, by: nil)
            case .targetBalanceByDate: .targetBalance(amount, by: store.currentMonth.advanced(by: 6))
            }
            store.setGoal(goal, for: category.id)
        }
    }

    private func amount(_ goal: Goal) -> Binding<Decimal> {
        Binding {
            goal.amount
        } set: { amount in
            switch goal {
            case .monthly: store.setGoal(.monthly(amount), for: category.id)
            case .targetBalance(_, let deadline): store.setGoal(.targetBalance(amount, by: deadline), for: category.id)
            }
        }
    }

    private func deadlineBinding(_ goal: Goal, deadline: Month) -> Binding<Date> {
        Binding {
            deadline.start
        } set: { date in
            store.setGoal(.targetBalance(goal.amount, by: Month(containing: date)), for: category.id)
        }
    }

    private func description(_ goal: Goal, progress: Double) -> String {
        let percent = progress.formatted(.percent.precision(.fractionLength(0)))
        switch goal {
        case .monthly(let amount):
            return "\(percent) of \(amount.formatted(currency: currencyCode)) assigned this month."
        case .targetBalance(let amount, nil):
            return "\(percent) of \(amount.formatted(currency: currencyCode)) saved."
        case .targetBalance(let amount, let deadline?):
            return "\(percent) of \(amount.formatted(currency: currencyCode)) saved, due \(deadline.title)."
        }
    }
}
