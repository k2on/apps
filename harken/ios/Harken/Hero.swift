// The pieces a catalogue's pages are made of, as the desktop's are
// (`rust/arkui/src/hero.rs`, and `view_cover`, `view_featured`,
// `work_heading` and `view_foot` in `harken/iced/src/view.rs`): a person's
// banner, run up under the navigation bar so its back button is Liquid
// Glass over the picture; a record's header, its cover beside its title in
// the serif; the one thing a section sets apart, with a Play; a section's
// serif heading; an entry over its hairline, and the "See All" under a
// list of them; a work's heading over its movements; and a record's small
// print. The pages (Pages.swift) put them together.
import SwiftUI
import UIKit

/// A person's banner: their wide picture across the page, cropped to fill,
/// `kind` in small capitals and their name in the serif on its lower edge,
/// over a shade that keeps both legible on a white collar or a white wig.
/// Until the picture arrives the ground is drawn, so the name is already
/// where it will be. Somebody with no wide picture has no banner.
struct Banner: View {
    let url: String
    let kind: String
    let name: String
    /// Its height on a phone, the bars it runs under included: a banner's
    /// own shape (2400×933) across a phone would be a strip.
    static let height: CGFloat = 380

    @State private var image: UIImage? = nil

    var body: some View {
        Color(uiColor: .secondarySystemBackground)
            .frame(maxWidth: .infinity)
            .frame(height: Banner.height)
            .overlay {
                if let i = image { Image(uiImage: i).resizable().scaledToFill().accessibilityHidden(true) }
            }
            .overlay {
                LinearGradient(stops: [.init(color: Color.clear, location: 0.45), .init(color: Color.black.opacity(0.6), location: 1)],
                               startPoint: .top, endPoint: .bottom)
            }
            .overlay(alignment: .bottomLeading) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(kind.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(Color.white.opacity(0.85))
                    Text(name).font(.serif(38, relativeTo: .largeTitle)).foregroundStyle(Color.white)
                        .lineLimit(3).minimumScaleFactor(0.6)
                }
                .padding(.horizontal, Space.page + 4)
                .padding(.bottom, Space.xl)
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
            }
            .clipped()
            .task(id: url) {
                image = url.isEmpty ? nil : Covers.shared.cached(url, side: Covers.bound)
                if url.isEmpty || image != nil { return }
                image = await Covers.shared.load(url, side: Covers.bound)
            }
    }
}

/// A line that opens somebody's page when it names somebody who has one.
struct Linked: View {
    let words: String
    let font: Font
    let color: Color
    var action: (() -> Void)? = nil

    var body: some View {
        if let a = action {
            Button(action: a) { Text(words).multilineTextAlignment(.leading) }
                .buttonStyle(.plain).font(font).foregroundStyle(color)
        } else {
            Text(words).font(font).foregroundStyle(color)
        }
    }
}

/// Play and, where a list has an order worth breaking, Shuffle: Liquid Glass, in one container.
struct PlayShuffle: View {
    let canPlay: Bool
    let onPlay: () -> Void
    var onShuffle: (() -> Void)? = nil

    var body: some View {
        GlassEffectContainer(spacing: Space.m) {
            HStack(spacing: Space.m) {
                Button(action: onPlay) { Label("Play", systemImage: "play.fill").frame(maxWidth: .infinity) }
                    .buttonStyle(.glassProminent)
                if let shuffle = onShuffle {
                    Button(action: shuffle) { Label("Shuffle", systemImage: "shuffle").frame(maxWidth: .infinity) }
                        .buttonStyle(.glass)
                }
            }
            .controlSize(.large)
        }
        .disabled(!canPlay)
    }
}

