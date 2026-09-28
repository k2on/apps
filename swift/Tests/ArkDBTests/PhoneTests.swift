import Foundation
import ArkDB
import ArkDBClient
@testable import HarkenPhone

// The iOS app's bridge (harken/ios/Harken/Rows.swift) over harken's domain
// as the phone compiles it, driven through a session alone and through two
// sessions on an in-process server — the paths Model.swift calls.

func phoneTests() throws {
    run("phone/alone") {
        let s = try Session.open(directory: try freshDir("phone-alone"), module: Harken.moduleBytes, procedures: Harken.procedures,
                                 user: "alice", server: nil)
        check("the phone holds its six procedures natively", Set(Harken.procedureNames) == ["library", "create_playlist", "add_to_playlist", "remove_from_playlist", "playlists", "playlist_items"])
        check("the form says why a blank name will not do", Harken.nameProblem(s, "   ") == "a playlist needs a name")
        check("and why a long one will not", Harken.nameProblem(s, String(repeating: "x", count: 121)) == "name: at most 120 characters")
        check("and nothing about a good one", Harken.nameProblem(s, " Mix ") == nil)
        check("a blank name is refused with the same words", Harken.createPlaylist(s, name: " ") == .refused("a playlist needs a name"))
        check("create_playlist", Harken.createPlaylist(s, name: " Mix ") == nil)
        check("a second of the same name is a no-op", Harken.createPlaylist(s, name: "Mix") == nil)
        let pls = try Harken.playlists(s)
        check("one playlist, trimmed, hers", pls.count == 1 && pls[0].name == "Mix" && pls[0].userId == "alice")
        guard let pid = pls.first?.id else { throw TestError("no playlist") }
        let t1 = ArkDB.Id(bytes: [UInt8](repeating: 0x11, count: 16))!
        let t2 = ArkDB.Id(bytes: [UInt8](repeating: 0x12, count: 16))!
        check("add two tracks", Harken.addToPlaylist(s, playlist: pid, track: t1) == nil && Harken.addToPlaylist(s, playlist: pid, track: t2) == nil)
        let items = try Harken.items(s, of: pid)
        check("in order, at 1 and 2", items.map { $0.pos } == [1, 2] && items.map { $0.trackId } == [t1, t2])
        check("a track not yet arrived reads as unavailable", Harken.rows(items, try Harken.library(s)).map { $0.title } == ["(unavailable)", "(unavailable)"])
        let removed = Harken.removeFromPlaylist(s, playlist: pid, track: t1)
        check("remove one", try removed == nil && Harken.items(s, of: pid).map { $0.trackId } == [t2])
        check("adding to a playlist that is not there is refused", Harken.addToPlaylist(s, playlist: t1, track: t2) == .refused("playlist_id: no such playlist"))
        check("everything the phone authored was sequenced by the interpreter too", s.status.pending == 0 && s.status.rejections == 0 && s.status.cursors["playlists"] == 5)
        s.close()
    }

    run("phone/not-yours") {
        let m = try Decode.fromValue(try Canon.decode(Harken.moduleBytes))
        let ex = MemoryExchange()
        for sc in m.schema.scopes { ex.host(Authority(m.schema, sc.name, Hash.closures(m))) }
        let url = URL(string: "mem://exchange/sync")!
        let a = try Session.open(directory: try freshDir("phone-a"), module: Harken.moduleBytes, procedures: Harken.procedures, user: "alice", server: url, dial: ex.dial)
        let b = try Session.open(directory: try freshDir("phone-b"), module: Harken.moduleBytes, procedures: Harken.procedures, user: "bob", server: url, dial: ex.dial)
        func settle() { for _ in 0..<8 { a.pump(); b.pump() } }
        settle()
        check("alice creates", Harken.createPlaylist(a, name: "Alice's") == nil)
        settle()
        check("bob sees none of his own", try Harken.playlists(b).isEmpty)
        let pid = try Harken.playlists(a)[0].id
        let t = ArkDB.Id(bytes: [UInt8](repeating: 7, count: 16))!
        check("bob cannot add to alice's playlist", Harken.addToPlaylist(b, playlist: pid, track: t) == .refused("not your playlist"))
        check("nor read it", { do { _ = try Harken.items(b, of: pid); return false } catch SessionError.refused(let r) { return r == .refused("not your playlist") } catch { return false } }())
        check("alice can", Harken.addToPlaylist(a, playlist: pid, track: t) == nil)
        settle()
        check("both confirmed at the same place", a.status.cursors == b.status.cursors && a.status.pending == 0, "\(a.status.cursors) \(b.status.cursors)")
        check("the hashes agree", a.stateHash("playlists")! == b.stateHash("playlists")!)
        a.close(); b.close()
    }
}
