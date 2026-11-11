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
// the media service finds it through [Listen.current] and attaches the
// phone's media player ([attach]: ark-media-player's `PhoneMediaPlayer`,
// the ExoPlayer) — so the player is reconciled while there is a session to
// reconcile against, however many screens are open on it. A press from
// anywhere — a button, a row, the lock screen, a headset — is [press], one
// way in: of the session while the phone is part of it, of the player's own
// queue alone (both decided in Rust, `harken/ffi`).
package dev.harken.android

import dev.harken.android.gen.Active
import dev.harken.android.gen.Harken
import dev.harken.android.onboarding.ArkSession
import android.os.Handler
import android.os.Looper
import dev.harken.ffi.Player as Reconciled
import dev.harken.ffi.PlayerShell
import uniffi.ark_media_player.NativeMediaPlayer
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
    /** What only this phone's own player has: filling its buffer, shuffled, repeating. */
    val buffering: Boolean = false,
    val shuffle: Boolean = false,
    val repeat: Repeat = Repeat.Off,
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
            val local = o.optJSONObject("local")
            return Heard(
                me = o.optString("me"),
                joined = o.optBoolean("joined"),
                listening = l,
                linked = o.optBoolean("linked"),
                epoch = o.optLong("epoch"),
                stats = RemoteStats.of(o.optJSONObject("stats")),
                refused = o.optString("refused").takeIf { o.has("refused") && !o.isNull("refused") },
                active = a,
                buffering = local?.optBoolean("buffering") ?: false,
                shuffle = local?.optBoolean("shuffle") ?: false,
                repeat =
                    when (local?.optString("repeat")) {
                        "all" -> Repeat.All
                        "one" -> Repeat.One
                        else -> Repeat.Off
                    },
            )
        }
    }
}

class Listen(private val session: ArkSession, val name: String) {
    /** This device: the login's session, which is what the `hello` declared. */
    val me: String = session.login.session

    /** The server a track's file is joined to. */
    val server: String = session.server

    /** A track's cover by its album and its creator — a show's, for an episode — as the library has it: the app's to say, and "" for none. */
    @Volatile var art: (String, String) -> String = { _, _ -> "" }

    // The session's thread only.
    private var player: Reconciled? = null
    private var unhook: () -> Unit = {}

    @Volatile private var media: NativeMediaPlayer? = null
    @Volatile private var closed = false
    private val main = Handler(Looper.getMainLooper())

    /** What the player asks of the phone: a step now or later, and a track's cover as the system draws it. */
    private val shell =
        object : PlayerShell {
            private val again = Runnable { poke() }

            override fun wake(inMs: Long) {
                if (inMs <= 0) return poke()
                main.removeCallbacks(again)
                main.postDelayed(again, inMs)
            }

            // A `content://` URI the app answers itself (CoverProvider.kt): what a car will load.
            override fun cover(album: String, creator: String): String? =
                CarArt.uri(art(album, creator).takeIf { it.isNotEmpty() }?.let { Media.url(server, it) }, album.ifEmpty { creator }, CarArt.PLAYING).toString()
        }

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
            it.attach(media, server, shell)
            player = it
        }

    // Reconcile, and publish what it came to.
    private fun step(h: Harken) {
        if (closed) return
        val now = Heard.of(player(h).step())
        if (now != heardFlow.value) heardFlow.value = now
    }

    /** The phone's media player: reconciled from now on, until [detach]. */
    fun attach(m: NativeMediaPlayer) {
        media = m
        session.post { h ->
            player(h).attach(m, server, shell)
            step(h)
        }
    }

    fun detach(m: NativeMediaPlayer) {
        if (media !== m) return
        media = null
        session.post { h -> player(h).attach(null, server, null) }
    }

    /** Alone: what is still to come shuffled, or not. */
    fun shuffle() =
        session.post { h ->
            player(h).shuffle()
            step(h)
        }

    /** Alone: off, all, one, off. */
    fun cycleRepeat() =
        session.post { h ->
            player(h).cycleRepeat()
            step(h)
        }

    /** The player said something happened to it: reconcile. */
    fun poke() = session.post { step(it) }

    /**
     * A press — a transition while this phone is part of the session — previewed on the session's
     * thread, so that what is published next already shows it; then `done`, on that thread.
     * Alone it is done to the player's own queue.
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
            player?.attach(null, server, null)
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
