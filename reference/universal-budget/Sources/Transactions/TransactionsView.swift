import SwiftUI

/// Every transaction across accounts, with search, filters (as search
/// tokens) and the queue of bank imports waiting for review.
struct TransactionsView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model
    @FocusState private var isSearchFieldFocused: Bool

    var body: some View {
        @Bindable var model = model
        let results = store.transactions(matching: model.searchTokens, text: model.searchText)

        TransactionsTable(transactions: results)
            .overlay {
                if results.isEmpty {
                    emptyState
                }
            }
            .safeAreaInset(edge: .top, spacing: 0) {
                if !store.unapproved.isEmpty && !model.searchTokens.contains(.needsReview) {
                    ReviewBanner()
                }
            }
            .navigationTitle("Transactions")
            .searchable(
                text: $model.searchText,
                tokens: $model.searchTokens,
                prompt: Text("Payee, category, memo or amount")
            ) { token in
                Label(token.title(in: store), systemImage: token.systemImage)
            }
            .searchSuggestions { suggestions }
            .searchFocused($isSearchFieldFocused)
            .onChange(of: model.isSearchFocused, initial: true) { _, wantsFocus in
                if wantsFocus {
                    isSearchFieldFocused = true
                    model.isSearchFocused = false
                }
            }
            .toolbar {
                ToolbarItemGroup(placement: .primaryAction) {
                    FilterMenu()
                    Button("New Transaction", systemImage: "plus") {
                        model.newTransaction(in: store)
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
    }

    @ViewBuilder
    private var emptyState: some View {
        if !model.searchText.isEmpty {
            ContentUnavailableView.search(text: model.searchText)
        } else if model.searchTokens == [.needsReview] {
            ContentUnavailableView(
                "All Caught Up",
                systemImage: "checkmark.circle",
                description: Text("New transactions from your banks will appear here for review.")
            )
        } else if !model.searchTokens.isEmpty {
            ContentUnavailableView(
                "No Matching Transactions",
                systemImage: "line.3.horizontal.decrease.circle",
                description: Text("Try removing a filter.")
            )
        } else {
            ContentUnavailableView {
                Label("No Transactions", systemImage: "list.bullet.rectangle")
            } description: {
                Text("Add a transaction, or connect a bank account to import them.")
            } actions: {
                Button("New Transaction") { model.newTransaction(in: store) }
            }
        }
    }

    /// Filters offered while the search field is empty.
    @ViewBuilder
    private var suggestions: some View {
        if model.searchText.isEmpty {
            let offered: [FilterToken] = [.needsReview, .uncategorized, .period(.thisMonth), .outflow, .inflow, .hasReceipt]
            ForEach(offered.filter { !model.searchTokens.contains($0) }) { token in
                Label(token.title(in: store), systemImage: token.systemImage)
                    .searchCompletion(token)
            }
            Section("Accounts") {
                ForEach(store.accounts) { account in
                    Label(account.name, systemImage: account.kind.systemImage)
                        .searchCompletion(FilterToken.account(account.id))
                }
            }
        } else {
            // Suggest the categories whose names match what's typed.
            ForEach(store.categories.filter { $0.name.localizedCaseInsensitiveContains(model.searchText) }) { category in
                Label(category.name, systemImage: "tag")
                    .searchCompletion(FilterToken.category(category.id))
            }
        }
    }
}

/// Shown above the list while bank imports are waiting.
private struct ReviewBanner: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        let count = store.unapproved.count
        HStack(spacing: 12) {
            Image(systemName: "tray.and.arrow.down.fill")
                .foregroundStyle(.tint)
                .imageScale(.large)
                .accessibilityHidden(true)
            Text("^[\(count) new transaction](inflect: true) from your banks")
                .font(.subheadline.weight(.medium))
            Spacer(minLength: 8)
            Button("Review") {
                model.searchTokens = [.needsReview]
            }
            Button("Approve All") {
                store.approve(Set(store.unapproved.map(\.id)))
            }
            .buttonStyle(.borderedProminent)
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.bar)
        .sensoryFeedback(.success, trigger: count) { old, new in new < old }
    }
}

/// Filters as a menu, for people who don't think of filtering as search.
/// Every choice becomes a token in the search field.
private struct FilterMenu: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        Menu {
            Picker("Account", selection: token(kind: "account")) {
                Text("All Accounts").tag(FilterToken?.none)
                ForEach(store.accounts) { account in
                    Text(account.name).tag(FilterToken?.some(.account(account.id)))
                }
            }
            .pickerStyle(.menu)

            Picker("Category", selection: token(kind: "category")) {
                Text("All Categories").tag(FilterToken?.none)
                ForEach(store.groups) { group in
                    Section(group.name) {
                        ForEach(group.categories) { category in
                            Text(category.name).tag(FilterToken?.some(.category(category.id)))
                        }
                    }
                }
            }
            .pickerStyle(.menu)

            Picker("Date", selection: token(kind: "period")) {
                Text("Any Time").tag(FilterToken?.none)
                ForEach(FilterToken.Period.allCases) { period in
                    Text(period.title).tag(FilterToken?.some(.period(period)))
                }
            }
            .pickerStyle(.menu)

            Picker("Inflow or Outflow", selection: token(kind: "flow")) {
                Text("Both").tag(FilterToken?.none)
                Text("Inflow").tag(FilterToken?.some(.inflow))
                Text("Outflow").tag(FilterToken?.some(.outflow))
            }
            .pickerStyle(.menu)

            Divider()

            Toggle("Needs Review", isOn: flag(.needsReview))
            Toggle("Uncategorized", isOn: flag(.uncategorized))
            Toggle("Has Receipt", isOn: flag(.hasReceipt))

            if !model.searchTokens.isEmpty {
                Divider()
                Button("Clear Filters", role: .destructive) { model.searchTokens = [] }
            }
        } label: {
            Label("Filter", systemImage: model.searchTokens.isEmpty
                  ? "line.3.horizontal.decrease.circle"
                  : "line.3.horizontal.decrease.circle.fill")
        }
        .help("Filter transactions")
    }

    private func token(kind: String) -> Binding<FilterToken?> {
        Binding {
            model.searchTokens.first { $0.kind == kind }
        } set: { newValue in
            model.searchTokens.removeAll { $0.kind == kind }
            if let newValue { model.searchTokens.append(newValue) }
        }
    }

    private func flag(_ token: FilterToken) -> Binding<Bool> {
        Binding {
            model.searchTokens.contains(token)
        } set: { isOn in
            model.searchTokens.removeAll { $0 == token }
            if isOn { model.searchTokens.append(token) }
        }
    }
}

/// The inspector beside the transaction table: the selected transaction,
/// or a summary of a multiple selection.
struct TransactionInspector: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model

    var body: some View {
        let selection = model.transactionSelection
        if selection.count == 1, let id = selection.first {
            TransactionDetailView(transactionID: id, isInInspector: true)
        } else if selection.count > 1 {
            let total = selection.compactMap { store.transaction($0)?.amount }.reduce(0, +)
            ContentUnavailableView {
                Label("\(selection.count) Transactions", systemImage: "square.stack")
            } description: {
                Text("Total \(total.formatted(currency: store.currencyCode))")
            } actions: {
                TransactionActions(ids: selection)
            }
        } else {
            ContentUnavailableView(
                "No Transaction Selected",
                systemImage: "list.bullet.rectangle",
                description: Text("Select a transaction to see its details, splits and receipt.")
            )
        }
    }
}
