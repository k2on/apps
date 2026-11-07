// The pieces a catalogue's pages are made of that are harken's, as the
// desktop's are (`view_cover`, `view_featured`, `work_heading` and
// `view_foot` in `harken/iced/src/view.rs`): a person's banner's picture,
// fetched; a record's header, its cover beside its title; the one thing a
// section sets apart, with a Play; a work's heading over its movements;
// and a record's small print. The banner itself, a section's heading, an
// entry over its hairline and the "See All" under a list of them are
// ArkUI's (`HeroBanner`, `HeroHeading`, `HeroEntry`, `HeroMore`, the twins
// of `rust/arkui/src/hero.rs`). The pages (Pages.swift) put them together.
import ArkUI
import SwiftUI
import UIKit

/// A person's banner: ArkUI's `HeroBanner`, with their wide picture
/// fetched and kept as a cover is (`Covers`). Somebody with no wide picture
/// has no banner.
struct PersonBanner: View {
    let url: String
    let kind: String
    let name: String
    @State private var image: UIImage? = nil

    var body: some View {
        HeroBanner(image: image, kind: kind, name: name)
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
/// cover beside a line of small type (`over`, whose it is), the title in
/// bold, who played it in the gold, a small line under (`small`) —
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
                    if !over.isEmpty { Linked(words: over, font: .caption.weight(.semibold), color: .quiet, action: onOver) }
                    Text(title).font(.title2.bold()).fixedSize(horizontal: false, vertical: true)
                    if !by.isEmpty { Linked(words: by, font: .headline, color: .accentText, action: onBy) }
                    if !small.isEmpty { Linked(words: small, font: .caption, color: .quiet, action: onSmall) }
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
/// cover beside `label` in small type, its title, its facts, and a Play
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
            .buttonStyle(.press)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text(label).font(.caption2.weight(.semibold)).foregroundStyle(.quiet)
                Button { open(opens) } label: {
                    Text(title).font(.headline).multilineTextAlignment(.leading).fixedSize(horizontal: false, vertical: true)
                }
                .buttonStyle(.plain)
                if !facts.isEmpty { Text(facts).font(.footnote).foregroundStyle(.quiet) }
                Button(action: onPlay) {
                    Image(systemName: "play.fill").font(.body.weight(.semibold)).frame(width: 36, height: 36)
                }
                .buttonStyle(.glassProminent)
                .buttonBorderShape(.circle)
                .tint(Color.accent)
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

/// A work's heading over its movements: its composer in small type over
/// its title, and how long the run of it lasts, in the column
/// the tracks' lengths are in.
struct WorkHeading: View {
    let composer: String
    let work: String
    let ms: Int64
    let first: Bool

    var body: some View {
        HStack(alignment: .lastTextBaseline, spacing: Space.m) {
            VStack(alignment: .leading, spacing: 2) {
                if !composer.isEmpty { Text(composer).font(.caption2.weight(.semibold)).foregroundStyle(.quiet) }
                Text(work).font(.headline).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            Text(formatClock(ms: ms)).font(.caption).foregroundStyle(.quiet).monospacedDigit()
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

/// A record's small print: when, how much, on what terms, and whose label.
/// The terms are the record's, said once here rather than on every track.
struct RecordFoot: View {
    let year: Int64
    let tracks: [LibraryEntry]
    let label: String
    @EnvironmentObject private var model: Model

    var body: some View {
        let length = [formatCount(n: Int64(tracks.count), word: "track"), Spell.total(tracks)].compactMap { $0 }.joined(separator: ", ")
        VStack(alignment: .leading, spacing: 3) {
            if year > 0 { Text(String(year)) }
            Text(length)
            ForEach(Catalogue.licences(tracks.map { model.detail($0) }), id: \.self) { Text($0) }
            if !label.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Record Label").font(.caption2.weight(.semibold))
                    Text(label).font(.subheadline).foregroundStyle(Color.accentText)
                }
                .padding(.top, Space.m)
            }
        }
        .font(.footnote)
        .foregroundStyle(.quiet)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, Space.page)
        .padding(.top, Space.xl)
    }
}
