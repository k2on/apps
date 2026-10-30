// Settings: who is using the budget, which server and whether it can be
// reached, the bank, and the way out — signing out, or leaving the demo for
// a real server. Nothing here is a cursor or a count of pending intents
// beyond a plain "waiting to sync".
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
            Section {
                NavigationLink(value: Route.banks) { Label("Bank Connections", systemImage: "link") }
            }
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
