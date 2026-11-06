// An account's register, or every account's: the desktop's ledger as
// Wallet lists a card's transactions — its cleared, uncleared and working
// balances across the top, then the transactions a day at a time, newest
// first. Each row is its payee, its category (or "Needs a category" in
// orange), its memo, and its amount, an inflow in green, with the cleared
// mark the bank has seen beside it, and a receipt's when it has one.
// Swiping right clears or uncleared it, or approves one waiting for a
// look; swiping left deletes it; holding it
// edits, categorizes, approves or deletes it; tapping it edits it. The
// account's menu reconciles it with the bank, renames or closes it.
import ArkUI
import SwiftUI

struct RegisterView: View {
    /// The account; none is every account's.
    let account: Id?
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @State private var needle = ""
    @State private var cleared = 0

    var body: some View {
        let a = model.snapshot.account(account)
        // A search is the core's (`MoneyView.search`: payee, the bank's words, memo, category, account or amount), held while typed.
        let searched = needle.trimmingCharacters(in: .whitespaces)
        let all = model.register(account) ?? []
        let rows = searched.isEmpty ? all : model.search(account, searched) ?? []
        List {
            if let a {
                Section {
                    balances(a)
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                }
            }
            if model.register(account) == nil {
                Section { HStack { Spacer(); ProgressView(); Spacer() } }.listRowBackground(Color.clear)
            } else if rows.isEmpty {
                ContentUnavailableView(needle.isEmpty ? "No Transactions" : "No Results",
                                       systemImage: needle.isEmpty ? "list.bullet.rectangle" : "magnifyingglass",
                                       description: Text(needle.isEmpty ? "Add one with the + button." : "Nothing matches \u{201C}\(needle)\u{201D}."))
                    .listRowBackground(Color.clear)
            }
            ForEach(Registers.days(rows)) { day in
                Section {
                    ForEach(day.rows) { t in
                        TxnButton(txn: t, showAccount: account == nil)
                            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                                if !t.approved {
                                    Button { model.approve([t.id]) } label: { Label("Approve", systemImage: "checkmark") }
                                        .tint(Color.accent)
                                }
                                if t.cleared != .reconciled {
                                    Button {
                                        model.setCleared(t.id, t.cleared == .uncleared)
                                        cleared += 1
                                    } label: {
                                        Label(t.cleared == .uncleared ? "Clear" : "Unclear", systemImage: t.cleared == .uncleared ? "c.circle.fill" : "c.circle")
                                    }
                                    .tint(Color.chart[1])
                                }
                            }
                            .swipeActions(edge: .trailing) {
                                Button(role: .destructive) { model.delete([t.id]) } label: { Label("Delete", systemImage: "trash") }
                            }
                    }
                } header: {
                    DayHeader(day: day.day)
                }
            }
        }
        .listStyle(.insetGrouped)
        .holding(searched.isEmpty ? nil : MoneyView.search(account: account, text: searched))
        .navigationTitle(a?.name ?? "All Accounts")
        .navigationSubtitle(a.map { $0.subtitle() } ?? "Every transaction in the household")
        .searchable(text: $needle, prompt: "Payee, category, memo or amount")
        .refreshable { await model.refresh() }
        .sensoryFeedback(.selection, trigger: cleared)
        .holding(MoneyView.register(account: account))
        .toolbar {
            if let a {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button { present(.reconcile(a.id)) } label: { Label("Reconcile…", systemImage: "checkmark.seal") }
                            .disabled(a.closed)
                        let waiting = (model.register(account) ?? []).filter { !$0.approved }.map(\.id)
                        if !waiting.isEmpty {
                            Button { model.approve(waiting) } label: { Label("Approve All (\(waiting.count))", systemImage: "checkmark.circle") }
                        }
                        Divider()
                        Button(role: a.closed ? nil : .destructive) { model.close(account: a.id, !a.closed) } label: {
                            Label(a.closed ? "Reopen Account" : "Close Account", systemImage: a.closed ? "arrow.uturn.backward" : "xmark.bin")
                        }
                    } label: {
                        Image(systemName: "ellipsis")
                    }
                    .accessibilityLabel("Account options")
                }
            }
            ToolbarItem(placement: .topBarTrailing) { AddTransactionButton(account: account) }
        }
    }

    /// The working balance large, the cleared and uncleared under it, and
    /// what the bank last said when Plaid feeds the account.
    private func balances(_ a: AccountLine) -> some View {
        HeroFigure((a.accountKind()?.owes() ?? false) ? "Balance Owed" : "Working Balance", cents: a.balance, tint: .neutral,
                   note: a.bankBalance.map { "The bank says \(CentsText.string($0))" } ?? "") {
            HStack(spacing: Space.l) {
                figure("Cleared", a.cleared, "c.circle.fill")
                figure("Uncleared", a.uncleared, "c.circle")
                Spacer(minLength: 0)
            }
        }
        .padding(.horizontal, Space.page)
    }

    private func figure(_ label: String, _ cents: Int64, _ symbol: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Label(label, systemImage: symbol).font(.caption).foregroundStyle(.quiet)
            CentsText(cents).font(.subheadline.weight(.semibold))
        }
        .accessibilityElement(children: .combine)
    }
}

