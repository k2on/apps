// What a phone's launch costs before its first screen, stage by stage, over
// a replica the Kotlin client wrote with harken's module (docs/plan-android.md,
// "A remembered session opens straight into Home").
//
//     ColdOpen seed DIR N       a replica of N songs at DIR, as a phone keeps it
//     ColdOpen open DIR [K] [trusted]
//                               one launch over it, every stage timed; K
//                               times over, for the warm numbers; with the
//                               module remembered as verified
//     ColdOpen parts DIR        the replica's open taken apart
//     ColdOpen write DIR        Home's launch views written (and the floor's file)
//     ColdOpen launch DIR       a launch that publishes them first, then goes live
//     ColdOpen floor DIR        Home's lists off disk with no engine: the floor
//     ColdOpen empty            nothing: what the JVM costs before main
//
// **Seeded through the module.** Every song is `add_song` authored by the
// library account (the role the scanner's login holds), sequenced by an
// `Authority` over the same closures — what the server does with the
// scanner's entries — and confirmed into the peer's replica in batches, each
// written down by the peer's own writer: a snapshot and journal pages, byte
// for byte the records a phone holds. Then the owner signs in on the same
// replica, makes three playlists of twenty songs each, confirmed, and leaves
// three more adds pending, so the open has something to replay. The catalogue
// is synthetic: an artist per forty songs, an album per ten, every fifth
// album a work with a catalogue number and movements.
//
// The records are seeded into memory and then written to DIR as files, one
// per key: the same bytes, without twenty thousand fsyncs of a seeding the
// phone never does.
//
// `open` is one process doing what a launch does in its order, so run it in a
// fresh JVM per measurement: a phone's launch is a cold process too.
package dev.harken.peer

import dev.arkdb.Canon
import dev.arkdb.Decode
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.MemoryStore
import dev.arkdb.Replica
import dev.arkdb.Sha256
import dev.arkdb.Value
import dev.arkdb.Verify
import dev.arkdb.client.Autos
import dev.arkdb.client.ClientView
import dev.arkdb.client.DirStorage
import dev.arkdb.client.Domain
import dev.arkdb.client.LaunchBinding
import dev.arkdb.client.LaunchViews
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import dev.arkdb.client.Records
import dev.arkdb.client.ReplicaFile
import dev.arkdb.client.Update
import dev.arkdb.localCommit
import java.io.File

object ColdOpen {
    private fun module(): ByteArray =
        ColdOpen::class.java.getResourceAsStream("/harken.ark")!!.use { it.readBytes() }

    private fun ms(t0: Long): Double = (System.nanoTime() - t0) / 1e6

    private fun song(i: Int): Map<String, Value> {
        val artist = i / 40
        val album = i / 10
        val work = album % 5 == 0
        return mapOf(
            "title" to Value.text(if (work) "${(i % 10) + 1}. Movement" else "Song $i"),
            "artist" to Value.text("Artist $artist"),
            "album" to Value.text("Album $album"),
            "duration_ms" to Value.int(180_000L + (i % 97) * 1000L),
            "file" to Value.text("music/Artist $artist/Album $album/${i % 10 + 1}.flac"),
            "track" to Value.int((i % 10 + 1).toLong()),
            "part" to Value.text(if (work) "Part ${(i % 10) / 4 + 1}" else ""),
            "catalogue" to Value.text(if (work) "Op. $album" else ""),
            "performer" to Value.text(if (work) "Orchestra $artist" else ""),
            "bpm" to Value.int(0),
            "album_art" to Value.text(""),
            "artist_art" to Value.text(""),
            "disc" to Value.int(1),
            "work_title" to Value.text(if (work) "Work $album" else ""),
            "movement_no" to Value.int(if (work) (i % 10 + 1).toLong() else 0L),
        )
    }

