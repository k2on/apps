import SwiftUI

extension Month {
    var title: String { start.formatted(.dateTime.month(.wide).year()) }
}

/// The month's plan: Ready to Assign at the top, then each group of
/// categories with what's assigned, spent and available.
///
/// One list serves every width. In a regular-width window the rows show
/// Assigned / Activity / Available columns, and Assigned is editable in
/// place (Tab moves to the next row's field on Mac and iPad). In compact
/// width, or at accessibility text sizes, each row shows only what's
/// available, and opening it shows the rest.
struct BudgetView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SceneModel.self) private var model
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @IsCompact private var isCompact

    private var showsColumns: Bool { !isCompact && !dynamicTypeSize.isAccessibilitySize }

    var body: some View {
        @Bindable var model = model
        List(selection: $model.categorySelection) {
            Section {
                ReadyToAssignCard(month: model.month)
            }

            ForEach(store.groups) { group in
                Section {
                    ForEach(group.categories) { category in
                        BudgetRow(category: category, month: model.month, showsColumns: showsColumns)
                            .tag(category.id)
                    }
                } header: {
                    GroupHeader(
                        group: group, month: model.month, showsColumns: showsColumns,
                        showsColumnTitles: group.id == store.groups.first?.id
                    )
                }
            }
        }
        .contextMenu(forSelectionType: Category.ID.self) { ids in
            if let id = ids.first {
                categoryMenu(for: id)
            }
        } primaryAction: { ids in
            if let id = ids.first { model.open(.category(id)) }
        }
        .navigationTitle(model.month.title)
        .toolbar { toolbar }
        .inspector(isPresented: $model.showsInspector) {
            inspector
                .inspectorColumnWidth(min: 300, ideal: 340, max: 440)
        }
        .animation(.default, value: model.month)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .navigation) {
            Button("Previous Month", systemImage: "chevron.backward") {
                model.goToPreviousMonth(in: store)
            }
            .disabled(model.month <= store.budgetMonths.lowerBound)
            .help("Previous Month (⌘[)")

            Button("Next Month", systemImage: "chevron.forward") {
                model.goToNextMonth(in: store)
            }
            .disabled(model.month >= store.budgetMonths.upperBound)
            .help("Next Month (⌘])")
        }

        ToolbarItemGroup(placement: .primaryAction) {
            if model.month != store.currentMonth {
                Button("This Month") { model.goToCurrentMonth(in: store) }
            }
            AutoAssignMenu(month: model.month)
            Button("Category Details", systemImage: "sidebar.trailing") {
                model.showsInspector.toggle()
            }
            .help("Show or hide category details (⌥⌘I)")
        }

        #if os(iOS)
        // Mac has a Settings window under the app menu; iPhone and iPad keep
        // settings one tap away from the main screen.
        ToolbarItem(placement: .secondaryAction) {
            Button("Settings", systemImage: "gearshape") { model.sheet = .settings }
        }
        #endif
    }

    @ViewBuilder
    private var inspector: some View {
        if let id = model.categorySelection {
            CategoryDetailView(categoryID: id, linksTransactions: false)
        } else {
            ContentUnavailableView(
                "No Category Selected",
                systemImage: "envelope.open",
                description: Text("Select a category to see its goal, this month’s numbers and recent activity.")
            )
        }
    }

    @ViewBuilder
    private func categoryMenu(for id: Category.ID) -> some View {
        let summary = store.category(id).map { store.summary(for: $0, in: model.month) }
        Button("Show Details", systemImage: "info.circle") {
            model.categorySelection = id
            model.showsInspector = true
        }
        if let summary, summary.neededThisMonth > 0 {
            Button("Assign Amount Needed", systemImage: "target") {
                store.assign(summary.assigned + summary.neededThisMonth, to: id, in: model.month)
            }
        }
        if let summary, summary.available < 0 {
            CoverOverspendingMenu(categoryID: id, amount: -summary.available, month: model.month)
        }
        Button("Show Transactions", systemImage: "list.bullet.rectangle") {
            model.showTransactions(matching: [.category(id), .period(.thisMonth)])
        }
    }
}

// MARK: - Ready to Assign

/// The number zero-based budgeting is about: money that hasn't been given a
/// job yet. Green when there's money to assign, red when more has been
/// assigned than exists, grey at zero.
struct ReadyToAssignCard: View {
    var month: Month

    @Environment(BudgetStore.self) private var store
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        let amount = store.readyToAssign(in: month)
        let counts = store.attentionCounts(in: month)
        let layout = dynamicTypeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12))
            : AnyLayout(HStackLayout(alignment: .center, spacing: 16))

        layout {
            VStack(alignment: .leading, spacing: 2) {
                Text(amount < 0 ? "Assigned Too Much" : "Ready to Assign")
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
                AmountText(amount)
                    .font(.system(.largeTitle, design: .rounded, weight: .bold))
                    .foregroundStyle(amount < 0 ? .red : amount > 0 ? .green : .primary)
                    .contentTransition(.numericText(value: amount.doubleValue))
                Text(caption(amount: amount, overspent: counts.overspent, underfunded: counts.underfunded))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)

            if !dynamicTypeSize.isAccessibilitySize { Spacer(minLength: 0) }

            AutoAssignMenu(month: month, style: .prominent)
        }
        .padding(.vertical, 6)
    }

    private func caption(amount: Decimal, overspent: Int, underfunded: Int) -> String {
        var parts: [String] = []
        if overspent > 0 { parts.append("\(overspent) overspent") }
        if underfunded > 0 { parts.append("\(underfunded) underfunded") }
        if parts.isEmpty {
            return amount == 0 ? "Every dollar has a job." : "Give these dollars a job."
        }
        return parts.joined(separator: " · ")
    }
}

