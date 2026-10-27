// The onboarding's screens, drawn from a step and nothing else (Android's
// `ui/Onboarding.kt`): the server's address, signing in, and the library on
// its way. The sign-in has no button: it goes by itself, in the system's
// browser sheet (`ASWebAuthenticationSession`, through SwiftUI's
// `webAuthenticationSession`), and this page says what it is waiting for —
// or, once a sign-in came back without a login, why, and how to try again
// (leave and come back, the web's reload), never by going round by itself.
import ArkUI
import SwiftUI

/// One onboarding page: the mark in the gold, a title, a sentence, and what to do.
struct OnboardingPage<Content: View>: View {
    let title: String
    let sentence: String
    @ViewBuilder let content: () -> Content

    var body: some View {
        ScrollView {
            VStack(spacing: Space.s) {
                Image(systemName: "waveform")
                    .font(.system(size: 44, weight: .semibold))
                    .foregroundStyle(Color.gold)
                    .frame(width: 88, height: 88)
                    .glassEffect(.regular, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
                    .padding(.bottom, Space.l)
                Text(title).font(.largeTitle.bold()).multilineTextAlignment(.center)
                Text(sentence).font(.body).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    .padding(.bottom, Space.xxl)
                content()
            }
            .frame(maxWidth: 420)
            .padding(.horizontal, Space.xxl)
            .padding(.vertical, Space.xl)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .safeAreaPadding(.top, 60)
    }
}

/// The server's address: one field, and under it the last check's error or a
/// sentence on what to type. While checking, nothing can be changed.
struct ServerScreen: View {
    let typed: String
    let checking: Bool
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        OnboardingPage(title: "Welcome to Harken", sentence: "Harken plays from your own server. Where is it?") {
            VStack(alignment: .leading, spacing: Space.s) {
                TextField("music.example.com", text: Binding(get: { typed }, set: { model.send(.typed($0)) }))
                    .textContentType(.URL)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .onSubmit { model.send(.check) }
                    .disabled(checking)
                    .padding(Space.l)
                    .glassEffect(.regular.interactive(), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                Text(error ?? "A name or an address, with a port if it has one.")
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
            .tint(.gold)
            .controlSize(.large)
            .disabled(checking || typed.trimmingCharacters(in: .whitespaces).isEmpty)
            .padding(.top, Space.l)
        }
    }
}

/// Signing in on the server's own page: what it is waiting for, and the error the last attempt or the server said.
struct SignInScreen: View {
    let server: String
    let phase: SignInPhase
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        OnboardingPage(title: "Sign In", sentence: "You'll sign in on \(Probe.display(server))'s own page, and come straight back here.") {
            if let e = error {
                Text(e).font(.callout).foregroundStyle(.red).multilineTextAlignment(.center).padding(.bottom, Space.l)
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

/// Signed in just now, and the library on its way: said quietly, and only after a beat.
struct OpeningScreen: View {
    let server: String
    @State private var shown = false

    var body: some View {
        Group {
            if shown {
                OnboardingPage(title: "Harken", sentence: "Getting your library from \(Probe.display(server))…") { ProgressView().tint(.gold) }
                    .transition(.opacity)
            } else {
                Color.clear
            }
        }
        .task {
            try? await Task.sleep(nanoseconds: 600_000_000)
            withAnimation(Springs.fade) { shown = true }
        }
    }
}