    fun seed(dir: File, n: Int) {
        val domain = Domain.fromBytes(module())
        val mem = MemoryStorage()
        val p =
            Peer.open(
                domain,
                mem,
                Options.server("library", "scanner", null).copy(autos = Autos.Seeded(1)))
        p.setRoles(listOf("library"))
        val authority = dev.arkdb.Authority(domain.module.schema, domain.closures)
        fun commit() {
            localCommit(authority, p.replica)
            p.persist()
        }
        for (i in 0 until n) {
            p.mutate("add_song", song(i))
            if (i % 500 == 499) commit()
        }
        commit()
        p.signIn("owner", "phone", null)
        p.setRoles(emptyList())
        val ids = p.query("library", mapOf("playlist_id" to Value.id(Id.nil))).asList()
        for (k in 0 until 3) {
            p.mutate("create_playlist", mapOf("name" to Value.text("Playlist $k")))
        }
        commit()
        val lists = p.query("playlists", emptyMap()).asList().map { it.field("id") }
        for ((k, l) in lists.withIndex()) for (j in 0 until minOf(20, ids.size)) p.mutate(
            "add_to_playlist",
            mapOf("playlist_id" to l, "media_id" to ids[(k * 20 + j) % ids.size].field("id")))
        commit()
        for (j in 0 until minOf(3, ids.size)) p.mutate(
            "add_to_playlist",
            mapOf("playlist_id" to lists[0], "media_id" to ids[ids.size - 1 - j].field("id")))
        p.persist()
        dir.mkdirs()
        var bytes = 0L
        for (k in mem.keys()) {
            val b = mem.load(k)!!
            File(dir, k).writeBytes(b)
            bytes += b.size
        }
        println(
            "seeded $n songs at $dir: cursor ${p.cursor}, ${p.replica.pending.size} pending, " +
                "${mem.keys().size} records, $bytes bytes")
    }

    /** The views Home draws first, then the rest — the order the app hydrates in. */
    private val views =
        listOf(
            "library" to mapOf("playlist_id" to Value.id(Id.nil)),
            "playlists" to emptyMap(),
            "albums" to emptyMap(),
            "artists" to emptyMap(),
            "track_details" to emptyMap<String, Value>(),
        )

    fun open(dir: File, trusted: Boolean) {
        val out = LinkedHashMap<String, Double>()
        val start = System.nanoTime()
        var t = System.nanoTime()
        val bytes = module()
        out["module: read"] = ms(t)
        t = System.nanoTime()
        val decoded = Decode.module(Canon.decode(bytes))
        out["module: decode"] = ms(t)
        t = System.nanoTime()
        // A launch with the bytes that passed before (`ModuleMemory`): their
        // SHA-256 checked, and prepared as the verifier prepares them.
        val verified =
            if (trusted) {
                check(Hex.encode(Sha256.hash(bytes)).isNotEmpty())
                Verify.prepare(decoded)
            } else Verify.verify(decoded)
        out[if (trusted) "module: verify (remembered: hash, prepare)" else "module: verify"] = ms(t)
        t = System.nanoTime()
        val domain = Domain(verified)
        out["module: closures and hash"] = ms(t)
        t = System.nanoTime()
        val p = Peer.open(domain, DirStorage(dir), Options.server("owner", "phone", null))
        out["Peer.open (load, store, replay, persist)"] = ms(t)
        p.takeChanges()
        val held = ArrayList<ClientView>()
        for ((i, v) in views.withIndex()) {
            t = System.nanoTime()
            val cv = p.view(v.first, v.second)
            held.add(cv)
            out["hydrate ${v.first} (${cv.rows.size} rows)"] = ms(t)
            if (i == 2) {
                t = System.nanoTime()
                val home = held.map { it.rows.toList() }
                out["first publish (Home's three)"] = ms(t)
                check(home.size == 3)
                out["  first publish, from launch"] = ms(start)
            }
        }
        t = System.nanoTime()
        val copies = held.map { it.rows.toList() }
        out["second publish (the rest)"] = ms(t)
        out["total"] = ms(start)
        check(copies.isNotEmpty())
        for ((k, v) in out) println("%-46s %9.1f ms".format(k, v))
    }

    private fun launches(dir: File, bytes: ByteArray): LaunchViews =
        // Inside the replica's directory, so that each size here has its own; the app keeps them
        // beside it (`LaunchViews.directory`).
        LaunchViews(File(dir, "launch-owner"), LaunchBinding.of(bytes, "owner", emptyList()))

    /** Open, hydrate Home's three, and write their launch views — each write timed. */
    fun write(dir: File) {
        val bytes = module()
        val p =
            Peer.open(
                Domain.fromBytes(bytes), DirStorage(dir), Options.server("owner", "phone", null))
        val ls = launches(dir, bytes)
        val home = views.take(3).map { (n, a) -> ls.view(n, n, a).also { it.live(p, null) } }
        for (round in 1..3) for (v in home) {
            var t = System.nanoTime()
            val c = v.capture(p)!!
            val taken = ms(t)
            t = System.nanoTime()
            v.write(c)
            println(
                "%-46s %9.1f ms"
                    .format("write ${v.name} (take %.1f ms), round $round".format(taken), ms(t)))
        }
        // The floor's file: exactly Home's three lists, one plain CBOR value, nothing bound.
        File(dir, "home.cbor")
            .writeBytes(Canon.encode(Value.VList(home.map { Value.VList(it.view!!.rows) })))
    }

