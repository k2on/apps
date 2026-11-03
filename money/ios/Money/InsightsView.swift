// The Insights tab: the desktop's Reflect, in Swift Charts. The month is
// paged as the budget's is (one month for both). Where the month's spending
// went is a donut of its groups, the most first, which a finger on it
// selects — the middle then says that group and its share — and a legend
// of amounts under it. Then the month as a whole (income, spent, what was
// left, and what of the cards' debt is not covered), and each group's
// assigned against spent as paired bars, the way Screen Time compares
// days — the cards' group not among them: paying a card is no spending.
import ArkUI
import Charts
import SwiftUI

struct InsightsView: View {
    @EnvironmentObject private var model: Model
    @State private var picked: Int64? = nil

    var body: some View {
        let s = model.snapshot
        let spending = s.spending.filter { $0.spent > 0 }.sorted { $0.spent > $1.spent }
        let total = spending.reduce(0) { $0 + $1.spent }
        List {
            Section {
                Pager(model.monthName,
                      home: model.month == model.thisMonth ? nil : { withAnimation(Springs.move) { model.showThisMonth() } },
                      back: { withAnimation(Springs.move) { model.stepMonth(-1) } },
                      forward: { withAnimation(Springs.move) { model.stepMonth(1) } })
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
            }
            Section("Spending") {
                if spending.isEmpty {
                    ContentUnavailableView("Nothing Spent Yet", systemImage: "chart.pie", description: Text("What \(model.monthName) spends shows here."))
                } else {
                    donut(spending, total)
                        .frame(height: 260)
                        .padding(.vertical, Space.s)
                    ForEach(Array(spending.enumerated()), id: \.element.groupId) { i, g in
                        HStack {
                            Circle().fill(InsightsView.palette[i % InsightsView.palette.count]).frame(width: 10, height: 10)
                            Text(g.group)
                            Spacer()
                            Text(share(g.spent, total)).foregroundStyle(.secondary).monospacedDigit()
                            CentsText(g.spent).frame(minWidth: 90, alignment: .trailing)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
            if let m = s.summary {
                Section("The Month") {
                    LabeledContent("Income") { CentsText(m.income).foregroundStyle(Color.green) }
                    LabeledContent("Spent") { CentsText(-total) }
                    LabeledContent("Left Over") {
                        CentsText(m.income - total).foregroundStyle(m.income - total < 0 ? Color.red : Color.primary)
                    }
                    .fontWeight(.semibold)
                    // Paying a card is not spending (what it bought was), so
                    // it is in neither figure; what the cards owe beyond
                    // what is set aside for them is said apart.
                    if s.lines.contains(where: Cards.isPayment) {
                        let short = Cards.uncovered(s.lines)
                        LabeledContent("Card Debt Not Covered") {
                            CentsText(short).foregroundStyle(short > 0 ? Color.orange : Color.green)
                        }
                    }
                }
            }
            let groups = Months.groups(s.groups, s.lines, hidden: false).filter { !$0.isCards && ($0.assigned != 0 || $0.activity != 0) }
            if !groups.isEmpty {
                Section {
                    bars(groups)
                        .frame(height: CGFloat(max(groups.count, 1)) * 52 + 40)
                        .padding(.vertical, Space.s)
                } header: {
                    Text("Assigned and Spent")
                } footer: {
                    Text("Each group's plan for the month beside what it spent.")
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Insights")
        .refreshable { await model.refresh() }
        .toolbar { ToolbarItem(placement: .topBarLeading) { AccountButton() } }
    }

    /// The groups' colours, in the order the most spent takes them.
    static let palette: [Color] = [.blue, .green, .orange, .purple, .pink, .teal, .yellow, .indigo, .red, .mint, .brown, .cyan]

    private func share(_ part: Int64, _ whole: Int64) -> String {
        guard whole > 0 else { return "" }
        return (Double(part) / Double(whole)).formatted(.percent.precision(.fractionLength(0)))
    }

    /// The group a selected angle falls in: the running sum passes it.
    private func selected(_ spending: [SpendingLine]) -> SpendingLine? {
        guard let p = picked else { return nil }
        var run: Int64 = 0
        for g in spending {
            run += g.spent
            if p <= run { return g }
        }
        return nil
    }

    private func donut(_ spending: [SpendingLine], _ total: Int64) -> some View {
        let sel = selected(spending)
        return Chart(Array(spending.enumerated()), id: \.element.groupId) { i, g in
            SectorMark(angle: .value("Spent", Double(g.spent) / 100),
                       innerRadius: .ratio(0.62),
                       outerRadius: .ratio(sel == nil || sel?.groupId == g.groupId ? 1 : 0.92),
                       angularInset: 1.5)
                .cornerRadius(5)
                .foregroundStyle(InsightsView.palette[i % InsightsView.palette.count])
                .opacity(sel == nil || sel?.groupId == g.groupId ? 1 : 0.4)
                .accessibilityLabel(g.group)
                .accessibilityValue(CentsText.string(g.spent))
        }
        .chartAngleSelection(value: Binding(get: { picked.map { Double($0) / 100 } },
                                            set: { picked = $0.map { Int64(($0 * 100).rounded()) } }))
        .chartBackground { proxy in
            GeometryReader { geo in
                if let frame = proxy.plotFrame {
                    let r = geo[frame]
                    VStack(spacing: 2) {
                        Text(sel?.group ?? "Spent")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)
                        CentsText(sel?.spent ?? total)
                            .font(.title2.weight(.bold))
                            .contentTransition(.numericText())
                        if let g = sel {
                            Text(share(g.spent, total)).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .position(x: r.midX, y: r.midY)
                }
            }
        }
        .sensoryFeedback(.selection, trigger: sel?.groupId)
        .animation(Springs.move, value: sel?.groupId)
    }

    private func bars(_ groups: [GroupMonth]) -> some View {
        Chart {
            ForEach(groups) { g in
                BarMark(x: .value("Amount", Double(g.assigned) / 100), y: .value("Group", g.group.name))
                    .foregroundStyle(by: .value("Kind", "Assigned"))
                    .position(by: .value("Kind", "Assigned"))
                    .cornerRadius(4)
                BarMark(x: .value("Amount", Double(max(-g.activity, 0)) / 100), y: .value("Group", g.group.name))
                    .foregroundStyle(by: .value("Kind", "Spent"))
                    .position(by: .value("Kind", "Spent"))
                    .cornerRadius(4)
            }
        }
        .chartForegroundStyleScale(["Assigned": Color.money.opacity(0.45), "Spent": Color.money])
        .chartXAxis {
            AxisMarks { v in
                AxisGridLine()
                AxisValueLabel {
                    if let d = v.as(Double.self) { Text(d, format: .currency(code: CentsText.code).precision(.fractionLength(0))) }
                }
            }
        }
        .chartLegend(position: .top, alignment: .leading)
    }
}
