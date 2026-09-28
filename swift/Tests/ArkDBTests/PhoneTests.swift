import Foundation
import ArkDB
import ArkDBClient
@testable import HarkenPhone

// The iOS app's bridge (harken/ios/Harken/Rows.swift) over harken's domain
// as the phone compiles it, driven through a session alone and through
// sessions on an in-process server that holds the whole of harken.ark —
// the paths Model.swift calls. The phone carries seven procedures; the
// library arrives from a scanner peer that authors `add_song`, which the
// phone does not have and applies by what the server sends for it.

/// harken.ark, the whole module the server and the scanner run.
func harkenArk() throws -> [UInt8] {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().appendingPathComponent("harken/domain/harken.ark")
    return [UInt8](try Data(contentsOf: url))
}

/// `add_song`'s input, every field said.
func song(_ title: String, _ artist: String, _ file: String) -> Args {
    return ["title": .text(title), "artist": .text(artist), "album": .text(""), "duration_ms": .int(180_000),
            "file": .text(file), "track": .int(0), "part": .text(""), "catalogue": .text(""), "performer": .text(""),
            "bpm": .int(0), "album_art": .text(""), "artist_art": .text(""), "disc": .int(0), "work_title": .text(""),
            "movement_no": .int(0)]
}

func phoneTests() throws {
    run("phone/alone") {
        let s = try Session.open(directory: try freshDir("phone-alone"), module: Phone.moduleBytes, procedures: Phone.procedures,
                                 user: "alice", server: nil)
        check("the phone holds its seven procedures natively",
              Set(Phone.procedureNames) == ["library", "create_playlist", "add_to_playlist", "remove_from_playlist", "playlists", "playlists_of", "playlist"],
              "\(Phone.procedureNames)")
        check("the form says why a blank name will not do", Phone.nameProblem(s, "   ") == "a playlist needs a name")
        check("and why a long one will not", Phone.nameProblem(s, String(repeating: "x", count: 121)) == "name: at most 120 characters")
        check("and nothing about a good one", Phone.nameProblem(s, " Mix ") == nil)
        check("a blank name is refused with the same words", {
            if case .failure(let why) = Phone.createPlaylist(s, name: " ") { return why == .refused("a playlist needs a name") }
            return false
        }())
        check("no default while the view is not known to hold her playlists", try Phone.ensureDefault(s, known: false) == nil && (try Phone.playlists(s)).isEmpty)
        guard case .success(let fav)? = Phone.ensureDefault(s, known: true) else { throw TestError("no default made") }
        check("the default is made for somebody with no playlist, and is hers",
              try Phone.playlists(s).map { $0.name } == ["Favorites"] && Phone.playlists(s)[0].userId == "alice")
        check("and alone it is in the log at once", s.standing(fav) == .confirmed && Phone.caption(s.standing(fav)) == nil)
        check("somebody with a playlist gets no second default", try Phone.ensureDefault(s, known: true) == nil
              && (try Phone.playlists(s)).count == 1)
        guard case .success = Phone.createPlaylist(s, name: "Favorites"), case .success = Phone.createPlaylist(s, name: " Mix "),
              case .success = Phone.createPlaylist(s, name: "Favorites") else { throw TestError("create refused") }
        check("a name she already has is numbered, the smallest free number first",
              try Phone.playlists(s).map { $0.name } == ["Favorites", "Favorites (1)", "Mix", "Favorites (2)"])
        guard case .success(let first) = Phone.playlistForAdding(s) else { throw TestError("no playlist for adding") }
        let pid = first.playlist
        check("adding with playlists goes on the first, and makes none", try pid == Phone.playlists(s)[0].id && first.made == nil && Phone.playlists(s).count == 4)
        let t1 = ArkDB.Id(bytes: [UInt8](repeating: 0x11, count: 16))!
        check("adding what is not in the library is authored, and is a no-op",
              try { if case .success = Phone.addToPlaylist(s, playlist: pid, media: t1) { return true }; return false }()
              && (try Phone.playlist(s, pid)).isEmpty && (try Phone.library(s, against: pid)).isEmpty)
        check("adding to a playlist that is not there is refused", {
            if case .failure(let why) = Phone.addToPlaylist(s, playlist: t1, media: t1) { return why == .refused("playlist_id: no such playlist") }
            return false
        }())
        check("everything the phone authored was sequenced by the interpreter too",
              s.status.pending == 0 && s.status.rejections == 0 && s.status.cursor == 5, "\(s.status)")
        s.close()
    }

    run("phone/signed-out-then-signed-in") {
        let full = try harkenArk()
        let m = try Decode.fromValue(try Canon.decode(full))
        let server = InProcessServer(Authority(m.schema, Hash.closures(m)))
        let ex = MemoryExchange(server: server)
        let url = URL(string: "mem://exchange/sync")!
        // The scanner: a peer of the whole module, authoring through the
        // interpreter, as harken-server's is.
        let scanner = try Session.open(directory: try freshDir("phone-scanner"), module: full, user: "library", server: url, dial: ex.dial)
        for (t, a, f) in [("Air", "Bach", "music/air.flac"), ("Aria", "Bach", "music/aria.flac"), ("Spring", "Vivaldi", "music/spring.flac")] {
            guard case .success = scanner.author(name: "add_song", args: song(t, a, f)) else { throw TestError("add_song refused") }
        }
        for _ in 0..<6 { scanner.pump() }
        check("the scanner's songs are in the log", scanner.status.cursor == 3 && scanner.status.pending == 0, "\(scanner.status)")

        // A phone opened with nobody signed in: it works, and says nothing.
        let dir = try freshDir("phone-signed-out")
        let p = try Session.openSignedOut(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures, server: url, dial: ex.dial)
        for _ in 0..<4 { p.pump() }
        check("signed out: no connection and an empty library", try !p.status.signedIn && !p.status.linked && (try Phone.library(p)).isEmpty, "\(p.status)")
        guard case .success(let target) = Phone.playlistForAdding(p), let made = target.made else { throw TestError("no playlist made for adding") }
        let pid = target.playlist
        check("adding with no playlist makes the default, nobody's for now",
              try Phone.playlists(p).map { $0.name } == ["Favorites"] && Phone.playlists(p)[0].id == pid && Phone.playlists(p)[0].userId == ""
              && p.standing(made) == .pending)
        guard case .success(let road) = Phone.createPlaylist(p, name: "Road trip") else { throw TestError("create refused") }
        check("the work is pending and says so", p.status.pending == 2 && Phone.caption(p.standing(road)) == "not synced yet")
        p.close()

        let p2 = try Session.openSignedOut(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures, server: url, dial: ex.dial)
        check("reopened signed out, the work was kept", try p2.status.pending == 2 && p2.standing(road) == .pending
              && (try Phone.playlists(p2)).map { $0.name } == ["Favorites", "Road trip"])
        p2.signIn(user: "alice")
        check("signing in makes it hers at once", try Phone.playlists(p2).allSatisfy { $0.userId == "alice" } && p2.status.signedIn)
        for _ in 0..<8 { p2.pump() }
        check("and syncs it: confirmed, and the library arrived", try p2.standing(road) == .confirmed && p2.status.pending == 0
              && (try Phone.library(p2)).map { $0.title } == ["Air", "Aria", "Spring"], "\(p2.standing(road)) \(p2.status)")
        check("the server has both as hers", server.authority.store.scan("playlist").filter { $0["user_id"] == .text("alice") }.count == 2)
        let lib = try Phone.library(p2)
        guard case .success(let added) = Phone.addToPlaylist(p2, playlist: pid, media: lib[1].id),
              case .success = Phone.addToPlaylist(p2, playlist: pid, media: lib[0].id) else { throw TestError("add refused") }
        for _ in 0..<6 { p2.pump() }
        check("adding lands at the end, in the order added", try Phone.playlist(p2, pid).map { $0.title } == ["Aria", "Air"]
              && Phone.playlist(p2, pid).map { $0.playlistPos } == [1, 2] && p2.standing(added) == .confirmed)
        check("the library read against the playlist says which are on it",
              try Phone.library(p2, against: pid).map { $0.playlistPos } == [2, 1, nil])
        check("playlists_of is what the menu ticks", try Phone.playlistsOf(p2, media: lib[1].id).map { $0.name } == ["Favorites"]
              && (try Phone.playlistsOf(p2, media: lib[2].id)).isEmpty)
        guard case .success = Phone.removeFromPlaylist(p2, playlist: pid, media: lib[1].id) else { throw TestError("remove refused") }
        for _ in 0..<6 { p2.pump() }
        check("remove one", try Phone.playlist(p2, pid).map { $0.title } == ["Air"])

        // Her second phone: signed in, caught up, and she has playlists, so
        // it makes no default.
        let q = try Session.open(directory: try freshDir("phone-second"), module: Phone.moduleBytes, procedures: Phone.procedures,
                                 user: "alice", server: url, dial: ex.dial)
        for _ in 0..<8 { q.pump() }
        check("a second device of hers sees her playlists and makes no default",
              try Phone.ensureDefault(q, known: true) == nil && (try Phone.playlists(q)).map { $0.name } == ["Favorites", "Road trip"])
        let bob = try Session.open(directory: try freshDir("phone-bob"), module: Phone.moduleBytes, procedures: Phone.procedures,
                                   user: "bob", server: url, dial: ex.dial)
        for _ in 0..<8 { bob.pump() }
        check("bob has the library and none of her playlists", try Phone.library(bob).count == 3 && Phone.playlists(bob).isEmpty)
        check("and cannot read hers", {
            do { _ = try Phone.playlist(bob, pid); return false } catch SessionError.refused(let r) { return r == .refused("not your playlist") } catch { return false }
        }())
        bob.close()

        // A third, used signed out, makes its own "Favorites": signed in as
        // her, the log keeps it under its own id, as "Favorites (1)".
        let r = try Session.openSignedOut(directory: try freshDir("phone-third"), module: Phone.moduleBytes, procedures: Phone.procedures,
                                          server: url, dial: ex.dial)
        guard case .success(let third) = Phone.createPlaylist(r, name: "Favorites") else { throw TestError("create refused") }
        r.signIn(user: "alice")
        for _ in 0..<8 { r.pump(); q.pump() }
        check("a taken name is renamed, not refused", try r.standing(third) == .confirmed
              && (try Phone.playlists(r)).map { $0.name } == ["Favorites", "Road trip", "Favorites (1)"], "\(r.standing(third))")
        check("and every device agrees", try Phone.playlists(q).map { $0.name } == ["Favorites", "Road trip", "Favorites (1)"]
              && q.stateHash() == r.stateHash())
        for x in [scanner, p2, q, r] { x.close() }
    }

    // Signing in again keeps what was pending: an entry authored under an
    // older login of hers is accepted by a server that knows she owns it —
    // and one that does not rejects it, with the reason beside the item.
    run("phone/re-login-keeps-pending-work") {
        let full = try harkenArk()
        let m = try Decode.fromValue(try Canon.decode(full))
        func attempt(owns: Bool) throws -> Standing {
            let server = InProcessServer(Authority(m.schema, Hash.closures(m)), authenticate: loginOf)
            if owns { server.withOwns { user, session in user == "alice" && session == "old" } }
            let ex = MemoryExchange(server: server)
            let url = URL(string: "mem://exchange/sync")!
            let dir = try freshDir("phone-relogin-\(owns)")
            let old = try Session.open(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures,
                                       user: "alice", session: "old", server: url, token: "alice:old", dial: ex.dial)
            old.goOffline()
            guard case .success(let mix) = Phone.createPlaylist(old, name: "Mix") else { throw TestError("create refused") }
            old.close()
            // Her login expired; she signed in again, on the same phone.
            let again = try Session.open(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures,
                                         user: "alice", session: "new", server: url, token: "alice:new", dial: ex.dial)
            check("the pending work survived the new login", again.standing(mix) == .pending)
            for _ in 0..<8 { again.pump() }
            let st = again.standing(mix)
            if case .rejected = st {
                check("a rejected change leaves the view, and is listed with its reason",
                      (try Phone.playlists(again)).isEmpty && again.rejections.map { $0.reason } == ["not yours"])
            }
            again.close()
            return st
        }
        let kept = try attempt(owns: true)
        check("a server that knows she owns the older login confirms it", kept == .confirmed, "\(kept)")
        let lost = try attempt(owns: false)
        check("one that does not rejects it, and the item says why", lost == .rejected("not yours")
              && Phone.caption(lost) == "not saved: not yours", "\(lost)")
    }
}
