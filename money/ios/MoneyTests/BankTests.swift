// Connecting banks as Settings draws it (`money_domain::banks`): the ask
// followed until Plaid's page arrives and opened once, what an ask and a
// bank say, and a bank's accounts with what each may feed. None of it
// needs a server: the asks and the banks are rows made by hand.
@testable import Money
import Foundation
import XCTest

final class BankTests: XCTestCase {
    /// Nothing is opened before the server answers, nor an older ask's
    /// page; the new ask's page is opened once it has an address, and an
    /// ask answered otherwise ends the wait. Falsified by following any
    /// ask that waits: the stale page of an earlier one opened again.
    func testAnAskIsFollowedUntilItsPageArrives() {
        let since: Int64 = 1_000_000
        let page = "https://hosted.plaid.test/link-1"
        XCTAssertEqual(BankAsks.follow([], since: since), .waiting)
        XCTAssertEqual(BankAsks.follow([Fx.ask(1, state: .asked, asked: since)], since: since), .waiting)
        XCTAssertEqual(BankAsks.follow([Fx.ask(1, state: .waiting, asked: since)], since: since), .waiting, "no address yet")
        XCTAssertEqual(BankAsks.follow([Fx.ask(1, state: .waiting, url: page, asked: since - 60_000)], since: since), .waiting,
                       "an earlier ask's page")
        // The clocks a moment apart still find it.
        XCTAssertEqual(BankAsks.follow([Fx.ask(2, state: .waiting, url: page, asked: since - 500),
                                        Fx.ask(1, state: .waiting, url: "https://old", asked: since - 60_000)], since: since),
                       .open(URL(string: page)!))
        for state in [AskState.done, .failed, .expired] {
            XCTAssertEqual(BankAsks.follow([Fx.ask(1, state: state, asked: since)], since: since), .answered)
        }
    }

    /// What Settings says of each ask, in the domain's terms: the page to
    /// log in on, how long it stays open, and what came of it.
    func testWhatAnAskSays() {
        let now: Int64 = 10_000_000
        let asked = Fx.ask(1, state: .asked, asked: now - 5_000)
        XCTAssertEqual(BankAsks.words(asked, now: now), "Asking the server for Plaid's page…")
        XCTAssertEqual(BankAsks.words(asked, now: now + 60_000), "The server hasn't answered yet. Does it have Plaid's keys?")
        let waiting = Fx.ask(1, state: .waiting, url: "https://p", asked: now, expires: now + 10 * 60_000 + 5_000)
        XCTAssertEqual(BankAsks.minutesLeft(waiting, now: now), 10)
        XCTAssertEqual(BankAsks.minutesLeft(waiting, now: now + 10 * 60_000), 0)
        XCTAssertNil(BankAsks.minutesLeft(waiting, now: now + 11 * 60_000))
        XCTAssertNil(BankAsks.minutesLeft(asked, now: now))
        XCTAssertEqual(BankAsks.words(Fx.ask(1, state: .done, institution: "Wells Fargo"), now: now),
                       "Connected Wells Fargo. Choose what each of its accounts feeds.")
        XCTAssertEqual(BankAsks.words(Fx.ask(1, "reconnect", state: .done, institution: "Chase"), now: now), "Logged in to Chase again.")
        XCTAssertEqual(BankAsks.words(Fx.ask(1, "disconnect", state: .asked, institution: "Chase", asked: now), now: now),
                       "Asking the server to disconnect Chase…")
        XCTAssertEqual(BankAsks.words(Fx.ask(1, state: .failed, message: "Plaid said no"), now: now), "Couldn't connect the bank: Plaid said no")
        XCTAssertEqual(BankAsks.words(Fx.ask(1, state: .expired), now: now), "Plaid's page closed before it was finished.")
        XCTAssertEqual(BankAsks.words(Fx.ask(1, state: .expired, message: "Nobody finished the page."), now: now), "Nobody finished the page.")
    }

