import SwiftUI
import ArkDB
import ArkDBClient

/// The server, the name, and whether to work alone; and what the session
/// says about itself. Applying reopens the session against the new answers.
struct SettingsView: View {
    @EnvironmentObject private var model: Model
    @State private var draft = Model.Settings.defaults

    var body: some View {
        NavigationStack {
            Form {
                Section("Server") {
                    TextField("ws://host:port/sync", text: $draft.server)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .disabled(draft.alone)
                    TextField("User name", text: $draft.user)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Toggle("Work alone", isOn: $draft.alone)
                    Text(draft.alone
                         ? "No server: this phone sequences its own log and never replays from zero on open. Tracks only arrive from a server, so the library stays empty."
                         : "Dev auth: the name is the login, and the server calls every login \"dev\".")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Button("Apply") { model.apply(draft) }
                        .disabled(draft == model.settings || draft.user.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                Section("Session") {
                    if let st = model.status {
                        row("Link", st.alone ? "alone" : (st.linked ? "linked" : st.link))
                        ForEach(st.cursors.keys.sorted(), id: \.self) { scope in
                            row("Cursor · \(scope)", "\(st.cursors[scope] ?? 0)")
                        }
                        row("Pending", "\(st.pending)")
                        row("Rejections", "\(st.rejections)")
                        if let d = st.denied { row("Denied", d) }
                        if let a = st.lastAgree { row("Last agree", "\(a.scope) @ \(a.seq): \(a.ok ? "ok" : "DIVERGED")") }
                    } else {
                        Text("No session").foregroundStyle(.secondary)
                    }
                    if let n = model.note { row("Note", n) }
                    Button("Verify against the authority") { model.verify() }
                    if !(model.status?.alone ?? true) {
                        if model.status?.linked ?? false {
                            Button("Go offline") { model.goOffline() }
                        } else {
                            Button("Go online") { model.goOnline() }
                        }
                    }
                }
                Section("Domain") {
                    row("Module", String(Harken.moduleHash.prefix(16)) + "…")
                    row("Native", Harken.procedureNames.joined(separator: ", "))
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
