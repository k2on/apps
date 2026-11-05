// Where a face's picture is loaded from and what it says without one, as
// ark-auth and harken have them.
@testable import Money
import Foundation
import XCTest

final class FacesTests: XCTestCase {
    /// ark-auth's own case (`picture_url` in rust/ark-auth/src/tests.rs), to the letter.
    func testPictureIsTheServersCopy() {
        XCTAssertEqual(Faces.url(server: "https://h.example", user: "u", picture: ""), "")
        XCTAssertEqual(Faces.url(server: "", user: "u", picture: "https://p.example/a.png"), "")
        XCTAssertEqual(Faces.url(server: "https://h.example/", user: "google|42", picture: "https://p.example/a.png"),
                       "https://h.example/auth/picture?user=google%7C42&v=2f37db63")
    }

    func testInitials() {
        XCTAssertEqual(Faces.initials("Ada Lovelace"), "AL")
        XCTAssertEqual(Faces.initials("alex"), "A")
        XCTAssertEqual(Faces.initials("?"), "")
    }
}
