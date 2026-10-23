// The account's listening session as the phone's screens read it, and what
// they ask of it — nothing of Apple's but Foundation, so every rule here is
// type-checked on Linux and held by `HarkenTests`. The reconciler that makes
// this phone's player agree with the session, and says what happened to it,
// is harken's one reconciler, in Rust (`harken/player`, through the FFI's
// `Player`); what is here is what the screens, the device picker and the lock
// screen draw and press: the session read ([LiveSession], [Heard]), every
// button a transition ([Press]), the picker's rows ([Picker]), volumes and
// groups, and the throttle a dragged slider sends through. The twin of
// `harken/android/.../Session.kt`, `Listen.kt`'s `Heard` and `Volume.kt`.
//
// **The state is the instruction**: every button on every device is a
// transition, and only the device the session names as its output makes a
// sound.
import Foundation

/// What a press is answered with while this phone is its own session: the player's, then.
let notPart = "not part of the session"

/// The account's listening session as this phone holds it: every device
/// standing in it, by id; where each one's scrubber is being dragged; and
/// what is queued, if anything. (`Listening` is the domain's live row.)
struct LiveSession: Equatable {
    var devices: [String: Present] = [:]
    var scrub: [String: Int64] = [:]
    var active: Active? = nil

    /// `{"devices": [Present], "scrub": {device: ms}, "session": Active or null}`.
    static func of(_ j: Any?) -> LiveSession {
        guard let o = j as? [String: Any] else { return LiveSession() }
        var s = LiveSession()
        for d in (o["devices"] as? [Any]) ?? [] {
            let p = Present(json: d)
            s.devices[p.id] = p
        }
        for (k, v) in (o["scrub"] as? [String: Any]) ?? [:] { s.scrub[k] = J.int(v) }
        if let a = o["session"], !(a is NSNull) { s.active = Active(json: a) }
        return s
    }
}

// The session read: the generated types say what is there, these what it means.

extension Clock {
    /// How far into the track at `now`.
    func position(_ now: Int64) -> Int64 {
        switch self {
        case let .paused(atMs): return atMs
        case let .playing(atMs, since): return atMs + max(now - since, 0)
        }
    }

    var playing: Bool {
        if case .playing = self { return true }
        return false
    }

    var atMs: Int64 {
        switch self {
        case let .paused(at): return at
        case let .playing(at, _): return at
        }
    }
}

extension Output {
    /// The device an output names, here or not; none for nowhere.
    var deviceId: String? {
        switch self {
        case let .device(id, _): return id
        case let .away(id, _): return id
        case .nowhere: return nil
        }
    }

    /// The sound is on its way to a device that has not said it started.
    var moving: Bool {
        if case let .device(_, m) = self { return m }
        return false
    }

    /// The device that has it is here.
    var here: String? {
        if case let .device(id, _) = self { return id }
        return nil
    }
}

extension Active {
    /// Where the session stands at `now`: the clock's position — except while
    /// the sound is on its way to a device, when it stands where it was
    /// handed over. The domain's `standing`.
    func position(_ now: Int64) -> Int64 { output.moving ? clock.atMs : clock.position(now) }

    /// Whether its position is moving: playing, and not waiting on a hand-off.
    var counting: Bool { clock.playing && !output.moving }

    /// Where in the queue, held inside it.
    var index: Int { queue.isEmpty ? 0 : min(max(Int(at), 0), queue.count - 1) }

    var now: Track? { queue.isEmpty ? nil : queue[index] }
}

extension LiveSession {
    /// Whether `id` is the output and is here.
    func outputs(_ id: String) -> Bool { active?.output.here == id }

    /// Something is making a sound: a clock running on an output that is here.
    var sounding: Bool { active.map { $0.output.here != nil && $0.clock.playing } ?? false }

    /// The sound is on its way to `id`, which has not said it has started.
    func movingTo(_ id: String) -> Bool { active.map { $0.output.here == id && $0.output.moving } ?? false }

    /// The device the sound is on, where it is here: the leader of whatever group plays.
    var leader: String? { active?.output.here }

    /// Who follows `leader`, by id.
    func followers(_ leader: String) -> [Present] { devices.values.filter { $0.follows == leader }.sorted { $0.id < $1.id } }

    /// The group `leader` leads: it, then whoever follows it.
    func group(_ leader: String) -> [Present] { (devices[leader].map { [$0] } ?? []) + followers(leader) }

