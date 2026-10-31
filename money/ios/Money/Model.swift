// The app's one model, on the main actor: the first run's machine run
// (Onboarding.swift) — its effects carried out, the probe and the exchange
// off the main thread — the session it opens ([Bridge]) and the snapshot it
// publishes, the month the budget shows, and every button the screens have,
// each one function of the domain. Nothing of SwiftUI but
// `ObservableObject` and `@Published`, which are Combine's: the views are
// SwiftUI's and read this; it is type-checked on Linux against a stand-in
// for Combine, as harken's is.
import ArkUICore
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Combine

@MainActor
final class Model: ObservableObject {
    // MARK: what is published

    @Published private(set) var step: Step
    @Published private(set) var snapshot = Snapshot()
    /// The month the budget, the spending and the month's figures are of: `2026-10`.
    @Published private(set) var month: String
    /// The last refusal, rejection or word from the bank, said once.
    @Published var notice: String? = nil
    /// The sign-in to open in the browser sheet, once: the view runs it and answers [signInReturned].
    @Published var browse: URL? = nil

    // MARK: what it holds

    private var machine: Onboarding
    private let remembered: Remembered
    private let root: URL
    /// The demo's day when it is a fixed one (previews, tests, screenshots); today when none.
    private let demoDay: String?
    private var bridge: Bridge? = nil
    /// How a session's snapshot reaches the model: the main queue in the app.
    private let deliver: (@escaping () -> Void) -> Void

