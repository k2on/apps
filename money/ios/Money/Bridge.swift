// The phone's session, and nothing of Apple's but Foundation: money's peer —
// the Rust kernel through the bridge (`money/ffi`), which drives its own
// link and keeps its own views — opened over the replica for one server and
// one login, or over the demo's household in memory, and the views the
// screens draw from kept spliced. harken's `Bridge`, for a budget.
//
// **Everything runs on one queue** (`money-session`), in order: every write,
// every read, and the drain after each change the peer reports (`Listener`)
// — never on a clock, so a budget at rest runs nothing at all. A burst of
// tells is one drain.
//
// **The views are the core's** (`Session`, money/ffi): it holds each one
// by count, keeps its rows spliced on the kernel's thread and says which
// moved (`Changed`); a page is what the domain says it is made of — the
// budget for a month is its lines, summary and spending. The session holds
// what every screen draws from (the accounts, the groups, the month's
// budget, what needs a look, the payees, the banks, what their accounts
// feed, the reader's asks, every receipt and photo) and reads again into
// the snapshot what moved; a screen holds the rest while it shows it
// (ArkUI's `.holding(view)`: a register, what paid for a transaction, a
// receipt's items) and reads it from the session.
//
// **Files** (docs/plan-files.md) are the peer's: a photo taken is kept on
// the phone and sent when there is a connection, and one a screen wants is
// fetched; each drain looks for what is still wanted, and the bytes go into
// the snapshot by hash.
//
// What the screens see goes to `deliver` (the main queue in the app) as a
// [Snapshot] after a drain that moved something.
import Foundation

/// Everything a screen reads, at one moment.
struct Snapshot {
    var loaded = false
    /// The month the budget is showing, `2026-10`.
    var month = ""
    var accounts = [AccountLine]()
    var groups = [GroupLine]()
    var lines = [BudgetLine]()
    var summary: Summary? = nil
    var spending = [SpendingLine]()
    var inbox = [TxnLine]()
    var payees = [PayeeLine]()
    var banks = [BankLine]()
    /// What each bank's accounts feed, or wait to.
    var linked = [LinkedLine]()
    /// The reader's own asks about banks, newest first.
    var asks = [AskLine]()
    /// Every receipt, and every photo of one with where reading it stands.
    var receipts = [ReceiptLine]()
    var photos = [PhotoLine]()
    /// The files a screen asked for, by hash, once they are on the phone.
    var files = [String: Data]()
    var status: Status? = nil
    /// How many changes the peer has reported.
    var moved = 0

    func line(_ id: Id?) -> BudgetLine? { id.flatMap { i in lines.first { $0.categoryId == i } } }
    func account(_ id: Id?) -> AccountLine? { id.flatMap { i in accounts.first { $0.id == i } } }
}

/// What the peer says when anything moved: the session's queue is told.
private final class Told: Listener, @unchecked Sendable {
    private let then: () -> Void
    init(_ then: @escaping () -> Void) { self.then = then }
    func changed() { then() }
}

/// What the session says when views moved: the screens told which.
private final class Moved: Changed, @unchecked Sendable {
    private let then: ([MoneyView]) -> Void
    init(_ then: @escaping ([MoneyView]) -> Void) { self.then = then }
    func moved(views: [MoneyView]) { then(views) }
}

/// How the session's peer is had: the household on a server, or the demo's.
enum Opening {
    /// The replica for `server` under `root`, as `login`, dialling the server.
    case server(server: String, login: Login, root: URL)
    /// The demo's household in memory, seeded up to `today` (today when none).
    case demo(today: String?)
}

final class Bridge {
    let opening: Opening
    private let queue = DispatchQueue(label: "money-session")
    /// How a snapshot or a word reaches the screens: the main queue in the app.
    private let deliver: (@escaping () -> Void) -> Void
    private let publish: (Snapshot) -> Void
    private let notice: (String) -> Void
    private let denied: (String) -> Void
    private let failed: (String) -> Void
    private let opened: (Session) -> Void
    private let moved: ([MoneyView]) -> Void

    // On `queue` only.
    private var money: Money? = nil
    private var bank: DemoBank? = nil
    private var closed = false
    private var draining = false
    private var deniedSaid = false
    private var snap = Snapshot()
    private var session: Session? = nil
    /// What every screen draws from, held while the session is open.
    private var always = [Holding]()
    /// The month on screen's budget.
    private var budget: Holding? = nil
    /// Each view's version when it was last read into the snapshot.
    private var seen = [MoneyView: UInt64]()
    /// The files asked for and not on the phone yet: the peer fetches them,
    /// and each drain looks again.
    private var wanted = [String: FileRef]()

