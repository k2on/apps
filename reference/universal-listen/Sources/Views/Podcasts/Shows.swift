import SwiftUI

/// Followed shows (with unplayed counts) and the latest episodes.
struct ShowsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 150, maximum: 220), spacing: 18, alignment: .top)], spacing: 22) {
                    ForEach(library.shows) { show in
                        NavigationLink(value: Route.show(show.id)) {
                            ShowTile(show: show, unplayed: library.unplayedCount(show))
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("show.\(show.id)")
                    }
                }
                VStack(alignment: .leading, spacing: 0) {
                    ShelfHeader(title: "Latest Episodes", route: .downloads)
                        .padding(.bottom, 8)
                    ForEach(library.latestEpisodes.prefix(8)) { e in
                        EpisodeRow(episode: e, showsArtwork: true)
                        Divider()
                    }
                }
            }
            .padding(20)
        }
        .navigationTitle("Podcasts & Sermons")
    }
}

struct ShowTile: View {
    let show: Show
    let unplayed: Int

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Artwork(art: show.art)
                .shadow(color: .black.opacity(0.12), radius: 6, y: 3)
                .overlay(alignment: .topTrailing) {
                    if unplayed > 0 {
                        Text("\(unplayed)")
                            .font(.caption.weight(.bold).monospacedDigit())
                            .foregroundStyle(.white)
                            .padding(.horizontal, 7)
                            .frame(minWidth: 24, minHeight: 24)
                            .background(Color.accentColor, in: Capsule())
                            .padding(8)
                            .accessibilityLabel("\(unplayed) unplayed")
                    }
                }
            Text(show.title).font(.subheadline.weight(.medium)).lineLimit(1)
            Text(show.kind == .sermons ? "Sermons · \(show.author)" : show.author)
                .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
        }
        .accessibilityElement(children: .combine)
    }
}

/// An episode in a list, as Podcasts shows it: date, title, summary, then
/// a play capsule with time left, and its download state.
struct EpisodeRow: View {
    let episode: Episode
    var showsArtwork = false
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        let show = library.show(episode.showID)
        let current = player.currentEpisodeID == episode.id
        HStack(alignment: .top, spacing: 14) {
            if showsArtwork, let show {
                Artwork(art: show.art).frame(width: 64, height: 64)
            }
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    if episode.isUnplayed {
                        Circle().fill(.tint).frame(width: 7, height: 7).accessibilityLabel("Unplayed")
                    }
                    Text(Format.day(episode.date).uppercased())
                    if showsArtwork, let show { Text("· \(show.title.uppercased())").lineLimit(1) }
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                Text(episode.title).font(.headline).lineLimit(2)
                Text(episode.scripture.map { "\($0) — \(episode.summary)" } ?? episode.summary)
                    .font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                HStack(spacing: 12) {
                    Button {
                        if current { player.togglePlay() } else { player.play(.episode(episode.id)) }
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: current && player.isPlaying ? "pause.fill" : "play.fill")
                            if episode.isInProgress {
                                ProgressView(value: episode.progress).frame(width: 36).tint(.accentColor)
                            }
                            Text(current ? "Playing" : Format.remaining(episode))
                                .monospacedDigit()
                        }
                        .font(.caption.weight(.semibold))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(.tint.opacity(0.14), in: Capsule())
                        .foregroundStyle(.tint)
                    }
                    .buttonStyle(PressableStyle(scale: 0.94))
                    .accessibilityLabel(current && player.isPlaying ? "Pause" : "Play, \(Format.remaining(episode))")
                    DownloadBadge(state: episode.download)
                    Spacer()
                    Menu {
                        EpisodeMenuItems(episode: episode)
                    } label: {
                        Image(systemName: "ellipsis").frame(width: 30, height: 30).contentShape(Rectangle())
                    }
                    .menuIndicator(.hidden)
                    .buttonStyle(.plain)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel("More")
                    .accessibilityIdentifier("more.\(episode.id)")
                }
                .padding(.top, 2)
            }
        }
        .padding(.vertical, 12)
        .contentShape(Rectangle())
        .onTapGesture { nav.go(.episode(episode.id)) }
        .contextMenu { EpisodeMenuItems(episode: episode) }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("episode.\(episode.id)")
    }
}

struct DownloadBadge: View {
    let state: DownloadState

    var body: some View {
        switch state {
        case .none:
            Image(systemName: "arrow.down.circle").foregroundStyle(.secondary).accessibilityLabel("Not downloaded")
        case .downloading(let p):
            ProgressView(value: p).progressViewStyle(.circular).controlSize(.small).accessibilityLabel("Downloading")
                .frame(width: 18, height: 18)
        case .downloaded:
            Image(systemName: "arrow.down.circle.fill").foregroundStyle(.secondary).accessibilityLabel("Downloaded")
        }
    }
}

struct ShowView: View {
    let id: String
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @State private var filter = "All"

