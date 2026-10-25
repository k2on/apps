// What of a catalogue's pages is a rule rather than a drawing, and nothing
// of Apple's but Foundation: which of a composer's works are the best
// known, and which one is set apart; how a work's composer is written over
// its title; what a performance's line says; an album's tracks in runs of
// one work; and who is on a performance. The desktop's (`harken/iced`'s
// `known`, `listed_works`, `view_composer`, `view_work`, `view_tracks`,
// `view_recording`), which the pages (Pages.swift) draw and `CatalogueTests`
// hold to it. Only sorting and filtering of what the domain's views say.
import Foundation

enum Catalogue {
    /// How many works a composer's page lists before its "See All": the desktop's `KNOWN_WORKS`.
    static let known = 6

    /// Works best known first: the most recorded, then the most tracks, the
    /// catalogue's order (the `works` view's) between equals.
    static func ranked(_ works: [WorkSummary]) -> [WorkSummary] {
        let order = works.enumerated().sorted { a, b in
            if a.element.recordings != b.element.recordings { return a.element.recordings > b.element.recordings }
            if a.element.tracks != b.element.tracks { return a.element.tracks > b.element.tracks }
            return a.offset < b.offset
        }
        return order.map { $0.element }
    }

    /// The works a composer's page lists, in the order it lists them: every
    /// one by catalogue when there are few or every one was asked for, and
    /// otherwise the best known, up to [known].
    static func listed(_ works: [WorkSummary], all: Bool) -> [WorkSummary] {
        if all || works.count <= known { return works }
        return Array(ranked(works).prefix(known))
    }

    /// The work a composer's page sets apart with a Play: the most recorded,
    /// when it is recorded more often than any other — a tie is no answer to
    /// "which is best known", and one work is no answer either.
    static func featured(_ works: [WorkSummary]) -> WorkSummary? {
        let r = ranked(works)
        guard r.count > 1, r[0].recordings > r[1].recordings else { return nil }
        return r[0]
    }

    /// The featured work's line: its catalogue number and how many performances of it there are.
    static func featuredFacts(_ w: WorkSummary) -> String {
        Spell.facts([w.catalogue, Spell.count(Int(w.recordings), "recording")])
    }

    /// What a work is, under its title: its catalogue number, its form, its period.
    static func facts(_ w: WorkSummary) -> String {
        Spell.facts([w.catalogue, w.form, w.period])
    }

    /// How a catalogue writes who a work is by over its title: every name
    /// but the last as an initial, a particle kept — `A. Vivaldi`,
    /// `J. S. Bach`, `L. van Beethoven`.
    static func initials(_ name: String) -> String {
        let words = name.split(whereSeparator: { $0.isWhitespace }).map(String.init)
        guard let last = words.last else { return "" }
        var out = [String]()
        for w in words.dropLast() {
            if let c = w.first, c.isUppercase { out.append("\(c).") } else { out.append(w) }
        }
        out.append(last)
        return out.joined(separator: " ")
    }

    /// When somebody lived, as a catalogue heads a person: `1685–1750`,
    /// `1950–` for the living, nothing when neither year is known.
    static func life(born: Int64, died: Int64) -> String {
        if born == 0 && died == 0 { return "" }
        return (born == 0 ? "" : "\(born)") + "\u{2013}" + (died == 0 ? "" : "\(died)")
    }

    /// Who played a performance — or, when nobody is credited on it, that
    /// nobody is, rather than a guess.
    static func who(_ r: RecordingsEntry) -> String {
        r.performers.isEmpty ? "Performer not named" : r.performers
    }

    /// A performance's line: when, how many tracks, how long, on what terms —
    /// "1992 · 3 tracks · 40 min".
    static func facts(_ r: RecordingsEntry) -> String {
        Spell.facts([r.recorded > 0 ? "\(r.recorded)" : nil, Spell.count(Int(r.tracks), "track"),
                     r.durationMs > 0 ? Spell.length(r.durationMs) : nil, r.licence])
    }

    /// Who played a track and on what terms, as one line under its title
    /// (the desktop's `credit`): just the names, or the names and the licence.
    static func credit(_ performer: String, _ licence: String) -> String {
        if licence.isEmpty { return performer }
        return performer.isEmpty ? "(\(licence))" : "\(performer) (\(licence))"
    }

    /// The line under a track on a page whose it is (`creator`): who played
    /// it — or, on a record of several people's songs, whose this one is.
    static func under(_ t: LibraryEntry, _ d: TrackDetailsEntry?, creator: String) -> String {
        let performer = d?.performer ?? ""
        if !performer.isEmpty { return credit(performer, d?.licence ?? "") }
        return t.creator == creator ? "" : t.creator
    }

    /// A run of a page's tracks that are movements of one work, under its
    /// heading: its composer, its title, and how long the run lasts. A run of
    /// no work has no heading, and its tracks their numbers.
    struct Run: Equatable {
        let composer: String
        let work: String
        let range: Range<Int>
        let ms: Int64
    }

    /// `tracks` in runs of one work by their details (`details[i]` is
    /// `tracks[i]`'s); with no `headings`, the whole of them one run of none.
    static func runs(_ tracks: [LibraryEntry], details: [TrackDetailsEntry?], headings: Bool = true) -> [Run] {
        func ms(_ r: Range<Int>) -> Int64 { tracks[r].reduce(Int64(0)) { $0 + $1.durationMs } }
        if tracks.isEmpty { return [] }
        if !headings { return [Run(composer: "", work: "", range: 0..<tracks.count, ms: ms(0..<tracks.count))] }
        func of(_ i: Int) -> (String, String) {
            let d = i < details.count ? details[i] : nil
            return (d?.composer ?? "", d?.work ?? "")
        }
        var out = [Run]()
        var at = 0
        while at < tracks.count {
            let run = of(at)
            var end = at + 1
            while end < tracks.count && of(end) == run { end += 1 }
            out.append(Run(composer: run.0, work: run.1, range: at..<end, ms: ms(at..<end)))
            at = end
        }
        return out
    }

    /// Somebody on a performance, and what they did on it.
    struct Billed: Equatable, Identifiable {
        let name: String
        var role: String
        var id: String { name }
    }

    /// Everybody on a performance: the composer, then its credits in billing
    /// order, each once, their roles and instruments joined.
    static func people(composer: String, credits: [CreditsEntry]) -> [Billed] {
        var out = [Billed]()
        if !composer.isEmpty { out.append(Billed(name: composer, role: "Composer")) }
        for c in credits {
            var role = c.role
            if !c.instrument.isEmpty && c.instrument != c.role { role = role.isEmpty ? c.instrument : role + ", " + c.instrument }
            if let i = out.firstIndex(where: { $0.name == c.name }) {
                if !role.isEmpty { out[i].role = out[i].role.isEmpty ? role : out[i].role + ", " + role }
            } else {
                out.append(Billed(name: c.name, role: role))
            }
        }
        return out
    }

    /// The key of the work a performance is of: everything before the `@` of its own.
    static func workKey(_ recording: String) -> String? {
        guard let at = recording.firstIndex(of: "@") else { return nil }
        return String(recording[..<at])
    }

    /// A performance's title on its page: the work's, with its catalogue number.
    static func title(_ w: WorkSummary?) -> String {
        guard let w = w else { return "" }
        return w.catalogue.isEmpty ? w.title : w.title + ", " + w.catalogue
    }
}
