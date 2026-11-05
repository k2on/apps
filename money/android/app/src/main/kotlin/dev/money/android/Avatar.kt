// Who is signed in, as a face: arkui's `Face`, which harken's account button
// draws too, so the two apps' faces are one.
package dev.money.android

import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.Dp
import dev.arkui.Face
import dev.arkui.FaceSide

/** Who is signed in: arkui's [Face], the one every app's account button draws. */
@Composable
fun Avatar(name: String, picture: String = "", side: Dp = FaceSide) = Face(name, picture, side)

/** Faces: their letters and where their picture is. */
object Faces {
    /** What a face says without a picture: arkui's [dev.arkui.Faces.initials]. */
    fun initials(name: String): String = dev.arkui.Faces.initials(name)

    /**
     * Where this phone loads `user`'s picture from: `server`'s `/auth/picture`, with the login's
     * `picture` as its version (`v`), so a new picture is a new address — ark-auth's
     * `picture_url`, letter for letter. Empty when there is no picture or no server.
     */
    fun url(server: String, user: String, picture: String): String = dev.arkui.Pictures.url(server, user, picture)
}