    var body: some View {
        if let show = library.show(id) {
            let all = library.episodes(of: show)
            let episodes = filter == "Unplayed" ? all.filter { $0.progress < 1 } : (filter == "Downloaded" ? all.filter { $0.download == .downloaded } : all)
            List {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: 24) {
                        Artwork(art: show.art).frame(width: 200, height: 200)
                        showText(show, all: all).frame(width: 440, alignment: .leading)
                        Spacer(minLength: 0)
                    }
                    VStack(alignment: .leading, spacing: 16) {
                        Artwork(art: show.art).frame(width: 200, height: 200).frame(maxWidth: .infinity)
                        showText(show, all: all)
                    }
                }
                .listRowSeparator(.hidden)
                .padding(.vertical, 8)
                Picker("Episodes", selection: $filter) {
                    ForEach(["All", "Unplayed", "Downloaded"], id: \.self) { Text($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .listRowSeparator(.hidden)
                ForEach(episodes) { e in
                    EpisodeRow(episode: e)
                }
            }
            .listStyle(.plain)
            .navigationTitle(show.title)
            .inlineTitle()
            .modifier(RemoveDownloadDialog())
        }
    }

    private func showText(_ show: Show, all: [Episode]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(show.kind.rawValue.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            Text(show.title).font(.title.bold())
            Text(show.author).font(.title3).foregroundStyle(.tint)
            Text(show.summary).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 12) {
                if let latest = all.first(where: { $0.progress < 1 }) {
                    Button { player.play(.episode(latest.id)) } label: {
                        Label(latest.isInProgress ? "Resume" : "Latest Episode", systemImage: "play.fill")
                    }
                    .buttonStyle(.borderedProminent)
                }
                Button {} label: { Label("Following", systemImage: "checkmark") }
                    .buttonStyle(.bordered)
            }
            .padding(.top, 4)
            Label(show.feed, systemImage: "dot.radiowaves.up.forward")
                .font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// An episode: details, and its transcript to read along or jump into.
struct EpisodeView: View {
    let id: String
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player

    var body: some View {
        if let e = library.episode(id), let show = library.show(e.showID) {
            let current = player.currentEpisodeID == e.id
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    HStack(alignment: .top, spacing: 16) {
                        Artwork(art: show.art).frame(width: 96, height: 96)
                        VStack(alignment: .leading, spacing: 4) {
                            NavigationLink(value: Route.show(show.id)) {
                                Text(show.title).font(.subheadline.weight(.semibold)).foregroundStyle(.tint)
                            }
                            .buttonStyle(.plain)
                            Text(e.title).font(.title2.bold())
                            Text("\(Format.day(e.date)) · \(Format.minutes(e.duration))\(e.scripture.map { " · \($0)" } ?? "")")
                                .font(.subheadline).foregroundStyle(.secondary)
                        }
                    }
                    HStack(spacing: 12) {
                        Button {
                            if current { player.togglePlay() } else { player.play(.episode(e.id)) }
                        } label: {
                            Label(current && player.isPlaying ? "Pause" : (e.isInProgress ? "Resume · \(Format.remaining(e))" : "Play · \(Format.minutes(e.duration))"),
                                  systemImage: current && player.isPlaying ? "pause.fill" : "play.fill")
                                .frame(minWidth: 160)
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                        Menu {
                            EpisodeMenuItems(episode: e)
                        } label: {
                            Label("More", systemImage: "ellipsis")
                        }
                        .labelStyle(.iconOnly)
                        .menuIndicator(.hidden)
                        .buttonStyle(.bordered)
                        .buttonBorderShape(.circle)
                        .controlSize(.large)
                        DownloadBadge(state: e.download).font(.title3)
                    }
                    Text(e.summary).font(.body)
                    Divider()
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Transcript").font(.title3.bold())
                        Text("Tap a line to listen from there.").font(.subheadline).foregroundStyle(.secondary)
                    }
                    VStack(alignment: .leading, spacing: 14) {
                        ForEach(e.transcript) { line in
                            Button {
                                if !current { player.play(.episode(e.id)) }
                                player.seek(line.time)
                            } label: {
                                HStack(alignment: .firstTextBaseline, spacing: 12) {
                                    Text(Format.clock(line.time)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                                        .frame(width: 44, alignment: .leading)
                                    VStack(alignment: .leading, spacing: 2) {
                                        if let s = line.speaker { Text(s).font(.caption.weight(.semibold)).foregroundStyle(.secondary) }
                                        Text(line.text)
                                            .foregroundStyle(current && player.currentLineID == line.id ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
                .padding(20)
                .frame(maxWidth: 760, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .navigationTitle(e.title)
            .inlineTitle()
            .modifier(RemoveDownloadDialog())
        }
    }
}

struct DownloadsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        List {
            ForEach(library.shows) { show in
                let episodes = library.downloadedEpisodes.filter { $0.showID == show.id }
                if !episodes.isEmpty {
                    Section(show.title) {
                        ForEach(episodes) { EpisodeRow(episode: $0) }
                    }
                }
            }
            Section {
                LabeledContent("On This Device", value: "2.4 GB of episodes")
                LabeledContent("Keep", value: "Latest 5 per show")
            }
        }
        .listStyle(.plain)
        .navigationTitle("Downloaded")
        .modifier(RemoveDownloadDialog())
    }
}