    /// The newest ask is shown while it is open and for a while after it is
    /// answered, unless it was put away; an older one never.
    func testAnAskIsShownForAWhile() {
        let now: Int64 = 100_000_000
        let open = Fx.ask(2, state: .waiting, url: "https://p", asked: now - 3_600_000)
        let done = Fx.ask(2, state: .done, answered: now - 60_000)
        let old = Fx.ask(2, state: .done, answered: now - BankAsks.shownMs - 1)
        XCTAssertEqual(BankAsks.shown([open], now: now, dismissed: nil)?.id, Fx.id(2))
        XCTAssertEqual(BankAsks.shown([done, Fx.ask(1, state: .waiting)], now: now, dismissed: nil)?.state, .done)
        XCTAssertNil(BankAsks.shown([old], now: now, dismissed: nil))
        XCTAssertNil(BankAsks.shown([done], now: now, dismissed: Fx.id(2)))
        XCTAssertNil(BankAsks.shown([], now: now, dismissed: nil))
        XCTAssertTrue(BankAsks.isOpen(open))
        XCTAssertFalse(BankAsks.isOpen(done))
    }

    /// A login run out comes before a failure, which comes before when it
    /// synced; Settings is marked for each login and each account waiting.
    func testABankSaysHowItIs() {
        XCTAssertEqual(Banks.health(Fx.bank("chase", error: "ITEM_LOGIN_REQUIRED", needsLogin: true)), .needsLogin)
        XCTAssertEqual(Banks.health(Fx.bank("chase", synced: 5, error: "the bank is down")), .failed("the bank is down"))
        XCTAssertEqual(Banks.health(Fx.bank("chase")), .new)
        XCTAssertEqual(Banks.health(Fx.bank("chase", synced: 5)), .synced(5))
        XCTAssertEqual(Banks.title(Fx.bank("chase")), "Chase")
        XCTAssertEqual(Banks.title(Fx.bank("wells", institution: "Wells Fargo")), "Wells Fargo")
        XCTAssertEqual(Banks.wanting([Fx.bank("a", needsLogin: true, waiting: 2), Fx.bank("b"), Fx.bank("c", waiting: 1)]), 4)
        XCTAssertEqual(Banks.wanting([]), 0)
    }

    /// A bank's accounts, those waiting for a choice first; the choice to
    /// open one names the kind it would be; and what one may feed is an
    /// open account no bank feeds, those of its kind first.
    func testABanksAccountsAndWhatEachMayFeed() {
        let checking = Fx.account(1, "Checking", pos: 1)
        let visa = Fx.account(2, "Visa", kind: "credit", pos: 2)
        let accounts = [checking, visa, Fx.account(3, "Old Card", kind: "credit", closed: true),
                        Fx.account(4, "Chase Sapphire", kind: "credit", bank: "chase"), Fx.account(5, "Cash", kind: "cash", pos: 3)]
        let fed = Fx.linked("p1", name: "Everyday Checking", mask: "1234", feeds: checking)
        let card = Fx.linked("p2", name: "Signature Visa", mask: "4321", kind: "credit", subtype: "credit card", waiting: true, opensAs: "credit")
        let other = Fx.linked("p3", bank: "chase", name: "Sapphire", kind: "credit", opensAs: "credit")
        XCTAssertEqual(Banks.accounts(of: "wells", [fed, other, card]).map(\.plaidAccountId), ["p2", "p1"])
        XCTAssertEqual(Banks.name(card), "Signature Visa ••4321")
        XCTAssertEqual(Banks.name(other), "Sapphire")
        XCTAssertEqual(Banks.kind(card), "Credit Card")
        XCTAssertEqual(Banks.kind(fed), "Depository")
        XCTAssertEqual(Banks.opens(card), "New Credit Card Account")
        XCTAssertEqual(Banks.opens(fed), "New Checking Account")
        XCTAssertEqual(Banks.linkable(card, accounts).map(\.name), ["Visa", "Checking", "Cash"])
        XCTAssertEqual(Banks.linkable(fed, accounts).map(\.name), ["Checking", "Visa", "Cash"])
    }
}
