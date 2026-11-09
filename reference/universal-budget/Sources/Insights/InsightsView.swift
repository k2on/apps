import Charts
import SwiftUI

/// Reports: headline numbers, then spending by category, income against
/// spending by month, and net worth over time.
///
/// Cards flow in an adaptive grid: one column on iPhone and narrow windows,
/// two or three side by side on iPad and Mac, with no size checks.
struct InsightsView: View {
    enum Period: String, CaseIterable, Identifiable {
        case thisMonth, lastThreeMonths
        var id: Self { self }
        var title: String {
            switch self {
            case .thisMonth: "This Month"
            case .lastThreeMonths: "Last 3 Months"
            }
        }
    }

    @Environment(BudgetStore.self) private var store
    @State private var period: Period = .lastThreeMonths
    @ScaledMetric(relativeTo: .body) private var minimumCardWidth: CGFloat = 340

    private var months: ClosedRange<Month> {
        switch period {
        case .thisMonth: store.currentMonth...store.currentMonth
        case .lastThreeMonths: max(store.firstMonth, store.currentMonth.advanced(by: -2))...store.currentMonth
        }
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                SummaryTiles(months: months)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: minimumCardWidth), spacing: 16, alignment: .top)], spacing: 16) {
                    SpendingByCategoryCard(months: months)
                    IncomeVsSpendingCard()
                    NetWorthCard()
                }
            }
            .padding()
        }
        .navigationTitle("Insights")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Picker("Period", selection: $period) {
                    ForEach(Period.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
    }
}

// MARK: - Headline numbers

private struct SummaryTiles: View {
    var months: ClosedRange<Month>
    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        let flows = store.incomeVsSpending(in: months)
        let income = flows.reduce(Decimal(0)) { $0 + $1.income }
        let spending = flows.reduce(Decimal(0)) { $0 + $1.spending }
        let saved = income > 0 ? ((income - spending) / income).doubleValue : 0

        ViewThatFits(in: .horizontal) {
            HStack(spacing: 16) { tiles(income: income, spending: spending, saved: saved) }
            VStack(spacing: 16) { tiles(income: income, spending: spending, saved: saved) }
        }
    }

    @ViewBuilder
    private func tiles(income: Decimal, spending: Decimal, saved: Double) -> some View {
        StatTile(title: "Income", value: income.formatted(currency: currencyCode))
        StatTile(title: "Spending", value: spending.formatted(currency: currencyCode))
        StatTile(title: "Kept", value: saved.formatted(.percent.precision(.fractionLength(0))),
                 detail: "of income not spent")
    }
}

