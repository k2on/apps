// The listening session, as the rest of the phone reaches it: harken's one
// reconciler (`harken/player`, through `dev.harken.ffi.Player`) on the
// session's thread, fed every change the session reports and every event the
// player raises — no socket of its own and no timer; live changes arrive as
// the log's do, through `ArkSession.onChange` — and what the screens and the
// media session draw, published as one value ([Heard]) whenever it moves:
// the one view of what plays, the session's while this phone is part of it
// and its own player's while it is alone, so that a screen draws one thing.
//
// It is kept with the session (Kept.kt), made by whichever end asks first —
// the app's screens or the media service — and closed when the session is;
// the media service finds it through [Listen.current] and attaches its
// player ([attach]) — so the player is reconciled while there is a session
// to reconcile against, however many screens are open on it. A press from
// anywhere — a button, a row, the lock screen, a headset — is [press], one
// way in.
package dev.harken.android

import dev.harken.android.gen.Active
import dev.harken.android.gen.Harken
import dev.harken.android.onboarding.ArkSession
import dev.harken.ffi.PhoneSound
import dev.harken.ffi.Player as Reconciled
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject

/** The session as this phone last heard it: what the screens, the picker and the lock screen draw. */
data class Heard(
    /** This device: its login's session id. */
    val me: String = "",
    /** Part of the session: linked, and standing in it. False is this phone being its own session. */
    val joined: Boolean = false,
    /** The session, while joined. */
    val listening: Listening? = null,
    val linked: Boolean = false,
    /** How many connections have opened. */
    val epoch: Long = 0,
    val stats: RemoteStats = RemoteStats(),
    /** The last refusal the server sent this phone's calls. */
    val refused: String? = null,
    /** What plays: the session's, while joined; this phone's own player's, alone. */
    val active: Active? = listening?.active,
) {
    /** The clock a position is counted against: this phone's own, as every entry carries its author's. */
    fun serverNow(): Long = System.currentTimeMillis()

    /** The sound is on another device, here or not: this phone is a controller of it. */
    val elsewhere: Boolean
        get() = joined && listening?.active?.output?.id?.let { it != me } == true

    /** Where another device's scrubber is being dragged to, if one is: the newest of theirs. */
    val scrubbing: Long?
        get() = listening?.scrub?.entries?.firstOrNull { it.key != me }?.value

    companion object {
        /** The player's view (`harken_player::Player::view`), as JSON. */
        fun of(json: String): Heard {
            val o = JSONObject(json)
            val l = o.optJSONObject("listening")?.let(Listening::of)
            val a = o.opt("active").takeIf { it != null && it != JSONObject.NULL }?.let(Active::of)
            return Heard(
                me = o.optString("me"),
                joined = o.optBoolean("joined"),
                listening = l,
                linked = o.optBoolean("linked"),
                epoch = o.optLong("epoch"),
                stats = RemoteStats.of(o.optJSONObject("stats")),
                refused = o.optString("refused").takeIf { o.has("refused") && !o.isNull("refused") },
                active = a,
            )
        }
    }
}

class Listen(private val session: ArkSession, val name: String) {
    /** This device: the login's session, which is what the `hello` declared. */
    val me: String = session.login.session

    /** The server a track's file is joined to. */
    val server: String = session.server

    /** An album's cover, by its name, as the library has it: the app's to say, and "" for none. */
    @Volatile var art: (String) -> String = { "" }

    // The session's thread only.
    private var player: Reconciled? = null
    private var unhook: () -> Unit = {}

    @Volatile private var sound: PhoneSound? = null
    @Volatile private var closed = false

    private val heardFlow = MutableStateFlow(Heard(me = me))

    /** The session as this phone last heard it, published when it moves and only then. */
    val heard: StateFlow<Heard> = heardFlow.asStateFlow()

    init {
        unhook = session.onChange { step(it) }
        session.post { step(it) }
    }

    // This phone's player, over the session's peer: what it counted kept beside the replica.
    private fun player(h: Harken): Reconciled =
        player ?: Reconciled(h.peer, me, session.dir).also {
            it.attach(sound)
            player = it
        }

    // Reconcile, and publish what it came to.
    private fun step(h: Harken) {
        if (closed) return
        val now = Heard.of(player(h).step())
        if (now != heardFlow.value) heardFlow.value = now
    }

    /** The player the media service owns: reconciled from now on, until [detach]. */
    fun attach(s: PhoneSound) {
        sound = s
        session.post { h ->
            player(h).attach(s)
            step(h)
        }
    }

    fun detach(s: PhoneSound) {
        if (sound !== s) return
        sound = null
        session.post { h -> player(h).attach(null) }
    }

    /** The player said something happened to it: reconcile. */
    fun poke() = session.post { step(it) }

    /**
     * A press — a transition while this phone is part of the session — previewed on the session's
     * thread, so that what is published next already shows it; then `done`, on that thread.
     * Unlinked it is refused here ([NOT_PART]), and the caller does it to the player itself.
     */
    fun press(press: Press, done: (String?) -> Unit = {}) =
        session.post { h ->
            val why = player(h).press(press.json())
            step(h)
            done(why)
        }

    /** Stop listening; the player is this phone's own from here on. */
    fun close() {
        closed = true
        session.post {
            unhook()
            player?.attach(null)
        }
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
