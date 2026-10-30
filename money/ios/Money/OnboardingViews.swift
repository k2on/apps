// The first run's screens, drawn from a step and nothing else, on ArkUI's
// `WelcomePage`: the server's address, already filled with the household's
// (money.koon.us), with the demo beside it for anyone who wants to look
// first; signing in, which has no button — it goes by itself in the
// system's browser sheet and this page says what it waits for; and the
// budget on its way.
import ArkUI
import SwiftUI

struct ServerScreen: View {
    let typed: String
    let checking: Bool
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        WelcomePage(symbol: "dollarsign.circle.fill", tint: Color.money, title: "Money",
                    sentence: "Your household's budget, every dollar given a job, on your own server.") {
            VStack(alignment: .leading, spacing: Space.s) {
                TextField("money.example.com", text: Binding(get: { typed }, set: { model.send(.typed($0)) }))
                    .textContentType(.URL)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .onSubmit { model.send(.check) }
                    .disabled(checking)
                    .padding(Space.l)
                    .glassEffect(.regular.interactive(), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                    .accessibilityLabel("Server address")
                Text(error ?? "Where your household's budget is kept.")
                    .font(.footnote)
                    .foregroundStyle(error == nil ? Color.secondary : Color.red)
                    .padding(.horizontal, Space.xs)
            }
            Button { model.send(.check) } label: {
                HStack {
                    if checking { ProgressView() }
                    Text(checking ? "Checking…" : "Continue")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.glassProminent)
            .tint(Color.money)
            .controlSize(.large)
            .disabled(checking || typed.trimmingCharacters(in: .whitespaces).isEmpty)
            .padding(.top, Space.l)
            Button { model.send(.tryDemo) } label: {
                Text("Try the Demo").frame(maxWidth: .infinity)
            }
            .buttonStyle(.glass)
            .controlSize(.large)
            .disabled(checking)
            Text("A household of two with three months of spending, on this phone alone.")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
    }
}

struct SignInScreen: View {
    let server: String
    let phase: SignInPhase
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        WelcomePage(symbol: "person.badge.key.fill", tint: Color.money, title: "Sign In",
                    sentence: "You'll sign in on \(Probe.display(server))'s own page, and come straight back here.") {
            if let e = error {
                Text(e).font(.callout).foregroundStyle(Color.red).multilineTextAlignment(.center).padding(.bottom, Space.l)
            }
            switch phase {
            case .browser:
                Label("Waiting for the sign-in…", systemImage: "safari").foregroundStyle(.secondary)
            case .exchanging:
                HStack(spacing: Space.m) { ProgressView(); Text("Signing in…") }.foregroundStyle(.secondary)
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

/// Signed in just now, and the budget on its way: said quietly, and only after a beat.
struct OpeningScreen: View {
    let server: String
    @State private var shown = false

    var body: some View {
        SwiftUI.Group {
            if shown {
                WelcomePage(symbol: "dollarsign.circle.fill", tint: Color.money, title: "Money",
                            sentence: "Getting your budget from \(Probe.display(server))…") { ProgressView() }
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
