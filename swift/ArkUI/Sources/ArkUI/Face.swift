// Who is signed in, as a face: their initials on the accent, and their
// picture over them once it has arrived. One face for every app — money's
// account button and harken's draw this, the same size in the same place —
// so a person looks the same whichever app they opened. `AsyncImage` fetches
// the picture (`Pictures.url`, the server's `/auth/picture`) through the
// shared URL cache, which keeps it the day the server says; it answers
// anybody, so no login goes with it.
import SwiftUI

/// How large an account button's face is: the toolbar's, in every app.
public let faceSide: CGFloat = 32

/// Who is signed in, round, `side` across: their picture (`picture`, a
/// whole URL, "" for none) once it has loaded, cropped to fill, over their
/// initials on the accent.
public struct Face: View {
    let name: String
    let picture: String
    var side: CGFloat

    public init(name: String, picture: String, side: CGFloat = faceSide) {
        self.name = name
        self.picture = picture
        self.side = side
    }

    public var body: some View {
        ZStack {
            Circle().fill(Color.accent)
            Text(letters)
                .font(.system(size: side * 0.4, weight: .semibold))
                .foregroundStyle(Color.onAccent)
            if let url = URL(string: picture), !picture.isEmpty {
                AsyncImage(url: url) { phase in phase.image?.resizable().scaledToFill() }
            }
        }
        .frame(width: side, height: side)
        .clipShape(Circle())
    }

    // The initials, or the name's first character when it has no letters ("?").
    private var letters: String {
        let l = Pictures.initials(name)
        return l.isEmpty ? String(name.prefix(1)) : l
    }
}
