// Where a face's picture is loaded from and what it says without one, as
// ark-auth and harken have them.
package dev.money.android

import org.junit.Assert.assertEquals
import org.junit.Test

class FacesTests {
    /** ark-auth's own case (`picture_url` in rust/ark-auth/src/tests.rs), to the letter. */
    @Test fun pictureIsTheServersCopy() {
        assertEquals("", Faces.url("https://h.example", "u", ""))
        assertEquals("", Faces.url("", "u", "https://p.example/a.png"))
        assertEquals(
            "https://h.example/auth/picture?user=google%7C42&v=2f37db63",
            Faces.url("https://h.example/", "google|42", "https://p.example/a.png"),
        )
    }

    @Test fun initials() {
        assertEquals("AL", Faces.initials("Ada Lovelace"))
        assertEquals("A", Faces.initials("alex"))
        assertEquals("", Faces.initials("?"))
    }
}
