// ark-client's tests, as a plain `main` like the runtime's conformance
// runner: the demo domain authored in Kotlin (whose emitted module is
// spec/vectors/module/demo.json), a peer alone, two peers through an
// in-process authority over the in-memory transport — the same `Link` and
// the same frame bytes a socket would carry — and native procedures against
// the interpreter.
package dev.arkdb.client

import dev.arkdb.Args
import dev.arkdb.Changes
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.Plan
import dev.arkdb.Row
import dev.arkdb.Store
import dev.arkdb.Value
import java.io.File
import kotlin.system.exitProcess

object ClientTests {
    class Failed(message: String) : AssertionError(message)

    private fun check(cond: Boolean, what: () -> String) {
        if (!cond) throw Failed(what())
    }

    private fun <T> eq(got: T, want: T, what: String) {
        if (got != want) throw Failed("$what:\n  got  $got\n  want $want")
    }

    private var failures = 0

    private fun test(name: String, body: () -> Unit) {
        try {
            body()
            println("  ok    $name")
        } catch (t: Throwable) {
            failures += 1
            println("  FAIL  $name\n        ${t.toString().lines().joinToString("\n        ")}")
            if (t !is Failed) t.stackTrace.take(8).forEach { println("          at $it") }
        }
    }

    // Deterministic autos: ids count up, the clock steps, so two sessions
    // given one source each author byte-identical entries.
    class Counting(start: Int = 1) : AutoSource {
        var n = start
        var clock = 1_700_000_000_000L

        override fun newId(table: String): Id = Id(ByteArray(16).also { it[14] = (n shr 8).toByte(); it[15] = n.toByte() }).also { n += 1 }

        override fun now(): Long = clock.also { clock += 1000 }
    }

    private fun tempDir(name: String): File {
        val f = File.createTempFile("ark-$name-", "")
        f.delete()
        f.mkdirs()
        return f
    }

    private fun rowsOf(s: Session, table: String): List<Value> = s.read { db -> db.select(Plan.from(table)).asList() }

    private fun posOf(db: Store, pid: Id, k: Int): Value {
        val row = db.get("item", listOf(Value.id(pid), Value.text("t$k")))
        return if (row is Row) row["pos"] else Value.VNull
    }

    private fun create(name: String): Args = mapOf("name" to Value.text(name))

    private fun add(pid: Id, k: Int): Args = mapOf("playlist_id" to Value.id(pid), "track_id" to Value.text("t$k"))

    private fun hashOf(s: Session): String = Hex.encode(s.replica.verifyAt().second)

