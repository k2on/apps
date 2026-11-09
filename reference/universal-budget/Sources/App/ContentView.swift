import SwiftUI

/// The window's root: a tab view that is a tab bar on iPhone, a tab bar that
/// can become a sidebar on iPad, and a sidebar on Mac (`.sidebarAdaptable`).
/// Each tab owns a navigation stack; the sidebar also lists every account.
struct ContentView: View {
    @Environment(BudgetStore.self) private var store
    @State private var model = SceneModel(month: Month(containing: SampleData.today))

    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #endif

    /// Launch arguments apply to the first window only, not to windows the
    /// person opens later.
    @MainActor private static var hasAppliedLaunchOptions = false

    var body: some View {
        TabView(selection: Binding(get: { model.tab }, set: { model.show($0) })) {
            Tab("Budget", systemImage: "envelope", value: AppTab.budget) {
                stack(for: .budget) { BudgetView() }
            }

            Tab("Accounts", systemImage: "building.columns", value: AppTab.accounts) {
                stack(for: .accounts) { AccountsView() }
            }

            Tab("Transactions", systemImage: "list.bullet.rectangle", value: AppTab.transactions) {
                stack(for: .transactions) { TransactionsView() }
            }
            .badge(store.unapproved.count)

            Tab("Insights", systemImage: "chart.bar.xaxis", value: AppTab.insights) {
                stack(for: .insights) { InsightsView() }
            }

            // Accounts get their own sidebar entries on iPad and Mac, like
            // mailboxes in Mail. On iPhone they stay off the tab bar and
            // are reached through the Accounts tab.
            TabSection("Budget Accounts") {
                ForEach(store.budgetAccounts) { account in
                    Tab(account.name, systemImage: account.kind.systemImage, value: AppTab.account(account.id)) {
                        stack(for: .account(account.id)) {
                            AccountRegisterView(accountID: account.id)
                        }
                    }
                }
            }
            .defaultVisibility(.hidden, for: .tabBar)

            TabSection("Tracking Accounts") {
                ForEach(store.trackingAccounts) { account in
                    Tab(account.name, systemImage: account.kind.systemImage, value: AppTab.account(account.id)) {
                        stack(for: .account(account.id)) {
                            AccountRegisterView(accountID: account.id)
                        }
                    }
                }
            }
            .defaultVisibility(.hidden, for: .tabBar)
        }
        .tabViewStyle(.sidebarAdaptable)
        #if os(iOS)
        // iOS only: the Mac always shows the sidebar (BUILD-NOTES.md).
        .defaultAdaptableTabBarPlacement(model.prefersSidebar ? .sidebar : .automatic)
        #endif
        .sheet(item: $model.sheet) { route in
            SheetContent(route: route)
        }
        .environment(model)
        .focusedSceneValue(\.sceneModel, model)
        .onAppear(perform: applyLaunchOptions)
    }

    private func stack<Content: View>(for tab: AppTab, @ViewBuilder content: () -> Content) -> some View {
        NavigationStack(path: model.path(for: tab)) {
            content()
                .routeDestinations()
        }
    }

    private func applyLaunchOptions() {
        guard !Self.hasAppliedLaunchOptions else { return }
        Self.hasAppliedLaunchOptions = true
        let options = LaunchOptions.current
        model.apply(options, store: store)
        #if os(macOS)
        if options.screen == .settings {
            model.sheet = nil
            openSettings()
        }
        #endif
    }
}

extension View {
    /// Every pushed destination, registered once per stack.
    func routeDestinations() -> some View {
        navigationDestination(for: Route.self) { route in
            switch route {
            case .category(let id): CategoryDetailView(categoryID: id)
            case .account(let id): AccountRegisterView(accountID: id)
            case .transaction(let id): TransactionDetailView(transactionID: id)
            }
        }
    }
}

/// The content of the window's single sheet.
private struct SheetContent: View {
    var route: SheetRoute
    @Environment(BudgetStore.self) private var store

    var body: some View {
        switch route {
        case .addTransaction(let draft):
            TransactionEditor(draft: draft, isNew: true)
        case .editTransaction(let id):
            if let transaction = store.transaction(id) {
                TransactionEditor(draft: transaction, isNew: false)
            }
        case .addAccount:
            AddAccountView()
        case .reconcile(let id):
            ReconcileView(accountID: id)
        case .settings:
            SettingsView()
        }
    }
}
