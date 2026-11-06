// A face's letters and where its picture is loaded from, with nothing of
// Apple's but Foundation, so Linux's type-check runs their tests. The face
// itself, a SwiftUI view, is ArkUI's `Face`, which harken draws too.
import ArkUICore
import Foundation

enum Faces {
    /// What a face says without a picture: the first letters of the name's
    /// first and last words, upper case — `Ada Lovelace` is `AL`, `Ada` is
    /// `A` — as harken's `Spell.initials` has it.
    static func initials(_ name: String) -> String { Pictures.initials(name) }

    /// Where this phone loads `user`'s picture from: `server`'s
    /// `/auth/picture`, with the login's `picture` as its version (`v`), so
    /// a new picture is a new address — ark-auth's `picture_url`, letter for
    /// letter. A login holds the provider's address, which a phone cannot
    /// always load (a dev server's is a file on that machine). Empty when
    /// there is no picture or no server.
    static func url(server: String, user: String, picture: String) -> String {
        Pictures.url(server: server, user: user, picture: picture)
    }
}
