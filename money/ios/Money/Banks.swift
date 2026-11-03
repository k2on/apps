// Connecting banks, as Settings needs it, and nothing of Apple's but
// Foundation (`money_domain::banks`, money/README.md's banks): a bank's
// login is the server's alone, so the phone only asks — `banks.connect`,
// `banks.reconnect`, `banks.disconnect` — and the server's importer
// answers each ask in `banks.asks`: Plaid's page to open while it waits,
// then done, failed with a sentence, or expired. The page does not come
// back to the app; the server finishes the connection by asking Plaid, so
// the app only watches the ask. A new bank's accounts then wait for
// somebody to say what each feeds: an account opened for it, one already
// here, or nothing. Here are what to open and when, what each ask and
// each bank says, and which accounts a bank's may feed — the desktop's
// (`money/iced/src/banks.rs`), held by `MoneyTests`.
import Foundation

/// What the ask a phone is waiting on asks of it now.
enum Following: Equatable {
    /// Not answered yet: keep watching.
    case waiting
    /// Plaid's page, to open once.
    case open(URL)
    /// Answered otherwise — done, failed, expired: nothing to open.
    case answered
}

enum BankAsks {
    /// How long an answered ask stays in Settings.
    static let shownMs: Int64 = 15 * 60 * 1000
    /// How long an ask goes unanswered before Settings wonders why.
    static let quietMs: Int64 = 30 * 1000

    /// The newest of the reader's asks made since `since` (a second's
    /// grace for the clocks), and what it wants: the desktop's `follow_ask`.
    static func follow(_ asks: [AskLine], since: Int64) -> Following {
        guard let a = asks.first, a.askedMs >= since - 1_000 else { return .waiting }
        switch a.state {
        case .asked: return .waiting
        case .waiting:
            guard let url = URL(string: a.url), !a.url.isEmpty else { return .waiting }
            return .open(url)
        case .done, .failed, .expired: return .answered
        }
    }

    /// Whether an ask is still the server's or Plaid's to finish.
    static func isOpen(_ a: AskLine) -> Bool { a.state == .asked || a.state == .waiting }

    /// The ask Settings shows: the newest, while it is open or lately
    /// answered, unless it was put away.
    static func shown(_ asks: [AskLine], now: Int64, dismissed: Id?) -> AskLine? {
        guard let a = asks.first, a.id != dismissed else { return nil }
        return isOpen(a) || now - a.answeredMs < shownMs ? a : nil
    }

    /// The bank an ask is about, as a sentence names it.
    static func name(_ a: AskLine) -> String {
        if !a.institution.isEmpty { return a.institution }
        return a.bank.isEmpty ? "the bank" : a.bank.capitalized
    }

    /// What Settings says of an ask.
    static func words(_ a: AskLine, now: Int64) -> String {
        let who = name(a)
        switch a.state {
        case .asked where now - a.askedMs > quietMs: return "The server hasn't answered yet. Does it have Plaid's keys?"
        case .asked: return a.what == "disconnect" ? "Asking the server to disconnect \(who)…" : "Asking the server for Plaid's page…"
        case .waiting: return "Log in to your bank on Plaid's page. This finishes by itself once you have."
        case .done:
            switch a.what {
            case "connect": return "Connected \(who). Choose what each of its accounts feeds."
            case "reconnect": return "Logged in to \(who) again."
            default: return "Disconnected \(who). Its accounts stay, kept by hand from now on."
            }
        case .failed: return a.message.isEmpty ? "Couldn't \(a.what) \(who)." : "Couldn't \(a.what) \(who): \(a.message)"
        case .expired: return a.message.isEmpty ? "Plaid's page closed before it was finished." : a.message
        }
    }

    /// How long Plaid's page stays open, in whole minutes; none once it has closed.
    static func minutesLeft(_ a: AskLine, now: Int64) -> Int64? {
        guard a.state == .waiting, a.expiresMs > now else { return nil }
        return (a.expiresMs - now) / 60_000
    }
}

/// How a connected bank is doing, as its row says.
enum BankHealth: Equatable {
    /// Its login ran out: Reconnect, not a retry, mends it.
    case needsLogin
    /// Its last import failed, with what Plaid said.
    case failed(String)
    /// Connected, and not imported yet.
    case new
    /// Imported, last at this time.
    case synced(Int64)
}

enum Banks {
    /// What a bank is called: the institution's name, or the name the server keeps it under.
    static func title(_ b: BankLine) -> String { b.institution.isEmpty ? b.name.capitalized : b.institution }

    static func health(_ b: BankLine) -> BankHealth {
        if b.needsLogin { return .needsLogin }
        if !b.error.isEmpty { return .failed(b.error) }
        return b.syncedMs == 0 ? .new : .synced(b.syncedMs)
    }

    /// What wants somebody in Settings: logins run out, and accounts
    /// waiting for a choice.
    static func wanting(_ banks: [BankLine]) -> Int { banks.reduce(0) { $0 + Int($1.waiting) + ($1.needsLogin ? 1 : 0) } }

    /// A bank's accounts, those waiting for a choice first.
    static func accounts(of bank: String, _ linked: [LinkedLine]) -> [LinkedLine] {
        let mine = linked.filter { $0.bank == bank }
        return mine.filter(\.waiting) + mine.filter { !$0.waiting }
    }

    /// A bank's account as its row names it: `Sapphire ••4321`.
    static func name(_ l: LinkedLine) -> String { l.mask.isEmpty ? l.name : "\(l.name) ••\(l.mask)" }

    /// What kind of account the bank says it is: its subtype, or its kind.
    static func kind(_ l: LinkedLine) -> String { (l.subtype.isEmpty ? l.kind : l.subtype).capitalized }

    /// What the choice to open an account for it says: "New Credit Card Account".
    static func opens(_ l: LinkedLine) -> String { "New \(Accounts.name(ofKind: l.opensAs)) Account" }

    /// The accounts here a bank's account may feed: open, and fed by no
    /// bank yet; those of the kind it would open as first, then by name.
    static func linkable(_ l: LinkedLine, _ accounts: [AccountLine]) -> [AccountLine] {
        accounts.filter { !$0.closed && $0.bank.isEmpty }
            .sorted { ($0.kind == l.opensAs ? 0 : 1, $0.pos, $0.name) < ($1.kind == l.opensAs ? 0 : 1, $1.pos, $1.name) }
    }
}

/// Now, as the domain counts time: milliseconds since 1970.
func nowMs(_ date: Date = Date()) -> Int64 { Int64((date.timeIntervalSince1970 * 1000).rounded()) }
