// What lives as long as the session does, whoever holds it: the library's
// views and the listening session's reconciler.
//
// The session is the process's (`kotlin/ark-android`, Sessions.kt): the
// activity holds it while it is there, and the media service holds it while
// it lives — which is while something plays, or a controller is bound to it:
// the app's own, a car, a headset, the lock screen. So swiping the app away
// while the phone is the output is the activity letting go, and nothing
// else: the socket stays, the phone stays in the account's session as its
// output, and the music it is making goes on — where it used to be the
// socket going with the activity, the server marking the phone away and
// paused, and every other device saying "paused" over music still playing.
// And a car binding to the service with no activity at all finds the same
// library and the same session the screens would.
//
// What is kept with it is made once, by whichever end asks first
// (`ArkSession.kept`), and closed with it. The screens' own ends — the
// controller a screen draws the player through, the covers it decodes — are
// still the composition's.
package dev.harken.android

import android.content.Context
import dev.arkdb.DeviceDecl
import dev.arkdb.android.ArkSession
import dev.arkdb.android.ArkSessions
import dev.arkdb.android.SessionSpec
import dev.arkdb.android.Sessions

class Kept private constructor(val session: ArkSession, name: String) : AutoCloseable {
    val library = Library(session)
    val listen = Listen(session, name)

    init {
        // An album's cover by its name, for what the session carries — which names an album, not
        // a row: the albums by name, made again only when the list is another one.
        listen.art = library.artOf
        // The one listening the media service follows (Playback.kt).
        Listen.current.value = listen
    }

    override fun close() {
        listen.close()
        library.close()
    }

    companion object {
        /** What is kept with `session`: made by the first to ask, the same for everyone after. */
        fun of(session: ArkSession, name: String): Kept = session.kept(Kept::class) { Kept(session, name) }

        /** Throw it away, as a process killed would: a test's. */
        internal fun drop(session: ArkSession) = session.drop(Kept::class)

        /** harken's module, from the APK's assets. */
        fun module(context: Context): () -> ByteArray {
            val app = context.applicationContext ?: context
            return { app.assets.open("harken.ark").use { it.readBytes() } }
        }

        /**
         * This phone at `hello`: a phone, which can play, named as its owner named it; its id is its
         * login's session, so that a device is a login (docs/plan-live.md).
         */
        fun device(context: Context): DeviceDecl = DeviceDecl("", "phone", Device.name(context), true)

        /** The process's sessions, over harken's module: the activity's and the media service's. */
        fun sessions(context: Context): Sessions {
            val app = context.applicationContext ?: context
            return ArkSessions.of(app, SessionSpec(module(app), device(app)))
        }
    }
}
