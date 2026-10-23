// Where a track's bytes are, how a length is said, a page's place, and the
// derived square's hash — pinned to the desktop's and the Android app's.
@testable import Harken
import Foundation
import XCTest

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
    }

    func testRecentSearchesAreNewestFirstOnce() {
        XCTAssertEqual(Recent.adding(" Bach ", to: ["handel", "bach"]), ["Bach", "handel"])
        XCTAssertEqual(Recent.adding("", to: ["a"]), ["a"])
        XCTAssertEqual(Recent.adding("z", to: (0..<8).map { "\($0)" }).count, 8)
    }

    func testASearchFindsAlbumsAndArtistsByName() {
        let albums = [AlbumsEntry(art: "", creator: "Handel", name: "Water Music", tracks: 3), AlbumsEntry(art: "", creator: "Bach", name: "Goldberg", tracks: 1)]
        let artists = [ArtistsEntry(art: "", name: "Handel", tracks: 3)]
        let r = Results.of(" handel", songs: [], albums: albums, artists: artists)
        XCTAssertEqual(r.albums.map { $0.name }, ["Water Music"])
        XCTAssertEqual(r.artists.count, 1)
        XCTAssertFalse(r.isEmpty)
    }
}