private struct StatTile: View {
    var title: LocalizedStringKey
    var value: String
    var detail: LocalizedStringKey?

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 2) {
                Text(value)
                    .font(.system(.title, design: .rounded, weight: .bold))
                    .monospacedDigit()
                if let detail {
                    Text(detail).font(.footnote).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            Text(title).foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Spending by category

/// Magnitude across named categories: horizontal bars, longest first, one
/// colour (the label carries identity, so the bars don't need hues).
private struct SpendingByCategoryCard: View {
    var months: ClosedRange<Month>
    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @ScaledMetric(relativeTo: .body) private var rowHeight: CGFloat = 26

    var body: some View {
        let spending = topCategories()
        GroupBox("Spending by Category") {
            if spending.isEmpty {
                ContentUnavailableView("No Spending", systemImage: "cart",
                                       description: Text("Nothing has been spent in this period."))
            } else {
                Chart(spending, id: \.name) { item in
                    BarMark(
                        x: .value("Spent", item.amount.doubleValue),
                        y: .value("Category", item.name)
                    )
                    .foregroundStyle(Color.accentColor.gradient)
                    .cornerRadius(4)
                    .annotation(position: .trailing, alignment: .leading, spacing: 4) {
                        Text(item.amount.formatted(currency: currencyCode))
                            .font(.caption)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                    .accessibilityLabel(item.name)
                    .accessibilityValue(item.amount.formatted(currency: currencyCode))
                }
                .chartXAxis(.hidden)
                .chartYAxis {
                    AxisMarks { _ in AxisValueLabel() }
                }
                .chartXScale(range: .plotDimension(endPadding: 72))
                .frame(height: CGFloat(spending.count) * rowHeight + 8)
            }
        }
    }

    /// The eight largest categories, with the rest folded into "Other".
    private func topCategories() -> [(name: String, amount: Decimal)] {
        let all = store.spendingByCategory(in: months)
        var items = all.prefix(8).map { (name: $0.category.name, amount: $0.amount) }
        let rest = all.dropFirst(8).reduce(Decimal(0)) { $0 + $1.amount }
        if rest > 0 { items.append((name: "Other", amount: rest)) }
        return items
    }
}

// MARK: - Income vs spending

/// Every month with data (not just the chosen period: the comparison is
/// between months). Two series per month, side by side on one axis, with a legend; select a
/// month to read exact values.
private struct IncomeVsSpendingCard: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @State private var selectedMonth: String?

    var body: some View {
        let flows = store.incomeVsSpending(in: store.firstMonth...store.currentMonth)
        GroupBox("Income vs Spending") {
            Chart {
                ForEach(flows) { flow in
                    BarMark(x: .value("Month", flow.month.shortTitle), y: .value("Amount", flow.income.doubleValue))
                        .foregroundStyle(by: .value("Series", "Income"))
                        .position(by: .value("Series", "Income"))
                        .cornerRadius(4)
                        .accessibilityLabel("\(flow.month.title) income")
                        .accessibilityValue(flow.income.formatted(currency: currencyCode))
                    BarMark(x: .value("Month", flow.month.shortTitle), y: .value("Amount", flow.spending.doubleValue))
                        .foregroundStyle(by: .value("Series", "Spending"))
                        .position(by: .value("Series", "Spending"))
                        .cornerRadius(4)
                        .accessibilityLabel("\(flow.month.title) spending")
                        .accessibilityValue(flow.spending.formatted(currency: currencyCode))
                }
                if let selectedMonth, let flow = flows.first(where: { $0.month.shortTitle == selectedMonth }) {
                    RuleMark(x: .value("Month", selectedMonth))
                        .foregroundStyle(.secondary.opacity(0.2))
                        .lineStyle(StrokeStyle(lineWidth: 40))
                        .annotation(position: .top, overflowResolution: .init(x: .fit(to: .chart), y: .disabled)) {
                            ChartCallout(title: flow.month.title, lines: [
                                ("Income", flow.income.formatted(currency: currencyCode)),
                                ("Spending", flow.spending.formatted(currency: currencyCode)),
                                ("Difference", flow.net.formatted(currency: currencyCode, signed: flow.net > 0)),
                            ])
                        }
                }
            }
            .chartForegroundStyleScale(["Income": Color.accentColor, "Spending": Color.indigo])
            .chartLegend(position: .top, alignment: .leading)
            .chartYAxis {
                AxisMarks { value in
                    AxisGridLine().foregroundStyle(.quaternary)
                    AxisValueLabel {
                        if let amount = value.as(Double.self) {
                            Text(Decimal(amount).formatted(.currency(code: currencyCode).precision(.fractionLength(0))))
                        }
                    }
                }
            }
            .chartXSelection(value: $selectedMonth)
            .frame(minHeight: 240)
        }
    }
}

// MARK: - Net worth

/// Change over time: a line with a light area under it; drag or hover to
/// read a day's value.
private struct NetWorthCard: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @State private var selectedDate: Date?

    var body: some View {
        let history = store.netWorthHistory()
        GroupBox("Net Worth") {
            VStack(alignment: .leading, spacing: 4) {
                AmountText(store.netWorth)
                    .font(.system(.title2, design: .rounded, weight: .bold))
                if let first = history.first {
                    let change = store.netWorth - first.netWorth
                    Text("\(change.formatted(currency: currencyCode, signed: change > 0)) since \(first.date.formatted(.dateTime.month(.wide).day()))")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                Chart {
                    ForEach(history) { point in
                        AreaMark(x: .value("Date", point.date), y: .value("Net Worth", point.netWorth.doubleValue))
                            .foregroundStyle(Color.accentColor.opacity(0.12))
                            .interpolationMethod(.monotone)
                        LineMark(x: .value("Date", point.date), y: .value("Net Worth", point.netWorth.doubleValue))
                            .foregroundStyle(Color.accentColor)
                            .lineStyle(StrokeStyle(lineWidth: 2))
                            .interpolationMethod(.monotone)
                    }
                    if let selectedDate, let point = nearest(to: selectedDate, in: history) {
                        RuleMark(x: .value("Date", point.date))
                            .foregroundStyle(.secondary)
                            .annotation(position: .top, overflowResolution: .init(x: .fit(to: .chart), y: .disabled)) {
                                ChartCallout(title: point.date.formatted(date: .abbreviated, time: .omitted), lines: [
                                    ("Net Worth", point.netWorth.formatted(currency: currencyCode)),
                                ])
                            }
                        PointMark(x: .value("Date", point.date), y: .value("Net Worth", point.netWorth.doubleValue))
                            .foregroundStyle(Color.accentColor)
                            .symbolSize(64)
                    }
                }
                .chartYScale(domain: .automatic(includesZero: false))
                .chartYAxis {
                    AxisMarks { value in
                        AxisGridLine().foregroundStyle(.quaternary)
                        AxisValueLabel {
                            if let amount = value.as(Double.self) {
                                Text(Decimal(amount).formatted(.currency(code: currencyCode).notation(.compactName)))
                            }
                        }
                    }
                }
                .chartXSelection(value: $selectedDate)
                .frame(minHeight: 220)
                .accessibilityLabel("Net worth over time")
            }
        }
    }

    private func nearest(to date: Date, in history: [BudgetStore.NetWorthPoint]) -> BudgetStore.NetWorthPoint? {
        history.min { abs($0.date.timeIntervalSince(date)) < abs($1.date.timeIntervalSince(date)) }
    }
}

/// The value box shown above a selected point or bar.
private struct ChartCallout: View {
    var title: String
    var lines: [(String, String)]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.caption.weight(.semibold))
            ForEach(lines, id: \.0) { line in
                HStack {
                    Text(line.0).foregroundStyle(.secondary)
                    Spacer(minLength: 12)
                    Text(line.1).monospacedDigit()
                }
                .font(.caption)
            }
        }
        .padding(8)
        .background(.regularMaterial, in: .rect(cornerRadius: 8))
        .fixedSize()
    }
}

extension Month {
    var shortTitle: String { start.formatted(.dateTime.month(.abbreviated)) }
}
