// A peer's live state (`docs/plan-live.md`), as
// `kotlin/ark-client/.../LiveTests.kt` holds the Kotlin one and the
// reference's `live_tests` hold `rust/ark-client`: a device of the demo's
// `room` (spec/vectors/module/live.json) calls, and the server's frames are
// made the way a server makes them — each `applied` carrying the hash of the
// state its transition produces, run through the same `Runner`. Sans-io,
// with time handed in (`Peer.pumpAt`, `Peer.callThrottledAt`).
//
// Every claim was falsified once by breaking what it holds (the comment
// beside each says how, and what failed).
import ArkDB
import ArkDBClient
import Foundation

enum LiveTests {
    static var problems = [String]()

    static func claim(_ what: @autoclosure () -> String, _ ok: Bool) {
        if !ok { problems.append(what()) }
    }

    static let room = Instance("room", "alice")
    static let phone = DeviceDecl(id: "phone", kind: "phone", name: "Phone", canPlay: true)

    /// A device signed in as alice, and the server's side of what it hears.
    final class Rig {
        let domain: Domain
        let peer: Peer
        // What the server holds of the room, and its sequence.
        var value: Value
        var seq: Int64 = 0

        init(_ domain: Domain, device: DeviceDecl? = LiveTests.phone) throws {
            self.domain = domain
            peer = try Peer.open(domain, MemoryStorage(), Options("alice", "phone-login", "t", autos: .seeded(5), device: device))
            value = try domain.live.initial("room")
        }

        /// The server's frames are sent to the peer, one settle each.
        func hear(_ msg: ServerMsg) throws { try peer.recvFrame(Canon.encode(msg.toValue())) }

        /// Linked, then the snapshot a server sends on joining.
        func join() throws {
            peer.connected()
            _ = peer.takeOutgoingFrames()
            try hear(.liveSnapshot(LiveSnapshot(LiveTests.room, seq, value, [:])))
        }

        /// An ordered call the server took, applied to what it holds and told to the peer.
        func applied(_ transition: String, _ input: [String: Value], _ id: Id?, _ device: String, at: Int64 = 5000, hash: [UInt8]? = nil, seqAt: Int64? = nil) throws {
            let ctx = LiveCtx(user: "alice", session: "phone-login", roles: [], device: device, now: at)
            value = try domain.live.run("room", transition, value, ctx, input)
            seq = seqAt ?? seq + 1
            let h = hash ?? domain.live.hash("room", value)
            try hear(.applied(LiveApplied(LiveTests.room, transition, input, device, .ordered(seq: seq, id: id, user: "alice", session: "phone-login", roles: [], at: at, hash: h))))
        }

        /// The frames the peer sent since the last ask, read back.
        func sent() throws -> [ClientMsg] { try peer.takeOutgoingFrames().map { try ClientMsg.fromValue(try Canon.decode($0)) } }

        /// The calls among them.
        func calls() throws -> [LiveCall] {
            try sent().compactMap { if case .call(let c) = $0 { return c } else { return nil } }
        }

        func count() -> Int64? {
            if case .record(let r)? = peer.live("room"), case .int(let n)? = r["count"] { return n }
            return nil
        }

        func pointer(_ device: String) -> Value? {
            if case .record(let r)? = peer.live("room"), case .record(let p)? = r["pointer"] { return p[device] }
            return nil
        }
    }

    /// A device's `hello` declares it; a peer that declares none says the
    /// bytes it always did, and holds no live state. Falsified by dropping
    /// `live?.device` from `Client.hello`: the device's hello came out
    /// without one.
    static func hello(_ domain: Domain) throws {
        let r = try Rig(domain)
        r.peer.connected()
        guard case .hello(_, _, _, let d)? = try r.sent().first else { return claim("hello: a hello", false) }
        claim("hello: a device declares itself", d == phone)
        let plain = try Rig(domain, device: nil)
        plain.peer.connected()
        guard case .hello(_, _, _, let p)? = try plain.sent().first else { return claim("hello: a plain hello", false) }
        claim("hello: no device, no field", p == nil)
        claim("hello: no device, no live state", plain.peer.live("room") == nil)
        print("live: a device's hello declares it, and a peer that is none says nothing new")
    }