/// Quick ways to fill in a month. Each option shows how much it would assign.
struct AutoAssignMenu: View {
    enum Style { case toolbar, prominent }

    var month: Month
    var style: Style = .toolbar

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        let menu = Menu {
            ForEach(BudgetStore.AutoAssign.allCases.filter { $0 != .reset }) { strategy in
                Button {
                    withAnimation { store.autoAssign(strategy, in: month) }
                } label: {
                    Label(strategy.title, systemImage: strategy.systemImage)
                    Text(store.preview(strategy, in: month).formatted(currency: currencyCode, signed: true))
                }
            }
            Divider()
            Button(role: .destructive) {
                withAnimation { store.autoAssign(.reset, in: month) }
            } label: {
                Label(BudgetStore.AutoAssign.reset.title, systemImage: BudgetStore.AutoAssign.reset.systemImage)
            }
        } label: {
            Label("Auto-Assign", systemImage: "wand.and.stars")
        }
        .help("Fill in this month automatically")

        switch style {
        case .toolbar:
            menu
        case .prominent:
            menu
                .labelStyle(.titleAndIcon)
                .buttonStyle(.borderedProminent)
                .fixedSize()
        }
    }
}

/// Moves money from categories that have some into an overspent one.
struct CoverOverspendingMenu: View {
    var categoryID: Category.ID
    var amount: Decimal
    var month: Month

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        Menu("Cover Overspending From", systemImage: "arrow.left.arrow.right") {
            ForEach(store.categories.filter { $0.id != categoryID && store.available($0.id, in: month) > 0 }) { source in
                let available = store.available(source.id, in: month)
                Button {
                    store.move(min(amount, available), from: source.id, to: categoryID, in: month)
                } label: {
                    Text(source.name)
                    Text("\(available.formatted(currency: currencyCode)) available")
                }
            }
        }
    }
}

// MARK: - Rows

private struct GroupHeader: View {
    var group: CategoryGroup
    var month: Month
    var showsColumns: Bool
    var showsColumnTitles: Bool

    @Environment(BudgetStore.self) private var store
    @ScaledMetric(relativeTo: .body) private var columnWidth: CGFloat = 112

    var body: some View {
        HStack(alignment: .lastTextBaseline) {
            Text(group.name)
            Spacer()
            if showsColumns {
                column("Assigned", total { store.assigned($0, in: month) })
                column("Activity", total { store.activity($0, in: month) })
                column("Available", total { store.available($0, in: month) })
            } else {
                AmountText(total { store.available($0, in: month) })
                    .accessibilityLabel("\(group.name) available")
            }
        }
    }

    private func total(_ value: (Category.ID) -> Decimal) -> Decimal {
        group.categories.reduce(0) { $0 + value($1.id) }
    }

    private func column(_ title: LocalizedStringKey, _ amount: Decimal) -> some View {
        VStack(alignment: .trailing, spacing: 2) {
            if showsColumnTitles {
                Text(title).font(.caption2.weight(.semibold)).foregroundStyle(.tertiary)
            }
            AmountText(amount)
        }
        .frame(width: columnWidth, alignment: .trailing)
        .accessibilityElement(children: .combine)
    }
}

struct BudgetRow: View {
    var category: Category
    var month: Month
    var showsColumns: Bool

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @ScaledMetric(relativeTo: .body) private var columnWidth: CGFloat = 112

    var body: some View {
        let summary = store.summary(for: category, in: month)
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .lineLimit(2)
                if let progress = summary.goalProgress {
                    GoalProgressBar(progress: progress, status: summary.status)
                        .frame(maxWidth: showsColumns ? 220 : .infinity)
                }
                if let message = summary.status.message(currency: currencyCode), summary.status.isAttentionNeeded {
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(summary.status.tint)
                }
            }
            Spacer(minLength: 8)
            if showsColumns {
                AssignedField(categoryID: category.id, month: month)
                    .frame(width: columnWidth)
                AmountText(summary.activity)
                    .foregroundStyle(.secondary)
                    .frame(width: columnWidth, alignment: .trailing)
                    .accessibilityLabel("Activity")
                    .accessibilityValue(summary.activity.formatted(currency: currencyCode))
                AvailableBadge(amount: summary.available, status: summary.status)
                    .frame(width: columnWidth, alignment: .trailing)
            } else {
                AvailableBadge(amount: summary.available, status: summary.status)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: showsColumns ? .contain : .combine)
        .accessibilityLabel(showsColumns ? Text(category.name) : Text("\(category.name), \(summary.available.formatted(currency: currencyCode)) available"))
    }

    private var title: String {
        [category.emoji, category.name].compactMap { $0 }.joined(separator: " ")
    }
}

/// The amount assigned to a category this month, edited in place.
struct AssignedField: View {
    var categoryID: Category.ID
    var month: Month

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        TextField(
            "Assigned",
            value: Binding(
                get: { store.assigned(categoryID, in: month) },
                set: { store.assign($0, to: categoryID, in: month) }
            ),
            format: .currency(code: currencyCode)
        )
        .multilineTextAlignment(.trailing)
        .monospacedDigit()
        .textFieldStyle(.plain)
        .decimalKeyboard()
        .accessibilityLabel("Assigned")
    }
}