    /// The session `opening` says, showing `month`; what it sees told
    /// through `deliver` — the main queue unless a test says otherwise.
    init(_ opening: Opening, month: String,
         deliver: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) },
         publish: @escaping (Snapshot) -> Void, notice: @escaping (String) -> Void,
         denied: @escaping (String) -> Void, failed: @escaping (String) -> Void,
         opened: @escaping (Session) -> Void = { _ in }, moved: @escaping ([MoneyView]) -> Void = { _ in }) {
        self.opening = opening
        self.deliver = deliver
        self.publish = publish
        self.notice = notice
        self.denied = denied
        self.failed = failed
        self.opened = opened
        self.moved = moved
        snap.month = month
        queue.async { self.open() }
    }

    /// What every screen draws from, held as long as the session is open.
    private static let always: [MoneyView] = [.accounts, .groups, .inbox, .payees, .banks, .linked, .asks, .receipts, .photos]

    /// The replica's directory for `server` under `root`: one per server, not
    /// per person — the household's log is the server's and everyone in it
    /// reads it, and a person signing in again finds their pending intents.
    static func directory(_ root: URL, _ server: String) -> URL {
        let place = "server-" + String(server.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) && $0.isASCII ? Character($0) : "_" })
        return root.appendingPathComponent("money").appendingPathComponent(place).appendingPathComponent("replica")
    }

    private func main(_ f: @escaping () -> Void) { deliver(f) }

    // On `queue`: the peer opened (and dialling, for a server), every view read, and shown.
    private func open() {
        let peer: Peer
        do {
            switch opening {
            case let .server(server, login, root):
                let dir = Bridge.directory(root, server)
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                peer = try moneyOpen(dir: dir.path, login: login)
                peer.connect(url: socketUrl(server: server))
            case let .demo(today):
                peer = try moneyDemo(today: today)
                bank = DemoBank()
            }
        } catch {
            closed = true
            let w = why(error)
            main { self.failed(w) }
            return
        }
        let m = Money(peer: peer)
        money = m
        let s = Session(peer: peer)
        session = s
        peer.setListener(listener: Told { [weak self] in self?.drainSoon() })
        s.setListener(listener: Moved { [weak self] views in
            guard let self else { return }
            self.main { self.moved(views) }
            self.drainSoon()
        })
        always = Bridge.always.map { s.hold(view: $0) }
        budget = s.hold(view: .budget(month: snap.month))
        main { self.opened(s) }
        read()
        snap.loaded = true
        drain()
    }

    /// Whether `v` moved since it was last read here; it counts as read from now.
    private func fresh(_ v: MoneyView) -> Bool {
        guard let s = session else { return false }
        let now = s.version(view: v)
        defer { seen[v] = now }
        return seen[v] != now
    }

    // On `queue`: what moved since it was last read, read again.
    private func read() {
        guard let s = session else { return }
        if fresh(.accounts) { snap.accounts = s.accounts() }
        if fresh(.groups) { snap.groups = s.groups() }
        if fresh(.budget(month: snap.month)) {
            let b = s.budget(month: snap.month)
            snap.lines = b.lines
            snap.summary = b.summary
            snap.spending = b.spending
        }
        if fresh(.inbox) { snap.inbox = s.inbox() }
        if fresh(.payees) { snap.payees = s.payees() }
        if fresh(.banks) { snap.banks = s.banks() }
        if fresh(.linked) { snap.linked = s.linked() }
        if fresh(.asks) { snap.asks = s.asks() }
        if fresh(.receipts) { snap.receipts = s.receipts().map { ReceiptLine(json: J.parse($0)) } }
        if fresh(.photos) { snap.photos = s.photos().map { PhotoLine(json: J.parse($0)) } }
    }

    private func drainSoon() {
        queue.async {
            if self.draining { return }
            self.draining = true
            self.queue.async { self.drain() }
        }
    }

    // On `queue`: the views spliced, a rejection said once, a denial said once, and what moved shown.
    private func drain() {
        draining = false
        guard !closed, let m = money else { return }
        snap.moved += 1
        read()
        fetch()
        let st = m.peer.status()
        snap.status = st
        if let d = st.denied, !deniedSaid {
            deniedSaid = true
            main { self.denied(d) }
        }
        if let last = m.peer.takeRejections().last {
            let w = last.why
            main { self.notice("Not saved: \(w)") }
        }
        show()
    }

    // On `queue`: every file still wanted looked for again — one arrived rings
    // the listener, so a drain follows it.
    private func fetch() {
        guard let m = money else { return }
        for (h, f) in wanted {
            if let d = m.peer.file(hash: f.hash, size: f.size, media: f.media) {
                snap.files[h] = d
                wanted[h] = nil
            }
        }
    }

    private func show() {
        let s = snap
        main { self.publish(s) }
    }

    /// Something to do with the peer, on its queue; then a drain, so what it moved is shown.
    private func onPeer(_ f: @escaping (Money) -> Void) {
        queue.async {
            guard !self.closed, let m = self.money else { return }
            f(m)
            self.drain()
        }
    }

    // MARK: what the screens ask for

    /// The budget shows `month` now: its budget held for it.
    func show(month: String) {
        queue.async {
            guard month != self.snap.month else { return }
            self.snap.month = month
            guard !self.closed, let s = self.session else { return }
            self.budget = s.hold(view: .budget(month: month))
            self.read()
            self.show()
        }
    }

    /// A file's bytes for a screen: in the snapshot once the peer has them,
    /// fetched from the server when this phone does not.
    func want(file f: FileRef) {
        queue.async {
            guard self.snap.files[f.hash] == nil, self.wanted[f.hash] == nil else { return }
            self.wanted[f.hash] = f
            self.fetch()
            self.show()
        }
    }

    /// A photo of a receipt, taken or chosen: kept on the phone and sent to
    /// the server on this connection or the next, and read there, for
    /// `txn` when it was taken for one.
    func addPhoto(_ jpeg: Data, txn: Id?, done: @escaping (Bool) -> Void = { _ in }) {
        write("keep the photo", { m in
            let hash = try m.peer.putFile(bytes: jpeg, media: Receipts.jpeg)
            self.snap.files[hash] = jpeg
            try m.addPhoto(AddPhoto(file: FileRef(hash: hash, size: Int64(jpeg.count), media: Receipts.jpeg), txnId: txn))
        }, done: done)
    }

    /// A function of the domain, `what` it does said if it is refused:
    /// "Couldn't assign it: <the domain's sentence>". `done` is told on the
    /// screens' queue whether it went.
    func write(_ what: String, _ f: @escaping (Money) throws -> Void, done: @escaping (Bool) -> Void = { _ in }) {
        onPeer { m in
            do {
                try f(m)
                self.main { done(true) }
            } catch {
                let w = why(error)
                self.main {
                    self.notice("Couldn't \(what): \(w)")
                    done(false)
                }
            }
        }
    }

    /// What the form's input fails first, as the domain says it, or none:
    /// asked before the button is pressed.
    func check(_ name: String, _ json: String, done: @escaping (String?) -> Void) {
        queue.async {
            let w = self.money?.peer.check(name: name, input: json)
            self.main { done(w) }
        }
    }

    /// Ask for the bank's news: the demo's bank runs its next import here;
    /// a server's importer is asked to sync now (`banks.sync`, which any
    /// household member may ask), and the link dialled in case it was resting.
    /// `done` is told what to say.
    func syncBank(done: @escaping (String) -> Void) {
        queue.async {
            guard !self.closed, let m = self.money else { return self.main { done("") } }
            var said: String
            if let bank = self.bank {
                switch Result(catching: { try bank.sync(peer: m.peer, today: Bridge.today(self.opening)) }) {
                case let .success(s): said = s.note
                case let .failure(e): said = "The bank didn't answer: \(why(e))"
                }
            } else {
                m.peer.dialNow()
                do {
                    try m.sync()
                    said = "Asked the bank for anything new"
                } catch {
                    said = "Couldn't ask the bank: \(why(error))"
                }
            }
            self.drain()
            self.main { done(said) }
        }
    }

    private static func today(_ o: Opening) -> String? {
        if case let .demo(t) = o { return t }
        return nil
    }

    /// Dial the server now, whatever the link's backoff says: the app came to the front.
    func dialNow() { queue.async { self.money?.peer.dialNow() } }

    /// Signed out: the session revoked (pushed while linked, pending
    /// otherwise), nothing dialled after it, what is pending kept.
    func signOut() { onPeer { $0.peer.logout() } }

    /// Wait for everything asked of the session so far: what a test does
    /// before it reads what was published.
    func settle() { for _ in 0..<3 { queue.sync {} } }

    /// Stop telling and let the peer go; a server's disk stays for the next session.
    func close() {
        queue.async {
            self.closed = true
            self.money?.peer.setListener(listener: nil)
            self.session?.setListener(listener: nil)
            self.always = []
            self.budget = nil
            self.session = nil
            self.wanted = [:]
            self.money = nil
            self.bank = nil
        }
    }
}

/// A refusal's sentence, or what went wrong.
func why(_ error: Error) -> String {
    if let r = error as? Refusal, case let .Refused(why: w) = r { return w }
    return "\(error)"
}
