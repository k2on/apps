// A peer's live state (`docs/plan-live.md`), as the reference's `live_tests`
// hold `rust/ark-client`: a device of the demo's `room` (spec/vectors/
// module/live.json) calls, and the server's frames are made the way a server
// makes them — each `applied` carrying the hash of the state its transition
// produces, run through the same `Runner`. Sans-io, with time handed in
// (`Peer.pumpAt`, `Peer.callThrottledAt`): what is claimed is what a frame,
// a view, a pump and a deadline say, never what a clock happened to do.
//
// Every claim was falsified once by breaking what it holds (the comment
// beside each says how, and what failed).
package dev.arkdb.client

import dev.arkdb.Canon
import dev.arkdb.ClientMsg
import dev.arkdb.DeviceDecl
import dev.arkdb.FactError
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.Instance
import dev.arkdb.Kind
import dev.arkdb.LiveApplied
import dev.arkdb.LiveCtx
import dev.arkdb.LiveRefused
import dev.arkdb.LiveSnapshot
import dev.arkdb.Order
import dev.arkdb.ServerMsg
import dev.arkdb.Value
import dev.arkdb.conformance.Json
import java.io.File
import kotlin.system.exitProcess

object LiveTests {
    private val problems = ArrayList<String>()

    private fun claim(what: String, ok: Boolean) {
        if (!ok) problems.add(what)
    }

    private val room = Instance("room", "alice")
    private val phone = DeviceDecl("phone", "phone", "Phone", true)

    private fun int(n: Long): Value = Value.VInt(n)

    /** A device signed in as alice, and the server's side of what it hears. */
    private class Rig(val domain: Domain, device: DeviceDecl? = phone) {
        val peer: Peer =
            Peer.open(
                domain,
                MemoryStorage(),
                Options.server("alice", "phone-login", "t")
                    .copy(autos = Autos.Seeded(5), device = device))
        // What the server holds of the room, and its sequence.
        var value: Value = domain.live.init("room")
        var seq: Long = 0

        /** The server's frames are sent to the peer, one settle each. */
        fun hear(msg: ServerMsg) = peer.recvFrame(Canon.encode(msg.toValue()))

        /** Linked, then the snapshot a server sends on joining. */
        fun join() {
            peer.connected()
            peer.takeOutgoingFrames()
            hear(ServerMsg.SnapshotLive(LiveSnapshot(room, seq, value, emptyMap())))
        }

        /** An ordered call the server took, applied to what it holds and told to the peer. */
        fun applied(
            transition: String,
            input: Map<String, Value>,
            id: Id?,
            device: String,
            at: Long = 5_000,
            hash: ByteArray? = null,
            seqAt: Long? = null,
        ) {
            val ctx = LiveCtx("alice", "phone-login", emptySet(), device, at)
            value = domain.live.run("room", transition, value, ctx, input)
            seq = seqAt ?: (seq + 1)
            val h = hash ?: domain.live.hash("room", value)
            hear(
                ServerMsg.AppliedLive(
                    LiveApplied(
                        room,
                        transition,
                        input,
                        device,
                        Order.Ordered(seq, id, "alice", "phone-login", emptyList(), at, h))))
        }

        /** The calls the peer sent since the last ask. */
        fun calls(): List<dev.arkdb.Call> =
            peer.takeOutgoingFrames().mapNotNull {
                (ClientMsg.fromValue(Canon.decode(it)) as? ClientMsg.CallLive)?.call
            }

        fun count(): Long? = peer.live("room")?.field("count")?.asInt()

        fun pointer(device: String): Value? =
            (peer.live("room")?.field("pointer") as? Value.VStruct)?.fields?.get(device)
    }

