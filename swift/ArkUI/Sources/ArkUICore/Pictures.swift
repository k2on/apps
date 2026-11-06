// Where a client loads a signed-in person's picture from: their server's
// `/auth/picture`, with the login's own picture as its version, as ark-auth's
// `picture_url` builds it (rust/ark-auth/src/util.rs), letter for letter. A
// login holds the provider's address, which a phone cannot always load: a dev
// server's is a file on that machine, and a provider's may need the server to
// fetch it. Every app's face is loaded through this. Foundation only, so the
// Linux type-check builds it.
import Foundation

public enum Pictures {
    /// What a face says without a picture: the first letters of the name's
    /// first and last words, upper case — `Ada Lovelace` is `AL`, `Ada` is
    /// `A`, `?` is nothing.
    public static func initials(_ name: String) -> String {
        func first(_ w: Substring) -> Character? { w.first { $0.isLetter || $0.isNumber } }
        let words = name.split(whereSeparator: { $0.isWhitespace }).filter { first($0) != nil }
        guard let a = words.first.flatMap(first) else { return "" }
        guard words.count > 1, let z = words.last.flatMap(first) else { return String(a).uppercased() }
        return (String(a) + String(z)).uppercased()
    }

    /// `user`'s picture on `server`, versioned by the login's `picture`; empty for none.
    public static func url(server: String, user: String, picture: String) -> String {
        if picture.isEmpty || server.isEmpty { return "" }
        // FNV-1a: a version, not a secret.
        var h: UInt32 = 0x811c_9dc5
        for b in picture.utf8 { h = (h ^ UInt32(b)) &* 0x0100_0193 }
        let v = String(h, radix: 16)
        var base = server
        while base.hasSuffix("/") { base.removeLast() }
        return base + "/auth/picture?user=" + encode(user) + "&v=" + String(repeating: "0", count: 8 - v.count) + v
    }

    /// A query value, encoded as ark-auth encodes one: everything but the unreserved characters.
    private static func encode(_ s: String) -> String {
        let hex = Array("0123456789ABCDEF")
        var out = ""
        for b in s.utf8 {
            switch b {
            case UInt8(ascii: "A")...UInt8(ascii: "Z"), UInt8(ascii: "a")...UInt8(ascii: "z"), UInt8(ascii: "0")...UInt8(ascii: "9"),
                 UInt8(ascii: "-"), UInt8(ascii: "_"), UInt8(ascii: "."), UInt8(ascii: "~"):
                out.append(Character(UnicodeScalar(b)))
            default:
                out.append("%")
                out.append(hex[Int(b >> 4)])
                out.append(hex[Int(b & 0xf)])
            }
        }
        return out
    }
}
