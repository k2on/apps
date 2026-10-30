// The Accounts tab: the desktop's sidebar of accounts as Wallet and
// Settings would list them — what the household is worth large at the top,
// All Accounts, then the accounts on budget, tracking only and closed, each
// with its kind's mark, its bank and last digits when Plaid feeds it, its
// balance, and how many of its transactions wait for a look. A row opens
// its register; holding it renames, closes or reopens it. Adding an account
// is the + (a sheet). The banks Plaid feeds are their own page, Bank
// Connections. Pull to refresh asks the bank for anything new.
import ArkUI
import SwiftUI

struct AccountsView: View {
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @State private var renaming: AccountLine? = nil
    @State private var typed = ""

    var body: some View {
        let rows = model.snapshot.accounts
        List {
            Section {
                HeroFigure("Net Worth", cents: Accounts.netWorth(rows), tint: .neutral, note: "Every open account, less what's owed")
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
            }
            Section {
                NavigationLink(value: Route.register(nil)) {
                    Label {
                        LabeledContent("All Accounts") {
                            CentsText(rows.filter { !$0.closed && $0.onBudget }.reduce(0) { $0 + $1.balance })
                        }
                    } icon: {
                        Image(systemName: "tray.full.fill")
                            .foregroundStyle(.white)
                            .frame(width: 32, height: 32)
                            .background(Color.gray.gradient, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                    }
                }
            }
            ForEach(Accounts.shelves(rows)) { shelf in
                Section {
                    ForEach(shelf.rows) { a in
                        NavigationLink(value: Route.register(a.id)) { AccountRow(account: a) }
                            .contextMenu {
                                Button { typed = a.name; renaming = a } label: { Label("Rename", systemImage: "pencil") }
                                if !a.closed {
                                    Button { present(.reconcile(a.id)) } label: { Label("Reconcile…", systemImage: "checkmark.seal") }
                                }
                                Button(role: a.closed ? nil : .destructive) { model.close(account: a.id, !a.closed) } label: {
                                    Label(a.closed ? "Reopen" : "Close Account", systemImage: a.closed ? "arrow.uturn.backward" : "xmark.bin")
                                }
                            }
                    }
                } header: {
                    HStack {
                        Text(shelf.kind.rawValue)
                        Spacer()
                        if shelf.kind != .closed { CentsText(shelf.total) }
                    }
                }
            }
            Section {
                NavigationLink(value: Route.banks) {
                    Label("Bank Connections", systemImage: "link")
                }
                Button { present(.addAccount) } label: { Label("Add Account", systemImage: "plus.circle.fill") }
            } footer: {
                if let b = model.snapshot.banks.first, b.syncedMs > 0 {
                    Text("Last synced with \(b.institution) \(Date(timeIntervalSince1970: Double(b.syncedMs) / 1000), format: .relative(presentation: .named)).")
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Accounts")
        .refreshable { await model.refresh() }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) { AccountButton() }
            ToolbarItem(placement: .topBarTrailing) {
                Button { present(.addAccount) } label: { Image(systemName: "plus") }
                    .accessibilityLabel("Add Account")
            }
        }
        .alert("Rename Account", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Name", text: $typed)
            Button("Cancel", role: .cancel) { renaming = nil }
            Button("Rename") {
                if let a = renaming { model.rename(account: a.id, typed.trimmingCharacters(in: .whitespaces)) }
                renaming = nil
            }
        }
    }
}

/// One account: its mark, its name and what feeds it, how many wait for a look, and its balance.
struct AccountRow: View {
    let account: AccountLine

    var body: some View {
        HStack(spacing: Space.m) {
            AccountMark(kind: account.kind)
            VStack(alignment: .leading, spacing: 2) {
                Text(account.name).lineLimit(1)
                Text(Accounts.subtitle(account)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: Space.s)
            if account.unapproved > 0 {
                Text("\(account.unapproved)")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(Color.money, in: Capsule())
                    .accessibilityLabel("\(account.unapproved) to review")
            }
            CentsText(account.balance)
                .foregroundStyle(account.balance < 0 ? Color.red : Color.primary)
        }
        .accessibilityElement(children: .combine)
    }
}

/// The banks Plaid feeds the household's accounts from: each one's
/// institution, when it last synced and what that brought, and the accounts
/// it feeds. Asking for a sync is the button, or pulling down.
struct BanksView: View {
    @EnvironmentObject private var model: Model
    @State private var syncing = false

    var body: some View {
        let banks = model.snapshot.banks
        let fed = model.snapshot.accounts.filter { !$0.bank.isEmpty }
        List {
            if banks.isEmpty {
                ContentUnavailableView("No Banks Connected", systemImage: "building.columns",
                                       description: Text("A bank is connected on the server, through Plaid. Its accounts appear here once it has synced."))
                    .listRowBackground(Color.clear)
            }
            ForEach(banks) { b in
                Section {
                    LabeledContent("Last Synced") {
                        if b.syncedMs > 0 {
                            Text(Date(timeIntervalSince1970: Double(b.syncedMs) / 1000), format: .relative(presentation: .named))
                        } else {
                            Text("Never")
                        }
                    }
                    LabeledContent("Brought In", value: "\(b.added) new, \(b.updated) changed")
                    if b.deleted > 0 { LabeledContent("Removed", value: "\(b.deleted)") }
                    if b.locked > 0 { LabeledContent("Left as Reconciled", value: "\(b.locked)") }
                    ForEach(fed.filter { $0.bank == b.name }) { a in
                        NavigationLink(value: Route.register(a.id)) { AccountRow(account: a) }
                    }
                } header: {
                    Label(b.institution.isEmpty ? b.name.capitalized : b.institution, systemImage: "building.columns")
                } footer: {
                    Text("\(b.accounts) account\(b.accounts == 1 ? "" : "s") followed")
                }
            }
            Section {
                Button {
                    syncing = true
                    Task {
                        await model.refresh()
                        syncing = false
                    }
                } label: {
                    HStack {
                        Label("Sync Now", systemImage: "arrow.triangle.2.circlepath")
                        Spacer()
                        if syncing { ProgressView() }
                    }
                }
                .disabled(syncing)
            } footer: {
                Text(model.isDemo ? "The demo's bank has a few days of news for its first two syncs." : "The server asks the bank every hour; this asks it now.")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Bank Connections")
        .navigationBarTitleDisplayMode(.inline)
        .refreshable { await model.refresh() }
    }
}