    /**
     * A launch with launch views: the three files read, checked and decoded — the first publish —
     * then the module, the peer and the live views, which answer against what was shown.
     */
    fun launch(dir: File) {
        val out = LinkedHashMap<String, Double>()
        out["jvm: start to main"] = uptime.toDouble()
        val start = System.nanoTime()
        var t = System.nanoTime()
        val bytes = module()
        val ls = launches(dir, bytes)
        val home = views.take(3).map { (n, a) -> ls.view(n, n, a) }
        val shown = home.map { it.provisional() }
        check(shown.all { it != null })
        out["launch views: read, check, decode"] = ms(t)
        out["  first publish, from main"] = ms(start)
        t = System.nanoTime()
        val domain = Domain(Verify.prepare(Decode.module(Canon.decode(bytes))))
        out["module: decode, prepare, hash"] = ms(t)
        t = System.nanoTime()
        val p = Peer.open(domain, DirStorage(dir), Options.server("owner", "phone", null))
        out["Peer.open"] = ms(t)
        t = System.nanoTime()
        val answers = home.zip(shown).map { (v, s) -> v.live(p, s) }
        out["live views, against what was shown"] = ms(t)
        check(answers.all { it == Update.Unchanged }) { "the live views moved: $answers" }
        out["  live, from main (nothing published)"] = ms(start)
        for ((k, v) in out) println("%-46s %9.1f ms".format(k, v))
    }

    /** The floor: Home's three lists off disk, one file, no engine but the decoder. */
    fun floor(dir: File) {
        val up = uptime
        val t = System.nanoTime()
        val v = Canon.decode(File(dir, "home.cbor").readBytes()) as Value.VList
        val lists = v.items.map { (it as Value.VList).items.toList() }
        println("%-46s %9.1f ms".format("jvm: start to main", up.toDouble()))
        println(
            "%-46s %9.1f ms"
                .format(
                    "floor: read and decode Home's lists (${lists.sumOf { it.size }} rows)", ms(t)))
    }

    private val uptime: Long
        get() = java.lang.management.ManagementFactory.getRuntimeMXBean().uptime

    fun parts(dir: File) {
        val domain = Domain.fromBytes(module())
        val schema = domain.module.schema
        val storage = DirStorage(dir)
        val out = LinkedHashMap<String, Double>()
        var t = System.nanoTime()
        val raw = storage.load(ReplicaFile.KEY)!!
        out["replica: read the snapshot (${raw.size} bytes)"] = ms(t)
        t = System.nanoTime()
        val v = Canon.decode(raw) as Value.VStruct
        out["replica: CBOR decode"] = ms(t)
        t = System.nanoTime()
        val st0 = MemoryStore.fromValue(schema, v.fields["confirmed"]!!)
        out["replica: store build"] = ms(t)
        check(st0.schema == schema)
        t = System.nanoTime()
        val st = Records.load(storage, schema)!!
        out["replica: Records.load whole (again)"] = ms(t)
        t = System.nanoTime()
        val r =
            Replica.open(
                st.file.confirmed.schema,
                domain.closures,
                st.file.confirmed,
                st.file.cursor,
                st.file.pending)
        out["replica: replay ${st.file.pending.size} pending"] = ms(t)
        check(r.cursor == st.file.cursor)
        for ((k, x) in out) println("%-46s %9.1f ms".format(k, x))
    }

    @JvmStatic
    fun main(args: Array<String>) {
        when (args.getOrNull(0)) {
            "seed" -> seed(File(args[1]), args[2].toInt())
            "open" ->
                repeat(args.getOrNull(2)?.toIntOrNull() ?: 1) {
                    open(File(args[1]), "trusted" in args)
                }
            "parts" -> parts(File(args[1]))
            "write" -> write(File(args[1]))
            "launch" -> launch(File(args[1]))
            "floor" -> floor(File(args[1]))
            "empty" -> println("%-46s %9.1f ms".format("jvm: start to main", uptime.toDouble()))
            else -> println("usage: ColdOpen seed DIR N | open DIR [TIMES] [trusted] | parts DIR")
        }
    }
}
