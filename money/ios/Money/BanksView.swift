// Bank Connections, which the Accounts tab opens: the banks Plaid feeds
// the household's accounts, and Sync Now.
import ArkUI
import SwiftUI

/// The banks Plaid feeds the household's accounts from, as Settings lists
/// them (`BanksSection`: each bank, how it is doing and when it last
/// synced, and Connect a Bank), and Sync Now. Asking for a sync is the
/// button, or pulling down.
struct BanksView: View {
    @EnvironmentObject private var model: Model
    @State private var syncing = false

    var body: some View {
        List {
            BanksSection()
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
                .disabled(syncing || model.snapshot.banks.isEmpty)
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
