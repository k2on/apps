// The app's one model, on the main actor: the sign-in machine run under
// the first run every Ark app shares (`FirstRunApp`: ark_auth::first_run
// over ark_app::welcome, money's words money_app::welcome's; the pages the
// generated WelcomeView) — its effects carried out, the probe and the exchange
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
    /// What the first run's page says now: its question, what it waits on, its error.
    @Published private(set) var said = Said(question: nil, status: nil, error: nil)
    /// The first run's page, as the generated WelcomeView reads it.
    let first = FirstRunScreen()
    /// money's pages of what it is show (over the app, when their route asked).
    @Published private(set) var introShown = false
    /// "Scan a Code" asked: the camera's code scanner up, its text to [scanned].
    @Published var scanning = false
    @Published private(set) var snapshot = Snapshot()
    /// The month the budget, the spending and the month's figures are of: `2026-10`.
    @Published private(set) var month: String
    /// The last refusal, rejection or word from the bank, said once.
    @Published var notice: String? = nil
    /// The sign-in to open in the browser sheet, once: the view runs it and answers [signInReturned].
    @Published var browse: URL? = nil
    /// Plaid's page, once the server has answered an ask with it: Settings
    /// shows it in Safari's sheet and answers [bankPageClosed].
    @Published private(set) var bankPage: URL? = nil
    /// The answered ask put away from Settings.
    @Published private(set) var dismissedAsk: Id? = nil
    /// Bumped whenever a view a screen holds moved, or was first held: the
    /// screens read it again.
    @Published private(set) var moves = 0

    /// The views, held and kept by the core: what a screen holding one
    /// (ArkUI's `.holding`) reads it from.
    private(set) var session: Session? = nil
    /// The Accounts tab's runtime (Screens.swift), while a session is open.
    @Published private(set) var accounts: AccountsScreen? = nil
    /// The Budget tab, running in Rust (money/app/src/budget) once a session is open.
    @Published private(set) var budget: BudgetScreen? = nil
    /// The shell's runtime (Screens.swift): the app's for its life, running
    /// while a session is open.
    let shell = MoneyShellScreen()
    /// The phone's device modules (money/app.toml), given every screen run in Rust.
    var devices: Devices? = nil
    /// A receipt being given, in Rust, while a session is open.
    private(set) var receipts: ReceiptsScreen? = nil
    /// The Transactions tab, running in Rust (money/app/src/transactions).
    @Published private(set) var transactions: TransactionsScreen? = nil
    /// Told which views moved, on the main queue: the app's `Watched`.
    var onMoved: ([MoneyView]) -> Void = { _ in }

    // MARK: what it holds

    private let machine: FirstRunApp
    private let kept: Kept
    private let root: URL
    /// The demo's day when it is a fixed one (previews, tests, screenshots); today when none.
    private let demoDay: String?
    /// Opened on the demo by the launch (`-demo`, the screenshots'): the
    /// demo for this launch alone, whatever sign-in is remembered, which
    /// it leaves as it was for the next.
    private let demoLaunch: Bool
    private var bridge: Bridge? = nil
    /// How a session's snapshot reaches the model: the main queue in the app.
    private let deliver: (@escaping () -> Void) -> Void
    /// When this phone asked for a Plaid page it has not opened yet.
    private var awaiting: Id?? = nil

    /// The model over `shelf` (the app's `UserDefaults`), replicas under
    /// `root`; on the demo for this launch alone when `demoLaunch`.
    init(shelf: Shelf, root: URL, demoDay: String? = nil, demoLaunch: Bool = false, scan: Bool = false, intro: Bool = true,
         deliver: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) }) {
        machine = FirstRunApp(app: .money, scan: scan, demo: true)
        kept = Kept(shelf)
        self.root = root
        self.demoDay = demoDay
        self.demoLaunch = demoLaunch
        self.deliver = deliver
        step = machine.step()
        month = Months.of(demoDay ?? moneyToday())
        let remembered = demoLaunch ? Remembered(server: nil, login: nil, demo: true) : kept.remembered
        first.press = { [weak self] m in self?.press(m) }
        // A screenshot's launch, or the demo's, goes past money's pages, as
        // a returning person's would; their route shows them (`money://welcome`).
        if !intro || demoLaunch { machine.skipIntro() }
        send(.launched(remembered: remembered, code: nil, returned: false))
    }

    // MARK: the first run

    /// An event from a screen, the browser sheet, a link or the app's phase.
    func send(_ e: Event) {
        if case .foreground = e { bridge?.dialNow() }
        let effects = machine.event(event: e)
        moved()
        for e in effects { carry(e) }
    }

    /// A press on the first run's page.
    func press(_ m: WelcomeMsg) {
        let asked = machine.send(msg: m)
        moved()
        if asked.scan { scanning = true }
        for e in asked.effects { carry(e) }
    }

    /// What the code scanner read (none: closed).
    func scanned(_ text: String?) {
        scanning = false
        let effects = machine.scanned(text: text)
        moved()
        for e in effects { carry(e) }
    }

    private func moved() {
        step = machine.step()
        said = machine.said()
        first.page = machine.page()
        introShown = first.page.intro != nil
    }

    private func carry(_ e: Effect) {
        switch e {
        case let .findServer(typed):
            let domain = moneyDomain()
            Task {
                let found = await Probe.find(typed, domain: domain, get: Probe.get)
                self.send(.checked(found: Found(found)))
            }
        case let .remember(r): if !demoLaunch { kept.remembered = r }
        case let .browse(_, url): browse = URL(string: url)
        case let .exchange(server, code): trade { try exchange(server: server, code: code) }
        case let .redeem(server, code): trade { try redeem(server: server, code: code) }
        case let .openSession(server, login): open(.server(server: server, login: login, root: root))
        case .openDemo: open(.demo(today: demoDay))
        case .closeSession: close()
        }
    }

    // A code traded off the main thread: the login, or the sentence why not.
    private func trade(_ f: @escaping @Sendable () throws -> Login) {
        Task {
            let answer: Event = await Task.detached {
                do { return Event.signedIn(login: try f()) } catch { return Event.signInFailed(why: why(error)) }
            }.value
            self.send(answer)
        }
    }

    /// What the browser sheet came back with: the redirect, or why there
    /// was none (none for closed).
    func signInReturned(_ url: URL?, error: String?) {
        browse = nil
        if let u = url, let code = Probe.code(u, scheme: AppInfo.money.scheme) { return send(.code(code: code)) }
        send(.cameBack(why: url == nil ? error : "it came back without a code"))
    }

    /// A link the system opened the app with, or a code scanned.
    /// `money://auth?code=…` is a sign-in coming back. Any other link of this
    /// app's is a route (the domain's `Route`): `money://join?…`, a server and
    /// a code to join with, goes to the sign-in machine (`Event.joined`,
    /// asked about first); the rest is the shell's to navigate to ([link]).
    /// A join link of another scheme (a page's `https://…/join?…`) joins too.
    func open(url: URL) {
        if let code = Probe.code(url, scheme: AppInfo.money.scheme) { return send(.code(code: code)) }
        if case .welcome? = Navigator<Route>.route(of: url, scheme: AppInfo.money.scheme, parse: { routeFromUrl(url: $0) }) {
            machine.showIntro()
            return moved()
        }
        // Any other link goes straight where it says: past money's pages.
        machine.skipIntro()
        moved()
        if let args = Navigator<Route>.launchArgs(of: url, scheme: AppInfo.money.scheme) {
            launchLink = args
            return
        }
        if let route = Navigator<Route>.route(of: url, scheme: AppInfo.money.scheme, parse: { routeFromUrl(url: $0) }) {
            if case let .join(server, code) = route { return send(.joined(server: server, code: code)) }
            link = url
            return
        }
        if let j = joinOf(link: url.absoluteString) { send(.joined(server: j.server, code: j.code)) }
    }

    /// A link the system opened the app with that names a page
    /// (`money://…`), for the shell to navigate to (`Navigator.handle`);
    /// nil again once it has.
    @Published var link: URL? = nil

    /// A launch link's arguments (`money://?screen=…`, ui/docs/shots.md),
    /// for the shell to take as a launch while the app runs; nil again once
    /// it has.
    @Published var launchLink: [String]? = nil

    /// What a link names that is not a page: a sign-in link (`money://join?…`)
    /// is the sign-in machine's ([open(url:)]), and is not navigated to.
    func takes(_ route: Route) -> Bool {
        if case .join = route { return true }
        if case .welcome = route { return true }
        return false
    }

    // MARK: the session

    private func open(_ o: Opening) {
        close()
        bridge = Bridge(o, month: month, deliver: deliver,
                        publish: { [weak self] s in self?.took(s) },
                        notice: { [weak self] n in self?.notice = n },
                        denied: { [weak self] d in self?.send(.denied(reason: d)) },
                        failed: { [weak self] w in self?.failed(w) },
                        opened: { [weak self] s in self?.opened(s) },
                        moved: { [weak self] views in
                            self?.moves += 1
                            self?.accounts?.reload()
                            self?.budget?.reload()
                            self?.transactions?.reload()
                            self?.shell.reload()
                            self?.onMoved(views)
                        })
    }

    private func opened(_ s: Session) {
        session = s
        accounts = AccountsScreen(session: s, devices: devices,
                                  refresh: { [weak self] in Task { await self?.refresh() } },
                                  today: { [weak self] in self?.today ?? moneyToday() })
        budget = BudgetScreen(session: s, devices: devices, month: month,
                              refresh: { [weak self] in Task { await self?.refresh() } },
                              today: { [weak self] in self?.today ?? moneyToday() })
        let r = ReceiptsScreen(session: s, devices: devices,
                               refresh: { [weak self] in Task { await self?.refresh() } },
                               today: { [weak self] in self?.today ?? moneyToday() })
        r.keep = { [weak self] jpeg, txn in self?.addPhoto(jpeg, txn: txn) }
        r.say = { [weak self] why in self?.notice = why }
        receipts = r
        transactions = TransactionsScreen(session: s, devices: devices, month: month,
                                          refresh: { [weak self] in Task { await self?.refresh() } },
                                          today: { [weak self] in self?.today ?? moneyToday() })
        shell.connect(session: s,
                      refresh: { [weak self] in Task { await self?.refresh() } },
                      today: { [weak self] in self?.today ?? moneyToday() })
    }

    private func close() {
        bridge?.close()
        bridge = nil
        accounts = nil
        budget = nil
        transactions = nil
        receipts = nil
        shell.disconnect()
        session = nil
        snapshot = Snapshot()
    }

    private func failed(_ why: String) {
        send(.openFailed(why: why))
    }

    private func took(_ s: Snapshot) {
        snapshot = s
        // What is drawn may have moved: a screenshot waits again (ArkUI's `Ready`).
        Ready.shared.changed()
        followAsk()
        // Signed in just now: opening, until the replica has what the server had —
        // or the server cannot be reached, and there is nothing more to wait for.
        if case .opening = step, let st = s.status, st.linked || st.cursor > 0 || st.link == "waiting to dial" || st.link == "offline" {
            send(.synced)
        }
    }

    /// Wait for the session to have done everything asked of it: a test's.
    func settle() { bridge?.settle() }

    /// Nothing the session asked for still on its way (no session: nothing
    /// to wait for): what a screenshot's ready signal asks.
    var settled: Bool { bridge == nil || snapshot.settled }

    // MARK: who and where

    var isDemo: Bool { if case .demo = step { return true } else { return false } }
    var server: String { machine.remembered().server ?? "" }
    var login: Login? {
        switch step {
        case let .running(_, l), let .opening(_, l): return l
        default: return nil
        }
    }
    /// What to call whoever is using the budget.
    var who: String { isDemo ? "Alex" : login.map { loginName(login: $0) } ?? "" }
    /// Their picture, as this phone loads it (`Faces.url`); empty for none, and for the demo.
    var picture: String { login.map { Faces.url(server: server, user: $0.user, picture: $0.picture) } ?? "" }

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
        budget?.send(.month(month: m))
        transactions?.send(.month(month: m))
    }

    func stepMonth(_ n: Int64) { showMonth(moneyAddMonths(month: month, n: n)) }
    func showThisMonth() { showMonth(thisMonth) }
    var monthName: String { formatMonth(month: month) }

    // MARK: what a screen holds

    /// `view` held for a screen until the answer is let go (ArkUI's
    /// `.holding`, through the app's `Watched`).
    func hold(_ view: MoneyView) -> Holding? {
        let h = session?.hold(view: view)
        moves += 1
        return h
    }

    /// `f` of the session's `view`, once something holds it.
    private func read<T>(_ view: MoneyView, _ f: (Session) -> T) -> T? {
        guard let s = session, s.version(view: view) > 0 else { return nil }
        return f(s)
    }

    /// `account`'s register (none: every account's), while a screen holds it.
    func register(_ account: Id?) -> [TxnLine]? { read(.register(account: account)) { $0.register(account: account) } }

    /// What a search finds in `account`'s register (none: every account's), while a screen holds it.
    func search(_ account: Id?, _ text: String) -> [TxnLine]? {
        read(.search(account: account, text: text)) { $0.search(account: account, text: text) }
    }

    /// What paid for `txn`, while a screen holds it.
    func paid(_ txn: Id) -> [ReceiptLine] {
        read(.paid(txn: txn)) { $0.paid(txn: txn) } ?? []
    }

    /// A receipt's items, while a screen holds them.
    func items(_ receipt: String) -> [ItemLine] {
        read(.items(receipt: receipt)) { $0.items(receipt: receipt) } ?? []
    }

    // MARK: the budget's buttons

    private func write(_ what: String, _ change: MoneyChange, done: @escaping (Bool) -> Void = { _ in }) {
        guard let b = bridge else { return done(false) }
        b.write(what, change, done: done)
    }

    /// Give a category exactly `cents` this month.
    func assign(_ id: Id, _ cents: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let m = month
        write("assign it", .assign(value: Assign(month: m, categoryId: id, amount: cents)), done: done)
    }

    /// Money moved between `line` and `other` (either none for Ready to Assign), as [Moves] says.
    func move(_ line: BudgetLine?, _ other: Id?, _ cents: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let mm = Moves.of(month: month, line: line, other: other, cents: cents)
        write("move the money", .moveMoney(value: mm), done: done)
    }

    /// The whole month (or `only` these) assigned at once.
    func autoAssign(_ how: Auto, only: [Id] = []) {
        let m = month
        write("auto-assign", .autoAssign(value: AutoAssign(month: m, how: how, only: only)))
    }

    func setTarget(_ id: Id, _ cents: Int64) { write("set the target", .setTarget(value: SetTarget(id: id, target: cents))) }
    func addGroup(_ name: String) { write("add the group", .addGroup(value: AddGroup(name: name))) }
    func addCategory(_ group: Id, _ name: String) { write("add the category", .addCategory(value: AddCategory(groupId: group, name: name))) }
    func rename(category id: Id, _ name: String) { write("rename it", .renameCategory(value: RenameCategory(id: id, name: name))) }
    func rename(group id: Id, _ name: String) { write("rename it", .renameGroup(value: RenameGroup(id: id, name: name))) }
    func hide(category id: Id, _ hidden: Bool) { write(hidden ? "hide it" : "show it", .hideCategory(value: HideCategory(id: id, hidden: hidden))) }
    func hide(group id: Id, _ hidden: Bool) { write(hidden ? "hide it" : "show it", .hideGroup(value: HideGroup(id: id, hidden: hidden))) }
    func delete(category id: Id, moveTo: Id?) { write("delete it", .deleteCategory(value: DeleteCategory(id: id, moveTo: moveTo))) }
    func delete(group id: Id, moveTo: Id?) { write("delete it", .deleteGroup(value: DeleteGroup(id: id, moveTo: moveTo))) }

    // MARK: transactions

    /// The form saved: a new transaction, or the one corrected.
    func save(_ f: TxnForm, done: @escaping (Bool) -> Void = { _ in }) {
        let d = f.sent
        guard let change = d.toChange() else {
            notice = d.problem() ?? "Couldn't save it."
            return done(false)
        }
        write(d.editing == nil ? "add it" : "save it", change, done: done)
    }

    func delete(_ ids: [Id]) { write("delete it", .deleteTransactions(value: Txns(ids: ids))) }
    func approve(_ ids: [Id]) { write("approve it", .approve(value: Txns(ids: ids))) }
    /// Given a category, which is looking at them: they are approved with it.
    func categorize(_ ids: [Id], _ category: Id?) { write("categorize it", .categorize(value: Categorize(ids: ids, categoryId: category))) }
    func setCleared(_ id: Id, _ cleared: Bool) { write("mark it", .setCleared(value: SetCleared(id: id, cleared: cleared))) }

    // MARK: receipts

    /// A file's bytes, once the phone has them; asked for when it does not.
    func file(_ f: FileRef) -> Data? {
        if let d = snapshot.files[f.hash] { return d }
        bridge?.want(file: f)
        return nil
    }

    /// A photo of a receipt kept and sent to be read, for `txn` when it was taken for one.
    func addPhoto(_ jpeg: Data, txn: Id?, done: @escaping (Bool) -> Void = { _ in }) {
        guard let b = bridge else { return done(false) }
        b.addPhoto(jpeg, txn: txn, done: done)
    }

    /// A receipt typed by hand, or one read put right.
    func save(_ r: ReceiptDraft, done: @escaping (Bool) -> Void = { _ in }) {
        if let problem = r.problem {
            notice = problem
            return done(false)
        }
        write("save the receipt", .writeReceipt(value: r.write), done: done)
    }

    /// A receipt said to be what paid for `txn`; none, for nothing.
    func pay(receipt: String, with txn: Id?) {
        write(txn == nil ? "take the receipt off" : "match the receipt", .pay(value: MatchReceipt(receiptId: receipt, txnId: txn)))
    }

    // MARK: accounts

    func openAccount(name: String, kind: AccountKind, onBudget: Bool, balance: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let input = OpenAccount(name: name, kind: kind, onBudget: onBudget, balance: balance, date: today)
        write("add the account", .openAccount(value: input), done: done)
    }

    func rename(account id: Id, _ name: String) { write("rename it", .renameAccount(value: RenameAccount(id: id, name: name))) }
    func close(account id: Id, _ closed: Bool) { write(closed ? "close it" : "reopen it", .closeAccount(value: CloseAccount(id: id, closed: closed))) }

    /// The account agreed with the bank at `balance`: an adjustment for any difference, and what was cleared locked.
    func reconcile(_ id: Id, balance: Int64, done: @escaping (Bool) -> Void = { _ in }) {
        let d = today
        write("reconcile it", .reconcile(value: Reconcile(accountId: id, balance: balance, date: d)), done: done)
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

    // MARK: connecting banks

    /// Whether a bank can be connected: through a server's importer, which
    /// the demo has none of.
    var connectsBanks: Bool { !isDemo }

    /// Connect a bank: the server asked for Plaid's page, which opens as
    /// soon as it answers with it ([bankPage]).
    func connectBank() { askForPage("connect a bank", .connect) }

    /// Log in to a bank again, on Plaid's page in update mode.
    func reconnect(bank: String) { askForPage("reconnect the bank", .reconnect(value: BankName(bank: bank))) }

    /// Plaid forgets the login and the bank stops importing; its accounts here stay.
    func disconnect(bank: String) {
        guard connectsBanks else { notice = "The demo has no server to disconnect a bank from."; return }
        dismissedAsk = nil
        write("disconnect the bank", .disconnect(value: BankName(bank: bank)))
    }

    /// A bank's account given an account of its own here, of the kind it is.
    func openAccount(for plaid: String) {
        write("open an account for it", .openLinked(value: OpenLinked(plaidAccountId: plaid, name: ""))) { [weak self] ok in
            if ok { self?.notice = "Opened. Its history comes with the bank's next import." }
        }
    }

    /// A bank's account made to feed an account already here.
    func link(_ plaid: String, to account: Id) {
        write("link it", .link(value: LinkAccount(plaidAccountId: plaid, accountId: account))) { [weak self] ok in
            if ok { self?.notice = "Linked. The bank's next import writes there." }
        }
    }

    /// A bank's account left out, or followed again.
    func ignore(_ plaid: String, _ ignored: Bool) {
        write(ignored ? "ignore it" : "follow it", .ignore(value: IgnoreLinked(plaidAccountId: plaid, ignored: ignored)))
    }

    /// Plaid's page opened again, from the ask in Settings.
    func openPage(_ a: AskLine) { bankPage = URL(string: a.url) }

    /// Safari's sheet closed, by its Done or because the ask was answered.
    func bankPageClosed() { bankPage = nil }

    /// The answered ask put away from Settings.
    func dismiss(ask: AskLine) { dismissedAsk = ask.id }

    /// An ask for a Plaid page: watched from now until the server answers it.
    private func askForPage(_ what: String, _ change: MoneyChange) {
        guard connectsBanks else { notice = "The demo has no server to connect a bank through."; return }
        // The newest ask before this one: the core follows the next by its id, not a clock.
        awaiting = .some(snapshot.asks.first?.id)
        dismissedAsk = nil
        write(what, change) { [weak self] ok in if !ok { self?.awaiting = nil } }
    }

    /// The asks moved: Plaid's page opened once its address arrives, and
    /// closed again when the server says the ask is over — the person
    /// logged in and the server finished, or the page ran out.
    private func followAsk() {
        if let page = bankPage, let a = snapshot.asks.first(where: { $0.url == page.absoluteString }), !a.isOpen() {
            bankPage = nil
        }
        guard let since = awaiting else { return }
        switch follow(asks: snapshot.asks, since: since) {
        case .wait: break
        case let .page(url):
            awaiting = nil
            bankPage = URL(string: url)
        case .over: awaiting = nil
        }
    }
}
