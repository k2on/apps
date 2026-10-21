// The listening session, as the rest of the phone reaches it: the
// reconciler (Remote.kt) on the session's thread, fed every change the
// session reports — no socket of its own and no timer; live changes arrive
// as the log's do, through `ArkSession.onChange` — and what the screens and
// the media session draw, published as one value ([Heard]) whenever it moves.
//
// It is kept with the session (Kept.kt), made by whichever end asks first —
// the app's screens or the media service — and closed when the session is;
// the media service finds it through [Listen.current] and attaches its
// player ([attach]) — so the player is reconciled while there is a session
// to reconcile against, however many screens are open on it, and is this
// phone's own the rest of the time. A press from anywhere — a button, a
// row, the lock screen, a headset — is [press], one way in.
package dev.harken.android

import dev.harken.android.gen.Harken
import dev.harken.android.onboarding.ArkSession
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** The session as this phone last heard it: what the screens, the picker and the lock screen draw. */
data class Heard(
    /** This device: its login's session id. */
    val me: String = "",
    /** Part of the session: linked, and the server's snapshot met. False is this phone being its own session. */
    val joined: Boolean = false,
    /** The session, while joined. */
    val listening: Listening? = null,
    val linked: Boolean = false,
    /** How many connections have opened. */
    val epoch: Long = 0,
    val stats: RemoteStats = RemoteStats(),
    /** The last refusal the server sent this phone's calls. */
    val refused: String? = null,
) {
    /** The clock a position is counted against: this phone's own, as every entry carries its author's. */
    fun serverNow(): Long = System.currentTimeMillis()

    /** The sound is on another device, here or not: this phone is a controller of it. */
    val elsewhere: Boolean
        get() = joined && listening?.active?.output?.id?.let { it != me } == true

    /** Where another device's scrubber is being dragged to, if one is: the newest of theirs. */
    val scrubbing: Long?
        get() = listening?.scrub?.entries?.firstOrNull { it.key != me }?.value
}

class Listen(private val session: ArkSession, val name: String) {
    /** This device: the login's session, which is what the `hello` declared. */
    val me: String = session.login.session

    /** The server a track's file is joined to. */
    val server: String = session.server

    /** An album's cover, by its name, as the library has it: the app's to say, and "" for none. */
    @Volatile var art: (String) -> String = { "" }

    // The session's thread only.
    private val remote = Remote(me, name)
    private var unhook: () -> Unit = {}

    @Volatile private var sound: Sound? = null
    @Volatile private var closed = false

    private val heardFlow = MutableStateFlow(Heard(me = me))

    /** The session as this phone last heard it, published when it moves and only then. */
    val heard: StateFlow<Heard> = heardFlow.asStateFlow()

    init {
        unhook = session.onChange { step(it) }
        session.post { step(it) }
    }

    // Reconcile, and publish what it came to.
    private fun step(h: Harken) {
        if (closed) return
        sound?.let { remote.reconcile(h, it) }
        val refused = remote.refusals(h).lastOrNull()
        val st = h.peer.status()
        val now =
            Heard(
                me = me,
                joined = remote.joined(h),
                listening = remote.session(h),
                linked = st.linked,
                epoch = st.epoch.toLong(),
                stats = remote.stats,
                refused = refused ?: heardFlow.value.refused,
            )
        if (now != heardFlow.value) heardFlow.value = now
    }

    /** The player the media service owns: reconciled from now on, until [detach]. */
    fun attach(s: Sound) {
        sound = s
        session.post { step(it) }
    }

    fun detach(s: Sound) {
        if (sound === s) sound = null
    }

    /** The player said something happened to it: reconcile. */
    fun poke() = session.post { step(it) }

    /**
     * A press — a transition while this phone is part of the session — previewed on the session's
     * thread, so that what is published next already shows it; then `done`, on that thread.
     * Unlinked it is refused here ("not part of the session"), and the caller does it to the player
     * itself.
     */
    fun press(press: Press, done: (String?) -> Unit = {}) =
        session.post { h ->
            val why = remote.press(h, press, sound)
            step(h)
            done(why)
        }

    /** Stop listening; the player is this phone's own from here on. */
    fun close() {
        closed = true
        session.post { unhook() }
        if (current.value === this) current.value = null
    }

    companion object {
        /**
         * The session's listening, while there is one: what the media service attaches its player
         * to. One process, one app, one session at a time.
         */
        val current: MutableStateFlow<Listen?> = MutableStateFlow(null)
    }
}
