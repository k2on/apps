import SwiftUI

extension FocusedValues {
    /// The frontmost window's navigation state, for menu commands.
    @Entry var sceneModel: SceneModel?
}

/// The menu bar on Mac, and the keyboard shortcut list on iPad (hold ⌘).
/// Every command acts on the frontmost window through its `SceneModel`.
struct BudgetCommands: Commands {
    var store: BudgetStore

    @FocusedValue(\.sceneModel) private var scene
    @Environment(\.openWindow) private var openWindow

    private var selection: Set<Transaction.ID> { scene?.transactionSelection ?? [] }

    var body: some Commands {
        SidebarCommands()
        InspectorCommands()

        CommandGroup(replacing: .newItem) {
            Button("New Transaction") {
                scene?.newTransaction(in: store, account: scene?.focusedAccount)
            }
            .keyboardShortcut("n")
            .disabled(scene == nil)

            Button("New Account…") { scene?.sheet = .addAccount }
                .keyboardShortcut("n", modifiers: [.command, .shift])
                .disabled(scene == nil)

            Divider()

            Button("New Window") { openWindow(id: "main") }
                .keyboardShortcut("n", modifiers: [.command, .option])
        }

        CommandGroup(after: .textEditing) {
            Button("Find Transactions…") { scene?.findTransactions() }
                .keyboardShortcut("f")
                .disabled(scene == nil)
        }

        CommandGroup(before: .sidebar) {
            Button("Budget") { scene?.show(.budget) }
                .keyboardShortcut("1")
            Button("Accounts") { scene?.show(.accounts) }
                .keyboardShortcut("2")
            Button("Transactions") { scene?.show(.transactions) }
                .keyboardShortcut("3")
            Button("Insights") { scene?.show(.insights) }
                .keyboardShortcut("4")
            Divider()
        }

        CommandMenu("Budget") {
            Button("Previous Month") { scene?.goToPreviousMonth(in: store) }
                .keyboardShortcut("[")
                .disabled(scene.map { $0.month <= store.budgetMonths.lowerBound } ?? true)
            Button("Next Month") { scene?.goToNextMonth(in: store) }
                .keyboardShortcut("]")
                .disabled(scene.map { $0.month >= store.budgetMonths.upperBound } ?? true)
            Button("Current Month") { scene?.goToCurrentMonth(in: store) }
                .keyboardShortcut("t", modifiers: [.command, .shift])
                .disabled(scene == nil)

            Divider()

            Menu("Auto-Assign") {
                ForEach(BudgetStore.AutoAssign.allCases) { strategy in
                    Button(strategy.title) {
                        if let month = scene?.month { store.autoAssign(strategy, in: month) }
                    }
                    .keyboardShortcut(strategy == .underfunded ? KeyboardShortcut("a", modifiers: [.command, .option]) : nil)
                }
            }
            .disabled(scene == nil)
        }

        CommandMenu("Transaction") {
            Button("Approve") { store.approve(selection) }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(!selection.contains { store.transaction($0)?.isApproved == false })
            Button("Approve All") { store.approve(Set(store.unapproved.map(\.id))) }
                .keyboardShortcut(.return, modifiers: [.command, .shift])
                .disabled(store.unapproved.isEmpty)

            Divider()

            Button("Edit Transaction…") {
                if let id = selection.first { scene?.sheet = .editTransaction(id) }
            }
            .keyboardShortcut("e")
            .disabled(selection.count != 1)

            Button(allCleared ? "Mark as Uncleared" : "Mark as Cleared") {
                store.setCleared(allCleared ? .uncleared : .cleared, for: selection)
            }
            .keyboardShortcut("c", modifiers: [.command, .shift])
            .disabled(selection.isEmpty)

            CategoryMenu("Categorize", groups: store.groups) { store.categorize(selection, as: $0) }
                .disabled(selection.isEmpty)

            Button("Duplicate") { store.duplicate(selection) }
                .keyboardShortcut("d")
                .disabled(selection.isEmpty)

            Divider()

            Button("Delete", role: .destructive) {
                store.delete(selection)
                scene?.transactionSelection = []
            }
            .keyboardShortcut(.delete, modifiers: .command)
            .disabled(selection.isEmpty)

            Divider()

            Button("Reconcile Account…") {
                if let id = scene?.focusedAccount { scene?.sheet = .reconcile(id) }
            }
            .keyboardShortcut("r", modifiers: [.command, .option])
            .disabled(scene?.focusedAccount == nil)

            Button("Sync with Banks") { store.syncAll() }
                .keyboardShortcut("r")
        }
    }

    private var allCleared: Bool {
        !selection.isEmpty && selection.allSatisfy { store.transaction($0)?.cleared != .uncleared }
    }
}