    /// Whether the output leads a group: something follows it.
    var grouped: Bool { leader.map { !followers($0).isEmpty } ?? false }

    /// The level of the group `leader` leads, as the domain reads it: the
    /// mean of its members' that have a volume, rounded down. None for a group
    /// in which nothing has one.
    func groupLevel(_ leader: String) -> Int? {
        let levels = group(leader).filter { $0.canVolume }.map { $0.volume }
        if levels.isEmpty { return nil }
        return Int(levels.reduce(0, +) / Int64(levels.count))
    }

    /// Whether every member of `leader`'s group with a volume is muted.
    func groupMuted(_ leader: String) -> Bool {
        let members = group(leader).filter { $0.canVolume }
        return !members.isEmpty && members.allSatisfy { $0.muted }
    }

    /// Whether `id` can be joined to the output's group now: the domain's
    /// `joinable` — a speaker that is here, can play, and is of the leader's
    /// family; not the leader, and a leader that follows nobody.
    func joinable(_ id: String) -> Bool {
        guard let lid = leader, let l = devices[lid], let d = devices[id] else { return false }
        return l.follows == nil && d.id != l.id && d.canPlay && d.family != nil && d.family == l.family
    }

    /// What is playing's volume: the output's group, where the output leads
    /// one; the output's own, where it has one; and, while the sound is
    /// nowhere, this phone's, which is where the next press plays. None where
    /// nothing it could move has a volume. `remote` is the sound on another device.
    func volume(_ me: String) -> Volume? {
        if let out = leader {
            let remote = out != me
            if grouped { return groupLevel(out).map { Volume(of: .group, level: $0, muted: groupMuted(out), remote: remote) } }
            guard let d = devices[out], d.canVolume else { return nil }
            return Volume(of: .device(out), level: Int(d.volume), muted: d.muted, remote: remote)
        }
        guard let d = devices[me], d.canVolume else { return nil }
        return Volume(of: .device(me), level: Int(d.volume), muted: d.muted, remote: false)
    }
}

// MARK: volume

/// How far a volume button moves a volume: five of a hundred, as the desktop's `+` and `-`.
let nudge = 5

/// What a volume control moves.
enum Level: Equatable {
    /// The output's group, every member with a volume in proportion.
    case group
    /// One device of the session.
    case device(String)
    /// This phone's own player, while it is its own session.
    case here
}

/// How a volume control moves what it moves. (`Change` is a view's patch.)
enum Turn: Equatable {
    case to(Int)
    /// What a volume button sends: which way, and not where to.
    case by(Int)
    case mute(Bool)
}

/// Whatever is playing's volume, as the full player's slider draws it.
struct Volume: Equatable {
    let of: Level
    let level: Int
    let muted: Bool
    let remote: Bool
}

// MARK: presses

/// What a button, a row or the lock screen asks of the session — or, alone, of the player.
enum Press: Equatable {
    case play
    case pause
    /// The one button that is both: pause what sounds, else play.
    case toggle
    case skip(Int)
    case seek(Int64)
    /// A track of the queue, from its start.
    case jump(Int)
    /// A track pressed: this queue, from here.
    case start(queue: [Track], at: Int, positionMs: Int64, from: String)
    /// Put these after what is playing, the track playing playing on.
    case next([Track])
    /// Put these at the end of the queue.
    case append([Track])
    /// Take the track at this place out of the queue.
    case remove(Int)
    /// Move the track at `from` to `to`.
    case move(from: Int, to: Int)
    /// Where this phone's scrubber is being dragged to, or none: ephemeral, and throttled.
    case scrub(Int64?)
    /// The sound to another device.
    case transfer(String)
    /// The sound to this device, if it is anywhere else.
    case here
    /// A volume moved.
    case loud(Level, Turn)
    /// A speaker joins the group that is playing.
    case join(String)
    /// A speaker leaves whatever group it is in, and goes quiet.
    case unjoin(String)

