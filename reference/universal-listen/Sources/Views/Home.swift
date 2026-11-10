import SwiftUI

/// Home: continue listening, who's listening at home, recent albums, new episodes, playlists.
struct HomeView: View {
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 32) {
                Shelf(title: "Continue Listening") {
                    if let info = player.info, let item = player.item {
                        ContinueCard(info: info, progress: player.position / player.duration, caption: "Now playing") {
                            nav.playerExpanded = true
                        }
                        .accessibilityIdentifier("continue.\(item)")
                    }
                    ForEach(library.inProgressEpisodes.filter { $0.id != player.currentEpisodeID }) { e in
                        if let info = library.info(.episode(e.id)) {
                            ContinueCard(info: info, progress: e.progress, caption: Format.remaining(e)) {
                                player.play(.episode(e.id))
                            }
                            .contextMenu { EpisodeMenuItems(episode: e) }
                        }
                    }
                }

                VStack(alignment: .leading, spacing: 12) {
                    ShelfHeader(title: "At Home").padding(.horizontal, 20)
                    ScrollView(.horizontal) {
                        HStack(spacing: 12) {
                            ForEach(library.members.filter { $0.activity != nil }) { m in
                                Button { nav.tab = .household } label: { MemberChip(member: m) }
                                    .buttonStyle(PressableStyle(scale: 0.96))
                            }
                        }
                        .padding(.horizontal, 20)
                    }
                    .scrollIndicators(.hidden)
                }

                Shelf(title: "Recently Added", route: .albums) {
                    ForEach(library.recentlyAddedAlbums.prefix(10)) { album in
                        NavigationLink(value: Route.album(album.id)) { AlbumTile(album: album, width: 160) }
                            .buttonStyle(.plain)
                            .contextMenu { AlbumMenuItems(album: album) }
                    }
                }

                VStack(alignment: .leading, spacing: 0) {
                    ShelfHeader(title: "New Episodes", route: .downloads).padding(.bottom, 4)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 24, alignment: .top)], spacing: 0) {
                        ForEach(library.latestEpisodes.filter(\.isUnplayed).prefix(4)) { e in
                            VStack(spacing: 0) {
                                EpisodeRow(episode: e, showsArtwork: true)
                                Divider()
                            }
                        }
                    }
                }
                .padding(.horizontal, 20)

                Shelf(title: "Playlists") {
                    ForEach(library.playlists) { p in
                        Button {
                            nav.go(.playlist(p.id))
                        } label: {
                            VStack(alignment: .leading, spacing: 6) {
                                Artwork(art: p.art).shadow(color: .black.opacity(0.12), radius: 6, y: 3)
                                Text(p.name).font(.subheadline.weight(.medium)).lineLimit(1)
                                Text(p.isSmart ? "Smart Playlist" : p.summary).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                            }
                            .frame(width: 160)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .padding(.vertical, 16)
        }
        .navigationTitle("Home")
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                SettingsButton()
            }
        }
        #endif
    }
}

/// iOS: settings is a sheet off Home. (The Mac has its Settings window.)
struct SettingsButton: View {
    @Environment(Navigator.self) private var nav
    @Environment(Library.self) private var library

    var body: some View {
        #if os(iOS)
        Button {
            nav.sheet = .settings
        } label: {
            if let me = library.member(library.meID) {
                Avatar(member: me, size: 30)
            }
        }
        .accessibilityLabel("Settings")
        #else
        EmptyView()
        #endif
    }
}

/// A titled horizontal shelf.
struct Shelf<Content: View>: View {
    let title: String
    var route: Route? = nil
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ShelfHeader(title: title, route: route).padding(.horizontal, 20)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 16) { content }
                    .padding(.horizontal, 20)
                    .scrollTargetLayout()
            }
            .scrollTargetBehavior(.viewAligned)
            .scrollIndicators(.hidden)
        }
    }
}

private struct ContinueCard: View {
    let info: ItemInfo
    let progress: Double
    let caption: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Artwork(art: info.art).frame(width: 72, height: 72)
                VStack(alignment: .leading, spacing: 4) {
                    Text(caption.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
                    Text(info.title).font(.headline).lineLimit(2)
                    Text(info.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    ProgressView(value: min(1, max(0, progress))).tint(.accentColor)
                }
                Spacer(minLength: 0)
            }
            .padding(12)
            .frame(width: 320, height: 104)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        }
        .buttonStyle(PressableStyle(scale: 0.97))
    }
}

/// Who's listening where, small.
struct MemberChip: View {
    let member: Member
    @Environment(Library.self) private var library

    var body: some View {
        HStack(spacing: 10) {
            Avatar(member: member, size: 36)
            VStack(alignment: .leading, spacing: 1) {
                if let a = member.activity, let info = library.info(a.item), let device = library.device(a.deviceID) {
                    Text("\(member.name) · \(device.name)").font(.subheadline.weight(.semibold))
                    Text(a.isPlaying ? info.title : "Paused — \(info.title)").font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .frame(maxWidth: 200, alignment: .leading)
        }
        .padding(.vertical, 8)
        .padding(.leading, 8)
        .padding(.trailing, 14)
        .background(.background.secondary, in: Capsule())
        .accessibilityElement(children: .combine)
    }
}