    /**
     * A device's `hello` declares it; a peer that declares none says the bytes it always did, and
     * holds no live state. Falsified by dropping `live?.device` from `Client.hello`: the device's
     * hello came out without one.
     */
    fun hello(domain: Domain) {
        val r = Rig(domain)
        r.peer.connected()
        val h = ClientMsg.fromValue(Canon.decode(r.peer.takeOutgoingFrames().first()))
        claim("hello: a device declares itself", (h as? ClientMsg.Hello)?.device == phone)
        val plain = Rig(domain, device = null)
        plain.peer.connected()
        val p = ClientMsg.fromValue(Canon.decode(plain.peer.takeOutgoingFrames().first()))
        claim("hello: no device, no field", (p as? ClientMsg.Hello)?.device == null)
        claim("hello: no device, no live state", plain.peer.live("room") == null)
        println("live: a device's hello declares it, and a peer that is none says nothing new")
    }

    /**
     * An ordered call moves the view at once, is sent, stays pending — said again on every
     * connection — until the server's `applied` confirms it, which drops it; `liveStatus` says each
     * of those in numbers. Falsified by not re-sending pending calls in `Watch.connected`: the
     * second connection sent none; and by `liveStatus` reading `asked` for `synced`: a new
     * connection read as synced before its snapshot.
     */
    fun ordered(domain: Domain) {
        val r = Rig(domain)
        r.join()
        val kind = r.peer.call("room", "bump", mapOf("by" to int(3)))
        claim("ordered: bump is ordered", kind == Kind.Ordered)
        claim("ordered: the view moved at once (${r.count()})", r.count() == 3L)
        val sent = r.calls()
        claim("ordered: sent once (${sent.size})", sent.size == 1 && sent[0].counter == null)
        val id = sent.first().id
        claim("ordered: a pump says the view moved", r.peer.pumpAt(0).live)
        claim(
            "ordered: the status says one pending on a synced view (${r.peer.liveStatus("room")})",
            r.peer.liveStatus("room") == LiveStatus(0, 1, true))
        r.peer.disconnected()
        r.peer.connected()
        val again = r.calls()
        claim("ordered: said again on the next connection", again.map { it.id } == listOf(id))
        claim(
            "ordered: a new connection is not synced until its snapshot",
            r.peer.liveStatus("room")?.synced == false)
        r.hear(ServerMsg.SnapshotLive(LiveSnapshot(room, r.seq, r.value, emptyMap())))
        claim("ordered: replayed over the snapshot (${r.count()})", r.count() == 3L)
        r.applied("bump", mapOf("by" to int(3)), id, "phone")
        claim(
            "ordered: confirmed, the status says so (${r.peer.liveStatus("room")})",
            r.peer.liveStatus("room") == LiveStatus(1, 0, true))
        r.peer.connected()
        claim("ordered: confirmed, so not said again", r.calls().isEmpty())
        claim("ordered: the view is the server's (${r.count()})", r.count() == 3L)
        println("live: an ordered call previews, stays pending, is said again, and is confirmed")
    }

    /**
     * An ephemeral call is applied at once, sent with its counter, and never pending: a connection
     * says nothing of it again. Falsified by keeping ephemeral calls pending in `Watch.call`: the
     * next connection said the pointer again.
     */
    fun ephemeral(domain: Domain) {
        val r = Rig(domain)
        r.join()
        val kind = r.peer.call("room", "point", mapOf("at" to int(7)))
        claim("ephemeral: point is ephemeral", kind == Kind.Ephemeral)
        claim("ephemeral: the pointer moved at once", r.pointer("phone") == int(7))
        val sent = r.calls()
        claim("ephemeral: sent with a counter", sent.size == 1 && sent[0].counter != null)
        r.peer.disconnected()
        r.peer.connected()
        claim("ephemeral: never pending, never said again", r.calls().isEmpty())
        println("live: an ephemeral call is applied at once and is never pending")
    }

