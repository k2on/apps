// The first run's screens, drawn from a step and nothing else, on ArkUI's
// `WelcomePage`: the server's address, already filled with the household's
// (money.koon.us), with the demo beside it for anyone who wants to look
// first; signing in, which has no button — it goes by itself in the
// system's browser sheet and this page says what it waits for; and the
// budget on its way; and a server from a link or a scanned code, asked
// about before it is used. Every sentence the machine says is its own
// (`model.said`).
import ArkUI
import SwiftUI

struct ServerScreen: View {
    let typed: String
    let checking: Bool
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        WelcomePage(symbol: "dollarsign.circle.fill", tint: Color.accent, title: "Money",
                    sentence: "Your household's budget, every dollar given a job, on your own server.") {
            VStack(alignment: .leading, spacing: Space.s) {
                TextField(usualServer, text: Binding(get: { typed }, set: { model.send(.typed(text: $0)) }))
                    .textContentType(.URL)
                    .keypad(.url)
                    .capitals(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .onSubmit { model.send(.check) }
                    .disabled(checking)
                    .padding(Space.l)
                    .surface(in: RoundedRectangle(cornerRadius: 16, style: .continuous), interactive: true)
                    .accessibilityLabel("Server address")
                Text(error ?? "Where your household's budget is kept.")
                    .font(.footnote)
                    .foregroundStyle(error == nil ? Color.quiet : Color.error)
                    .padding(.horizontal, Space.xs)
            }
            Button { model.send(.check) } label: {
                HStack {
                    if checking { ProgressView() }
                    Text(checking ? model.said.status ?? "Continue" : "Continue")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.glassProminent)
            .tint(Color.accent)
            .controlSize(.large)
            .disabled(checking || typed.trimmingCharacters(in: .whitespaces).isEmpty)
            .padding(.top, Space.l)
            Button { model.send(.tryDemo) } label: {
                Text("Try the Demo").frame(maxWidth: .infinity)
            }
            .buttonStyle(.glass)
            .controlSize(.large)
            .disabled(checking)
            ScanButton(disabled: checking)
            Text("A household of two with three months of spending, on this phone alone.")
                .font(.footnote)
                .foregroundStyle(.quiet)
                .multilineTextAlignment(.center)
        }
    }
}

struct SignInScreen: View {
    let server: String
    let phase: Phase
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        WelcomePage(symbol: "person.badge.key.fill", tint: Color.accent, title: "Sign In",
                    sentence: "You'll sign in on \(Probe.display(server))'s own page, and come straight back here.") {
            if let e = error {
                Text(e).font(.callout).foregroundStyle(Color.error).multilineTextAlignment(.center).padding(.bottom, Space.l)
            }
            switch phase {
            case .browser:
                Label(model.said.status ?? "", systemImage: "safari").foregroundStyle(.quiet)
            case .exchanging:
                HStack(spacing: Space.m) { ProgressView(); Text(model.said.status ?? "") }.foregroundStyle(.quiet)
            case .idle:
                EmptyView()
            }
            Button("Use Another Server") { model.send(.changeServer) }
                .buttonStyle(.glass)
                .disabled(phase == .exchanging)
                .padding(.top, Space.xl)
        }
    }
}

/// "Scan a Code": the camera, for another device's "Add a Device" code,
/// which joins its server signed in (asked about first).
struct ScanButton: View {
    let disabled: Bool
    @State private var scanning = false
    @EnvironmentObject private var model: Model

    var body: some View {
        if QrScanner.available {
            Button { scanning = true } label: { Text("Scan a Code").frame(maxWidth: .infinity) }
                .buttonStyle(.glass)
                .controlSize(.large)
                .disabled(disabled)
                .sheet(isPresented: $scanning) {
                    QrScanner { text in
                        scanning = false
                        if let u = URL(string: text) { model.open(url: u) }
                    }
                    .ignoresSafeArea()
                }
        }
    }
}

/// A server from outside — a join link opened, a code scanned — asked about
/// before it is used: whose it is, and Join or Cancel. A link that cannot be
/// used says why, and can only be cancelled.
struct ConfirmScreen: View {
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        WelcomePage(symbol: "link.circle.fill", tint: Color.accent, title: model.said.question ?? "Join",
                    sentence: error ?? model.said.status ?? "") {
            if error == nil {
                Button { model.send(.accept) } label: { Text("Join").frame(maxWidth: .infinity) }
                    .buttonStyle(.glassProminent)
                    .tint(Color.accent)
                    .controlSize(.large)
            }
            Button { model.send(.decline) } label: { Text("Cancel").frame(maxWidth: .infinity) }
                .buttonStyle(.glass)
                .controlSize(.large)
        }
    }
}

/// Signed in just now, and the budget on its way: said quietly, and only after a beat.
struct OpeningScreen: View {
    let server: String
    @State private var shown = false
    @EnvironmentObject private var model: Model

    var body: some View {
        SwiftUI.Group {
            if shown {
                WelcomePage(symbol: "dollarsign.circle.fill", tint: Color.accent, title: "Money",
                            sentence: model.said.status ?? "") { ProgressView() }
                    .transition(.opacity)
            } else {
                Color.clear
            }
        }
        .task {
            try? await Task.sleep(for: .milliseconds(600))
            withAnimation(Springs.fade) { shown = true }
        }
    }
}
