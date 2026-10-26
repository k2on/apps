// A catalogue's rules, held to the desktop's (`harken/iced`'s `known`,
// `listed_works` and `view_composer`; its tests/demo.rs): which works a
// composer's page lists and sets apart, how a work's composer and a
// performance are written, an album's tracks in runs of one work, and who
// is on a performance.
@testable import Harken
import Foundation
import XCTest

private func work(_ title: String, _ recordings: Int64, _ tracks: Int64, catalogue: String = "") -> WorkSummary {
    WorkSummary(art: "", catalogue: catalogue, composer: "Johann Sebastian Bach", description: "", form: "", id: "bach/" + title, period: "",
                recordings: recordings, title: title, tracks: tracks)
}

private func take(_ recorded: Int64, tracks: Int64, ms: Int64, performers: String = "Kimiko Ishizaka", licence: String = "") -> RecordingsEntry {
    RecordingsEntry(art: "", durationMs: ms, id: "bach/bwv-988@k", label: "", licence: licence, performers: performers, recorded: recorded, tracks: tracks)
}

private func detail(_ n: Int, work: String, composer: String = "Handel", track: Int64 = 0) -> TrackDetailsEntry {
    TrackDetailsEntry(album: "Water Music", bpm: 0, catalogue: "", composer: work.isEmpty ? "" : composer, licence: "", mediaId: Fx.id(n),
                      part: "", performer: "", track: track, work: work, workId: "")
}

final class CatalogueTests: XCTestCase {
    /// The desktop's own case: falsified by ranking on tracks alone, which
    /// puts the work of two short performances after the one long one.
    func testTheBestKnownWorksAreTheMostRecorded() {
        let works = [work("Air", 1, 9), work("Bourrée", 2, 2), work("Chaconne", 1, 9), work("Gigue", 2, 4)]
        XCTAssertEqual(Catalogue.ranked(works).map { $0.title }, ["Gigue", "Bourrée", "Air", "Chaconne"])
    }

    /// Equals keep the catalogue's order, however many there are.
    func testEqualsKeepTheCataloguesOrder() {
        let works = (0..<40).map { work("W\($0)", 1, 1) }
        XCTAssertEqual(Catalogue.ranked(works).map { $0.title }, works.map { $0.title })
    }

    /// Six best known, and every one by catalogue once "See All" is pressed —
    /// or when there are no more than six.
    func testAComposersPageListsSixBestKnownThenEveryOne() {
        let works = (0..<9).map { work("W\($0)", Int64($0 % 4), 1) }
        let listed = Catalogue.listed(works, all: false)
        XCTAssertEqual(listed.count, Catalogue.known)
        XCTAssertEqual(listed.first?.title, "W3")
        XCTAssertEqual(Catalogue.listed(works, all: true).map { $0.title }, works.map { $0.title })
        let few = Array(works.prefix(4))
        XCTAssertEqual(Catalogue.listed(few, all: false).map { $0.title }, few.map { $0.title }, "few are listed by catalogue")
        XCTAssertEqual(Catalogue.heading(works, all: false), "Well-Known Works")
        XCTAssertEqual(Catalogue.heading(works, all: true), "Works")
        XCTAssertEqual(Catalogue.heading(Array(works.prefix(6)), all: false), "Works", "six are every one")
    }

    /// The most recorded is set apart only when it is recorded more than
    /// the next: a tie, or a single work, sets nothing apart.
    func testTheMostRecordedIsSetApartOnlyWhenItLeads() {
        XCTAssertEqual(Catalogue.featured([work("Air", 1, 9), work("Gigue", 3, 4), work("Bourrée", 2, 2)])?.title, "Gigue")
        XCTAssertNil(Catalogue.featured([work("Air", 2, 9), work("Gigue", 2, 4)]))
        XCTAssertNil(Catalogue.featured([work("Air", 5, 9)]))
        XCTAssertNil(Catalogue.featured([]))
        XCTAssertEqual(Catalogue.featuredFacts(work("Goldberg Variations", 3, 32, catalogue: "BWV 988")), "BWV 988 · 3 recordings")
        XCTAssertEqual(Catalogue.featuredFacts(work("Air", 1, 1)), "1 recording")
    }

    func testAComposerIsWrittenAsACatalogueWritesThem() {
        XCTAssertEqual(Catalogue.initials("Antonio Vivaldi"), "A. Vivaldi")
        XCTAssertEqual(Catalogue.initials("Johann Sebastian Bach"), "J. S. Bach")
        XCTAssertEqual(Catalogue.initials("Ludwig van Beethoven"), "L. van Beethoven")
        XCTAssertEqual(Catalogue.initials("Hildegard"), "Hildegard")
        XCTAssertEqual(Catalogue.initials(" "), "")
        XCTAssertEqual(Catalogue.life(born: 1685, died: 1750), "1685\u{2013}1750")
        XCTAssertEqual(Catalogue.life(born: 1950, died: 0), "1950\u{2013}")
        XCTAssertEqual(Catalogue.life(born: 0, died: 0), "")
    }