    /// The press as harken's player reads it (`harken_player::Press`, externally tagged JSON).
    var json: String {
        func tagged(_ name: String, _ fields: [String]) -> String { "{" + J.q(name) + ":" + J.obj(fields) + "}" }
        func tracks(_ ts: [Track]) -> String { J.wlist(ts) { $0.json } }
        func field(_ k: String, _ v: String) -> String { J.q(k) + ":" + v }
        switch self {
        case .play: return "\"Play\""
        case .pause: return "\"Pause\""
        case .toggle: return "\"Toggle\""
        case .here: return "\"Here\""
        case let .skip(by): return tagged("Skip", [field("by", String(by))])
        case let .seek(ms): return tagged("Seek", [field("ms", String(ms))])
        case let .jump(i): return tagged("Jump", [field("index", String(i))])
        case let .start(q, at, ms, from):
            return tagged("Start", [field("queue", tracks(q)), field("at", String(at)), field("position_ms", String(ms)), field("from", J.q(from))])
        case let .next(ts): return tagged("Next", [field("tracks", tracks(ts))])
        case let .append(ts): return tagged("Append", [field("tracks", tracks(ts))])
        case let .remove(i): return tagged("Remove", [field("index", String(i))])
        case let .move(from, to): return tagged("Move", [field("from", String(from)), field("to", String(to))])
        case let .scrub(at): return tagged("Scrub", [field("at_ms", J.wopt(at) { String($0) })])
        case let .transfer(to): return tagged("Transfer", [field("to", J.q(to))])
        case let .join(d): return tagged("Join", [field("device", J.q(d))])
        case let .unjoin(d): return tagged("Unjoin", [field("device", J.q(d))])
        case let .loud(level, turn):
            let l: String
            switch level {
            case .group: l = "\"Group\""
            case .here: l = "\"Here\""
            case let .device(id): l = "{\"Device\":" + J.obj([field("id", J.q(id))]) + "}"
            }
            let c: String
            switch turn {
            case let .to(n): c = "{\"To\":" + J.obj([field("level", String(n))]) + "}"
            case let .by(n): c = "{\"By\":" + J.obj([field("delta", String(n))]) + "}"
            case let .mute(m): c = "{\"Mute\":" + J.obj([field("muted", String(m))]) + "}"
            }
            return tagged("Loud", [field("level", l), field("change", c)])
        }
    }
}

/// A queue edit, as a list of tracks being edited sees it, made the session's presses.
enum Queue {
    /// The rows `fromOffsets` of the up-next list (whose first row is the
    /// queue's `at + 1`) taken out: one `remove` each, last first, so every
    /// place named is still the one it was.
    static func removing(_ offsets: [Int], at: Int) -> [Press] {
        offsets.sorted(by: >).map { .remove(at + 1 + $0) }
    }

    /// Up-next rows moved before the row `to` (SwiftUI's `onMove` numbering,
    /// over a list of `count` rows whose first is the queue's `at + 1`): the
    /// `move`s of the queue that make that order, each the domain's — the
    /// track at `from` taken out and put back at `to` — in turn.
    static func moving(_ offsets: [Int], to: Int, count: Int, at: Int) -> [Press] {
        let set = Set(offsets)
        let moved = offsets.sorted().filter { $0 >= 0 && $0 < count }
        var wanted = (0..<count).filter { !set.contains($0) }
        let dest = to - moved.filter { $0 < to }.count
        wanted.insert(contentsOf: moved, at: min(max(dest, 0), wanted.count))
        // One row, the drag a list makes: one move, or none where it was.
        if moved.count == 1, let r = moved.first, let to = wanted.firstIndex(of: r) {
            return r == to ? [] : [.move(from: at + 1 + r, to: at + 1 + to)]
        }
        var now = Array(0..<count)
        var out = [Press]()
        for i in 0..<count {
            guard let j = now.firstIndex(of: wanted[i]), j != i else { continue }
            now.insert(now.remove(at: j), at: i)
            out.append(.move(from: at + 1 + j, to: at + 1 + i))
        }
        return out
    }

    /// A seek as the session takes it: within the track a seek; to the next or
    /// the one before, a skip — so `next` still means next if the queue moved
    /// meanwhile; anywhere else, a jump. None for a place not in the queue.
    static func seek(_ a: Active?, index: Int, positionMs: Int64) -> Press? {
        guard let a = a else { return nil }
        let at = Int(a.at)
        if index == at { return .seek(max(positionMs, 0)) }
        if index == at + 1 { return .skip(1) }
        if index == at - 1 { return .skip(-1) }
        return index >= 0 && index < a.queue.count ? .jump(index) : nil
    }
}

// MARK: what the reconciler says

/// Every number this phone's player holds that the peer does not, for the diagnostics.
struct RemoteStats: Equatable {
    var calls: Int64 = 0
    var refused: Int64 = 0
    var plays: Int64 = 0
    var cameBackPlaying: Int64 = 0
    var lastRefusal: String? = nil

