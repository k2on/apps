// The listening session, as the rest of the phone reaches it: harken's one
// reconciler (`harken/player`, through `dev.harken.ffi.Player`) on the
// session's thread, fed every change the session reports and every event the
// player raises — no socket of its own and no timer; live changes arrive as
// the log's do, through `ArkSession.onChange` — and what the screens and the
// media session draw, published as one value ([Heard]) whenever it moves:
// the one view of what plays, the session's while this phone is part of it
// and its own player's while it is alone, with the rows the player and the
// picker draw (harken's player's screens), so that a screen draws one thing.
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
import dev.harken.screens.Ask
import dev.harken.screens.Line
import dev.harken.screens.NowPlaying
import dev.harken.screens.Devices
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
    /** What plays: the session's, while joined; this phone's own player's, alone. */
    val active: Active? = listening?.active,
    /** The player screen's row, and the picker's: harken's player's (`dev.harken.screens`). */
    val now: NowPlaying = NOTHING,
    val picker: Devices = Devices(emptyList(), 0),
) {
    /** The clock a position is counted against: this phone's own, as every entry carries its author's. */
    fun serverNow(): Long = System.currentTimeMillis()

    companion object {
        /** Nothing playing, alone. */
        val NOTHING = NowPlaying(false, null, false, false, 0, 0, null, emptyList(), null, null, null)

        /** The player's view (`harken_player::Player::view`), as JSON, and the screens' rows. */
        fun of(json: String, now: NowPlaying = NOTHING, picker: Devices = Devices(emptyList(), 0)): Heard {
            val o = JSONObject(json)
            val l = o.optJSONObject("listening")?.let(Listening::of)
            val a = o.opt("active").takeIf { it != null && it != JSONObject.NULL }?.let(Active::of)
            return Heard(o.optString("me"), o.optBoolean("joined"), l, a, now, picker)
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
        val p = player(h)
        val now = Heard.of(p.step(), p.nowPlaying(name, server), p.picker())
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

    /**
     * A screen's control ([Ask]), resolved against the session in Rust and previewed as [press] is;
     * then `done`, with [NOT_PART] for what the caller does to its own player.
     */
    fun ask(ask: Ask, done: (String?) -> Unit = {}) =
        session.post { h ->
            val why = player(h).ask(ask)
            step(h)
            done(why)
        }

    /** The diagnostics' listening numbers, as the player has them now. */
    suspend fun numbers(): List<Line> = session.run { h -> player(h).numbers() }

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