    /// An ordered call moves the view at once, is sent, stays pending — said
    /// again on every connection — until the server's `applied` confirms it.
    /// Falsified by not re-sending pending calls in `Watch.connected`: the
    /// second connection sent none.
    static func ordered(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        let kind = try r.peer.call("room", "bump", ["by": .int(3)])
        claim("ordered: bump is ordered", kind == .ordered)
        claim("ordered: the view moved at once (\(String(describing: r.count())))", r.count() == 3)
        let sent = try r.calls()
        claim("ordered: sent once (\(sent.count))", sent.count == 1 && sent[0].counter == nil)
        guard let id = sent.first?.id else { return }
        claim("ordered: a pump says the view moved", r.peer.pumpAt(0).live)
        r.peer.disconnected()
        r.peer.connected()
        claim("ordered: said again on the next connection", try r.calls().map { $0.id } == [id])
        try r.hear(.liveSnapshot(LiveSnapshot(room, r.seq, r.value, [:])))
        claim("ordered: replayed over the snapshot", r.count() == 3)
        try r.applied("bump", ["by": .int(3)], id, "phone")
        r.peer.connected()
        claim("ordered: confirmed, so not said again", try r.calls().isEmpty)
        claim("ordered: the view is the server's", r.count() == 3)
        print("live: an ordered call previews, stays pending, is said again, and is confirmed")
    }

    /// An ephemeral call is applied at once, sent with its counter, and never
    /// pending. Falsified by keeping ephemeral calls pending in `Watch.call`:
    /// the next connection said the pointer again.
    static func ephemeral(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        let kind = try r.peer.call("room", "point", ["at": .int(7)])
        claim("ephemeral: point is ephemeral", kind == .ephemeral)
        claim("ephemeral: the pointer moved at once", r.pointer("phone") == .int(7))
        let sent = try r.calls()
        claim("ephemeral: sent with a counter", sent.count == 1 && sent[0].counter != nil)
        r.peer.disconnected()
        r.peer.connected()
        claim("ephemeral: never pending, never said again", try r.calls().isEmpty)
        print("live: an ephemeral call is applied at once and is never pending")
    }

    /// Another device's ephemeral call is taken only when its counter is above
    /// the last taken from that device. Falsified by taking every counter in
    /// `Watch.heard`: the stale 3 moved the laptop's pointer back.
    static func counters(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        func point(_ at: Int64, _ counter: Int64) throws {
            try r.hear(.applied(LiveApplied(room, "point", ["at": .int(at)], "laptop", .ephemeral(counter: counter))))
        }
        try point(1, 5)
        claim("counters: the laptop's pointer", r.pointer("laptop") == .int(1))
        try point(2, 3)
        claim("counters: a stale counter is ignored", r.pointer("laptop") == .int(1))
        try point(3, 5)
        claim("counters: an equal counter is ignored", r.pointer("laptop") == .int(1))
        try point(4, 6)
        claim("counters: a newer one is taken", r.pointer("laptop") == .int(4))
        print("live: another device's ephemeral call is taken only above its last counter")
    }

    /// A gap in the sequence asks one `resync`, and so does an `applied` whose
    /// hash is not what this replica reaches; nothing is applied from either.
    /// Falsified by applying an ordered frame whatever its sequence: the gap
    /// moved the count to 10 and asked nothing.
    static func resync(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        try r.applied("bump", ["by": .int(10)], nil, "laptop", seqAt: 3)
        let asked = try r.sent()
        let one: Bool
        if asked.count == 1, case .resync = asked[0] { one = true } else { one = false }
        claim("resync: a gap asks for the instance (\(asked.count))", one)
        claim("resync: and applies nothing (\(String(describing: r.count())))", r.count() == 0)
        let h = try Rig(domain)
        try h.join()
        try h.applied("bump", ["by": .int(2)], nil, "laptop", hash: [UInt8](repeating: 0, count: 32))
        let asked2 = try h.sent()
        let two: Bool
        if asked2.count == 1, case .resync = asked2[0] { two = true } else { two = false }
        claim("resync: a wrong hash asks for the instance", two)
        claim("resync: and applies nothing", h.count() == 0)
        try h.applied("bump", ["by": .int(1)], nil, "laptop", seqAt: 3)
        claim("resync: asked once until it comes", try h.sent().isEmpty)
        try h.hear(.liveSnapshot(LiveSnapshot(room, h.seq, h.value, [:])))
        claim("resync: the snapshot is the server's", h.count() == 3)
        print("live: a gap and a wrong hash each ask one resync and apply nothing")
    }