    static func of(_ j: Any?) -> RemoteStats {
        guard let o = j as? [String: Any] else { return RemoteStats() }
        func n(_ k: String) -> Int64 { o[k].map { J.int($0) } ?? 0 }
        return RemoteStats(calls: n("calls"), refused: n("refused"), plays: n("plays"), cameBackPlaying: n("came_back"), lastRefusal: o["last_refusal"] as? String)
    }
}

/// The session as this phone last heard it: what the screens, the picker and the lock screen draw.
struct Heard: Equatable {
    /// This device: its login's session id.
    var me: String = ""
    /// Part of the session: linked, and standing in it. False is this phone being its own session.
    var joined: Bool = false
    /// The session, while joined.
    var listening: LiveSession? = nil
    var linked: Bool = false
    /// How many connections have opened.
    var epoch: Int64 = 0
    var stats = RemoteStats()
    /// The last refusal the server sent this phone's calls.
    var refused: String? = nil
    /// What plays: the session's, while joined; this phone's own player's, alone.
    var active: Active? = nil

    /// The sound is on another device, here or not: this phone is a controller of it.
    var elsewhere: Bool { joined && (listening?.active?.output.deviceId).map { $0 != me } == true }

    /// Where another device's scrubber is being dragged to, if one is.
    var scrubbing: Int64? { listening?.scrub.first { $0.key != me }?.value }

    /// The player's view (`harken_player::Player::view`), as JSON.
    static func of(_ json: String) -> Heard {
        guard let o = J.parse(json) as? [String: Any] else { return Heard() }
        var h = Heard()
        h.me = o["me"] as? String ?? ""
        h.joined = o["joined"] as? Bool ?? false
        if let l = o["listening"], !(l is NSNull) { h.listening = LiveSession.of(l) }
        h.linked = o["linked"] as? Bool ?? false
        h.epoch = o["epoch"].map { J.int($0) } ?? 0
        h.stats = RemoteStats.of(o["stats"])
        h.refused = o["refused"] as? String
        if let a = o["active"], !(a is NSNull) { h.active = Active(json: a) }
        return h
    }
}

/// The clock a position is counted against: this phone's own, in ms, as every entry carries its author's.
func nowMs() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }

// MARK: the picker

/// One row of the picker: a device of this account — or the group that plays — and what it can be asked.
struct DeviceRow: Equatable, Identifiable {
    let id: String
    let name: String
    let kind: String
    /// Here and able to make a sound: a row that can be picked.
    let takeable: Bool
    /// The device the sound still belongs to, whose socket has gone.
    let away: Bool
    /// The sound is on its way to it.
    let moving: Bool
    /// Where the sound is — the output and every speaker following it — or,
    /// while it is nowhere, this phone, where the next press plays.
    let ticked: Bool
    let me: Bool
    /// Its volume, where anybody can move it: a row with none draws no slider.
    var volume: Int? = nil
    var muted: Bool = false
    /// What its button does: join it to the group that plays (true), take it out (false), or nothing.
    var join: Bool? = nil
    /// The row for the group as a whole, "Kitchen + 1", its volume every member's.
    var group: Bool = false

    /// What its volume controls move.
    var level: Level { group ? .group : .device(id) }

    /// What the row says under its name.
    var under: String? {
        if group { return "every speaker in the group" }
        if away { return "not answering" }
        if moving { return "connecting…" }
        if !takeable { return "can't make a sound" }
        if me { return "this phone" }
        return nil
    }
}

/// The picker, as the desktop's: no row that stops it everywhere; this phone
/// the default; a slider and a mute on every row with a volume; join and
/// unjoin on a speaker that can; and the group that plays first, as a whole.
enum DevicePicker {
    static func rows(_ s: LiveSession, me: String) -> [DeviceRow] {
        let output = s.active?.output
        let ticked = output?.deviceId ?? me
        let leader = s.leader
        var rows = s.devices.values.map { d -> DeviceRow in
            let follows = leader != nil && d.follows == leader
            let join: Bool? = follows ? false : (s.joinable(d.id) ? true : nil)
            return DeviceRow(id: d.id, name: d.name, kind: d.kind, takeable: d.canPlay, away: false, moving: s.movingTo(d.id),
                             ticked: d.id == ticked || follows, me: d.id == me, volume: d.canVolume ? Int(d.volume) : nil, muted: d.muted, join: join)
        }
        if case let .away(id, name)? = output, !rows.contains(where: { $0.id == id }) {
            rows.append(DeviceRow(id: id, name: name, kind: "computer", takeable: false, away: true, moving: false, ticked: true, me: id == me))
        }
        rows.sort { $0.name != $1.name ? $0.name < $1.name : $0.id < $1.id }
        guard let l = leader, s.grouped else { return rows }
        let head = s.devices[l]
        let all = DeviceRow(id: "", name: "\(head?.name ?? l) + \(s.followers(l).count)", kind: head?.kind ?? "speaker", takeable: false, away: false,
                            moving: false, ticked: false, me: false, volume: s.groupLevel(l), muted: s.groupMuted(l), group: true)
        return [all] + rows
    }

