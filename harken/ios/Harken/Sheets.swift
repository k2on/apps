// What rises over a page (Android's `Sheets.kt`): the playlists a song can
// go on — each ticked when it is there, a tap putting it on or taking it
// off; naming a new playlist, checked as it is typed by `create_playlist`'s
// own input checks, whose sentence is shown under the field; and the device
// picker — where the sound is, where it could go, each device's volume and
// the speakers joined. (A song's menu itself is the row's `Menu`,
// Components.swift.)
import SwiftUI

/// Every playlist, ticked where `track` is on it; "New Playlist" first.
struct AddToPlaylistSheet: View {
    let track: LibraryEntry
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @Environment(\.dismiss) private var dismiss
    @State private var on: Set<Id>? = nil

    var body: some View {
        NavigationStack {
            List {
                Button { present(.newPlaylist(then: track)) } label: { Label("New Playlist", systemImage: "plus") }
                    .foregroundStyle(Color.gold)
                ForEach(model.playlists) { p in
                    let ticked = on?.contains(p.id) == true
                    Button {
                        model.toggle(track, on: p, now: on ?? [])
                        var o = on ?? []
                        if ticked { o.remove(p.id) } else { o.insert(p.id) }
                        on = o
                    } label: {
                        HStack {
                            Label(p.name, systemImage: ticked ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(ticked ? Color.gold : .primary)
                            Spacer()
                        }
                    }
                    .sensoryFeedback(.selection, trigger: ticked)
                }
            }
            .navigationTitle("Add “\(track.title)” to")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .task(id: model.playlists.count) { model.playlistsOf(track) { on = $0 } }
    }
}

/// Naming a new playlist: the field, and the domain's sentence when the name will not do.
struct NewPlaylist: View {
    let then: LibraryEntry?
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var problem: String? = nil
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name).focused($focused).submitLabel(.done).onSubmit(make)
                } footer: {
                    if let p = problem, !name.isEmpty {
                        Text(p.prefix(1).uppercased() + p.dropFirst()).foregroundStyle(.red)
                    } else {
                        Text("A name you already use gets a number after it.")
                    }
                }
            }
            .navigationTitle("New Playlist")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel", role: .cancel) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(then == nil ? "Make It" : "Make and Add", action: make)
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || problem != nil)
                }
            }
        }
        .onAppear { focused = true }
        .onChange(of: name) { _, n in model.nameProblem(n) { p in if n == name { problem = p } } }
    }

    private func make() {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty, problem == nil else { return }
        model.createPlaylist(name, then: then)
        dismiss()
    }
}

/// Where the sound is, and everywhere it could be: a row per device of this
/// account, ticked where the sound is — every speaker of the group that
/// plays — or, while it is nowhere, on this phone, where the next press
/// plays. A device that cannot make a sound, or whose socket has gone, is
/// drawn and not selectable. A row with a volume carries its slider and its
/// mute; a speaker that can join what plays a `+`, one in its group a `−`;
/// the group first, as itself.
struct DevicesSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let rows = model.deviceRows
        let cursor = DevicePicker.cursor(rows)
        NavigationStack {
            List {
                if rows.isEmpty {
                    Text("Nothing else is listening. A device signed in as you appears here once it is open.").foregroundStyle(.secondary)
                }
                ForEach(Array(rows.enumerated()), id: \.element.id) { i, r in
                    DeviceLine(row: r, at: i == cursor) {
                        model.pick(r)
                        dismiss()
                    }
                }
            }
            .navigationTitle("Playing On")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }
}

private struct DeviceLine: View {
    let row: DeviceRow
    let at: Bool
    let pick: () -> Void
    @EnvironmentObject private var model: Model

    var body: some View {
        let lit = (row.ticked || row.group) && !row.away
        VStack(alignment: .leading, spacing: Space.xs) {
            HStack(spacing: Space.l) {
                Image(systemName: symbol(forDevice: row.kind, name: row.name)).frame(width: 24)
                VStack(alignment: .leading) {
                    Text(row.name).lineLimit(1)
                    if let u = row.under { Text(u).font(.caption).foregroundStyle(.secondary) }
                }
                Spacer()
                if let join = row.join {
                    Button { model.join(row) } label: { Image(systemName: join ? "plus.circle" : "minus.circle") }
                        .buttonStyle(.borderless)
                        .accessibilityLabel(join ? "Add \(row.name) to the group" : "Take \(row.name) out of the group")
                }
                if row.group { Image(systemName: "waveform").accessibilityLabel("Playing") }
                else if row.ticked { Image(systemName: "checkmark").accessibilityLabel("Playing here") }
            }
            .foregroundStyle(lit ? Color.gold : (row.takeable ? .primary : .secondary))
            .contentShape(Rectangle())
            .onTapGesture { if row.takeable { pick() } }
            if let v = row.volume {
                VolumeSlider(level: v, muted: row.muted, about: row.name, onLevel: { model.level(row, $0) }, onDone: { model.volumeDone() }, onMute: { model.mute(row) })
                    .padding(.leading, 40)
            }
        }
        .listRowBackground(at ? Color.gold.opacity(0.08) : nil)
    }
}
