// A listening session's server, in a test: the engine's sans-io desk
// (`ark::live::state::Desk`) as far as one instance of harken's `listening`
// needs it — each device's `hello` standing it in the account's instance and
// running `on_join` the first time, every ordered call sequenced and told to
// every subscriber with the hash of the state it made, a refusal to its
// author alone, an ephemeral call relayed to everybody else above its
// device's last counter, and a connection's close running `on_leave` when it
// was its device's last — all through harken's own module, interpreted. The
// devices are real Kotlin peers, their frames carried by hand, so what is
// tested is the reconciler against the session the module defines and the
// frames the client really reads, with no socket and no clock but the one a
// test moves.
package dev.harken.android

import dev.arkdb.Canon
import dev.arkdb.ClientMsg
import dev.arkdb.DeviceDecl
import dev.arkdb.EvalFault
import dev.arkdb.Instance
import dev.arkdb.Kind
import dev.arkdb.LiveApplied
import dev.arkdb.LiveCtx
import dev.arkdb.LiveRefused
import dev.arkdb.LiveSnapshot
import dev.arkdb.ON_JOIN
import dev.arkdb.ON_LEAVE
import dev.arkdb.Order
import dev.arkdb.ServerMsg
import dev.arkdb.Value
import dev.arkdb.client.Autos
import dev.arkdb.client.Domain
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import dev.arkdb.refusalText
import dev.harken.android.listening.Track as Queued
import java.io.File