/// The header a record's page opens with, as a catalogue sets one: the
/// cover beside a line of small capitals (`over`, whose it is), the title in
/// the serif, who played it in the gold, a small line under (`small`) —
/// each line that names somebody opening their page — and Play and Shuffle
/// under it all.
struct RecordHeader<Cover: View>: View {
    var over: String = ""
    var onOver: (() -> Void)? = nil
    let title: String
    var by: String = ""
    var onBy: (() -> Void)? = nil
    var small: String = ""
    var onSmall: (() -> Void)? = nil
    var canPlay = true
    var onPlay: (() -> Void)? = nil
    var onShuffle: (() -> Void)? = nil
    @ViewBuilder let cover: () -> Cover

    var body: some View {
        VStack(alignment: .leading, spacing: Space.l + 2) {
            HStack(alignment: .center, spacing: Space.l + 4) {
                cover().frame(width: Space.record, height: Space.record)
                VStack(alignment: .leading, spacing: Space.xs) {
                    if !over.isEmpty { Linked(words: over.uppercased(), font: .caption.weight(.semibold), color: .secondary, action: onOver) }
                    Text(title).font(.serif(24, relativeTo: .title2)).fixedSize(horizontal: false, vertical: true)
                    if !by.isEmpty { Linked(words: by, font: .headline, color: .gold, action: onBy) }
                    if !small.isEmpty { Linked(words: small, font: .caption, color: .secondary, action: onSmall) }
                }
                Spacer(minLength: 0)
            }
            if let play = onPlay { PlayShuffle(canPlay: canPlay, onPlay: play, onShuffle: onShuffle) }
        }
        .padding(.horizontal, Space.page)
        .padding(.vertical, Space.l)
    }
}

/// One thing set apart at the head of a section, the one to start with: its
/// cover beside `label` in small capitals, its title, its facts, and a Play
/// on glass. The cover and the title open it, the cover zooming into its page.
struct Featured: View {
    let label: String
    let seed: String
    let url: String
    let title: String
    let facts: String
    let opens: Route
    let onPlay: () -> Void
    @Environment(\.open) private var open
    @Environment(\.zoom) private var zoom

    var body: some View {
        HStack(alignment: .center, spacing: Space.l + 4) {
            Button { open(opens) } label: {
                CoverView(seed: seed, url: url, side: 120, corner: 6).frame(width: 120, height: 120).zoomSource(opens, zoom)
            }
            .buttonStyle(.plain)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text(label.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
                Button { open(opens) } label: {
                    Text(title).font(.headline).multilineTextAlignment(.leading).fixedSize(horizontal: false, vertical: true)
                }
                .buttonStyle(.plain)
                if !facts.isEmpty { Text(facts).font(.footnote).foregroundStyle(.secondary) }
                Button(action: onPlay) {
                    Image(systemName: "play.fill").font(.body.weight(.semibold)).frame(width: 36, height: 36)
                }
                .buttonStyle(.glassProminent)
                .buttonBorderShape(.circle)
                .tint(.gold)
                .padding(.top, Space.s)
                .accessibilityLabel("Play \(title)")
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Space.page)
        .padding(.top, Space.xl)
        .padding(.bottom, Space.xs)
    }
}

/// A section's heading, in the serif.
struct SerifHeading: View {
    let title: String

    var body: some View {
        Text(title).font(.serif(22, relativeTo: .title2))
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Space.page)
            .padding(.top, Space.xl + 4)
            .padding(.bottom, Space.xs)
            .accessibilityAddTraits(.isHeader)
    }
}

/// One entry of a catalogue's list, over its hairline: `picture` (a cover)
/// at its left when it has one, a line of small capitals over its title
/// (who it is by), a line under, a count at its right, and a chevron.
struct EntryRow<Picture: View>: View {
    var over: String = ""
    let title: String
    var under: String = ""
    var count: String = ""
    let onOpen: () -> Void
    @ViewBuilder let picture: () -> Picture