    func testAPerformanceSaysWhenHowMuchAndHowLong() {
        XCTAssertEqual(Catalogue.facts(take(1992, tracks: 3, ms: 40 * 60_000)), "1992 · 3 tracks · 40 min")
        XCTAssertEqual(Catalogue.facts(take(0, tracks: 1, ms: 0, licence: "CC BY 4.0")), "1 track", "the terms are the foot's")
        XCTAssertEqual(Catalogue.who(take(0, tracks: 1, ms: 0, performers: "")), "Performer not named")
        XCTAssertEqual(Catalogue.workKey("bach/bwv-988@kimiko-ishizaka"), "bach/bwv-988")
        XCTAssertNil(Catalogue.workKey("bach/bwv-988"))
        XCTAssertEqual(Catalogue.title(work("Goldberg Variations", 1, 1, catalogue: "BWV 988")), "Goldberg Variations, BWV 988")
    }

    /// Movements of one work are one run under its heading, its length theirs;
    /// a track of no work is a run of none.
    func testAnAlbumsTracksAreInRunsOfOneWork() {
        let tracks = (1...5).map { Fx.entry($0) }
        let details: [TrackDetailsEntry?] = [detail(1, work: "Suite No. 1"), detail(2, work: "Suite No. 1"), detail(3, work: "Suite No. 2"),
                                             detail(4, work: "", track: 4), nil]
        let runs = Catalogue.runs(tracks, details: details)
        XCTAssertEqual(runs.map { $0.work }, ["Suite No. 1", "Suite No. 2", ""])
        XCTAssertEqual(runs.map { $0.range }, [0..<2, 2..<3, 3..<5])
        XCTAssertEqual(runs[0].ms, 400_000)
        XCTAssertEqual(runs[0].composer, "Handel")
        XCTAssertEqual(Catalogue.runs(tracks, details: details, headings: false).map { $0.range }, [0..<5])
        XCTAssertEqual(Catalogue.runs([], details: []), [])
    }

    func testUnderATrackIsWhoPlayedItOrWhoseItIs() {
        var d = detail(1, work: "Suite No. 1")
        let t = Fx.entry(1)
        XCTAssertEqual(Catalogue.under(t, d, creator: "Handel"), "")
        XCTAssertEqual(Catalogue.under(t, d, creator: "Bach"), "Handel")
        d.performer = "English Baroque Soloists"
        d.licence = "CC BY 4.0"
        XCTAssertEqual(Catalogue.under(t, d, creator: "Handel"), "English Baroque Soloists", "the terms are the foot's")
    }

    /// A page's terms, for its foot: the licence alone, or each beside whoever gave it.
    func testTheTermsAreSaidOnceAtTheFoot() {
        var a = detail(1, work: "Suite No. 1")
        a.performer = "Busch Chamber Players"
        a.licence = "CC BY 3.0"
        var b = detail(2, work: "Suite No. 1")
        b.performer = "Advent Chamber Orchestra"
        b.licence = "CC BY-SA 3.0"
        let free = detail(3, work: "Suite No. 2")
        XCTAssertEqual(Catalogue.licences([a, a, free]), ["CC BY 3.0"])
        XCTAssertEqual(Catalogue.licences([a, b, free]), ["CC BY 3.0 (Busch Chamber Players)", "CC BY-SA 3.0 (Advent Chamber Orchestra)"])
        XCTAssertEqual(Catalogue.licences([free, nil]), [])
    }

    /// The composer first, then the credits in billing order, each once.
    func testEverybodyOnAPerformanceOnce() {
        let credits = [CreditsEntry(instrument: "harpsichord", name: "Ana", pos: 1, role: "soloist"),
                       CreditsEntry(instrument: "", name: "Ben", pos: 2, role: "conductor"),
                       CreditsEntry(instrument: "organ", name: "Ana", pos: 3, role: "organ")]
        let people = Catalogue.people(composer: "Handel", credits: credits)
        XCTAssertEqual(people.map { $0.name }, ["Handel", "Ana", "Ben"])
        XCTAssertEqual(people.map { $0.role }, ["Composer", "soloist, harpsichord, organ", "conductor"])
        XCTAssertEqual(Catalogue.people(composer: "", credits: []), [])
    }

    func testAnAlbumIsByWhoPlayedMostOfIt() {
        let a = AlbumsEntry(art: "", creator: "Handel", label: "", name: "Water Music", performer: "", released: 0, tracks: 3)
        XCTAssertEqual(a.playedBy, "Handel")
        var b = a
        b.performer = "English Baroque Soloists"
        XCTAssertEqual(b.playedBy, "English Baroque Soloists")
    }
}