/// A day's heading, as Wallet and Mail head one: Today, Yesterday, or the weekday and date.
struct DayHeader: View {
    let day: String
    @EnvironmentObject private var model: Model

    var body: some View {
        Text(formatDayHeading(day: day, today: model.today))
    }
}

/// A transaction as a button: tapped, it opens the form over it; held, its menu.
struct TxnButton: View {
    let txn: TxnLine
    var showAccount = false
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present

    var body: some View {
        Button { present(.transaction(TxnForm(txn))) } label: { TxnRow(txn: txn, showAccount: showAccount) }
            .foregroundStyle(.ink)
            .contextMenu {
                Button { present(.transaction(TxnForm(txn))) } label: { Label("Edit", systemImage: "pencil") }
                if !txn.transfer {
                    Button { present(.categorize([txn.id], approve: false)) } label: { Label("Categorize…", systemImage: "folder") }
                }
                if !txn.approved {
                    Button { model.approve([txn.id]) } label: { Label("Approve", systemImage: "checkmark.circle") }
                }
                if txn.cleared != .reconciled {
                    Button { model.setCleared(txn.id, txn.cleared == .uncleared) } label: {
                        Label(txn.cleared == .uncleared ? "Mark Cleared" : "Mark Uncleared", systemImage: "c.circle")
                    }
                }
                Divider()
                Button(role: .destructive) { model.delete([txn.id]) } label: { Label("Delete", systemImage: "trash") }
            }
    }
}

/// One transaction: the payee, what it was for, and the amount, with the
/// cleared mark and a dot when it is waiting for a look.
struct TxnRow: View {
    let txn: TxnLine
    var showAccount = false

    var body: some View {
        HStack(alignment: .center, spacing: Space.m) {
            Circle()
                .fill(txn.approved ? Color.clear : Color.accent)
                .frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 2) {
                Text(txn.payee.isEmpty ? "No Payee" : txn.payee)
                    .font(.body.weight(txn.approved ? .regular : .semibold))
                    .lineLimit(1)
                HStack(spacing: Space.xs) {
                    if txn.transfer { Image(systemName: "arrow.left.arrow.right").font(.caption2) }
                    Text(txn.category)
                        .foregroundStyle(txn.needsCategory() ? Color.caution : Color.quiet)
                    if showAccount { Text("· \(txn.account)").foregroundStyle(.quiet) }
                }
                .font(.caption)
                .lineLimit(1)
                if !txn.memo.isEmpty {
                    Text(txn.memo).font(.caption).foregroundStyle(.quiet).lineLimit(1)
                }
            }
            Spacer(minLength: Space.s)
            VStack(alignment: .trailing, spacing: 2) {
                CentsText(txn.amount, signed: txn.amount > 0)
                    .foregroundStyle(txn.amount > 0 ? Color.positive : Color.ink)
                    .font(.body.weight(.medium))
                HStack(spacing: Space.xs) {
                    // A receipt paid by it, or a photo of one taken for it.
                    if Receipts.marked(txn) { Image(systemName: "receipt").foregroundStyle(.quiet) }
                    cleared
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(txn.payee.isEmpty ? "No payee" : txn.payee)
        .accessibilityValue("\(CentsText.string(txn.amount)), \(txn.category), \(clearedWords)\(txn.approved ? "" : ", waiting for review")\(Receipts.marked(txn) ? ", has a receipt" : "")")
    }

    @ViewBuilder private var cleared: some View {
        switch txn.cleared {
        case .uncleared: Image(systemName: "c.circle").foregroundStyle(.outline)
        case .cleared: Image(systemName: "c.circle.fill").foregroundStyle(Color.accentText)
        case .reconciled: Image(systemName: "lock.fill").foregroundStyle(.quiet)
        }
    }

    private var clearedWords: String {
        switch txn.cleared {
        case .uncleared: return "uncleared"
        case .cleared: return "cleared"
        case .reconciled: return "reconciled"
        }
    }
}
