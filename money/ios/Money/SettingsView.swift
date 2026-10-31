// Settings: who is using the budget, which server and whether it can be
// reached, the banks, and the way out — signing out, or leaving the demo
// for a real server. Nothing here is a cursor or a count of pending intents
// beyond a plain "waiting to sync".
//
// **Banks** are connected from here (Banks.swift says how): Connect a Bank
// asks the server, and Plaid's page opens in Safari's view the moment the
// server answers with it (Shell.swift); the ask's row says where it stands
// until the server has finished. Each bank is a row with how it is doing,
// and opens its own page (`BankView`): when it last synced and what that
// brought, Reconnect when its login ran out, Disconnect behind a
// confirmation, and each of its accounts with what it feeds — or, while
// it waits, the choice: a new account, one already here, or nothing.
import ArkUI
import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var model: Model
    @State private var leaving = false

    var body: some View {
        Form {
            Section {
                HStack(spacing: Space.l) {
                    Image(systemName: "person.crop.circle.fill")
                        .font(.system(size: 44))
                        .foregroundStyle(Color.money.gradient)
                        .accessibilityHidden(true)
                    VStack(alignment: .leading) {
                        Text(model.who.isEmpty ? "Signed In" : model.who).font(.headline)
                        Text(model.isDemo ? "The demo's household, on this phone alone" : model.login.map { $0.email.isEmpty ? $0.user : $0.email } ?? "")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            if !model.isDemo {
                Section("Server") {
                    LabeledContent(Probe.display(model.server)) { Text(linkWords) }
                    if let p = model.snapshot.status?.pending, p > 0 {
                        LabeledContent("Waiting to Sync", value: "\(p) change\(p == 1 ? "" : "s")")
                    }
                }
            }
            BanksSection()
            Section {
                Button(role: .destructive) { leaving = true } label: {
                    Label(model.isDemo ? "Leave the Demo" : "Sign Out", systemImage: "rectangle.portrait.and.arrow.right")
                }
            } footer: {
                Text(model.isDemo ? "Nothing in the demo leaves this phone, and it starts again from the beginning next time."
                                  : "Anything not synced yet stays on this phone, and goes when you sign in again.")
            }
            Section("About") {
                LabeledContent("Money", value: "Your household's budget, on your own server")
                LabeledContent("Version", value: Self.version)
            }
        }
        .navigationTitle("Settings")
        .confirmationDialog(model.isDemo ? "Leave the demo?" : "Sign out?", isPresented: $leaving, titleVisibility: .visible) {
            Button(model.isDemo ? "Leave the Demo" : "Sign Out", role: .destructive) { model.signOut() }
        }
    }

    private var linkWords: String {
        guard let st = model.snapshot.status else { return "Opening…" }
        if st.linked { return "Connected" }
        return "Can't reach it right now"
    }

    /// "1.0 (1)", from the bundle.
    static var version: String {
        let i = Bundle.main.infoDictionary
        return "\(i?["CFBundleShortVersionString"] as? String ?? "?") (\(i?["CFBundleVersion"] as? String ?? "?"))"
    }
}

// MARK: banks

/// Settings' Banks: the ask of the moment, each connected bank with how it
/// is doing, and Connect a Bank.
struct BanksSection: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let s = model.snapshot
        let ask = BankAsks.shown(s.asks, now: nowMs(), dismissed: model.dismissedAsk)
        Section {
            if let ask { AskRow(ask: ask) }
            ForEach(s.banks) { b in
                NavigationLink(value: Route.bank(b.name)) { BankRow(bank: b) }
            }
            Button { model.connectBank() } label: { Label("Connect a Bank", systemImage: "plus.circle.fill") }
                .disabled(!model.connectsBanks || (ask.map(BankAsks.isOpen) ?? false))
        } header: {
            Text("Banks")
        } footer: {
            Text(model.connectsBanks
                 ? "Banks and cards connect through Plaid. Their logins are kept on your server, never on this phone."
                 : "The demo's card comes from a bank of its own. Connecting a real one needs your server.")
        }
    }
}

