// Settings: who is signed in, which server and whether it can be reached,
// signing out, and what this is (Android's `SettingsScreen.kt`). Nothing a
// person sees here is a cursor, a hash or a count of pending intents: those
// are the diagnostics, behind the version — tapped seven times, the way a
// phone's build number unlocks its developer options — and remembered once
// found.
import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @State private var taps = 0
    @State private var said: String? = nil

    var body: some View {
        Form {
            Section("Account") {
                HStack(spacing: Space.l) {
                    let name = model.login.map(Logins.name) ?? ""
                    Text(name.prefix(1).uppercased())
                        .font(.title2.bold())
                        .frame(width: 48, height: 48)
                        .glassEffect(.regular.tint(.gold), in: Circle())
                    VStack(alignment: .leading) {
                        Text(name).font(.headline)
                        Text(model.login.map { $0.email.isEmpty ? $0.user : $0.email } ?? "").foregroundStyle(.secondary)
                    }
                }
            }
            Section("Server") {
                LabeledContent(Probe.display(model.server)) {
                    Text(model.offline ? "Offline" : (model.snapshot.status?.linked == true ? "Connected" : "Can't reach it right now"))
                }
                Toggle("Work offline", isOn: Binding(get: { model.offline }, set: { _ in model.toggleOnline() }))
            }
            Section {
                Button(role: .destructive) { model.signOut() } label: { Label("Sign Out", systemImage: "rectangle.portrait.and.arrow.right") }
            } footer: {
                Text("Anything not synced yet stays on this phone, and goes when you sign in again.")
            }
            Section("About") {
                LabeledContent("Harken", value: "Your music, from your own server")
                Button { tapped() } label: { LabeledContent("Version", value: Self.version) }.foregroundStyle(.primary)
                if model.diagnosticsUnlocked {
                    Button { open(.diagnostics) } label: {
                        LabeledContent("Diagnostics", value: "What this phone's copy of the library is doing")
                    }
                    .foregroundStyle(.primary)
                }
            }
        }
        .navigationTitle("Settings")
        .overlay(alignment: .bottom) {
            if let s = said {
                Text(s).font(.footnote).padding(.horizontal, Space.l).padding(.vertical, Space.s)
                    .glassEffect(.regular, in: Capsule())
                    .padding(.bottom, Space.xl)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(Springs.move, value: said)
    }

    private func tapped() {
        taps += 1
        if taps >= 7 {
            taps = 0
            model.unlockDiagnostics()
            open(.diagnostics)
        } else if taps >= 4 && !model.diagnosticsUnlocked {
            said = "\(7 - taps) more taps for diagnostics"
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { said = nil }
        }
    }

    /// "1.0 (1)", from the bundle.
    static var version: String {
        let i = Bundle.main.infoDictionary
        return "\(i?["CFBundleShortVersionString"] as? String ?? "?") (\(i?["CFBundleVersion"] as? String ?? "?"))"
    }
}

/// Every number the old settings screen showed, every change this phone made
/// with where it stands, and every number this end of the listening session
/// holds — the desktop's `D`. Read again whenever the peer reports a change.
struct DiagnosticsView: View {
    @EnvironmentObject private var model: Model
    @State private var status: Status? = nil
    @State private var changes = [(String, String)]()

    var body: some View {
        Form {
            Section("Peer") {
                if let s = status { ForEach(Self.numbers(s), id: \.0) { Number(name: $0.0, value: $0.1) } }
                else { Text("Opening…").foregroundStyle(.secondary) }
            }
            Section("Listening") {
                ForEach(Array(Self.listening(model.heard).enumerated()), id: \.offset) { Number(name: $0.element.0, value: $0.element.1) }
            }
            Section("Changes from this phone") {
                if changes.isEmpty { Text("None yet — what you change here is listed with where it stands").foregroundStyle(.secondary) }
                ForEach(Array(changes.enumerated()), id: \.offset) { LabeledContent($0.element.0, value: $0.element.1) }
            }
        }
        .navigationTitle("Diagnostics")
        .task(id: model.snapshot.moved) {
            model.diagnostics { s, c in
                status = s
                changes = c
            }
        }
    }

    static func numbers(_ s: Status) -> [(String, String)] {
        [("user", s.user), ("session", s.session), ("link", s.link), ("linked", "\(s.linked)"), ("epoch", "\(s.epoch)"),
         ("cursor", "\(s.cursor)"), ("not synced", "\(s.pending)"), ("joining", "\(s.joining)"), ("alone", "\(s.alone)"),
         ("disk", s.failure ?? "—"), ("denied", s.denied ?? "—")]
    }

    static func listening(_ h: Heard) -> [(String, String)] {
        let s = h.listening
        let a = s?.active
        var out: [(String, String)] = [
            ("device", h.me.isEmpty ? "—" : h.me), ("part of the session", "\(h.joined)"), ("linked", "\(h.linked)"), ("epoch", "\(h.epoch)"),
            ("calls", "\(h.stats.calls)"), ("refused", "\(h.stats.refused)"), ("plays recorded", "\(h.stats.plays)"),
            ("came back playing", "\(h.stats.cameBackPlaying)"), ("last refusal", h.refused ?? h.stats.lastRefusal ?? "—"),
        ]
        if s == nil { out.append(("session", "—")) }
        else if let a = a { out.append(("session", "\(a.index + 1) of \(a.queue.count), \(a.clock.playing ? "playing" : "paused") at \(Spell.clock(a.position(nowMs())))")) }
        else { out.append(("session", "idle")) }
        switch a?.output {
        case nil: out.append(("output", "—"))
        case let .device(id, moving)?: out.append(("output", id + (moving ? " (moving)" : "")))
        case let .away(id, _)?: out.append(("output", "\(id) (away)"))
        case .nowhere?: out.append(("output", "nowhere"))
        }
        for d in (s?.devices.values).map(Array.init) ?? [] {
            out.append(("device \(d.name)", "\(d.kind), \(d.canPlay ? "can play" : "cannot play"), \(d.id)"))
            let v = !d.canVolume ? "none" : (d.muted ? "\(d.volume) (muted)" : "\(d.volume)")
            out.append(("  volume", "\(v) · family \(d.family ?? "none") · follows \(d.follows ?? "nobody")"))
        }
        if let s = s {
            if let l = s.leader, s.grouped {
                out.append(("group", "\(l) + \(s.followers(l).map { $0.id }.joined(separator: ", ")) · volume \(s.groupLevel(l).map(String.init) ?? "none")\(s.groupMuted(l) ? " (muted)" : "")"))
            } else {
                out.append(("group", "none"))
            }
            if s.scrub.isEmpty { out.append(("scrub", "none")) }
            for (id, at) in s.scrub { out.append(("scrub \(id == h.me ? "(this phone)" : id)", Spell.clock(at))) }
        }
        return out
    }
}

private struct Number: View {
    let name: String
    let value: String

    var body: some View {
        HStack {
            Text(name).foregroundStyle(.secondary)
            Spacer()
            Text(value).font(.callout.monospaced()).multilineTextAlignment(.trailing)
        }
    }
}
