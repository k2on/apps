import SwiftUI

enum SettingsPane: String, CaseIterable, Identifiable, Hashable {
    case general, household, banks
    var id: Self { self }

    var title: String {
        switch self {
        case .general: "General"
        case .household: "Household"
        case .banks: "Connected Banks"
        }
    }

    var systemImage: String {
        switch self {
        case .general: "gearshape"
        case .household: "person.2"
        case .banks: "building.columns"
        }
    }
}

/// Settings. On Mac, the Settings window with a tab per pane, as Mac apps
/// do. On iPhone and iPad, a sheet with a list of panes. The panes are the
/// same views on both.
struct SettingsView: View {
    /// `-settingsTab household` opens a pane directly.
    @AppStorage("settingsTab") private var pane: SettingsPane = .general

    #if os(macOS)
    var body: some View {
        TabView(selection: $pane) {
            ForEach(SettingsPane.allCases) { pane in
                SettingsPaneView(pane: pane)
                    .tabItem { Label(pane.title, systemImage: pane.systemImage) }
                    .tag(pane)
            }
        }
        .frame(width: 540, height: 460)
    }
    #else
    @Environment(\.dismiss) private var dismiss
    @State private var path: [SettingsPane] = []

    var body: some View {
        NavigationStack(path: $path) {
            List {
                Section {
                    HouseholdHeader()
                }
                Section {
                    ForEach(SettingsPane.allCases) { pane in
                        NavigationLink(value: pane) {
                            Label(pane.title, systemImage: pane.systemImage)
                        }
                    }
                }
            }
            .navigationTitle("Settings")
            .navigationDestination(for: SettingsPane.self) { pane in
                SettingsPaneView(pane: pane)
                    .navigationTitle(pane.title)
            }
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear {
                if pane != .general && path.isEmpty { path = [pane] }
            }
        }
    }
    #endif
}

private struct SettingsPaneView: View {
    var pane: SettingsPane

    var body: some View {
        switch pane {
        case .general: GeneralSettings()
        case .household: HouseholdSettings()
        case .banks: BankSettings()
        }
    }
}

private struct HouseholdHeader: View {
    @Environment(BudgetStore.self) private var store