    /// Where the cursor starts: on the output — not the group's row, nor a follower — or on this phone.
    static func cursor(_ rows: [DeviceRow]) -> Int { rows.firstIndex { $0.ticked && !$0.group && $0.join != false } ?? 0 }

    /// What picking `row` asks: a hand-off, or nothing — the device that has it, or one that cannot.
    static func pick(_ row: DeviceRow, _ s: LiveSession?) -> Press? {
        guard !row.group, row.takeable, let s = s, s.active != nil else { return nil }
        if s.outputs(row.id) && !s.movingTo(row.id) { return nil }
        return .transfer(row.id)
    }

    /// What a row's slider, let go or dragged to `level`, asks.
    static func level(_ row: DeviceRow, _ level: Int) -> Press? { row.volume.map { _ in .loud(row.level, .to(level)) } }

    /// What a row's mute asks: the other way from how it is.
    static func mute(_ row: DeviceRow) -> Press? { row.volume.map { _ in .loud(row.level, .mute(!row.muted)) } }

    /// What a row's join button asks: join, leave, or nothing.
    static func join(_ row: DeviceRow) -> Press? {
        switch row.join {
        case true?: return .join(row.id)
        case false?: return .unjoin(row.id)
        case nil: return nil
        }
    }
}

// MARK: the throttle

/// The desktop's and the relay's interval: a dragged control sends at most twenty times a second.
let throttleMs: Int64 = 50

/// At most one send per window, trailing edge, the last value always — what
/// a dragged slider's ordered calls go through. The caller owns the clock.
struct Throttle<T> {
    let windowMs: Int64
    private var sent = Int64.min / 2
    private(set) var held: T? = nil

    init(windowMs: Int64 = throttleMs) { self.windowMs = windowMs }

    /// `v` at `now`: handed back if it goes now, else held, replacing whatever was.
    mutating func offer(_ now: Int64, _ v: T) -> T? {
        if now >= sent + windowMs {
            sent = now
            held = nil
            return v
        }
        held = v
        return nil
    }

    /// When what is held is due; none with nothing held.
    var due: Int64? { held == nil ? nil : sent + windowMs }

    /// What is held, if it is due at `now` — or at once, `force`d by letting go.
    mutating func take(_ now: Int64, force: Bool = false) -> T? {
        guard let h = held else { return nil }
        if !force && now < sent + windowMs { return nil }
        held = nil
        sent = now
        return h
    }
}

/// A volume slider dragged: each level sent through a [Throttle] as the
/// transition it is — a speaker has to move while the finger does — the last
/// always, by `later` at the window's end, or at once when it is let go.
/// Another slider dragged while one is held sends the held one first.
final class Drag {
    private let clock: () -> Int64
    private let later: (Int64, @escaping () -> Void) -> Void
    private let send: (Level, Int) -> Void
    private var throttle = Throttle<(Level, Int)>()
    private var armed = false

    init(clock: @escaping () -> Int64, later: @escaping (Int64, @escaping () -> Void) -> Void, send: @escaping (Level, Int) -> Void) {
        self.clock = clock
        self.later = later
        self.send = send
    }

    /// `of` dragged to `level`.
    func to(_ of: Level, _ level: Int) {
        if let h = throttle.held, h.0 != of { done() }
        if let now = throttle.offer(clock(), (of, level)) { send(now.0, now.1) } else { arm() }
    }

    /// Let go: whatever is held goes now.
    func done() {
        if let h = throttle.take(clock(), force: true) { send(h.0, h.1) }
    }

    private func arm() {
        if armed { return }
        guard let due = throttle.due else { return }
        armed = true
        later(max(due - clock(), 0)) { [weak self] in
            guard let self = self else { return }
            self.armed = false
            if let h = self.throttle.take(self.clock()) { self.send(h.0, h.1) }
            self.arm()
        }
    }
}