    /// The server's verdict on a call takes it out of pending and is told
    /// once; a refusal of the preview's own is thrown and sends nothing.
    /// Falsified by keeping a refused call pending: the count stayed at 3.
    static func refused(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        try r.peer.call("room", "bump", ["by": .int(3)])
        guard let id = try r.calls().first?.id else { return claim("refused: a call", false) }
        try r.hear(.refused(LiveRefused(room, id, "not now")))
        claim("refused: the view without it", r.count() == 0)
        claim("refused: told", r.peer.takeLiveRefusals() == [Rejection(id: id, reason: "not now")])
        claim("refused: told once", r.peer.takeLiveRefusals().isEmpty)
        var preview: String? = nil
        do {
            try r.peer.call("room", "bump", ["by": .int(11)])
        } catch let e as Refused {
            preview = e.description
        }
        claim("refused: the preview's verdict (\(preview ?? "none"))", preview == "by: between 1 and 10")
        claim("refused: and nothing sent", try r.calls().isEmpty)
        print("live: a verdict takes a call out of pending, and a preview's refusal sends nothing")
    }

    /// A finger's calls go at most once per `throttleMs`, the last always: the
    /// view moves on every one, what is held is due at `deadline` and sent by
    /// the pump then. Falsified by never releasing what the throttle holds:
    /// the last value was not sent.
    static func throttle(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        try r.peer.callThrottledAt(1000, "room", "point", ["at": .int(1)])
        claim("throttle: the first goes at once", try r.calls().count == 1)
        try r.peer.callThrottledAt(1010, "room", "point", ["at": .int(2)])
        try r.peer.callThrottledAt(1020, "room", "point", ["at": .int(3)])
        claim("throttle: the view moved on every one", r.pointer("phone") == .int(3))
        claim("throttle: the rest held", try r.calls().isEmpty)
        claim("throttle: due at the window's end (\(String(describing: r.peer.deadline())))", r.peer.deadline() == 1050)
        r.peer.pumpAt(1040)
        claim("throttle: not before", try r.calls().isEmpty)
        r.peer.pumpAt(1050)
        let last = try r.calls()
        claim("throttle: the last sent, once", last.map { $0.input["at"] } == [.int(3)])
        claim("throttle: nothing waits", r.peer.deadline() == nil)
        let ordered = try r.peer.callThrottledAt(1051, "room", "bump", ["by": .int(1)])
        claim("throttle: an ordered call is never held", try ordered == .ordered && r.calls().count == 1)
        print("live: a finger's calls go once per \(throttleMs) ms, and the last always")
    }

    /// The server's clock is read off the `at` of the ordered frames heard.
    /// Falsified by not keeping the sample: the offset stayed nil.
    static func offset(_ domain: Domain) throws {
        let r = try Rig(domain)
        try r.join()
        claim("offset: none before any", r.peer.serverOffset == nil)
        let ahead = Int64(Date().timeIntervalSince1970 * 1000) + 60000
        try r.applied("bump", ["by": .int(1)], nil, "laptop", at: ahead)
        let o = r.peer.serverOffset
        claim("offset: about a minute ahead (\(String(describing: o)))", o.map { (55000...60000).contains($0) } ?? false)
        claim("offset: server_now follows it", r.peer.serverNow() >= ahead - 5000)
        print("live: the server's clock is read off the at of what it confirmed")
    }

    /// An `applied` carrying both a sequence and a counter — every field an
    /// ordered one has, and a counter — is refused by the decoder rather than
    /// read as either. Falsified by reading `seq` first whatever else is
    /// there: the frame decoded as an ordered one.
    static func bothOrders() {
        let both = Value.record(Record([
            ("t", .text("applied")), ("live", .text("room")), ("key", .text("alice")),
            ("transition", .text("point")), ("input", .record(Record([("at", .int(3))]))),
            ("device", .text("phone")), ("seq", .int(5)), ("user", .text("alice")),
            ("session", .text("phone-login")), ("at", .int(1000)),
            ("hash", .bytes([UInt8](repeating: 0, count: 32))), ("counter", .int(41)),
        ]))
        var refused = false
        do { _ = try ServerMsg.fromValue(both) } catch { refused = error is FactError }
        claim("decode: an applied with a seq and a counter is refused", refused)
        print("live: an applied with both a seq and a counter does not decode")
    }

    /// All of them; the problems found, empty when every claim held.
    static func run(_ vectors: String) -> [String] {
        problems = []
        do {
            let v = try readJson(vectors + "/module/live.json")
            let domain = try Domain.fromBytes(try hexBytes(try v.field("bytes")))
            try hello(domain)
            try ordered(domain)
            try ephemeral(domain)
            try counters(domain)
            try resync(domain)
            try refused(domain)
            try throttle(domain)
            try offset(domain)
            bothOrders()
        } catch {
            problems.append("\(error)")
        }
        if problems.isEmpty { print("live: every claim held") }
        return problems
    }
}