    /**
     * Another device's ephemeral call is taken only when its counter is above the last taken from
     * that device. Falsified by taking every counter in `Watch.heard`: the stale 3 moved the
     * laptop's pointer back.
     */
    fun counters(domain: Domain) {
        val r = Rig(domain)
        r.join()
        fun point(at: Long, counter: Long) =
            r.hear(
                ServerMsg.AppliedLive(
                    LiveApplied(
                        room, "point", mapOf("at" to int(at)), "laptop", Order.Ephemeral(counter))))
        point(1, 5)
        claim("counters: the laptop's pointer", r.pointer("laptop") == int(1))
        point(2, 3)
        claim("counters: a stale counter is ignored", r.pointer("laptop") == int(1))
        point(3, 5)
        claim("counters: an equal counter is ignored", r.pointer("laptop") == int(1))
        point(4, 6)
        claim("counters: a newer one is taken", r.pointer("laptop") == int(4))
        println("live: another device's ephemeral call is taken only above its last counter")
    }

    /**
     * A gap in the sequence asks one `resync`, and so does an `applied` whose hash is not what this
     * replica reaches; nothing is applied from either. Falsified by applying an ordered frame
     * whatever its sequence: the gap moved the count to 10 and asked nothing.
     */
    fun resync(domain: Domain) {
        val r = Rig(domain)
        r.join()
        r.applied("bump", mapOf("by" to int(10)), null, "laptop", seqAt = 3)
        val asked = r.peer.takeOutgoingFrames().map { ClientMsg.fromValue(Canon.decode(it)) }
        claim(
            "resync: a gap asks for the instance (${asked.size})",
            asked.singleOrNull() is ClientMsg.Resync)
        claim("resync: and applies nothing (${r.count()})", r.count() == 0L)
        val h = Rig(domain)
        h.join()
        h.applied("bump", mapOf("by" to int(2)), null, "laptop", hash = ByteArray(32))
        val asked2 = h.peer.takeOutgoingFrames().map { ClientMsg.fromValue(Canon.decode(it)) }
        claim(
            "resync: a wrong hash asks for the instance", asked2.singleOrNull() is ClientMsg.Resync)
        claim("resync: and applies nothing (${h.count()})", h.count() == 0L)
        h.applied("bump", mapOf("by" to int(1)), null, "laptop", seqAt = 3)
        claim("resync: asked once until it comes", h.peer.takeOutgoingFrames().isEmpty())
        h.hear(ServerMsg.SnapshotLive(LiveSnapshot(room, h.seq, h.value, emptyMap())))
        claim("resync: the snapshot is the server's (${h.count()})", h.count() == 3L)
        println("live: a gap and a wrong hash each ask one resync and apply nothing")
    }

    /**
     * The server's verdict on a call takes it out of pending and is told once; a refusal of the
     * preview's own is thrown and sends nothing. Falsified by keeping a refused call pending: the
     * count stayed at 3.
     */
    fun refused(domain: Domain) {
        val r = Rig(domain)
        r.join()
        r.peer.call("room", "bump", mapOf("by" to int(3)))
        val id = r.calls().first().id
        r.hear(ServerMsg.RefusedLive(LiveRefused(room, id, "not now")))
        claim("refused: the view without it (${r.count()})", r.count() == 0L)
        val told = r.peer.takeLiveRefusals()
        claim("refused: told", told == listOf(Rejection(id, "not now")))
        claim("refused: told once", r.peer.takeLiveRefusals().isEmpty())
        val preview =
            try {
                r.peer.call("room", "bump", mapOf("by" to int(11)))
                null
            } catch (e: Refused) {
                e.message
            }
        claim("refused: the preview's verdict ($preview)", preview == "by: between 1 and 10")
        claim("refused: and nothing sent", r.calls().isEmpty())
        println(
            "live: a verdict takes a call out of pending, and a preview's refusal sends nothing")
    }

