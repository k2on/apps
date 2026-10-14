// Launch views over harken's module, without a network: a replica confirmed
// by an `Authority` as a server would, the view written as an app writes it,
// and read back as the next launch reads it — against the live view the
// opened peer hydrates.
package dev.arkdb.client

import dev.arkdb.Authority
import dev.arkdb.Id
import dev.arkdb.Patch
import dev.arkdb.Sequenced
import dev.arkdb.Value
import dev.arkdb.Views
import dev.arkdb.localCommit
import java.io.File
import java.nio.file.Files

object LaunchViewTests {
    private val problems = ArrayList<String>()

    private fun claim(what: String, ok: Boolean) {
        if (!ok) problems.add(what)
    }

    private fun song(i: Int): Map<String, Value> =
        mapOf(
            "title" to Value.text("Song $i"),
            "artist" to Value.text("Artist ${i / 10}"),
            "album" to Value.text("Album ${i / 5}"),
            "duration_ms" to Value.int(180_000),
            "file" to Value.text("music/$i.flac"),
            "track" to Value.int((i % 5 + 1).toLong()),
            "part" to Value.text(""),
            "catalogue" to Value.text(""),
            "performer" to Value.text(""),
            "bpm" to Value.int(0),
            "album_art" to Value.text(""),
            "artist_art" to Value.text(""),
            "disc" to Value.int(1),
            "work_title" to Value.text(""),
            "movement_no" to Value.int(0),
        )

    private val home =
        listOf(
            "library" to mapOf("playlist_id" to Value.id(Id.nil)),
            "playlists" to emptyMap(),
            "albums" to emptyMap<String, Value>(),
        )

    private class World(val domain: Domain, val bytes: ByteArray) {
        val storage = MemoryStorage()
        val authority = Authority(domain.module.schema, domain.closures)

        /** The server's peer for the owner: thirty songs confirmed, one playlist pending. */
        fun owner(): Peer {
            val p =
                Peer.open(
                    domain,
                    storage,
                    Options.server("library", "scanner", null).copy(autos = Autos.Seeded(1)))
            p.setRoles(listOf("library"))
            repeat(30) { p.mutate("add_song", song(it)) }
            localCommit(authority, p.replica)
            p.signIn("owner", "phone", null)
            p.setRoles(emptyList())
            p.mutate("create_playlist", mapOf("name" to Value.text("Mid")))
            p.persist()
            return p
        }

        fun reopen(roles: List<String> = emptyList()): Peer =
            Peer.open(
                    domain,
                    storage,
                    Options.server("owner", "phone", null).copy(autos = Autos.Seeded(9)))
                .also { it.setRoles(roles) }
    }

    private fun views(
        dir: File,
        w: World,
        account: String = "owner",
        roles: List<String> = emptyList(),
        module: ByteArray = w.bytes
    ) = LaunchViews(dir, LaunchBinding.of(module, account, roles))

    private fun save(vs: LaunchViews, p: Peer) {
        for ((n, a) in home) {
            val v = vs.view(n, n, a)
            v.live(p, null)
            v.write(v.capture(p)!!)
        }
    }

    /** After a clean close, every file is the list the opened peer hydrates: nothing to publish. */
    private fun equalAfterACleanClose(
        w: World,
        p: Peer,
        what: String,
        roles: List<String> = emptyList()
    ) {
        val dir = Files.createTempDirectory("launch").toFile()
        try {
            save(views(dir, w, roles = roles), p)
            p.close()
            val q = w.reopen(roles)
            val vs = views(dir, w, roles = roles)
            for ((n, a) in home) {
                val v = vs.view(n, n, a)
                val shown = v.provisional()
                claim("$what: $n: a file was read", shown != null)
                val u = v.live(q, shown)
                claim("$what: $n: the live view is what was shown, $u", u == Update.Unchanged)
                claim("$what: $n: rows", shown == v.view!!.rows)
            }
            claim("$what: positions agreed", vs.positionMismatches.get() == 0)
        } finally {
            dir.deleteRecursively()
        }
    }

