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
// **The lists are spliced, not re-read.** The accounts, the groups, the
// month's lines and its summary, what the month spent, what needs a look,
// the payees and the banks are the kernel's maintained views (`LiveView<T>`
// over UniFFI's records, no JSON per row), read once and, after every
// change, asked what they did and their patches spliced into the lists the
// screens hold. The month's three are opened again when the month on screen
// changes. A register is a view too, held by its account while a screen
// shows it ([hold]) and let go when the last one leaves.
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
    /// The registers held, by account — `Registers.all` for every account's.
    var registers = [String: [TxnLine]]()
    var status: Status? = nil
    /// How many changes the peer has reported.
    var moved = 0

    func line(_ id: Id?) -> BudgetLine? { id.flatMap { i in lines.first { $0.categoryId == i } } }
    func account(_ id: Id?) -> AccountLine? { id.flatMap { i in accounts.first { $0.id == i } } }
}

extension Registers {
    /// The key of every account's register.
    static let all = "all"
    static func key(_ account: Id?) -> String { account?.hex ?? all }
}

/// What the peer says when anything moved: the session's queue is told.
private final class Told: Listener, @unchecked Sendable {
    private let then: () -> Void
    init(_ then: @escaping () -> Void) { self.then = then }
    func changed() { then() }
}

/// A view and the list it is spliced into; on the session's queue only.
final class Held<E> {
    private let open: (Money) throws -> LiveView<E>
    private var view: LiveView<E>? = nil
    private(set) var rows = [E]()
    init(_ open: @escaping (Money) throws -> LiveView<E>) { self.open = open }

    func hydrate(_ m: Money) {
        view = try? open(m)
        rows = view?.rows() ?? []
    }

    /// What it did since last asked, spliced; true when it moved.
    func update() -> Bool {
        guard let v = view else { return false }
        switch v.take() {
        case .unchanged: return false
        case .patched(let changes):
            rows.splice(changes)
            return true
        case .reset(let all):
            rows = all
            return true
        }
    }
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

    // On `queue` only.
    private var money: Money? = nil
    private var bank: DemoBank? = nil
    private var closed = false
    private var draining = false
    private var deniedSaid = false
    private var snap = Snapshot()
    private let accounts = Held<AccountLine> { try $0.accounts() }
    private let groups = Held<GroupLine> { try $0.groups() }
    private let inbox = Held<TxnLine> { try $0.inbox() }
    private let payees = Held<PayeeLine> { try $0.payees() }
    private let banks = Held<BankLine> { try $0.banks() }
    private var lines: Held<BudgetLine>
    private var summary: Held<Summary>
    private var spending: Held<SpendingLine>
    private var registers = [String: (held: Held<TxnLine>, refs: Int)]()

    /// The session `opening` says, showing `month`; what it sees told
    /// through `deliver` — the main queue unless a test says otherwise.
    init(_ opening: Opening, month: String,
         deliver: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) },
         publish: @escaping (Snapshot) -> Void, notice: @escaping (String) -> Void,
         denied: @escaping (String) -> Void, failed: @escaping (String) -> Void) {
        self.opening = opening
        self.deliver = deliver
        self.publish = publish
        self.notice = notice
        self.denied = denied
        self.failed = failed
        snap.month = month
        let held = Bridge.month(month)
        lines = held.0
        summary = held.1
        spending = held.2
        queue.async { self.open() }
    }

    private static func month(_ m: String) -> (Held<BudgetLine>, Held<Summary>, Held<SpendingLine>) {
        let input = Month(month: m)
        return (Held { try $0.month(input) }, Held { try $0.summary(input) }, Held { try $0.spending(input) })
    }

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
        peer.setListener(listener: Told { [weak self] in self?.drainSoon() })
        accounts.hydrate(m)
        groups.hydrate(m)
        lines.hydrate(m)
        summary.hydrate(m)
        spending.hydrate(m)
        inbox.hydrate(m)
        payees.hydrate(m)
        banks.hydrate(m)
        for (_, r) in registers { r.held.hydrate(m) }
        copyAll()
        snap.loaded = true
        drain()
    }

    private func copyAll() {
        snap.accounts = accounts.rows
        snap.groups = groups.rows
        snap.lines = lines.rows
        snap.summary = summary.rows.first
        snap.spending = spending.rows
        snap.inbox = inbox.rows
        snap.payees = payees.rows
        snap.banks = banks.rows
        for (k, r) in registers { snap.registers[k] = r.held.rows }
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
        splice()
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

    private func splice() {
        if accounts.update() { snap.accounts = accounts.rows }
        if groups.update() { snap.groups = groups.rows }
        if lines.update() { snap.lines = lines.rows }
        if summary.update() { snap.summary = summary.rows.first }
        if spending.update() { snap.spending = spending.rows }
        if inbox.update() { snap.inbox = inbox.rows }
        if payees.update() { snap.payees = payees.rows }
        if banks.update() { snap.banks = banks.rows }
        for (k, r) in registers where r.held.update() { snap.registers[k] = r.held.rows }
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

    /// The budget shows `month` now: its lines, summary and spending opened for it.
    func show(month: String) {
        queue.async {
            guard month != self.snap.month else { return }
            self.snap.month = month
            (self.lines, self.summary, self.spending) = Bridge.month(month)
            guard !self.closed, let m = self.money else { return }
            self.lines.hydrate(m)
            self.summary.hydrate(m)
            self.spending.hydrate(m)
            self.snap.lines = self.lines.rows
            self.snap.summary = self.summary.rows.first
            self.snap.spending = self.spending.rows
            self.show()
        }
    }

    /// A register held once more while a screen shows it: `account`'s, or every account's.
    func hold(register account: Id?) {
        let k = Registers.key(account)
        queue.async {
            if var e = self.registers[k] {
                e.refs += 1
                self.registers[k] = e
                return
            }
            let input = Register(accountId: account)
            let h = Held<TxnLine> { try $0.register(input) }
            self.registers[k] = (h, 1)
            guard let m = self.money else { return }
            h.hydrate(m)
            self.snap.registers[k] = h.rows
            self.show()
        }
    }

    /// Held once less; let go when no screen holds it.
    func release(register account: Id?) {
        let k = Registers.key(account)
        queue.async {
            guard var e = self.registers[k] else { return }
            e.refs -= 1
            if e.refs > 0 {
                self.registers[k] = e
            } else {
                self.registers[k] = nil
                self.snap.registers[k] = nil
            }
        }
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
            self.registers = [:]
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
