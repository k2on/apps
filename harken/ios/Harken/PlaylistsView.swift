import SwiftUI
import ArkDB

/// The playlists, by name — the `playlists` query. Tapping one opens it and
/// makes it the one the library adds to; `+` names a new one, which is
/// `create_playlist`.
struct PlaylistsView: View {
    @EnvironmentObject private var model: Model
    @State private var naming = false
    @State private var newName = ""

    var body: some View {
        NavigationStack {
            Group {
                if model.playlists.isEmpty {
                    ContentUnavailableView("No playlists", systemImage: "text.badge.plus",
                                           description: Text("Make one with the + button."))
                } else {
                    List(model.playlists) { p in
                        NavigationLink(value: p) {
                            HStack {
                                Text(p.name)
                                Spacer()
                                if p.id == model.selectedPlaylist {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Playlists")
            .navigationDestination(for: PlaylistSummary.self) { p in
                PlaylistView(playlist: p)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        newName = ""
                        naming = true
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel("New playlist")
                }
            }
            .sheet(isPresented: $naming) {
                NewPlaylistSheet(name: $newName, problem: model.nameProblem) {
                    model.createPlaylist(named: newName)
                    naming = false
                }
                .presentationDetents([.height(210)])
            }
        }
    }
}

/// Names a new playlist. What is wrong with the name is said under the
/// field as it is typed, by `create_playlist`'s own input checks through the
/// form validator — the same words the mutation would refuse with — and
/// Create is off while there is something to say.
struct NewPlaylistSheet: View {
    @Binding var name: String
    let problem: (String) -> String?
    let create: () -> Void
    @Environment(\.dismiss) private var dismiss
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name)
                        .focused($focused)
                        .submitLabel(.done)
                        .onSubmit { if problem(name) == nil { create() } }
                } footer: {
                    if !name.isEmpty, let why = problem(name) {
                        Text(why).foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("New playlist")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create", action: create)
                        .disabled(problem(name) != nil)
                }
            }
            .onAppear { focused = true }
        }
    }
}