    var body: some View {
        HStack(spacing: 12) {
            HStack(spacing: -8) {
                ForEach(store.household.members.filter { $0.role != .invited }) { member in
                    MemberAvatar(member: member)
                }
            }
            VStack(alignment: .leading) {
                Text(store.household.name).font(.headline)
                Text("^[\(store.household.members.count) member](inflect: true)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: - General

private struct GeneralSettings: View {
    @Environment(BudgetStore.self) private var store
    @AppStorage("appearance") private var appearance: Appearance = .system

    private let currencies = ["USD", "EUR", "GBP", "CAD", "AUD", "NZD", "JPY", "CHF", "SEK", "NOK", "DKK", "MXN"]

    var body: some View {
        @Bindable var store = store
        Form {
            Section {
                Picker("Currency", selection: $store.currencyCode) {
                    ForEach(currencies, id: \.self) { code in
                        Text("\(Locale.current.localizedString(forCurrencyCode: code) ?? code) (\(code))").tag(code)
                    }
                }
                LabeledContent("Example") {
                    Text(Decimal(1234.56).formatted(currency: store.currencyCode))
                        .monospacedDigit()
                }
            } header: {
                Text("Money")
            } footer: {
                Text("Digit grouping, the decimal mark and where the symbol goes follow your region settings.")
            }

            Section("Appearance") {
                Picker("Appearance", selection: $appearance) {
                    ForEach(Appearance.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - Household

private struct HouseholdSettings: View {
    @Environment(BudgetStore.self) private var store
    @State private var inviteEmail = ""

    var body: some View {
        @Bindable var store = store
        Form {
            Section("Household") {
                TextField("Name", text: $store.household.name)
            }

            Section {
                ForEach(store.household.members) { member in
                    HStack(spacing: 12) {
                        MemberAvatar(member: member)
                        VStack(alignment: .leading) {
                            Text(member.name)
                            Text(member.email).font(.subheadline).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(member.role.title)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    .accessibilityElement(children: .combine)
                    .deleteDisabled(member.role == .owner)
                }
                .onDelete { offsets in
                    store.household.members.remove(atOffsets: offsets)
                }
            } header: {
                Text("Members")
            } footer: {
                Text("Everyone in the household sees and edits the same budget.")
            }

            Section("Invite Someone") {
                TextField("Email Address", text: $inviteEmail)
                    #if os(iOS)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .textContentType(.emailAddress)
                    #endif
                    .autocorrectionDisabled()
                    .onSubmit(invite)
                Button("Send Invitation", action: invite)
                    .disabled(!inviteEmail.contains("@"))
            }
        }
        .formStyle(.grouped)
    }

    private func invite() {
        let email = inviteEmail.trimmingCharacters(in: .whitespaces)
        guard email.contains("@") else { return }
        store.household.members.append(Member(id: email, name: email, email: email, role: .invited))
        inviteEmail = ""
    }
}

struct MemberAvatar: View {
    var member: Member
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 32

    var body: some View {
        Text(member.role == .invited ? "?" : member.initials)
            .font(.system(size: size * 0.4, weight: .semibold, design: .rounded))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(color.gradient, in: .circle)
            .overlay(Circle().strokeBorder(.background, lineWidth: 2))
            .accessibilityHidden(true)
    }

    /// A stable colour per person, so the same member looks the same everywhere.
    private var color: Color {
        let palette: [Color] = [.blue, .purple, .teal, .pink, .indigo, .mint]
        let index = member.id.unicodeScalars.reduce(0) { $0 + Int($1.value) } % palette.count
        return member.role == .invited ? .gray : palette[index]
    }
}

// MARK: - Banks

private struct BankSettings: View {
    @Environment(BudgetStore.self) private var store
    @State private var bankToDisconnect: BankConnection?
    @State private var showsConnectInfo = false

    var body: some View {
        Form {
            if store.banks.isEmpty {
                ContentUnavailableView(
                    "No Connected Banks",
                    systemImage: "building.columns",
                    description: Text("Connect a bank to import transactions automatically.")
                )
            }
            ForEach(store.banks) { bank in
                Section {
                    LabeledContent("Accounts") {
                        Text(store.accounts(connectedTo: bank.id).map(\.name).formatted(.list(type: .and)))
                            .multilineTextAlignment(.trailing)
                    }
                    switch bank.status {
                    case .connected:
                        LabeledContent("Last Synced", value: bank.lastSynced.formatted(date: .abbreviated, time: .shortened))
                    case .needsAttention(let message):
                        Label(message, systemImage: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                    }
                    Button(bank.status == .connected ? "Sync Now" : "Reconnect") { store.sync(bank.id) }
                    Button("Disconnect…", role: .destructive) { bankToDisconnect = bank }
                } header: {
                    Text(bank.institution)
                }
            }
            Section {
                Button("Connect a Bank…", systemImage: "plus") { showsConnectInfo = true }
            }
        }
        .formStyle(.grouped)
        .confirmationDialog(
            "Disconnect \(bankToDisconnect?.institution ?? "")?",
            isPresented: Binding(get: { bankToDisconnect != nil }, set: { if !$0 { bankToDisconnect = nil } }),
            presenting: bankToDisconnect
        ) { bank in
            Button("Disconnect", role: .destructive) { store.disconnect(bank.id) }
        } message: { _ in
            Text("Its accounts and transactions stay; new transactions will no longer be imported.")
        }
        .alert("Connecting Banks", isPresented: $showsConnectInfo) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("This sample app has no network connection, so it can't link a real bank. The sample banks show how synced accounts behave.")
        }
    }
}
