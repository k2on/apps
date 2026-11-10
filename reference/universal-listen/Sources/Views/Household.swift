import SwiftUI

/// Who is listening where, and moving playback between devices.
struct HouseholdView: View {
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                VStack(alignment: .leading, spacing: 12) {
                    ShelfHeader(title: "Listening Now")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 300), spacing: 16, alignment: .top)], spacing: 16) {
                        if let me = library.member(library.meID), let info = player.info {
                            ListeningCard(member: me, info: info, device: player.output, progress: player.position / player.duration,
                                          isPlaying: player.isPlaying, isMe: true)
                        }
                        ForEach(library.members.filter { $0.id != library.meID }) { m in
                            if let a = m.activity, let info = library.info(a.item) {
                                ListeningCard(member: m, info: info, device: library.device(a.deviceID),
                                              progress: a.position / info.duration, isPlaying: a.isPlaying, isMe: false)
                            }
                        }
                    }
                }

                VStack(alignment: .leading, spacing: 12) {
                    ShelfHeader(title: "Hand Off")
                    Text("Move what you're playing to another speaker or device. It picks up where you are.")
                        .font(.subheadline).foregroundStyle(.secondary)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 12)], spacing: 12) {
                        ForEach(library.devices) { device in
                            DeviceTile(device: device)
                        }
                    }
                }
            }
            .padding(20)
        }
        .navigationTitle("Household")
        .modifier(HandOffDialog())
    }
}

private struct ListeningCard: View {
    let member: Member
    let info: ItemInfo
    let device: Device?
    let progress: Double
    let isPlaying: Bool
    let isMe: Bool
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Avatar(member: member, size: 34)
                VStack(alignment: .leading, spacing: 0) {
                    Text(isMe ? "\(member.name) (You)" : member.name).font(.headline)
                    Label(device?.name ?? "—", systemImage: device?.symbol ?? "speaker")
                        .font(.subheadline).foregroundStyle(.secondary)
                }
                Spacer()
                Text(isPlaying ? "Playing" : "Paused")
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(isPlaying ? AnyShapeStyle(.tint.opacity(0.15)) : AnyShapeStyle(.quaternary), in: Capsule())
                    .foregroundStyle(isPlaying ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
            }
            HStack(spacing: 12) {
                Artwork(art: info.art).frame(width: 56, height: 56)
                VStack(alignment: .leading, spacing: 2) {
                    Text(info.title).font(.subheadline.weight(.semibold)).lineLimit(1)
                    Text(info.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    ProgressView(value: min(1, max(0, progress))).tint(.secondary).padding(.top, 4)
                }
            }
            if !isMe, let device {
                HStack {
                    Button("Listen Here Too") { player.handOff(to: device.id) }
                    Button("Take Over \(device.name)") { nav.dialog = .handOff(device.id) }
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        .padding(16)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .accessibilityElement(children: .contain)
    }
}

private struct DeviceTile: View {
    let device: Device
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        let selected = player.outputID == device.id
        let user = library.members.first { $0.activity?.deviceID == device.id && $0.activity?.isPlaying == true }
        Button {
            if user != nil { nav.dialog = .handOff(device.id) } else { player.handOff(to: device.id) }
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                Image(systemName: device.symbol).font(.title2)
                    .foregroundStyle(selected ? AnyShapeStyle(.white) : AnyShapeStyle(.tint))
                Spacer(minLength: 0)
                Text(device.name).font(.subheadline.weight(.semibold)).lineLimit(1)
                Text(selected ? "Playing here" : (user.map { "\($0.name) is listening" } ?? device.room))
                    .font(.caption).lineLimit(1)
                    .foregroundStyle(selected ? AnyShapeStyle(.white.opacity(0.85)) : AnyShapeStyle(.secondary))
            }
            .foregroundStyle(selected ? AnyShapeStyle(.white) : AnyShapeStyle(.primary))
            .padding(14)
            .frame(maxWidth: .infinity, minHeight: 108, alignment: .leading)
            .background(selected ? AnyShapeStyle(.tint) : AnyShapeStyle(.background.secondary),
                        in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        }
        .buttonStyle(PressableStyle(scale: 0.96))
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("device.\(device.id)")
    }
}
