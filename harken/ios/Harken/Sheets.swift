// What rises over a page (Android's `Sheets.kt`): the playlists a song can
// go on — each ticked when it is there, a tap putting it on or taking it
// off; naming a new playlist, checked as it is typed by `create_playlist`'s
// own input checks, whose sentence is shown under the field; and the device
// picker — where the sound is, where it could go, each device's volume and
// the speakers joined; and finding a show to follow, in Apple's podcast
// directory or by its feed's address. (A song's menu itself is the row's
// `Menu`, Components.swift.)
import ArkUI
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

/// A show Apple's podcast directory found: what it is called, who makes it,
/// its feed and its picture — the desktop's `directory::Found`.
struct Offered: Identifiable, Equatable {
    let title: String
    let author: String
    let feed: String
    let art: String
    var id: String { feed }
}

/// Apple's podcast directory, asked from the phone and not through the
/// server: what is searched for is nobody else's business. Following what
/// it finds is the same `library.subscribe` a pasted address is (the
/// desktop's `directory.rs`).
enum Directory {
    /// Whether what was typed is a feed's address rather than words to search for.
    static func isFeed(_ typed: String) -> Bool {
        let t = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.hasPrefix("http://") || t.hasPrefix("https://")
    }

    /// What the directory's search leaves as it is; the rest is percent-encoded.
    private static let plain = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")

    /// The shows the directory finds for `words`, podcasts only, each that
    /// has a feed — every one that can be followed. Thrown, the sentence says why.
    static func search(_ words: String) async throws -> [Offered] {
        let term = words.trimmingCharacters(in: .whitespacesAndNewlines).addingPercentEncoding(withAllowedCharacters: plain) ?? ""
        guard let url = URL(string: "https://itunes.apple.com/search?media=podcast&entity=podcast&limit=25&term=" + term) else { return [] }
        let data: Data
        do {
            let (got, response) = try await URLSession.shared.data(from: url)
            if let http = response as? HTTPURLResponse, http.statusCode != 200 { throw Said("Could not search: the directory answered \(http.statusCode).") }
            data = got
        } catch let s as Said {
            throw s
        } catch {
            if Task.isCancelled { throw CancellationError() }
            throw Said("Could not search: \(error.localizedDescription)")
        }
        guard let answer = try? JSONDecoder().decode(Answer.self, from: data) else { throw Said("Could not search: the directory said something else.") }
        return answer.results.compactMap { s -> Offered? in
            guard let feed = s.feedUrl, !feed.isEmpty else { return nil }
            let art = (s.artworkUrl600 ?? "").isEmpty ? (s.artworkUrl100 ?? "") : (s.artworkUrl600 ?? "")
            return Offered(title: s.collectionName ?? "", author: s.artistName ?? "", feed: feed, art: art)
        }
    }

    /// Why a search failed, as the sheet says it.
    struct Said: Error {
        let text: String
        init(_ text: String) { self.text = text }
    }

    private struct Answer: Decodable { let results: [Show] }
    private struct Show: Decodable {
        let collectionName: String?
        let artistName: String?
        let feedUrl: String?
        let artworkUrl600: String?
        let artworkUrl100: String?
    }
}

/// Finding a show to follow, as `kind` — the + on Podcasts and on Sermons
/// (the desktop's): a field, focused, that searches Apple Podcasts a moment
/// after the typing stops, or takes a feed's address pasted in; each show
/// found with its picture, its name, who makes it and Follow, which is
/// Following once the library follows that feed.
struct FollowSheet: View {
    let kind: String
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""
    @State private var found = [Offered]()
    /// The words `found` and `problem` answer; while it is not what is typed, a search is on its way.
    @State private var answered = ""
    @State private var problem: String? = nil
    @FocusState private var focused: Bool

    var body: some View {
        let words = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        let following = Set(model.subscriptions.map { $0.url })
        let feed = Directory.isFeed(words)
        NavigationStack {
            List {
                HStack(spacing: Space.s) {
                    Image(systemName: feed ? "link" : "magnifyingglass").foregroundStyle(.secondary)
                    TextField("Search Apple Podcasts, or paste a feed", text: $typed)
                        .focused($focused)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .submitLabel(feed ? SubmitLabel.done : SubmitLabel.search)
                }
                if feed {
                    OfferedRow(found: Offered(title: words, author: "A feed's address", feed: words, art: ""),
                             following: following.contains(words)) { model.follow(kind, words) }
                } else if words.isEmpty {
                    Text(kind == "sermon"
                         ? "Search for a church or a preacher, or paste the address of its sermons' feed."
                         : "Search for a podcast by its name or who makes it, or paste the address of its feed.")
                        .font(.subheadline).foregroundStyle(.secondary)
                        .listRowSeparator(.hidden)
                } else {
                    if answered != words {
                        HStack(spacing: Space.s) {
                            ProgressView()
                            Text("Searching…").foregroundStyle(.secondary)
                        }
                        .listRowSeparator(.hidden)
                        .transition(.opacity)
                    } else if let p = problem {
                        Text(p).font(.subheadline).foregroundStyle(.secondary).listRowSeparator(.hidden)
                    } else if found.isEmpty {
                        Text("Nothing found for “\(words)”.").font(.subheadline).foregroundStyle(.secondary).listRowSeparator(.hidden)
                    }
                    ForEach(found) { f in
                        OfferedRow(found: f, following: following.contains(f.feed)) { model.follow(kind, f.feed) }
                            .transition(.opacity.combined(with: .move(edge: .bottom)))
                    }
                }
            }
            .listStyle(.plain)
            .animation(.snappy, value: found)
            .animation(.snappy, value: answered)
            .navigationTitle(kind == "sermon" ? "Follow Sermons" : "Follow a Podcast")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .onAppear { focused = true }
        // A search a moment after the last key; typed again before it answers, it is let go.
        .task(id: words) { await search(words) }
    }

    private func search(_ words: String) async {
        guard !words.isEmpty, !Directory.isFeed(words) else {
            found = []
            problem = nil
            answered = words
            return
        }
        do { try await Task.sleep(nanoseconds: 300_000_000) } catch { return }
        do {
            let got = try await Directory.search(words)
            if Task.isCancelled { return }
            found = got
            problem = nil
        } catch let s as Directory.Said {
            if Task.isCancelled { return }
            found = []
            problem = s.text
        } catch {
            return
        }
        answered = words
    }
}

/// A show that can be followed: its picture, its name, who makes it, and
/// Follow — or Following, ticked, once the library follows its feed.
private struct OfferedRow: View {
    let found: Offered
    let following: Bool
    let follow: () -> Void

    var body: some View {
        HStack(spacing: Space.m) {
            CoverView(seed: found.title, url: found.art, side: 56).frame(width: 56, height: 56)
            VStack(alignment: .leading, spacing: 2) {
                Text(found.title).font(.headline).lineLimit(2)
                if !found.author.isEmpty { Text(found.author).font(.subheadline).foregroundStyle(.secondary).lineLimit(1) }
            }
            Spacer(minLength: 0)
            if following {
                Label("Following", systemImage: "checkmark")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.gold)
                    .transition(.scale.combined(with: .opacity))
            } else {
                Button("Follow", action: follow)
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .tint(Color.gold)
                    .transition(.scale.combined(with: .opacity))
            }
        }
        .animation(.snappy, value: following)
        .sensoryFeedback(.success, trigger: following)
    }
}