    /// The model over `shelf` (the app's `UserDefaults`), replicas under `root`.
    init(shelf: Shelf, root: URL, demoDay: String? = nil,
         deliver: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) }) {
        remembered = Remembered(shelf)
        self.root = root
        self.demoDay = demoDay
        self.deliver = deliver
        machine = Onboarding(server: remembered.server, login: remembered.login, demo: remembered.demo) { loginUrl(server: $0, redirect: $1) }
        step = machine.step
        month = Months.of(demoDay ?? moneyToday())
        run(machine.begin())
    }

    // MARK: the first run

    /// An event from a screen, the browser sheet, a link or the app's phase.
    func send(_ e: Event) {
        if case .foreground = e { bridge?.dialNow() }
        let effects = machine.next(e)
        step = machine.step
        run(effects)
    }

    private func run(_ effects: [Effect]) {
        step = machine.step
        for e in effects { carry(e) }
    }

    private func carry(_ e: Effect) {
        switch e {
        case let .findServer(typed):
            let domain = moneyDomain()
            Task {
                let found = await Probe.find(typed, domain: domain, get: Probe.get)
                self.send(.checked(found))
            }
        case let .rememberServer(url): remembered.server = url
        case let .browse(url): browse = URL(string: url)
        case let .exchange(server, code):
            Task {
                let answer: Event = await Task.detached {
                    do {
                        return Event.signedIn(try exchange(server: server, code: code))
                    } catch {
                        return Event.signInFailed("Signing in didn't finish: \(why(error)).")
                    }
                }.value
                self.send(answer)
            }
        case let .rememberLogin(l): remembered.login = l
        case let .rememberDemo(d): remembered.demo = d
        case let .openSession(server, login): open(.server(server: server, login: login, root: root))
        case .openDemo: open(.demo(today: demoDay))
        case .closeSession: close()
        }
    }

    /// What the browser sheet came back with: the redirect, or why there was none.
    func signInReturned(_ url: URL?, error: String?) {
        browse = nil
        if let u = url, let code = Probe.code(u, scheme: Onboarding.scheme) { return send(.code(code)) }
        send(.cameBack(error ?? "The sign-in came back without a login."))
    }

    /// A link the system opened the app with: `money://auth?code=…` is a sign-in coming back.
    func open(url: URL) {
        if let code = Probe.code(url, scheme: Onboarding.scheme) { send(.code(code)) }
    }

    // MARK: the session

    private func open(_ o: Opening) {
        close()
        bridge = Bridge(o, month: month, deliver: deliver,
                        publish: { [weak self] s in self?.took(s) },
                        notice: { [weak self] n in self?.notice = n },
                        denied: { [weak self] d in self?.send(.denied(d)) },
                        failed: { [weak self] w in self?.failed(w) })
    }

    private func close() {
        bridge?.close()
        bridge = nil
        snapshot = Snapshot()
    }

    private func failed(_ why: String) {
        notice = "This phone's copy of the budget couldn't be opened (\(why))."
        send(.signOut)
    }

    private func took(_ s: Snapshot) {
        snapshot = s
        // Signed in just now: opening, until the replica has what the server had —
        // or the server cannot be reached, and there is nothing more to wait for.
        if case .opening = step, let st = s.status, st.linked || st.cursor > 0 || st.link == "waiting to dial" || st.link == "offline" {
            send(.synced)
        }
    }

    /// Wait for the session to have done everything asked of it: a test's.
    func settle() { bridge?.settle() }

    // MARK: who and where

    var isDemo: Bool { if case .demo = step { return true } else { return false } }
    var server: String { remembered.server ?? "" }
    var login: Login? {
        switch step {
        case let .running(_, l), let .opening(_, l): return l
        default: return nil
        }
    }
    /// What to call whoever is using the budget.
    var who: String { isDemo ? "Alex" : login.map(Logins.name) ?? "" }

    /// Today, as the budget writes a day.
    var today: String { demoDay ?? moneyToday() }
    var thisMonth: String { Months.of(today) }

    /// Signed out of the server, or out of the demo, back to the first page.
    func signOut() {
        if isDemo { return send(.signOut) }
        bridge?.signOut()
        send(.signOut)
    }

    // MARK: the month

    func showMonth(_ m: String) {
        guard m != month else { return }
        month = m
        bridge?.show(month: m)
    }

    func stepMonth(_ n: Int64) { showMonth(moneyAddMonths(month: month, n: n)) }
    func showThisMonth() { showMonth(thisMonth) }
    var monthName: String { moneyMonthName(month: month) }

    // MARK: registers

    func hold(register account: Id?) { bridge?.hold(register: account) }
    func release(register account: Id?) { bridge?.release(register: account) }
    func register(_ account: Id?) -> [TxnLine]? { snapshot.registers[Registers.key(account)] }

    // MARK: the budget's buttons

    private func write(_ what: String, _ f: @escaping (Money) throws -> Void, done: @escaping (Bool) -> Void = { _ in }) {
        guard let b = bridge else { return done(false) }
        b.write(what, f, done: done)
    }

    /// Give a category exactly `cents` this month.
    func assign(_ id: Id, _ cents: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let m = month
        write("assign it", { try $0.assign(Assign(month: m, categoryId: id, amount: cents)) }, done: done)
    }

    /// Money moved between `line` and `other` (either none for Ready to Assign), as [Moves] says.
    func move(_ line: BudgetLine?, _ other: Id?, _ cents: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let mm = Moves.of(month: month, line: line, other: other, cents: cents)
        write("move the money", { try $0.moveMoney(mm) }, done: done)
    }

    /// The whole month (or `only` these) assigned at once.
    func autoAssign(_ how: Auto, only: [Id] = []) {
        let m = month
        write("auto-assign", { try $0.autoAssign(AutoAssign(month: m, how: how, only: only)) })
    }

    func setTarget(_ id: Id, _ cents: Int64) { write("set the target", { try $0.setTarget(SetTarget(id: id, target: cents)) }) }
    func addGroup(_ name: String) { write("add the group", { try $0.addGroup(AddGroup(name: name)) }) }
    func addCategory(_ group: Id, _ name: String) { write("add the category", { try $0.addCategory(AddCategory(groupId: group, name: name)) }) }
    func rename(category id: Id, _ name: String) { write("rename it", { try $0.renameCategory(RenameCategory(id: id, name: name)) }) }
    func rename(group id: Id, _ name: String) { write("rename it", { try $0.renameGroup(RenameGroup(id: id, name: name)) }) }
    func hide(category id: Id, _ hidden: Bool) { write(hidden ? "hide it" : "show it", { try $0.hideCategory(HideCategory(id: id, hidden: hidden)) }) }
    func hide(group id: Id, _ hidden: Bool) { write(hidden ? "hide it" : "show it", { try $0.hideGroup(HideGroup(id: id, hidden: hidden)) }) }
    func delete(category id: Id, moveTo: Id?) { write("delete it", { try $0.deleteCategory(DeleteCategory(id: id, moveTo: moveTo)) }) }
    func delete(group id: Id, moveTo: Id?) { write("delete it", { try $0.deleteGroup(DeleteGroup(id: id, moveTo: moveTo)) }) }

    /// A row of Edit let go somewhere new, as [Arranging] reads it: a
    /// category into a group in front of another, or a group past others.
    func rearrange(_ r: Rearrange) {
        switch r {
        case let .category(m): write("move it", { try $0.moveCategory(m) })
        case let .group(m): write("move it", { try $0.moveGroup(m) })
        }
    }

    // MARK: transactions

    /// The form saved: a new transaction, or the one corrected.
    func save(_ d: Draft, done: @escaping (Bool) -> Void = { _ in }) {
        if let problem = d.problem {
            notice = problem
            return done(false)
        }
        if let e = d.edit {
            write("save it", { try $0.editTransaction(e) }, done: done)
        } else if let a = d.adding {
            write("add it", { try $0.addTransaction(a) }, done: done)
        }
    }

    func delete(_ ids: [Id]) { write("delete it", { try $0.deleteTransactions(Txns(ids: ids)) }) }
    func approve(_ ids: [Id]) { write("approve it", { try $0.approve(Txns(ids: ids)) }) }
    /// Given a category, which is looking at them: they are approved with it.
    func categorize(_ ids: [Id], _ category: Id?) { write("categorize it", { try $0.categorize(Categorize(ids: ids, categoryId: category)) }) }
    func setCleared(_ id: Id, _ cleared: Bool) { write("mark it", { try $0.setCleared(SetCleared(id: id, cleared: cleared)) }) }

    // MARK: accounts

    func openAccount(name: String, kind: String, onBudget: Bool, balance: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let input = OpenAccount(name: name, kind: kind, onBudget: onBudget, balance: balance, date: today)
        write("add the account", { try $0.openAccount(input) }, done: done)
    }

    func rename(account id: Id, _ name: String) { write("rename it", { try $0.renameAccount(RenameAccount(id: id, name: name)) }) }
    func close(account id: Id, _ closed: Bool) { write(closed ? "close it" : "reopen it", { try $0.closeAccount(CloseAccount(id: id, closed: closed)) }) }

    /// The account agreed with the bank at `balance`: an adjustment for any difference, and what was cleared locked.
    func reconcile(_ id: Id, balance: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let d = today
        write("reconcile it", { try $0.reconcile(Reconcile(accountId: id, balance: balance, date: d)) }, done: done)
    }

    // MARK: the bank

    /// Pull to refresh: the bank asked for anything new, and what came of it said.
    func refresh() async {
        guard let b = bridge else { return }
        let said: String = await withCheckedContinuation { done in b.syncBank { done.resume(returning: $0) } }
        if !said.isEmpty { notice = said }
    }

    /// The same, from a button.
    func syncBank() { Task { await refresh() } }
}
