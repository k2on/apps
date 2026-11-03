// What the receipt screens need decided, and nothing of Apple's but
// Foundation: where a transaction's receipt stands (read and sure, read
// and to check, still being read, or none), which of the receipts nobody
// matched could be what it paid for, nearest first, and the form a receipt
// is typed or put right on — its lines as people type amounts, and what the
// domain is given (`receipts.write_receipt`). The desktop's rules
// (`money/iced`), so the phone and the window agree; held by `MoneyTests`.
import ArkUICore
import Foundation

/// Where a transaction's receipt stands.
enum ReceiptState: Equatable {
    /// No receipt, and no photo of one.
    case none
    /// A photo taken, the server still reading it (or trying again: `why`).
    case reading(why: String)
    /// A receipt: read from its photo and adding up (`check` false), or not.
    case receipt(ReceiptLine, check: Bool)

    var hasReceipt: Bool {
        if case .receipt = self { return true }
        return false
    }
}

enum Receipts {
    /// The media a scanned or chosen photo is kept as.
    static let jpeg = "image/jpeg"

    /// A transaction's receipt, from what paid it (`receipts.for_txn`) and
    /// the photos taken for it: the first receipt, said to check when the
    /// photo it was read from did not add up; else a photo still reading.
    static func state(txn: Id, receipts: [ReceiptLine], photos: [PhotoLine]) -> ReceiptState {
        if let r = receipts.first {
            let check = photos.contains { $0.receiptId == r.id && $0.state == "check" }
            return .receipt(r, check: check)
        }
        if let p = photos.first(where: { $0.txnId == txn && $0.receiptId.isEmpty }) {
            return .reading(why: p.state == "failed" ? p.why : "")
        }
        return .none
    }

    /// The photo a receipt was read from, when it was.
    static func photo(of receipt: String, _ photos: [PhotoLine]) -> PhotoLine? {
        photos.first { $0.receiptId == receipt }
    }

    /// Whether a register row shows the receipt mark.
    static func marked(_ t: TxnLine) -> Bool { !t.receipt.isEmpty || t.photos > 0 }

    /// The receipts nobody has matched (no transaction, no charges), the
    /// nearest to a transaction of `amount` on `day` first — in amount, then
    /// in days — and at most `limit`.
    static func unmatched(_ all: [ReceiptLine], amount: Int64, day: String, limit: Int = 8) -> [ReceiptLine] {
        let free = all.filter { $0.txnId == nil && $0.charges == 0 }
        return Array(free.sorted { a, b in
            let x = ((a.total - amount).magnitude, days(a.date, day)), y = ((b.total - amount).magnitude, days(b.date, day))
            return x != y ? x < y : a.id < b.id
        }.prefix(limit))
    }

    /// How many days apart two days are, either way; far for one that is not a day.
    static func days(_ a: String, _ b: String) -> Int {
        guard let x = Days.date(a), let y = Days.date(b) else { return Int.max }
        return abs(Calendar.current.dateComponents([.day], from: x, to: y).day ?? Int.max)
    }

    /// Who the receipt is from, as its card heads it.
    static func heading(_ r: ReceiptLine) -> String {
        if !r.place.isEmpty { return r.place.uppercased() }
        return r.store.uppercased()
    }
}

/// One line of the receipt form: a name and what was paid for it, as typed.
struct PaperItem: Identifiable, Equatable {
    var id = UUID()
    var name = ""
    /// Dollars and cents as typed, `4.29`; a discount below zero, `-0.50`.
    var amount = ""
    var sku = ""
    var quantity: Int64 = 1000
    var unit = "ea"
}

/// A receipt typed by hand, or one read put right.
struct ReceiptDraft: Equatable {
    /// The receipt put right; blank for a new one.
    var receiptId = ""
    var txn: Id? = nil
    var place = ""
    var date: String
    var items = [PaperItem()]

    init(txn: Id?, place: String, date: String) {
        self.txn = txn
        self.place = place
        self.date = date
    }

    /// The form over a receipt already there: its items, a purchase
    /// positive as people read a receipt.
    init(_ r: ReceiptLine, items: [ItemLine], txn: Id?) {
        receiptId = r.id
        self.txn = txn ?? r.txnId
        place = r.place
        date = r.date
        self.items = items.map { i in
            PaperItem(name: i.name, amount: ReceiptDraft.typed(-i.amount), sku: i.sku, quantity: i.quantity, unit: i.unit)
        }
        if self.items.isEmpty { self.items = [PaperItem()] }
    }

    /// Cents as dollars typed: `429` is `4.29`, `-50` is `-0.50`.
    static func typed(_ cents: Int64) -> String {
        let sign = cents < 0 ? "-" : ""
        let m = cents.magnitude
        return "\(sign)\(m / 100).\(String(format: "%02d", Int(m % 100)))"
    }

    /// What was typed as cents, as the desktop reads an amount (`Cents.parse`:
    /// `4.29`, `$4.29`, `-0.5`); none when it is not one, or blank.
    static func cents(_ typed: String) -> Int64? {
        typed.trimmingCharacters(in: .whitespaces).isEmpty ? nil : Cents.parse(typed)
    }

    /// The lines with something in them.
    var filled: [PaperItem] { items.filter { !$0.name.trimmingCharacters(in: .whitespaces).isEmpty || !$0.amount.isEmpty } }

    /// What the items come to, purchases positive.
    var total: Int64 { filled.compactMap { ReceiptDraft.cents($0.amount) }.reduce(0, +) }

    /// What keeps it from being saved, said; none when it can be.
    var problem: String? {
        if filled.isEmpty { return "Add an item" }
        if filled.contains(where: { $0.name.trimmingCharacters(in: .whitespaces).isEmpty }) { return "Every item needs a name" }
        if filled.contains(where: { ReceiptDraft.cents($0.amount) == nil }) { return "An amount isn't one" }
        return nil
    }

    /// What the domain is given: amounts our sign, spent negative; the total the items'.
    var write: WriteReceipt {
        let items = filled.map { i in
            StoreItem(name: i.name.trimmingCharacters(in: .whitespaces), sku: i.sku, quantity: i.quantity, unit: i.unit,
                      amount: -(ReceiptDraft.cents(i.amount) ?? 0))
        }
        return WriteReceipt(receiptId: receiptId, txnId: txn, place: place.trimmingCharacters(in: .whitespaces),
                            date: date, total: nil, items: items)
    }
}
