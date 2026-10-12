import ArkDB
import ArkDBClient
import SwiftUI

/// Who is signed in, the server, and whether to work alone; what the peer
/// says about itself (`Peer.status`, field for field what a status line or a
/// debug screen wants); and where every change this phone made stands, with
/// the reason beside any the server would not keep.
struct SettingsView: View {
    @EnvironmentObject private var model: Model
    @State private var server = ""
    @State private var workAlone = false
    @State private var name = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Account") {
                    if let who = model.user {
                        row("Signed in as", who)
                        Button("Sign out") { model.signOut() }
                        Text("What you made and has not synced stays yours, and goes when you sign in again.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else {
                        TextField("Name", text: $name)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        Button("Sign in") { model.signIn(as: name) }
                            .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || workAlone)
                        Text("Not signed in: everything works and is kept on this phone, and nothing is sent anywhere. Signing in makes it yours and syncs it. Dev auth only: the name is the login, and a server that signs people in through a provider's page is not supported here.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Section("Server") {
                    TextField("http://host:port", text: $server)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .disabled(workAlone)
                    Toggle("Work alone", isOn: $workAlone)
                    Text(workAlone
                         ? "No server: this phone sequences its own log, in a replica of its own. Songs only arrive from a server, so the library stays empty."
                         : "The server this phone syncs with once somebody signs in.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Button("Apply") { model.apply(server: server, workAlone: workAlone) }
                        .disabled(server == model.prefs.server && workAlone == model.prefs.workAlone)
                }
                Section("Status") {
                    if let st = model.status {
                        row("Link", st.link)
                        row("Linked", st.linked ? "yes" : "no")
                        row("Cursor", "\(st.cursor)")
                        row("Not synced", "\(st.pending)")
                        if st.held > 0 { row("Held", "\(st.held)") }
                        if st.diverged > 0 { row("Diverged", "\(st.diverged)") }
                        if st.joining > 0 { row("Joining", "\(st.joining)") }
                        row("Connections", "\(st.opens)")
                        if st.behind { row("Behind", "the server runs another module") }
                        if let c = st.lastClose { row("Last close", c) }
                        if let d = st.denied { row("Denied", d) }
                    } else {
                        Text("No peer").foregroundStyle(.secondary)
                    }
                    if let n = model.notice { row("Note", n) }
                    Button("Verify against the authority") { model.verify() }
                }
                Section("Changes") {
                    if model.changes.isEmpty {
                        Text("Nothing changed on this phone yet").foregroundStyle(.secondary)
                    }
                    ForEach(model.changes) { c in
                        let why = model.caption(for: c)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(c.what)
                            Text(why)
                                .font(.caption)
                                .foregroundStyle(why.hasPrefix("not saved") ? .red : .secondary)
                        }
                    }
                }
            }
            .navigationTitle("Settings")
            .onAppear {
                server = model.prefs.server
                workAlone = model.prefs.workAlone
            }
        }
    }

    private func row(_ k: String, _ v: String) -> some View {
        HStack {
            Text(k)
            Spacer()
            Text(v)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.trailing)
        }
    }
}