    @JvmStatic
    fun main(argv: Array<String>) {
        val vector = File(argv.getOrNull(0) ?: "../spec/vectors/module/demo.json")
        check(vector.isFile) { "no demo module vector at ${vector.absolutePath}" }
        // The vector's `bytes` field is the module's canonical CBOR; no JSON
        // parser is needed to lift a hex string out of it.
        val hex = Regex("\"bytes\"\\s*:\\s*\"([0-9a-f]+)\"").find(vector.readText())?.groupValues?.get(1)
            ?: throw Failed("no bytes in ${vector.path}")
        val domain = demo.module()
        eq(Hex.encode(domain.emit()), hex, "the authored demo emits the vector's module bytes")
        val module = Session.moduleOfHex(hex)
        eq(module, domain.ir, "and decodes to its IR")

        test("alone: a session creates a playlist, adds an item, and restarts with the same hash") {
            val dir = tempDir("alone")
            val autos = Counting()
            val a = Session.open(dir, domain, "eve", autos = autos)
            eq(a.status.serverless, true, "serverless")
            val changes = ArrayList<Changes>()
            a.onChange { changes.add(it) }
            val made = a.mutate("create_playlist", create("  Road  "))
            check(made is Outcome.Applied) { "create_playlist: $made" }
            val pid = (made as Outcome.Applied).entry.autos.getValue("id").asId()
            eq(a.status.cursor, 1L, "alone, the intent is sequenced at once")
            eq(a.status.pending, 0, "nothing stays pending")
            eq(rowsOf(a, "playlist").single().field("name"), Value.text("Road") as Value, "trimmed")
            val added = a.mutate("add_to_playlist", add(pid, 5))
            check(added is Outcome.Applied) { "add_to_playlist: $added" }
            eq(a.read { posOf(it, pid, 5) }, Value.int(1) as Value, "the first item is at pos 1")
            eq(a.status.cursor, 2L, "two entries")
            eq(changes.size, 2, "the listener heard both mutations: $changes")
            eq(a.verify(), 2L to true, "the authority agrees with its own replica")
            val refused = a.mutate("create_playlist", create("   "))
            eq(refused, Outcome.Refused("a playlist needs a name") as Outcome, "a refusal is surfaced")
            eq(a.status.cursor, 2L, "a refusal sequences nothing")
            val h = hashOf(a)
            a.close()
            check(File(dir, "log.replica").isFile) { "the replica was written down" }
            check(!File(dir, "log.replica.tmp").exists()) { "no temp file is left behind" }

            val b = Session.open(dir, domain, "eve", autos = Counting(100))
            eq(hashOf(b), h, "restarted with the same hash")
            eq(b.status.cursor, 2L, "and the same cursor")
            eq(b.sessionId, a.sessionId, "and the same device id")
            eq(b.verify(), 2L to true, "the adopted authority agrees")
            val more = b.mutate("add_to_playlist", add(pid, 6))
            check(more is Outcome.Applied) { "after a restart the peer still authors: $more" }
            eq(b.read { posOf(it, pid, 6) }, Value.int(2) as Value, "and continues the sequence")
            eq(b.status.cursor, 3L, "sequenced at 3")
            b.close()
            val c = Session.open(dir, domain, "eve", autos = Counting(200))
            eq(hashOf(c), hashOf(b), "a second restart")
            c.close()
        }

        test("alone: a store that does not match its log is refused on open") {
            val dir = tempDir("tamper")
            val a = Session.open(dir, domain, "eve", autos = Counting())
            a.mutate("create_playlist", create("Road"))
            a.close()
            // Rewrite the file with the log kept and the confirmed store emptied.
            val d = Durable.readReplica(module.schema, dir)!!
            Durable.writeReplica(dir, DurableReplica(d.cursor, dev.arkdb.MemoryStore(module.schema), d.pending, d.log))
            val err = try {
                Session.open(dir, domain, "eve", autos = Counting())
                null
            } catch (e: IllegalStateException) {
                e
            }
            check(err != null) { "opened a replica whose store is not its log's" }
        }

        test("two peers through an authority: alice adds alone, bob adds meanwhile, alice rebases and lands last") {
            val hub = LocalHub(module)
            val ta = InMemoryTransport(hub)
            val tb = InMemoryTransport(hub)
            val dirA = tempDir("alice")
            val dirB = tempDir("bob")
            val alice = Session.open(dirA, domain, "alice", "mem://hub", Counting(1), ta)
            val bob = Session.open(dirB, domain, "bob", "mem://hub", Counting(1000), tb)
            eq(alice.status.serverless, false, "not serverless")
            alice.pump()
            bob.pump()
            check(alice.status.linked && bob.status.linked) { "both linked: ${alice.status} ${bob.status}" }
            eq(hub.handled.count { it == "Hello" }, 2, "two hellos")

            val made = alice.mutate("create_playlist", create(" Favorites "))
            val pid = (made as Outcome.Applied).entry.autos.getValue("id").asId()
            eq(alice.status.pending, 1, "pending until pumped")
            alice.pump()
            eq(alice.status.pending, 0, "acked")
            eq(alice.status.cursor, 1L, "alice at 1")
            bob.pump()
            eq(bob.status.cursor, 1L, "bob at 1")
            eq(rowsOf(bob, "playlist").single().field("name"), Value.text("Favorites") as Value, "bob sees the playlist")
            eq(hashOf(alice), hashOf(bob), "agree after step 1")

            // alice goes dark
            ta.online = false
            alice.pump()
            check(!alice.status.linked) { "alice is offline: ${alice.status}" }
            val bobChanges = ArrayList<Changes>()
            bob.onChange { bobChanges.add(it) }
            // bob's adds run natively, cross the wire, and are sequenced by an
            // authority that replays them with the interpreter.
            val r1 = bob.mutate("add_to_playlist", add(pid, 1))
            check(r1 is Outcome.Applied) { "bob adds 1: $r1" }
            bob.pump()
            val r2 = bob.mutate("add_to_playlist", add(pid, 2))
            check(r2 is Outcome.Applied) { "bob adds 2: $r2" }
            bob.pump()
            eq(bob.replica.diverged.size, 0, "nothing diverged")
            eq(hashOf(bob), Hex.encode(Hash.stateHash(hub.authority.store)), "the authority's interpreter reached the state bob's native procedures did")
            eq(bob.status.cursor, 3L, "bob at 3")
            eq(bob.read { posOf(it, pid, 1) }, Value.int(1) as Value, "bob's first")
            eq(bob.read { posOf(it, pid, 2) }, Value.int(2) as Value, "bob's second")
            check(bobChanges.all { it is Changes.Applied }) { "bob's own confirmed intents cost no rebuild: $bobChanges" }

            val aliceChanges = ArrayList<Changes>()
            alice.onChange { aliceChanges.add(it) }
            val a9 = alice.mutate("add_to_playlist", add(pid, 9))
            check(a9 is Outcome.Applied) { "alice adds 9 alone: $a9" }
            eq(alice.read { posOf(it, pid, 9) }, Value.int(1) as Value, "alone, alice's track is first on her view")
            eq(alice.status.pending, 1, "one pending while dark")
            check(aliceChanges.single() is Changes.Applied) { "a local mutation reports its changes" }
            alice.pump()
            eq(alice.status.pending, 1, "still pending: nothing to push to")

            // …and is written down with it pending, so a restart while dark keeps the edit
            alice.close()
            val alice2 = Session.open(dirA, domain, "alice", "mem://hub", Counting(50), ta)
            eq(alice2.status.pending, 1, "the pending intent survived the restart")
            eq(alice2.read { posOf(it, pid, 9) }, Value.int(1) as Value, "and is replayed on the view")
            // Opening reports one `Rebuilt` (the replay from what was durable); take it, so
            // that the rebuild asserted below can only be the rebase's.
            check(alice2.takeChanges() is Changes.Rebuilt) { "opening reports a rebuild" }
            alice2.onChange { aliceChanges.add(it) }
            aliceChanges.clear()

            // alice comes back: the link reconnects on the next pump once the backoff has passed
            ta.online = true
            alice2.pump()
            var turns = 0
            while (!alice2.status.linked && turns < 200) {
                Thread.sleep(10)
                alice2.pump()
                turns += 1
            }
            check(alice2.status.linked) { "alice reconnected: ${alice2.status}" }
            alice2.pump()
            bob.pump()
            eq(alice2.status.pending, 0, "alice's intent was pushed and acked")
            eq(alice2.status.cursor, 4L, "alice at 4")
            eq(bob.status.cursor, 4L, "bob at 4")
            eq(alice2.read { posOf(it, pid, 9) }, Value.int(3) as Value, "after the rebase alice's track is third")
            eq(bob.read { posOf(it, pid, 9) }, Value.int(3) as Value, "and bob agrees")
            eq(hashOf(alice2), hashOf(bob), "one hash")
            eq(aliceChanges.count { it is Changes.Rebuilt }, 1, "the rebase was reported as a rebuild, once: $aliceChanges")
            eq(alice2.replica.diverged.size, 0, "alice's replay agreed with the facts")
            eq(hub.authority.log.headSeq, 4L, "the authority's head")

            // verify over the wire
            alice2.verify()
            alice2.pump()
            eq(alice2.takeAgreed(), listOf(4L to true), "the authority agrees with alice")

            // the frame path: every frame that crossed decodes as the protocol's
            check(ta.frames.isNotEmpty() && ta.frames.any { it.first == "client" } && ta.frames.any { it.first == "server" }) { "frames crossed both ways" }
            val tags = ta.frames.map { (who, bytes) ->
                val v = dev.arkdb.Canon.decode(bytes)
                if (who == "client") dev.arkdb.Protocol.clientFromValue(v)::class.simpleName else dev.arkdb.Protocol.serverFromValue(v)::class.simpleName
            }
            check("Hello" in tags && "Push" in tags && "Batch" in tags && "Ack" in tags && "Verify" in tags && "Agree" in tags) { "the frames seen: $tags" }

            alice2.close()
            bob.close()
            val alice3 = Session.open(dirA, domain, "alice", "mem://hub", Counting(70), ta)
            eq(hashOf(alice3), hashOf(bob), "restarted from the directory with the same hash")
            alice3.close()
        }

        test("every intent says where it stands: pending, confirmed at a sequence, or rejected with the server's reason") {
            fun run(owns: Boolean): List<ItemState> {
                var now = 0L
                val hub = LocalHub(module)
                if (owns) hub.withOwns { user, session -> user == "alice" && session == "old" }
                val t = InMemoryTransport(hub)
                val dir = tempDir("verdicts")
                val a = Session.open(dir, domain, "alice", "mem://hub", Counting(1), t, { now }, login = "old", token = "alice:old")
                a.pump()
                check(a.status.linked) { "linked: ${a.status}" }
                // Offline, under the old login: authored, pending.
                t.online = false
                a.pump()
                val e1 = (a.mutate("create_playlist", create("Road")) as Outcome.Applied).entry
                eq(e1.session, "old", "authored under the old login")
                eq(a.statusOf(e1.id), ItemState.Pending as ItemState, "pending while offline")
                // Signed in again; the next intent is the new login's, the first keeps its own.
                a.signIn("alice", "new", "alice:new")
                val e2 = (a.mutate("create_playlist", create("Mix")) as Outcome.Applied).entry
                eq(e2.session, "new", "authored under the new login")
                t.online = true
                now += 60_000
                a.pump()
                a.pump()
                check(a.status.linked) { "back: ${a.status}" }
                eq(a.status.pending, 0, "both answered")
                val states = listOf(a.statusOf(e1.id), a.statusOf(e2.id))
                if (!owns) {
                    eq(a.takeRejections(), listOf(e1.id to "not yours"), "the rejection, once")
                    eq(a.takeRejections(), emptyList<Pair<Id, String>>(), "and the slate wiped")
                    eq(a.status.rejected, 1, "counted")
                    eq(rowsOf(a, "playlist").map { it.field("name") }, listOf(Value.text("Mix")), "only the accepted playlist is on the view")
                }
                eq(a.statusOf(Id(ByteArray(16) { 7 })), ItemState.Unknown as ItemState, "an id this peer never authored")
                a.close()
                // The verdicts are kept across a restart.
                val b = Session.open(dir, domain, "alice", "mem://hub", Counting(500), t, { now }, login = "new", token = "alice:new")
                eq(listOf(b.statusOf(e1.id), b.statusOf(e2.id)), states, "the same answers after a restart")
                b.forget(listOf(e1.id))
                eq(b.statusOf(e1.id), ItemState.Unknown as ItemState, "a verdict let go")
                b.close()
                return states
            }
            eq(run(owns = false), listOf(ItemState.Rejected("not yours"), ItemState.Confirmed(1)), "an older login the server does not know is the user's")
            eq(run(owns = true), listOf(ItemState.Confirmed(1), ItemState.Confirmed(2)), "an older login the user owns")
        }

        test("signed out, then signed in: everything authored as nobody syncs as the signer's, each item's standing kept") {
            var now = 0L
            val hub = LocalHub(module)
            // alice already has a "Mix", made on another device.
            val other = Session.open(tempDir("other"), domain, "alice", "mem://hub", Counting(1000), InMemoryTransport(hub), { now })
            other.pump()
            val theirs = (other.mutate("create_playlist", create("Mix")) as Outcome.Applied).entry
            other.pump()
            eq(other.statusOf(theirs.id), ItemState.Confirmed(1) as ItemState, "the other device's Mix is in the log")

            val t = InMemoryTransport(hub)
            val dir = tempDir("signed-out")
            val a = Session.open(dir, domain, null, "mem://hub", Counting(1), t, { now })
            check(!a.signedIn && a.ctx == dev.arkdb.Ctx.nobody) { "signed out: ${a.ctx}" }
            a.pump()
            check(!a.status.linked) { "no connection while signed out: ${a.status}" }
            eq(hub.connections, 1, "only the other device is connected")
            val mix = (a.mutate("create_playlist", create("Mix")) as Outcome.Applied).entry
            val mixId = mix.autos.getValue("id").asId()
            val addToMix = (a.mutate("add_to_playlist", add(mixId, 1)) as Outcome.Applied).entry
            val road = (a.mutate("create_playlist", create("Road")) as Outcome.Applied).entry
            val addToRoad = (a.mutate("add_to_playlist", add(road.autos.getValue("id").asId(), 2)) as Outcome.Applied).entry
            val mine = listOf(mix, addToMix, road, addToRoad)
            check(mine.all { it.actor == "" && it.session == "" }) { "authored as nobody" }
            eq(rowsOf(a, "playlist").map { it.field("user_id") }, listOf(Value.text(""), Value.text("")), "on the view, as nobody's")
            a.close()

            // Kept, pending, across a restart while still signed out.
            val b = Session.open(dir, domain, null, "mem://hub", Counting(100), t, { now })
            eq(b.status.pending, 4, "all four pending after a restart")
            eq(mine.map { b.statusOf(it.id) }, List(4) { ItemState.Pending }, "each one pending")
            b.pump()
            check(!b.status.linked) { "still no connection" }

            // Signed in: re-stamped, written down, connected, pushed.
            b.signIn("alice")
            check(b.replica.pending.all { it.actor == "alice" && it.session == "dev" }) { "re-stamped: ${b.replica.pending}" }
            eq(rowsOf(b, "playlist").map { it.field("user_id") }.toSet(), setOf(Value.text("alice") as Value), "the view says whose they are")
            eq(Durable.readReplica(module.schema, dir)!!.pending.map { it.actor }.toSet(), setOf("alice"), "written down re-stamped")
            b.pump()
            b.pump()
            check(b.status.linked) { "connected: ${b.status}" }
            eq(b.status.pending, 0, "every intent answered")
            eq(
                mine.map { b.statusOf(it.id) },
                listOf(ItemState.Confirmed(2), ItemState.Rejected("playlist_id: no such playlist"), ItemState.Confirmed(3), ItemState.Confirmed(4)),
                "a second Mix is the first one (insert .on), so adding to the one only this device knew is refused, and says why",
            )
            eq(hub.authority.store.scan("playlist").map { it["name"] to it["user_id"] }.toSet(), setOf("Mix", "Road").map { Value.text(it) as Value to Value.text("alice") as Value }.toSet(), "the server's rows are alice's")
            eq(hashOf(b), Hex.encode(Hash.stateHash(hub.authority.store)), "and the replica agrees with the server")
            b.close()
            other.close()
            val c = Session.open(dir, domain, "alice", "mem://hub", Counting(200), t, { now })
            eq(c.statusOf(addToMix.id), ItemState.Rejected("playlist_id: no such playlist") as ItemState, "the reason, after another restart")
            c.close()
        }

        test("the link: backoff doubles from 500 ms to 30 s, a denied peer stops reconnecting") {
            var now = 0L
            val hub = LocalHub(module)
            val t = InMemoryTransport(hub)
            t.online = false
            val client = dev.arkdb.Client(dev.arkdb.Replica.open(module.schema, Hash.closures(module), dev.arkdb.MemoryStore(module.schema), 0, emptyList()), dev.arkdb.Mode.Whole, "alice")
            val link = Link(client, t) { now }
            link.connect()
            link.pump()
            eq(link.retryIn(), 500L, "first backoff")
            now += 499
            link.pump()
            eq(link.retryIn(), 1L, "not yet")
            now += 1
            link.pump()
            eq(link.retryIn(), 1000L, "doubled")
            now += 1000
            link.pump()
            eq(link.retryIn(), 2000L, "doubled again")
            for (i in 0 until 10) {
                now += link.retryIn()!!
                link.pump()
            }
            eq(link.retryIn(), 30_000L, "capped at 30 s")
            t.online = true
            now += 30_000
            link.pump()
            check(link.linked) { "connected" }
            eq(link.retryIn(), null, "no retry while linked")
            link.pump()
            t.online = false
            link.pump()
            check(!link.linked) { "dropped" }
            eq(link.retryIn(), 500L, "a connection that opened reset the backoff")

            // denied: the hub turns away a peer with no token
            val nobody = dev.arkdb.Client(dev.arkdb.Replica.open(module.schema, Hash.closures(module), dev.arkdb.MemoryStore(module.schema), 0, emptyList()), dev.arkdb.Mode.Whole, null)
            t.online = true
            val l2 = Link(nobody, t) { now }
            l2.connect()
            l2.pump()
            l2.pump()
            eq(nobody.denied, "not signed in", "denied")
            check(!l2.enabled && l2.retryIn() == null) { "a denied peer stops reconnecting" }
            eq(hub.connections, 0, "and its connection is closed")
        }

        test("native procedures and the interpreter author the same entries and reach the same state") {
            val byInterp = Session.open(tempDir("interp"), module, "eve", autos = Counting())
            val byNative = Session.open(tempDir("native"), domain, "eve", autos = Counting())
            check(byInterp.natives.isEmpty() && byNative.natives.size == 3) { "one runs nothing natively, the other everything" }
            val chI = ArrayList<Changes>()
            val chN = ArrayList<Changes>()
            byInterp.onChange { chI.add(it) }
            byNative.onChange { chN.add(it) }

            val r1 = byInterp.mutate("create_playlist", create(" Road "))
            val n1 = byNative.mutate("create_playlist", create(" Road "))
            eq(n1, r1, "the same entry")
            val pid = (r1 as Outcome.Applied).entry.autos.getValue("id").asId()
            for (k in listOf(3, 1, 2, 1)) {
                val r = byInterp.mutate("add_to_playlist", add(pid, k))
                val n = byNative.mutate("add_to_playlist", add(pid, k))
                eq(n, r, "the same entry for $k")
            }
            eq(chN, chI, "the same changes reported")
            eq(hashOf(byNative), hashOf(byInterp), "the same hash")
            eq(byNative.read { it.scan("item") }, byInterp.read { it.scan("item") }, "the same rows")
            eq(byNative.query("items", mapOf("playlist_id" to Value.id(pid))), byInterp.query("items", mapOf("playlist_id" to Value.id(pid))), "the same query answer")
            eq(byNative.query("items", mapOf("playlist_id" to Value.id(pid))).asList().map { it.field("track_id") }, listOf("t3", "t1", "t2").map { Value.text(it) }, "in pos order")
            // a refusal is the same verdict
            val rr = byInterp.mutate("create_playlist", create(" "))
            val nr = byNative.mutate("create_playlist", create(" "))
            eq(nr, rr, "the same refusal")
            eq(nr, Outcome.Refused("a playlist needs a name") as Outcome, "and it is the check's message")
            val missing = Id(ByteArray(16).also { it[15] = 99 })
            eq(byNative.mutate("add_to_playlist", add(missing, 1)), Outcome.Refused("playlist_id: no such playlist") as Outcome, "exists, by its default message")
            eq(byNative.status.cursor, 5L, "a refusal sequences nothing")
            byInterp.close()
            byNative.close()
        }

        test("the form validator: what a dialog shows under a field") {
            val s = Session.open(tempDir("form"), domain, "eve", autos = Counting())
            val blank = s.check("create_playlist", create("   "))
            eq(blank.messages, listOf("name" to "a playlist needs a name"), "a blank name")
            eq(blank.values["name"], Value.text("") as Value?, "trimmed")
            eq(s.check("create_playlist", create(" Mix ")).ok, true, "a good name")
            eq(s.check("add_to_playlist", mapOf("track_id" to Value.text(""))).messages, listOf("track_id" to "track_id: at least 1 characters"), "a partial input")
            s.close()
        }

        test("a native procedure that bugs is refused, and sequences nothing") {
            val real = domain.procedures()
            val (h, create) = real.first { it.second.name == "create_playlist" }
            val broken = object : dev.arkdb.Procedure by create {
                override fun apply(sch: dev.arkdb.Schema, ctx: dev.arkdb.Ctx, autos: Args, args: Args, st: dev.arkdb.MemoryStore): dev.arkdb.Eval.Applied =
                    throw dev.arkdb.Fault.bug("deliberate")
            }
            val s = Session.open(tempDir("bug"), module, "eve", autos = Counting(), procedures = listOf(h to broken))
            val out = s.mutate("create_playlist", create("x"))
            check(out is Outcome.Refused && out.reason.startsWith("bug:")) { "a bug is surfaced: $out" }
            eq(s.status.cursor, 0L, "and sequences nothing")
            s.close()
        }

        println()
        if (failures > 0) {
            println("$failures failed")
            exitProcess(1)
        }
        println("all passed")
    }
}