/// An ask about a bank and where it stands: being asked, Plaid's page
/// waiting (and how long it stays open), or what came of it.
struct AskRow: View {
    let ask: AskLine
    @EnvironmentObject private var model: Model

    var body: some View {
        TimelineView(.periodic(from: .now, by: 15)) { t in
            let now = nowMs(t.date)
            HStack(alignment: .firstTextBaseline, spacing: Space.m) {
                mark
                VStack(alignment: .leading, spacing: Space.xs) {
                    Text(BankAsks.words(ask, now: now))
                        .fixedSize(horizontal: false, vertical: true)
                    if ask.state == .waiting {
                        Button("Open Plaid's Page") { model.openPage(ask) }
                            .fontWeight(.semibold)
                            .buttonStyle(.borderless)
                        if let m = BankAsks.minutesLeft(ask, now: now) {
                            Text(m < 1 ? "It closes in under a minute." : "It closes in \(m) min.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                Spacer(minLength: 0)
                if !BankAsks.isOpen(ask) {
                    Button { withAnimation(Springs.move) { model.dismiss(ask: ask) } } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Dismiss")
                }
            }
            .padding(.vertical, Space.xs)
        }
    }

    @ViewBuilder private var mark: some View {
        switch ask.state {
        case .asked: ProgressView()
        case .waiting: Image(systemName: "safari.fill").foregroundStyle(.blue)
        case .done: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .failed, .expired: Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
        }
    }
}

/// A connected bank: its mark, its name, how it is doing, and how many of
/// its accounts wait for a choice.
struct BankRow: View {
    let bank: BankLine

    var body: some View {
        HStack(spacing: Space.m) {
            Image(systemName: "building.columns.fill")
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 32, height: 32)
                .background(Color.blue.gradient, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(Banks.title(bank)).lineLimit(1)
                BankHealthText(bank: bank).font(.caption).lineLimit(1)
            }
            Spacer(minLength: Space.s)
            if bank.waiting > 0 {
                Text("\(bank.waiting)")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(Color.orange, in: Capsule())
                    .accessibilityLabel("\(bank.waiting) account\(bank.waiting == 1 ? "" : "s") to choose for")
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// How a bank is doing, in a line: red when it wants somebody.
struct BankHealthText: View {
    let bank: BankLine

    var body: some View {
        switch Banks.health(bank) {
        case .needsLogin: Text("Log in again to keep it syncing").foregroundStyle(.red)
        case .failed: Text("The last import failed").foregroundStyle(.red)
        case .new: Text("Waiting for its first import").foregroundStyle(.secondary)
        case let .synced(ms):
            Text("Synced \(Date(timeIntervalSince1970: Double(ms) / 1000), format: .relative(presentation: .named))")
                .foregroundStyle(.secondary)
        }
    }
}

/// One connected bank: how it is doing, when it last synced and what that
/// brought, each of its accounts and what it feeds, and Reconnect and
/// Disconnect.
struct BankView: View {
    let name: String
    @EnvironmentObject private var model: Model
    @State private var disconnecting = false

    var body: some View {
        if let b = model.snapshot.banks.first(where: { $0.name == name }) {
            page(b)
        } else {
            ContentUnavailableView("Not Connected", systemImage: "building.columns",
                                   description: Text("This bank is no longer connected. Its accounts here stay, kept by hand."))
        }
    }

    private func page(_ b: BankLine) -> some View {
        let s = model.snapshot
        let title = Banks.title(b)
        let accounts = Banks.accounts(of: b.name, s.linked)
        let ask = BankAsks.shown(s.asks, now: nowMs(), dismissed: model.dismissedAsk).flatMap { $0.bank == b.name ? $0 : nil }
        return List {
            if let ask {
                Section { AskRow(ask: ask) }
            }
            switch Banks.health(b) {
            case .needsLogin:
                Section {
                    Label {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Log In Again").font(.headline)
                            Text("\(title) wants its login again before it sends anything new.")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
                    }
                    Button { model.reconnect(bank: b.name) } label: { Label("Reconnect", systemImage: "arrow.clockwise") }
                        .fontWeight(.semibold)
                        .disabled(!model.connectsBanks)
                }
            case let .failed(why):
                Section("Last Import") {
                    Text(why).foregroundStyle(.red)
                    LabeledContent("Failed") {
                        Text(Date(timeIntervalSince1970: Double(b.failedMs) / 1000), format: .relative(presentation: .named))
                    }
                }
            case .new, .synced:
                EmptyView()
            }
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
            }
            if !accounts.isEmpty {
                Section {
                    ForEach(accounts) { l in LinkedRow(line: l) }
                } header: {
                    Text("Accounts")
                } footer: {
                    Text(b.waiting > 0
                         ? "Choose what each account feeds: a new account here, one you already keep, or nothing. One you kept by hand takes what the bank has from the day it was connected."
                         : "Each of the bank's accounts feeds an account here, or nothing.")
                }
            }
            Section {
                if !b.needsLogin {
                    Button { model.reconnect(bank: b.name) } label: { Label("Reconnect", systemImage: "arrow.clockwise") }
                        .disabled(!model.connectsBanks)
                }
                Button(role: .destructive) { disconnecting = true } label: { Label("Disconnect", systemImage: "minus.circle") }
                    .disabled(!model.connectsBanks)
            } footer: {
                if !model.connectsBanks { Text("The demo's bank stays connected: there is no server to ask.") }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .refreshable { await model.refresh() }
        .confirmationDialog("Disconnect \(title)?", isPresented: $disconnecting, titleVisibility: .visible) {
            Button("Disconnect", role: .destructive) { model.disconnect(bank: b.name) }
        } message: {
            Text("Plaid forgets the login and the bank stops importing. Its accounts here, and what's in them, stay.")
        }
    }
}

/// One of a bank's accounts: what the bank calls it and says it holds,
/// what it feeds here, and the choice — a menu, as Mail's move to a
/// mailbox is: a new account of its kind, an account already here, or
/// nothing.
struct LinkedRow: View {
    let line: LinkedLine
    @EnvironmentObject private var model: Model

    var body: some View {
        HStack(spacing: Space.m) {
            AccountMark(kind: line.opensAs.isEmpty ? line.kind : line.opensAs)
            VStack(alignment: .leading, spacing: 2) {
                Text(Banks.name(line)).lineLimit(1)
                Text(about).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                feeds.font(.caption).lineLimit(1)
            }
            Spacer(minLength: Space.s)
            Menu {
                choices
            } label: {
                Text(line.waiting ? "Choose" : "Change")
            }
            .buttonStyle(.bordered)
            .tint(line.waiting ? Color.orange : nil)
            .controlSize(.small)
            .accessibilityLabel(line.waiting ? "Choose what \(line.name) feeds" : "Change what \(line.name) feeds")
        }
    }

    private var about: String {
        guard let b = line.balance else { return Banks.kind(line) }
        return "\(Banks.kind(line)) · the bank says \(CentsText.string(b))"
    }

    @ViewBuilder private var feeds: some View {
        if line.waiting {
            Text("Waiting for a choice").foregroundStyle(.orange)
        } else if line.ignored {
            Text("Not imported").foregroundStyle(.secondary)
        } else {
            Text("Feeds \(line.account)").foregroundStyle(.secondary)
        }
    }

    @ViewBuilder private var choices: some View {
        let free = Banks.linkable(line, model.snapshot.accounts)
        if line.waiting {
            Button { model.openAccount(for: line.plaidAccountId) } label: { Label(Banks.opens(line), systemImage: "plus.circle") }
        }
        Menu {
            if free.isEmpty {
                Text("Every open account is fed by a bank already")
            }
            ForEach(free) { a in
                Button { model.link(line.plaidAccountId, to: a.id) } label: { Label(a.name, systemImage: Accounts.symbol(a.kind)) }
            }
        } label: {
            Label("An Existing Account", systemImage: "arrow.triangle.merge")
        }
        Divider()
        if line.ignored {
            Button { model.ignore(line.plaidAccountId, false) } label: { Label("Import It Again", systemImage: "eye") }
        } else {
            Button(role: .destructive) { model.ignore(line.plaidAccountId, true) } label: { Label("Don't Import It", systemImage: "eye.slash") }
        }
    }
}
