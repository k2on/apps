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
    /// Add a Device's page up, as Settings asked (`MoneyRequest.addDevice`).
    @Published var addingDevice = false
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
    /// The Transactions tab, running in Rust (money/app/src/transactions).
    @Published private(set) var transactions: TransactionsScreen? = nil
    /// Insights, running in Rust (money/app/src/insights).
    @Published private(set) var insights: InsightsScreen? = nil
    /// The sheets over the app, running in Rust (money/app/src/sheets).
    @Published private(set) var sheets: SheetsScreen? = nil
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

    /// The model over `shelf` (the app's `UserDefaults`) and `secure` (the
    /// keychain, where the login is), replicas under `root`; on the demo for
    /// this launch alone when `demoLaunch`.
    init(shelf: Shelf, secure: SecureStore?, root: URL, demoDay: String? = nil, demoLaunch: Bool = false, scan: Bool = false, intro: Bool = true,
         deliver: @escaping (@escaping () -> Void) -> Void = { DispatchQueue.main.async(execute: $0) }) {
        machine = FirstRunApp(app: .money, scan: scan, demo: true)
        kept = Kept(shelf, secure: secure)
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
    /// asked about first); the rest is the shell's to navigate to ([takeLink]).
    /// A join link of another scheme (a page's `https://…/join?…`) joins too.
    func open(url: URL) {
        if let code = Probe.code(url, scheme: AppInfo.money.scheme) { return send(.code(code: code)) }
        if case .welcome? = Navigator<Route>.route(of: url, scheme: AppInfo.money.scheme, parse: { routeFromUrl(url: $0) }) {
            machine.showIntro()
            return moved()
        }
        if case let .join(server, code)? = Navigator<Route>.route(of: url, scheme: AppInfo.money.scheme, parse: { routeFromUrl(url: $0) }) {
            machine.skipIntro()
            moved()
            return send(.joined(server: server, code: code))
        }
        if url.scheme == AppInfo.money.scheme {
            // A page or a launch (`money://?screen=…`) of the app's: past money's
            // pages, kept by the first run until the app shows (`FirstRun::open_link`),
            // so one that came while the pages or the sign-in showed is not lost.
            machine.openLink(url: url.absoluteString)
            moved()
            links += 1
            return
        }
        if let j = joinOf(link: url.absoluteString) {
            machine.skipIntro()
            moved()
            send(.joined(server: j.server, code: j.code))
        }
    }

    /// Grows as a link for the app itself is kept: the shell takes it
    /// ([takeLink]) as it shows, and as this grows while it does.
    @Published private(set) var links = 0

    /// The link kept for the app, once the app itself shows; once.
    func takeLink() -> URL? { machine.takeLink().flatMap { URL(string: $0) } }

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
                            self?.sheets?.reload()
                            self?.insights?.reload()
                            self?.shell.reload()
                            self?.onMoved(views)
                        })
    }

    private func opened(_ s: Session) {
        session = s
        accounts = AccountsScreen(session: s,
                                  refresh: { [weak self] in Task { await self?.refresh() } },
                                  today: { [weak self] in self?.today ?? moneyToday() })
        budget = BudgetScreen(session: s, month: month,
                              refresh: { [weak self] in Task { await self?.refresh() } },
                              today: { [weak self] in self?.today ?? moneyToday() })
        transactions = TransactionsScreen(session: s, month: month,
                                          refresh: { [weak self] in Task { await self?.refresh() } },
                                          today: { [weak self] in self?.today ?? moneyToday() })
        insights = InsightsScreen(session: s, month: month,
                                  refresh: { [weak self] in Task { await self?.refresh() } },
                                  today: { [weak self] in self?.today ?? moneyToday() })
        sheets = SheetsScreen(session: s, month: month,
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
        sheets = nil
        insights = nil
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
    /// Who is signed in where, as Settings says it (money_app's SessionFacts).
    var facts: SessionFacts {
        let i = Bundle.main.infoDictionary
        let version = "\(i?["CFBundleShortVersionString"] as? String ?? "?") (\(i?["CFBundleVersion"] as? String ?? "?"))"
        return SessionFacts(who: who, login: login.map { $0.email.isEmpty ? $0.user : $0.email } ?? "", demo: isDemo,
                            server: isDemo ? "" : Probe.display(server), linked: snapshot.status.map { $0.linked },
                            pending: Int64(snapshot.status?.pending ?? 0), version: version, leaves: true, addsDevices: !isDemo)
    }
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
        sheets?.send(.month(month: m, today: today))
        insights?.send(.month(month: m))
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