    var body: some View {
        Button(action: onOpen) {
            VStack(spacing: 0) {
                HStack(spacing: Space.m) {
                    picture()
                    VStack(alignment: .leading, spacing: 2) {
                        if !over.isEmpty { Text(over.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.secondary) }
                        Text(title).font(.body).foregroundStyle(.primary).multilineTextAlignment(.leading)
                        if !under.isEmpty { Text(under).font(.footnote).foregroundStyle(.secondary).lineLimit(2) }
                    }
                    Spacer(minLength: Space.s)
                    if !count.isEmpty { Text(count).font(.subheadline).foregroundStyle(.secondary).monospacedDigit() }
                    Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
                }
                .padding(.vertical, Space.m - 2)
                Divider()
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, Space.page)
    }
}

/// The line under a list that opens the rest of it — "See All 40 Works" —
/// in the gold, its chevron where an entry's is.
struct MoreRow: View {
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack {
                Text(title).foregroundStyle(Color.gold)
                Spacer()
                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
            }
            .padding(.vertical, Space.m)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, Space.page)
    }
}

/// A work's heading over its movements: its composer in small capitals over
/// its title in the serif, and how long the run of it lasts, in the column
/// the tracks' lengths are in.
struct WorkHeading: View {
    let composer: String
    let work: String
    let ms: Int64
    let first: Bool

    var body: some View {
        HStack(alignment: .lastTextBaseline, spacing: Space.m) {
            VStack(alignment: .leading, spacing: 2) {
                if !composer.isEmpty { Text(composer.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.secondary) }
                Text(work).font(.serif(17, relativeTo: .headline)).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            Text(Spell.clock(ms)).font(.caption).foregroundStyle(.secondary).monospacedDigit()
        }
        // In from where a track's title starts; and short of where its menu
        // is, so the lengths line up.
        .padding(.leading, Space.page + Space.thumb + Space.m)
        .padding(.trailing, Space.page + 32 + Space.m)
        .padding(.top, first ? Space.s : Space.xl + 2)
        .padding(.bottom, Space.xs)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// A page's tracks as a catalogue lists a record's: with `headings`, in runs
/// of one work, each under its heading and its movements unnumbered; each
/// over who played it, or — on a page that is somebody's (`creator`) — whose
/// it is when it is somebody else's.
struct CatalogueTracks: View {
    let tracks: [LibraryEntry]
    let page: Page
    let creator: String
    var headings = true
    @EnvironmentObject private var model: Model

    var body: some View {
        let details = tracks.map { model.detail($0) }
        let runs = Catalogue.runs(tracks, details: details, headings: headings)
        ForEach(Array(runs.enumerated()), id: \.offset) { n, run in
            if !run.work.isEmpty { WorkHeading(composer: run.composer, work: run.work, ms: run.ms, first: n == 0) }
            ForEach(run.range, id: \.self) { i in
                track(i, details: details, numbered: run.work.isEmpty)
                if i + 1 < run.range.upperBound { Divider().padding(.leading, Space.page + Space.thumb + Space.m) }
            }
        }
    }

    private func track(_ i: Int, details: [TrackDetailsEntry?], numbered: Bool) -> some View {
        let t = tracks[i]
        let d = details[i]
        return TrackRow(track: t, under: Catalogue.under(t, d, creator: creator), from: page, onPlay: { model.play(tracks, at: i, from: page) }) {
            TrackNumber(n: numbered ? (d?.track ?? 0) : 0, playing: model.playingId == t.id.hex)
        }
        .padding(.horizontal, Space.page)
    }
}

/// A record's small print: when, how much, and whose label.
struct RecordFoot: View {
    let year: Int64
    let tracks: [LibraryEntry]
    let label: String

    var body: some View {
        let length = [Spell.count(tracks.count, "track"), Spell.total(tracks)].compactMap { $0 }.joined(separator: ", ")
        VStack(alignment: .leading, spacing: 3) {
            if year > 0 { Text(String(year)) }
            Text(length)
            if !label.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Text("RECORD LABEL").font(.caption2.weight(.semibold))
                    Text(label).font(.subheadline).foregroundStyle(Color.gold)
                }
                .padding(.top, Space.m)
            }
        }
        .font(.footnote)
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, Space.page)
        .padding(.top, Space.xl)
    }
}
