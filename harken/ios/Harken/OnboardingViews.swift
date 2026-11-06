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
    /// The mark grows with the person's text size, as the title under it does.
    @ScaledMetric(relativeTo: .largeTitle) private var mark: CGFloat = 88

    var body: some View {
        ScrollView {
            VStack(spacing: Space.s) {
                Image(systemName: "waveform")
                    .font(.system(size: mark / 2, weight: .semibold))
                    .foregroundStyle(Color.accentText)
                    .frame(width: mark, height: mark)
                    .surface(in: RoundedRectangle(cornerRadius: mark * 3 / 11, style: .continuous))
                    .padding(.bottom, Space.l)
                Text(title).font(.largeTitle.bold()).multilineTextAlignment(.center)
                Text(sentence).font(.body).foregroundStyle(.quiet).multilineTextAlignment(.center)
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
                TextField("music.example.com", text: Binding(get: { typed }, set: { model.send(.typed(text: $0)) }))
                    .textContentType(.URL)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .onSubmit { model.send(.check) }
                    .disabled(checking)
                    .padding(Space.l)
                    .surface(in: RoundedRectangle(cornerRadius: 16, style: .continuous), interactive: true)
                Text(error ?? "A name or an address, with a port if it has one.")
                    .font(.footnote)
                    .foregroundStyle(error == nil ? Color.quiet : Color.error)
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
            .tint(Color.accent)
            .controlSize(.large)
            .disabled(checking || typed.trimmingCharacters(in: .whitespaces).isEmpty)
            .padding(.top, Space.l)
            ScanButton(disabled: checking)
        }
    }
}

/// Signing in on the server's own page: what it is waiting for, and the error the last attempt or the server said.
struct SignInScreen: View {
    let server: String
    let phase: Phase
    let error: String?
    @EnvironmentObject private var model: Model

    var body: some View {
        OnboardingPage(title: "Sign In", sentence: "You'll sign in on \(Probe.display(server))'s own page, and come straight back here.") {
            if let e = error {
                Text(e).font(.callout).foregroundStyle(.error).multilineTextAlignment(.center).padding(.bottom, Space.l)
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
        OnboardingPage(title: model.said.question ?? "Join", sentence: error ?? model.said.status ?? "") {
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

/// Signed in just now, and the library on its way: said quietly, and only after a beat.
struct OpeningScreen: View {
    let server: String
    @State private var shown = false
    @EnvironmentObject private var model: Model

    var body: some View {
        Group {
            if shown {
                OnboardingPage(title: "Harken", sentence: model.said.status ?? "") { ProgressView().tint(Color.accent) }
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
