// Where a track's bytes are, how a length is said, a page's place, and the
// derived square's hash — pinned to the desktop's and the Android app's.
@testable import Harken
import ArkUICore
import Foundation
import XCTest

/// A view's rows as the peer writes them, read as its typed views read them.
private func rows(_ json: String) -> [Any] {
    ((try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [Any]) ?? []
}

final class RowsTests: XCTestCase {
    func testATracksFileIsJoinedToItsServer() {
        XCTAssertEqual(Files.url("https://h", "music/Bach/air.flac"), "https://h/media/music/Bach/air.flac")
        XCTAssertEqual(Files.url("https://h/", "music/Bach/air.flac"), "https://h/media/music/Bach/air.flac")
        XCTAssertEqual(Files.url("http://10.0.2.2:8787", "music/Water Music/No. 1 #?/Bourrée.mp3"),
                       "http://10.0.2.2:8787/media/music/Water%20Music/No.%201%20%23%3F/Bourr%C3%A9e.mp3")
        XCTAssertEqual(Files.url("https://h", "https://upload.example/a.mp3"), "https://upload.example/a.mp3")
        XCTAssertEqual(Files.url("https://h", ""), "")
    }

    func testLengthsAsTheyAreSaid() {
        XCTAssertEqual(Spell.clock(229_000), "3:49")
        XCTAssertEqual(Spell.clock(3_723_000), "1:02:03")
        XCTAssertEqual(Spell.length(48 * 60_000), "48 min")
        XCTAssertEqual(Spell.length(150 * 60_000), "2 hr 30 min")
        XCTAssertEqual(Spell.length(60 * 60_000), "1 hr")
        XCTAssertEqual(Spell.count(1, "song"), "1 song")
        XCTAssertEqual(Spell.count(12, "song"), "12 songs")
        XCTAssertEqual(Spell.facts([Spell.count(2, "song"), nil, ""]), "2 songs")
    }

    /// An episode's row says its length, then what is left of it, then that it is played.
    func testAnEpisodesRowSaysWhatIsLeft() {
        func mine(_ at: Int64, _ of: Int64, finished: Bool = false) -> ProgressEntry {
            ProgressEntry(durationMs: of, finished: finished, mediaId: Id.none, name: "", positionMs: at, updatedMs: 0, userId: "u")
        }
        XCTAssertEqual(Spell.left(47 * 60_000, nil), "47 min")
        XCTAssertEqual(Spell.left(47 * 60_000, mine(35 * 60_000, 47 * 60_000)), "12 min left")
        XCTAssertEqual(Spell.left(47 * 60_000, mine(0, 0)), "47 min", "not begun")
        XCTAssertEqual(Spell.left(47 * 60_000, mine(47 * 60_000, 47 * 60_000, finished: true)), "Played")
        XCTAssertEqual(Spell.left(0, nil), "")
    }

    /// `arkui::art::hash`'s own pins: one album is one square everywhere.
    func testTheDerivedSquaresHashIsTheDesktops() {
        XCTAssertEqual(Art.hash(""), 0x811c9dc5)
        XCTAssertEqual(Art.hash("a"), 0xe40c292c)
        XCTAssertEqual(Art.hash("Water Music"), 0x74417273)
        XCTAssertEqual(Art.hash("Für Elise"), 0x939e323b)
        XCTAssertEqual(Art.hash("🎵"), 0x442e75ca)
        XCTAssertEqual(Art.pick(6, "a"), Int(0xe40c292c as UInt32 % 6))
    }

    func testAPlaceIsTheDesktopsSpelling() {
        let p = Playlist(id: Fx.id(7), name: "Road trip", pos: 0, createdMs: 0, userId: "ana")
        XCTAssertEqual(Page.album("Water Music").place([]), "#album/Water%20Music")
        XCTAssertEqual(Page.playlist(p.id).place([p]), "#playlist/Road%20trip")
        XCTAssertEqual(Page.playlist(Fx.id(8)).place([p]), "")
        XCTAssertEqual(Page.songs.place([]), "#library")
        XCTAssertEqual(Page.of(place: "#album/Bourr%C3%A9e", playlists: []), .album("Bourrée"))
        XCTAssertEqual(Page.of(place: "#playlist/Road%20trip", playlists: [p]), .playlist(p.id))
        XCTAssertEqual(Page.of(place: "#artist/", playlists: []), nil)
        XCTAssertEqual(Page.of(place: "", playlists: []), nil)
        // A work's and a performance's places are their keys, `/` and `@` and all.
        XCTAssertEqual(Page.work("johann-sebastian-bach/bwv-988").place([]), "#work/johann-sebastian-bach%2Fbwv-988")
        XCTAssertEqual(Page.of(place: Page.recording("bach/bwv-988@kimiko").place([]), playlists: []), .recording("bach/bwv-988@kimiko"))
        XCTAssertEqual(Page.of(place: "#composer/Antonio%20Vivaldi", playlists: []), .artist("Antonio Vivaldi"), "a composer's page is their artist page")
        // A show's place is its key, as a work's is.
        XCTAssertEqual(Page.show("podcast/full-time-nix").place([]), "#show/podcast%2Ffull-time-nix")
        XCTAssertEqual(Page.of(place: "#show/podcast%2Ffull-time-nix", playlists: []), .show("podcast/full-time-nix"))
    }

    /// A page reads its tracks, the catalogue's views beside them, or both,
    /// as the desktop's `open_page` opens them.
    func testAPageHoldsWhatTheDesktopsOpens() {
        XCTAssertEqual(Page.artist("Bach").listings, [.works("Bach"), .appearances("Bach")])
        XCTAssertTrue(Page.artist("Bach").listsTracks)
        XCTAssertEqual(Page.work("bach/bwv-988").listings, [.work("bach/bwv-988"), .recordings("bach/bwv-988")])
        XCTAssertFalse(Page.work("bach/bwv-988").listsTracks)
        XCTAssertEqual(Page.recording("bach/bwv-988@k").listings, [.credits("bach/bwv-988@k"), .work("bach/bwv-988"), .recordings("bach/bwv-988")])
        XCTAssertEqual(Page.recording("loose").listings, [.credits("loose")])
        XCTAssertEqual(Page.album("Water Music").listings, [])
        XCTAssertEqual(Page.show("podcast/nix").listings, [.progress("podcast/nix"), .episodes("podcast/nix")])
    }

    /// `people` and `transcript` rows as the peer hands them over: JSON, by the feed's names.
    func testAnEpisodesPeopleAndTranscriptAreReadFromTheirRows() {
        let people = rows(#"[{"art":"people/jacek.jpg","link":"https://galowicz.de","name":"Jacek Galowicz","role":"Host"},{"art":"","link":"","name":"Kiara Grouwstra","role":"Guest"}]"#)
            .map { PeopleEntry(json: $0) }
        XCTAssertEqual(people, [PeopleEntry(art: "people/jacek.jpg", link: "https://galowicz.de", name: "Jacek Galowicz", role: "Host"),
                                PeopleEntry(art: "", link: "", name: "Kiara Grouwstra", role: "Guest")])
        let lines = rows(#"[{"end_ms":4200,"speaker":"Jacek","start_ms":1500,"text":"Welcome back."},{"end_ms":9000,"speaker":"","start_ms":4200,"text":"Today: flakes."}]"#)
            .map { TranscriptEntry(json: $0) }
        XCTAssertEqual(lines, [TranscriptEntry(endMs: 4200, speaker: "Jacek", startMs: 1500, text: "Welcome back."),
                               TranscriptEntry(endMs: 9000, speaker: "", startMs: 4200, text: "Today: flakes.")])
    }

    /// A show's hosts are said once, over it; each episode names everybody
    /// else on it, once each, whatever case the feed writes "host" in.
    func testTheHostsAreSaidOnceAndTheRestOnEachEpisode() {
        func on(_ name: String, _ role: String) -> PeopleEntry { PeopleEntry(art: "", link: "", name: name, role: role) }
        let all = [on("Jacek Galowicz", "Host"), on("Kiara Grouwstra", "Guest"), on("Ada", "host"), on("Bo", "Producer"), on("Kiara Grouwstra", "Guest")]
        XCTAssertEqual(PeopleEntry.hosts(all).map { $0.name }, ["Jacek Galowicz", "Ada"])
        XCTAssertEqual(PeopleEntry.guests(all).map { $0.name }, ["Kiara Grouwstra", "Bo"])
        XCTAssertEqual(PeopleEntry.named(PeopleEntry.guests(all)), "Kiara Grouwstra, Bo")
        // A host who is also listed as a guest is a host.
        XCTAssertEqual(PeopleEntry.guests([on("Ada", "Host"), on("Ada", "Guest")]).count, 0)
        XCTAssertEqual(PeopleEntry.named([]), "")
    }

    /// A person's link opens in the browser only when it is a web page.
    func testAPersonsLinkIsTheirWebPage() {
        func linked(_ link: String) -> PeopleEntry { PeopleEntry(art: "", link: link, name: "Ada", role: "Guest") }
        XCTAssertEqual(linked("https://www.galowicz.de/about").web?.absoluteString, "https://www.galowicz.de/about")
        XCTAssertEqual(linked("https://www.galowicz.de/about").site, "galowicz.de")
        XCTAssertEqual(linked(" http://ada.example ").site, "ada.example")
        XCTAssertNil(linked("").web)
        XCTAssertNil(linked("mailto:ada@example.com").web)
        XCTAssertNil(linked("galowicz.de").web, "no scheme is no page")
        XCTAssertEqual(linked("").site, "")
    }

    /// The line playing is the last that has begun: none before the first,
    /// the last after it ends, and a line from the moment it starts.
    func testATranscriptsLineIsTheLastBegun() {
        func line(_ start: Int64, _ speaker: String = "") -> TranscriptEntry { TranscriptEntry(endMs: start + 1_000, speaker: speaker, startMs: start, text: "") }
        let lines = [line(1_000), line(2_000), line(2_000), line(5_000)]
        XCTAssertNil(TranscriptEntry.at(lines, 0))
        XCTAssertNil(TranscriptEntry.at(lines, 999))
        XCTAssertEqual(TranscriptEntry.at(lines, 1_000), 0)
        XCTAssertEqual(TranscriptEntry.at(lines, 1_999), 0)
        XCTAssertEqual(TranscriptEntry.at(lines, 2_000), 2, "of two that start together, the later")
        XCTAssertEqual(TranscriptEntry.at(lines, 4_999), 2, "a gap between lines keeps the last")
        XCTAssertEqual(TranscriptEntry.at(lines, 5_000), 3)
        XCTAssertEqual(TranscriptEntry.at(lines, 3_600_000), 3)
        XCTAssertNil(TranscriptEntry.at([], 1_000))
        // Held against the obvious rule, at every moment.
        for ms in stride(from: Int64(0), through: 6_000, by: 250) {
            XCTAssertEqual(TranscriptEntry.at(lines, ms), lines.lastIndex { $0.startMs <= ms }, "at \(ms)")
        }
    }

    /// A speaker is named where they begin speaking, not on every line, and
    /// a line nobody is named on does not make the next one say them again.
    func testASpeakerIsNamedWhereTheSpeakerChanges() {
        func line(_ speaker: String) -> TranscriptEntry { TranscriptEntry(endMs: 0, speaker: speaker, startMs: 0, text: "") }
        let lines = [line(""), line("Ada"), line("Ada"), line(""), line("Ada"), line("Bo"), line("Ada")]
        XCTAssertEqual(lines.indices.map { TranscriptEntry.names(lines, $0) }, [false, true, false, false, false, true, true])
    }

    /// Scrolled away by hand, the transcript follows again once the line
    /// playing comes down to the middle of the lines that show — not while it
    /// is below them, nor when it was already past their middle.
    func testFollowingResumesWhenPlaybackCatchesUp() {
        let showing = Array(10...20)
        XCTAssertTrue(TranscriptEntry.caughtUp(from: 14, to: 15, showing: showing))
        XCTAssertTrue(TranscriptEntry.caughtUp(from: 3, to: 16, showing: showing), "a seek into the middle")
        XCTAssertFalse(TranscriptEntry.caughtUp(from: 8, to: 9, showing: showing), "still above what shows")
        XCTAssertFalse(TranscriptEntry.caughtUp(from: 16, to: 17, showing: showing), "past the middle already: reading back")
        XCTAssertFalse(TranscriptEntry.caughtUp(from: 20, to: 21, showing: showing))
        XCTAssertFalse(TranscriptEntry.caughtUp(from: 14, to: 15, showing: []))
        XCTAssertFalse(TranscriptEntry.caughtUp(from: 14, to: nil, showing: showing))
    }

    func testRecentSearchesAreNewestFirstOnce() {
        XCTAssertEqual(Recent.adding(" Bach ", to: ["handel", "bach"]), ["Bach", "handel"])
        XCTAssertEqual(Recent.adding("", to: ["a"]), ["a"])
        XCTAssertEqual(Recent.adding("z", to: (0..<8).map { "\($0)" }).count, 8)
    }

    func testASearchFindsAlbumsAndArtistsByName() {
        let albums = [AlbumsEntry(art: "", creator: "Handel", label: "", name: "Water Music", performer: "", released: 0, tracks: 3), AlbumsEntry(art: "", creator: "Bach", label: "", name: "Goldberg", performer: "", released: 0, tracks: 1)]
        let artists = [ArtistsEntry(art: "", banner: "", born: 0, description: "", died: 0, name: "Handel", tracks: 3)]
        let r = Results.of(" handel", songs: [], albums: albums, artists: artists)
        XCTAssertEqual(r.albums.map { $0.name }, ["Water Music"])
        XCTAssertEqual(r.artists.count, 1)
        XCTAssertFalse(r.isEmpty)
    }
}
