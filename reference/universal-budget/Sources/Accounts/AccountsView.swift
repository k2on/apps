import Charts
import SwiftUI

/// All accounts with their balances, split into budget and tracking
/// accounts, under the household's net worth.
struct AccountsView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        List {
            Section {
                NetWorthSummary()
            }
            accountSection(
                "Budget Accounts", accounts: store.budgetAccounts,
                footer: "Money in budget accounts is given a job in the budget."
            )
            accountSection(
                "Tracking Accounts", accounts: store.trackingAccounts,
                footer: "Tracking accounts count towards net worth but not the budget."
            )
        }
        .overlay {
            if store.accounts.isEmpty {
                ContentUnavailableView {
                    Label("No Accounts", systemImage: "building.columns")
                } description: {
                    Text("Add the accounts your household's money is in to start budgeting.")
                } actions: {
                    Button("Add Account") { model.sheet = .addAccount }
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        .navigationTitle("Accounts")
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button("Sync with Banks", systemImage: "arrow.triangle.2.circlepath") {
                    store.syncAll()
                }
                .help("Sync with Banks (⌘R)")
                Button("Add Account", systemImage: "plus") { model.sheet = .addAccount }
                    .help("Add Account (⇧⌘N)")
            }
        }
        .refreshable { store.syncAll() }
    }

    @ViewBuilder
    private func accountSection(_ title: LocalizedStringKey, accounts: [Account], footer: LocalizedStringKey) -> some View {
        if !accounts.isEmpty {
            Section {
                ForEach(accounts) { account in
                    NavigationLink(value: Route.account(account.id)) {
                        AccountRow(account: account)
                    }
                    .badge(store.unapproved(in: account.id).count)
                }
            } header: {
                HStack {
                    Text(title)
                    Spacer()
                    AmountText(store.total(of: accounts))
                }
            } footer: {
                Text(footer)
            }
        }
    }
}

struct AccountRow: View {
    var account: Account

    @Environment(BudgetStore.self) private var store

    var body: some View {
        HStack(spacing: 12) {
            AccountIcon(kind: account.kind)
            VStack(alignment: .leading, spacing: 2) {
                Text(account.name)
                    .fontWeight(.medium)
                Text(subtitle)
                    .font(.subheadline)
                    .foregroundStyle(needsAttention ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
            }
            Spacer(minLength: 8)
            AmountText(store.balance(of: account.id))
                .fontWeight(.medium)
        }
        .accessibilityElement(children: .combine)
    }

    private var needsAttention: Bool {
        if case .needsAttention = store.bank(account.bankID)?.status { return true }
        return false
    }

    private var subtitle: String {
        guard let bank = store.bank(account.bankID) else { return account.kind.title }
        if case .needsAttention = bank.status { return "\(bank.institution) · Needs attention" }
        return "\(account.kind.title) · \(bank.institution)"
    }
}

/// An account's type as a symbol in a tinted rounded square.
struct AccountIcon: View {
    var kind: AccountKind
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 32

    var body: some View {
        Image(systemName: kind.systemImage)
            .font(.system(size: size * 0.45, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(kind.isBudget ? Color.accentColor : Color.gray, in: .rect(cornerRadius: size * 0.25))
            .accessibilityHidden(true)
    }
}

/// Net worth, assets and liabilities, with a small trend line.
struct NetWorthSummary: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        let history = store.netWorthHistory()
        VStack(alignment: .leading, spacing: 8) {
            Text("Net Worth")
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.secondary)
            AmountText(store.netWorth)
                .font(.system(.largeTitle, design: .rounded, weight: .bold))
            HStack(spacing: 16) {
                LabeledContent("Assets") { AmountText(store.assets) }
                LabeledContent("Liabilities") { AmountText(store.liabilities) }
            }
            .font(.footnote)
            .labeledContentStyle(.vertical)

            Chart(history) { point in
                LineMark(x: .value("Date", point.date), y: .value("Net Worth", point.netWorth.doubleValue))
                    .interpolationMethod(.monotone)
            }
            .chartXAxis(.hidden)
            .chartYAxis(.hidden)
            .chartYScale(domain: .automatic(includesZero: false))
            .frame(height: 56)
            .accessibilityLabel("Net worth trend")
            .accessibilityValue(trendDescription(history))

            Button("Net Worth Report") { model.show(.insights) }
                .font(.footnote)
                .buttonStyle(.borderless)
        }
        .padding(.vertical, 6)
    }

    private func trendDescription(_ history: [BudgetStore.NetWorthPoint]) -> String {
        guard let first = history.first, let last = history.last else { return "" }
        let change = last.netWorth - first.netWorth
        return "\(change >= 0 ? "Up" : "Down") \(abs(change).formatted(currency: store.currencyCode)) since \(first.date.formatted(.dateTime.month(.wide).day()))"
    }
}

/// Stacks LabeledContent's label above its value, for compact summaries.
struct VerticalLabeledContentStyle: LabeledContentStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            configuration.label
                .foregroundStyle(.secondary)
            configuration.content
                .fontWeight(.semibold)
        }
    }
}

extension LabeledContentStyle where Self == VerticalLabeledContentStyle {
    static var vertical: Self { .init() }
}