    /**
     * A finger's calls go at most once per THROTTLE_MS, the last always: the view moves on every
     * one, what is held is due at `deadline` and sent by the pump then. Falsified by leaving the
     * throttled deadline out of `Peer.deadline`: nothing asked for the pump that sends the last.
     */
    fun throttle(domain: Domain) {
        val r = Rig(domain)
        r.join()
        r.peer.callThrottledAt(1_000, "room", "point", mapOf("at" to int(1)))
        claim("throttle: the first goes at once", r.calls().size == 1)
        r.peer.callThrottledAt(1_010, "room", "point", mapOf("at" to int(2)))
        r.peer.callThrottledAt(1_020, "room", "point", mapOf("at" to int(3)))
        claim("throttle: the view moved on every one", r.pointer("phone") == int(3))
        claim("throttle: the rest held", r.calls().isEmpty())
        claim(
            "throttle: due at the window's end (${r.peer.deadline()})", r.peer.deadline() == 1_050L)
        r.peer.pumpAt(1_040)
        claim("throttle: not before", r.calls().isEmpty())
        r.peer.pumpAt(1_050)
        val last = r.calls()
        claim(
            "throttle: the last sent, once (${last.map { it.input }})",
            last.map { it.input["at"] } == listOf(int(3)))
        claim("throttle: nothing waits", r.peer.deadline() == null)
        val ordered = r.peer.callThrottledAt(1_051, "room", "bump", mapOf("by" to int(1)))
        claim(
            "throttle: an ordered call is never held",
            ordered == Kind.Ordered && r.calls().size == 1)
        println("live: a finger's calls go once per ${THROTTLE_MS} ms, and the last always")
    }

    /**
     * The server's clock is read off the `at` of the ordered frames heard. Falsified by reading the
     * offset off ephemeral frames too: none carries an `at`, so it stayed null.
     */
    fun offset(domain: Domain) {
        val r = Rig(domain)
        r.join()
        claim("offset: none before any", r.peer.serverOffset == null)
        val ahead = System.currentTimeMillis() + 60_000
        r.applied("bump", mapOf("by" to int(1)), null, "laptop", at = ahead)
        val o = r.peer.serverOffset
        claim("offset: about a minute ahead ($o)", o != null && o in 55_000..60_000)
        claim("offset: server_now follows it", r.peer.serverNow() >= ahead - 5_000)
        println("live: the server's clock is read off the at of what it confirmed")
    }

    /**
     * An `applied` carrying both a sequence and a counter — every field an ordered one has, and a
     * counter — is refused by the decoder rather than read as either. Falsified by reading `seq`
     * first whatever else is there: the frame decoded as an ordered one.
     */
    fun bothOrders() {
        val both =
            Value.record(
                "t" to Value.VText("applied"),
                "live" to Value.VText("room"),
                "key" to Value.VText("alice"),
                "transition" to Value.VText("point"),
                "input" to Value.record("at" to int(3)),
                "device" to Value.VText("phone"),
                "seq" to int(5),
                "user" to Value.VText("alice"),
                "session" to Value.VText("phone-login"),
                "at" to int(1_000),
                "hash" to Value.VBytes(ByteArray(32)),
                "counter" to int(41))
        val refused =
            try {
                ServerMsg.fromValue(both)
                false
            } catch (e: FactError) {
                true
            }
        claim("decode: an applied with a seq and a counter is refused", refused)
        println("live: an applied with both a seq and a counter does not decode")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val vectors = File(args.getOrElse(0) { "../spec/vectors" })
        val v = Json.parse(File(vectors, "module/live.json").readText(Charsets.UTF_8))
        val domain = Domain.fromBytes(Hex.decode(v.field("bytes").asText()))
        hello(domain)
        ordered(domain)
        ephemeral(domain)
        counters(domain)
        resync(domain)
        refused(domain)
        throttle(domain)
        offset(domain)
        bothOrders()
        if (problems.isNotEmpty()) {
            println("live: ${problems.size} problem(s)")
            for (p in problems) println("  $p")
            exitProcess(1)
        }
        println("live: every claim held")
    }
}
