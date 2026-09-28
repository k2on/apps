import SwiftUI
import ArkDB
import ArkDBClient

/// Who is signed in, the server, and whether to work alone; what the
/// session says about itself; and where every change this phone made
/// stands, with the reason beside any the server would not keep.
struct SettingsView: View {
    @EnvironmentObject private var model: Model
    @State private var draft = Model.Settings.defaults
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
                            .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                        Text("Not signed in: everything works and is kept on this phone, and nothing is sent anywhere. Signing in makes it yours and syncs it. Dev auth: the name is the login.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Section("Server") {
                    TextField("ws://host:port/sync", text: $draft.server)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .disabled(draft.alone)
                    Toggle("Work alone", isOn: $draft.alone)
                    Text(draft.alone
                         ? "No server: this phone sequences its own log. Songs only arrive from a server, so the library stays empty."
                         : "The server this phone syncs with once somebody signs in.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Button("Apply") { model.apply(draft) }
                        .disabled(draft == model.settings)
                }
                Section("Session") {
                    if let st = model.status {
                        row("Link", st.alone ? "alone" : (!st.signedIn ? "signed out" : (st.linked ? "linked" : st.link)))
                        row("Cursor", "\(st.cursor)")
                        row("Not synced", "\(st.pending)")
                        row("Not saved", "\(st.rejections)")
                        if let d = st.denied { row("Denied", d) }
                        if let a = st.lastAgree { row("Last agree", "\(a.seq): \(a.ok ? "ok" : "DIVERGED")") }
                    } else {
                        Text("No session").foregroundStyle(.secondary)
                    }
                    if let n = model.note { row("Note", n) }
                    Button("Verify against the authority") { model.verify() }
                    if let st = model.status, !st.alone, st.signedIn {
                        if st.linked {
                            Button("Go offline") { model.goOffline() }
                        } else {
                            Button("Go online") { model.goOnline() }
                        }
                    }
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
                Section("Domain") {
                    row("Module", String(Phone.moduleHash.prefix(16)) + "…")
                    row("Native", Phone.procedureNames.joined(separator: ", "))
                }
            }
            .navigationTitle("Settings")
            .onAppear { draft = model.settings }
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
