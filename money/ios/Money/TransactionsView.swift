// The Transactions tab: what needs a look first — the desktop's Needs a
// Look, the bank's imports not yet approved and anything on budget with no
// category — and every transaction in the household after, as a segmented
// control at the top switches, the way Mail switches its inboxes. In
// Review, swiping right approves (a full swipe, a success tap under the
// finger), swiping left categorizes, a row that needs a category asks for
// one and is approved with it, and Approve All clears what has a category.
// When nothing waits, it says so the way the system does. The search field
// is the page's own row under the large title, as Android's is, not the
// bar's drawer: a drawer would stand over the large title and push it down
// from where every other tab has it.
import ArkUI
import SwiftUI

struct TransactionsView: View {
    enum Showing: String, CaseIterable { case review = "To Review", all = "All" }

    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @State private var showing: Showing = .review
    @State private var needle = ""
    @State private var approved = 0

    /// How tall the large title's row is: past it, the inline title comes into the bar.

    var body: some View {
        // A search is the core's (`MoneyView.search`), held while typed; what needs a look is the
        // inbox's rows among what it finds.
        let searched = needle.trimmingCharacters(in: .whitespaces)
        let found = searched.isEmpty ? nil : model.search(nil, searched).map { Set($0.map(\.id)) }
        let inbox = found.map { f in model.snapshot.inbox.filter { f.contains($0.id) } } ?? model.snapshot.inbox
        let all = searched.isEmpty ? model.register(nil) ?? [] : model.search(nil, searched) ?? []
        List {
            // The large title, and at the end of its line what the tab offers and the account's face.
            // With the search field and To Review / All under it, the title's
            // own gap all there is between them (`TitleSection`).
            Section {
                SearchField(text: $needle, prompt: "Payee, category, memo or amount")
                Picker("Showing", selection: $showing) {
                    ForEach(Showing.allCases, id: \.self) { s in
                        Text(s == .review && !model.snapshot.inbox.isEmpty ? "\(s.rawValue) (\(model.snapshot.inbox.count))" : s.rawValue).tag(s)
                    }
                }
                .pickerStyle(.segmented)
            }
            switch showing {
            case .review: review(inbox)
            case .all: everything(all)
            }
        }
        .groupedList()
        .refreshable { await model.refresh() }
        .sensoryFeedback(.success, trigger: approved)
        .holding(MoneyView.register(account: nil))
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
            let ready = rows.filter { !$0.approved && !$0.needsCategory() }.map(\.id)
            Section {
                ForEach(rows) { t in
                    Button {
                        if t.needsCategory() { present(.categorize([t.id], approve: true)) }
                        else { present(.transaction(TxnForm(t))) }
                    } label: {
                        TxnRow(txn: t, showAccount: true)
                    }
                    .foregroundStyle(.ink)
                    .swipeActions(edge: .leading, allowsFullSwipe: true) {
                        if !t.needsCategory() {
                            Button {
                                model.approve([t.id])
                                approved += 1
                            } label: { Label("Approve", systemImage: "checkmark") }
                                .tint(Color.accent)
                        }
                    }
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button { present(.categorize([t.id], approve: true)) } label: { Label("Categorize", systemImage: "folder") }
                            .tint(Color.caution)
                        Button { present(.transaction(TxnForm(t))) } label: { Label("Edit", systemImage: "pencil") }
                            .tint(Color.quiet)
                    }
                    .contextMenu {
                        Button { present(.categorize([t.id], approve: true)) } label: { Label("Categorize and Approve…", systemImage: "folder") }
                        if !t.needsCategory() && !t.approved {
                            Button { model.approve([t.id]); approved += 1 } label: { Label("Approve", systemImage: "checkmark.circle") }
                        }
                        Button { present(.transaction(TxnForm(t))) } label: { Label("Edit", systemImage: "pencil") }
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