    fun run(harken: File) {
        val bytes = harken.readBytes()
        val domain = Domain.fromBytes(bytes)

        // 1. Pending intents present.
        run {
            val w = World(domain, bytes)
            val p = w.owner()
            claim("pending: an intent is pending", p.replica.pending.isNotEmpty())
            equalAfterACleanClose(w, p, "pending")
        }
        // 1. A confirmed batch that lands under a pending intent and rebases it: another device
        //    of the same person made a playlist the server sequenced first.
        run {
            val w = World(domain, bytes)
            val p = w.owner()
            val other =
                Peer.open(
                    domain,
                    MemoryStorage(),
                    Options.server("owner", "laptop", null).copy(autos = Autos.Seeded(4)))
            other.mutate("create_playlist", mapOf("name" to Value.text("Aaa")))
            val e = other.replica.pending.single()
            val s = w.authority.sequenceEntry(e) as Sequenced.Appended
            p.replica.receiveWith(s.seq, e, s.facts)
            p.replica.settle()
            p.persist()
            val names = p.query("playlists", emptyMap()).asList().map { it.field("name") }
            claim("rebase: both playlists, the server's first in the log, $names", names.size == 2)
            equalAfterACleanClose(w, p, "rebase")
        }
        // 1. A role change: the file is bound to the roles it was written under.
        run {
            val w = World(domain, bytes)
            val p = w.owner()
            p.setRoles(listOf("listener"))
            equalAfterACleanClose(w, p, "roles", roles = listOf("listener"))
            val dir = Files.createTempDirectory("launch").toFile()
            save(views(dir, w, roles = listOf("listener")), p)
            val v = views(dir, w, roles = emptyList()).view("library", "library", home[0].second)
            claim("roles: written under other roles, not read", v.provisional() == null)
            claim("roles: and deleted", dir.listFiles()!!.size == 2)
            dir.deleteRecursively()
        }
        // 2 and 3. A stale file — the store moved behind it — is corrected to the live list by
        //    patches, and only the difference moves.
        run {
            val w = World(domain, bytes)
            val p = w.owner()
            val dir = Files.createTempDirectory("launch").toFile()
            save(views(dir, w), p)
            p.mutate("create_playlist", mapOf("name" to Value.text("Later")))
            p.persist()
            p.close()
            val q = w.reopen()
            val vs = views(dir, w)
            val v = vs.view("playlists", "playlists", emptyMap())
            val shown = v.provisional()!!
            val u = v.live(q, shown)
            claim("stale: patched, $u", u is Update.Patched)
            if (u is Update.Patched) {
                claim(
                    "stale: the patches take what was shown to the live list",
                    Views.splice(u.patches, shown) == v.view!!.rows)
                claim(
                    "stale: one row moved, ${u.patches}",
                    u.patches.size == 1 && u.patches[0] is Patch.Insert)
            }
            claim(
                "stale: the position disagreed, and was counted", vs.positionMismatches.get() == 1)
            dir.deleteRecursively()
        }
        // 4. Bound elsewhere: another module, another view, another account — not read, deleted.
        run {
            val w = World(domain, bytes)
            val p = w.owner()
            for ((what, read) in
                listOf<Pair<String, (File) -> LaunchView>>(
                    "module" to
                        { d ->
                            views(d, w, module = bytes + byteArrayOf(0))
                                .view("library", "library", home[0].second)
                        },
                    "view" to
                        { d ->
                            views(d, w)
                                .view(
                                    "library",
                                    "library",
                                    mapOf("playlist_id" to Value.id(Id.ofHex("%032x".format(7)))))
                        },
                    "account" to
                        { d ->
                            views(d, w, account = "someone")
                                .view("library", "library", home[0].second)
                        },
                )) {
                val dir = Files.createTempDirectory("launch").toFile()
                save(views(dir, w), p)
                val v = read(dir)
                claim("$what: a file bound elsewhere is not read", v.provisional() == null)
                claim("$what: and is deleted", dir.listFiles()!!.size == 2)
                dir.deleteRecursively()
            }
        }
        // 5. Torn or corrupt: not read, deleted, nothing thrown.
        run {
            val w = World(domain, bytes)
            val p = w.owner()
            for ((what, spoil) in
                listOf<Pair<String, (ByteArray) -> ByteArray>>(
                    "truncated" to { b -> b.copyOf(b.size / 2) },
                    "a byte flipped" to
                        { b ->
                            b.copyOf().also {
                                it[it.size / 2] = (it[it.size / 2].toInt() xor 1).toByte()
                            }
                        },
                    "empty" to { _ -> ByteArray(0) },
                )) {
                val dir = Files.createTempDirectory("launch").toFile()
                val vs = views(dir, w)
                save(vs, p)
                val f = vs.file("library")
                f.writeBytes(spoil(f.readBytes()))
                val v = vs.view("library", "library", home[0].second)
                val got = runCatching { v.provisional() }
                claim("$what: no throw, ${got.exceptionOrNull()}", got.isSuccess)
                claim("$what: not read", got.getOrNull() == null)
                claim("$what: deleted", !f.exists())
                claim("$what: counted", vs.refused.get() == 1)
                dir.deleteRecursively()
            }
        }
        // The difference, in general: whatever two lists, its patches take one to the other.
        run {
            var seed = 11L
            fun next(n: Int): Int {
                seed = seed * 6364136223846793005L + 1442695040888963407L
                return ((seed ushr 33) % n).toInt()
            }
            repeat(500) { k ->
                val a = List(next(12)) { Value.int(next(6).toLong()) }
                val b = List(next(12)) { Value.int(next(6).toLong()) }
                val ps = LaunchViews.diff(a, b)
                claim("diff $k: $a to $b by $ps", Views.splice(ps, a) == b)
                if (a == b) claim("diff $k: equal lists, no patches", ps.isEmpty())
            }
        }
        println(
            "launch views: equal after a clean close (pending, a rebase, other roles), corrected when stale, refused when bound elsewhere or torn")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        run(File(args.getOrElse(0) { "../harken/domain/harken.ark" }))
        if (problems.isNotEmpty()) {
            println("launch views: ${problems.size} problem(s)")
            for (p in problems) println("  $p")
            kotlin.system.exitProcess(1)
        }
        println("launch views: every claim held")
    }
}