/** harken's module, as the domain crate writes it and the repository commits it. */
fun harkenModule(): ByteArray {
    var dir: File? = File("").absoluteFile
    while (dir != null) {
        File(dir, "domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
        File(dir, "harken/domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
        dir = dir.parentFile
    }
    error("no harken.ark above ${File("").absolutePath}")
}

/** A track of a test's queue: `n` names it, and every one is a hundred seconds long. */
fun queuedTrack(title: String, n: Int, ms: Long = 100_000): Queued {
    val b = ByteArray(16)
    b[15] = n.toByte()
    return Queued("Water Music", "Handel", ms, "music/$title.mp3", dev.arkdb.Id(b), title)
}

/**
 * A phone's media stream as `AudioManager` keeps it, in a test: fifteen steps, as most phones have,
 * and a mute; every write counted, so a test can say nothing was written.
 */
class FakeStream(override var index: Int = 7, override val max: Int = 15) : Stream {
    override val min: Int = 0
    override var muted: Boolean = false
    var writes = 0

    override fun set(index: Int) {
        this.index = index.coerceIn(min, max)
        writes++
    }

    override fun mute(muted: Boolean) {
        this.muted = muted
        writes++
    }
}

/** A player that does what it is told, and moves only when a test says so — with a volume, given a stream. */
class FakeSound(val stream: FakeStream? = null) : Sound {
    private val level = stream?.let(::StreamLevel)

    override var queue: List<Queued> = emptyList()
    override var index: Int = -1
    override var playing: Boolean = false
    override var buffering: Boolean = false
    override var positionMs: Long = 0
    override var ended: Boolean = false
    override var from: String = ""
    var stalled = false
    var loads = 0
    var jumps = 0
    var seeks = 0
    var wake: Long? = null

    override fun takeStalled(): Boolean = stalled.also { stalled = false }

    override fun load(queue: List<Queued>, at: Int, positionMs: Long, playing: Boolean, from: String) {
        this.queue = queue
        index = at
        this.positionMs = positionMs
        this.playing = playing
        this.from = from
        ended = false
        loads++
    }

    override fun jump(at: Int, positionMs: Long) {
        index = at
        this.positionMs = positionMs
        ended = false
        jumps++
    }

    override fun resume() {
        playing = true
        ended = false
    }

    override fun pause() {
        playing = false
    }

    override fun seek(ms: Long) {
        positionMs = ms
        ended = false
        seeks++
    }

    override fun wakeIn(ms: Long) {
        wake = ms
    }

    override fun volume(): Pair<Int, Boolean>? = level?.read()

    override fun setVolume(level: Int, muted: Boolean) {
        this.level?.apply(level, muted)
    }

    /** The track played to its end, and the player went on to the next as ExoPlayer does — or, at the last, stopped there. */
    fun finish() {
        if (index + 1 < queue.size) {
            index++
            positionMs = 0
        } else {
            ended = true
            positionMs = queue[index].durationMs
        }
    }

    /** Played this phone's own queue, as a press did while it was alone. */
    fun own(queue: List<Queued>, at: Int, positionMs: Long, from: String) {
        load(queue, at, positionMs, true, from)
    }
}

class Hall {
    val domain: Domain = Domain.fromBytes(harkenModule())
    private val runner = domain.live
    private val instance = Instance(LISTENING, "alice")
    var value: Value = runner.init(LISTENING)
    var seq = 0L
    var now: Long = System.currentTimeMillis()

    private class Stand(val peer: Peer, val conn: Int, var device: String? = null, var linked: Boolean = true)

    private val stands = ArrayList<Stand>()
    private val counters = HashMap<String, Long>()
    private var nextConn = 1
    private var seeded = 0L

    /** A device of alice's, its login `session`, linked. */
    fun device(session: String, canPlay: Boolean, kind: String = "phone"): Peer {
        seeded++
        val p =
            Peer.open(
                domain,
                MemoryStorage(),
                Options.server("alice", session, "t").copy(autos = Autos.Seeded(seeded)).withDevice(DeviceDecl("", kind, session, canPlay)))
        p.connected()
        stands.add(Stand(p, nextConn++))
        carry()
        return p
    }

    /** Its socket goes: not a decision about the music. */
    fun drop(p: Peer) {
        val s = stands.first { it.peer === p && it.linked }
        p.disconnected()
        s.linked = false
        part(s)
        carry()
    }

    /** Back, on a connection of its own. */
    fun relink(p: Peer) {
        stands.removeAll { it.peer === p && !it.linked }
        p.connected()
        stands.add(Stand(p, nextConn++))
        carry()
    }

    fun state(): dev.harken.android.listening.Listening = dev.harken.android.listening.Listening.of(value)

    private fun deliver(to: Stand, msg: ServerMsg) {
        if (to.linked) to.peer.recvFrame(Canon.encode(msg.toValue()))
    }

    private fun others(of: Stand): List<Stand> = stands.filter { it !== of && it.linked && it.device != null }

    private fun subscribed(): List<Stand> = stands.filter { it.linked && it.device != null }

    /** What every linked peer has to say, said, until nobody has more. */
    fun carry() {
        repeat(8) {
            var said = false
            for (s in stands.toList()) {
                if (!s.linked) continue
                for (f in s.peer.takeOutgoingFrames()) {
                    said = true
                    when (val m = ClientMsg.fromValue(Canon.decode(f))) {
                        is ClientMsg.Hello -> m.device?.let { hello(s, it) }
                        is ClientMsg.CallLive -> call(s, m.call)
                        else -> {}
                    }
                }
            }
            if (!said) return
        }
    }

    private fun hello(s: Stand, d: DeviceDecl) {
        val id = d.id.ifEmpty { s.peer.ctx.session }
        val arrived = stands.none { it !== s && it.linked && it.device == id }
        if (arrived) sequence(null, ON_JOIN, mapOf("device" to d.copy(id = id).record("alice")), id, s.peer)
        s.device = id
        deliver(s, ServerMsg.SnapshotLive(LiveSnapshot(instance, seq, value, HashMap(counters))))
    }

    private fun part(s: Stand) {
        val id = s.device ?: return
        s.device = null
        if (stands.any { it.linked && it.device == id }) return
        counters.remove(id)
        sequence(null, ON_LEAVE, mapOf("device" to DeviceDecl(id, "phone", id, true).record("alice")), id, s.peer)
    }

    private fun call(s: Stand, c: dev.arkdb.Call) {
        val device = s.device ?: return
        when (runner.kind(LISTENING, c.transition)) {
            Kind.Ordered -> sequence(s, c.transition, c.input, device, s.peer, c.id)
            Kind.Ephemeral -> {
                val counter = c.counter ?: return
                if ((counters[device] ?: Long.MIN_VALUE) >= counter) return
                try {
                    value = runner.run(LISTENING, c.transition, value, LiveCtx(device = device), c.input)
                } catch (e: EvalFault) {
                    return deliver(s, ServerMsg.RefusedLive(LiveRefused(instance, c.id, (e as? EvalFault.Verdict)?.refusal?.let(::refusalText) ?: "refused")))
                }
                counters[device] = counter
                for (o in others(s)) deliver(o, ServerMsg.AppliedLive(LiveApplied(instance, c.transition, c.input, device, Order.Ephemeral(counter))))
            }
        }
    }

    // An ordered call run at the server's time, sequenced, and told to every subscriber; refused,
    // told to its author. A hook has none.
    private fun sequence(author: Stand?, transition: String, input: Map<String, Value>, device: String, who: Peer, id: dev.arkdb.Id? = null) {
        val ctx = LiveCtx(who.ctx.user, who.ctx.session, emptySet(), device, now)
        val next =
            try {
                runner.run(LISTENING, transition, value, ctx, input)
            } catch (e: EvalFault) {
                if (author != null && id != null)
                    deliver(author, ServerMsg.RefusedLive(LiveRefused(instance, id, (e as? EvalFault.Verdict)?.refusal?.let(::refusalText) ?: "refused")))
                return
            }
        value = next
        seq++
        val applied = LiveApplied(instance, transition, input, device, Order.Ordered(seq, id, ctx.user, ctx.session, emptyList(), now, runner.hash(LISTENING, value)))
        for (o in subscribed()) deliver(o, ServerMsg.AppliedLive(applied))
    }
}
