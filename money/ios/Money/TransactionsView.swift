// The Transactions tab: what needs a look first — the desktop's Needs a
// Look, the bank's imports not yet approved and anything on budget with no
// category — and every transaction in the household after, as a segmented
// control at the top switches, the way Mail switches its inboxes. In
// Review, swiping right approves (a full swipe, a success tap under the
// finger), swiping left categorizes, a row that needs a category asks for
// one and is approved with it, and Approve All clears what has a category.
// When nothing waits, it says so the way the system does.
import ArkUI
import SwiftUI

struct TransactionsView: View {
    enum Showing: String, CaseIterable { case review = "Review", all = "All" }

    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @State private var showing: Showing = .review
    @State private var needle = ""
    @State private var approved = 0

    var body: some View {
        let inbox = Registers.search(model.snapshot.inbox, needle)
        let all = Registers.search(model.register(nil) ?? [], needle)
        List {
            Section {
                Picker("Showing", selection: $showing) {
                    ForEach(Showing.allCases, id: \.self) { s in
                        Text(s == .review && !model.snapshot.inbox.isEmpty ? "Review (\(model.snapshot.inbox.count))" : s.rawValue).tag(s)
                    }
                }
                .pickerStyle(.segmented)
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
            }
            switch showing {
            case .review: review(inbox)
            case .all: everything(all)
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Transactions")
        .searchable(text: $needle, prompt: "Payee, category, memo or amount")
        .refreshable { await model.refresh() }
        .sensoryFeedback(.success, trigger: approved)
        .onAppear { model.hold(register: nil) }
        .onDisappear { model.release(register: nil) }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) { AccountButton() }
            ToolbarItem(placement: .topBarTrailing) { AddTransactionButton() }
        }
    }

    @ViewBuilder private func review(_ rows: [TxnLine]) -> some View {
        if rows.isEmpty {
            ContentUnavailableView {
                Label(needle.isEmpty ? "All Caught Up" : "No Results", systemImage: needle.isEmpty ? "checkmark.circle" : "magnifyingglass")
            } description: {
                Text(needle.isEmpty ? "Everything from the bank has been looked at, and everything has a category. Pull down to ask the bank for anything new." : "Nothing waiting matches \u{201C}\(needle)\u{201D}.")
            }
            .listRowBackground(Color.clear)
        } else {
            let ready = rows.filter { !$0.approved && !Registers.needsCategory($0) }.map(\.id)
            Section {
                ForEach(rows) { t in
                    Button {
                        if Registers.needsCategory(t) { present(.categorize([t.id], approve: true)) }
                        else { present(.transaction(Draft(t))) }
                    } label: {
                        TxnRow(txn: t, showAccount: true)
                    }
                    .foregroundStyle(.primary)
                    .swipeActions(edge: .leading, allowsFullSwipe: true) {
                        if !Registers.needsCategory(t) {
                            Button {
                                model.approve([t.id])
                                approved += 1
                            } label: { Label("Approve", systemImage: "checkmark") }
                                .tint(.green)
                        }
                    }
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button { present(.categorize([t.id], approve: true)) } label: { Label("Categorize", systemImage: "folder") }
                            .tint(.orange)
                        Button { present(.transaction(Draft(t))) } label: { Label("Edit", systemImage: "pencil") }
                            .tint(.gray)
                    }
                    .contextMenu {
                        Button { present(.categorize([t.id], approve: true)) } label: { Label("Categorize and Approve…", systemImage: "folder") }
                        if !Registers.needsCategory(t) && !t.approved {
                            Button { model.approve([t.id]); approved += 1 } label: { Label("Approve", systemImage: "checkmark.circle") }
                        }
                        Button { present(.transaction(Draft(t))) } label: { Label("Edit", systemImage: "pencil") }
                        Divider()
                        Button(role: .destructive) { model.delete([t.id]) } label: { Label("Delete", systemImage: "trash") }
                    }
                }
            } header: {
                HStack {
                    Text("\(rows.count) to Review")
                    Spacer()
                    if !ready.isEmpty {
                        Button("Approve All") {
                            model.approve(ready)
                            approved += 1
                        }
                        .font(.subheadline.weight(.semibold))
                        .textCase(nil)
                        .accessibilityHint("Approves the \(ready.count) that already have a category")
                    }
                }
            } footer: {
                Text("Swipe right to approve, left to choose a category.")
            }
        }
    }

    @ViewBuilder private func everything(_ rows: [TxnLine]) -> some View {
        if model.register(nil) == nil {
            Section { HStack { Spacer(); ProgressView(); Spacer() } }.listRowBackground(Color.clear)
        }
        ForEach(Registers.days(rows)) { day in
            Section {
                ForEach(day.rows) { t in
                    TxnButton(txn: t, showAccount: true)
                        .swipeActions(edge: .trailing) {
                            Button(role: .destructive) { model.delete([t.id]) } label: { Label("Delete", systemImage: "trash") }
                        }
                }
            } header: {
                DayHeader(day: day.day)
            }
        }
    }
}
