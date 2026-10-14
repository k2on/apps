// Launch views (`docs/plan-android.md`, "Launch views"): a maintained view
// whose first value may come from disk, so that an app's first screen draws
// before its peer has opened.
//
// A launch view is a query the app names with a stable key — harken names
// `library`, `playlists` and `albums`, what Home draws. Its output rows are
// kept in one file per key, beside the replica: written off the UI thread a
// beat after the last change and whenever the app goes to the background,
// read at the next launch before `Peer.open`, and handed to the screen as
// *provisional* — Zero's resultType `unknown`. Then the peer opens behind
// it, the live view hydrates, and the live result is compared with what
// was handed out: equal, and nothing is published at all; different, and
// the difference goes out as patches against the provisional list, so a
// list's scroll survives the correction. The live view always wins.
//
// **Why it is right, almost always.** The replica cannot change while the
// app is closed: nothing writes it but this process. So a file written at
// the replica's last position holds exactly what hydrating the same query
// over the same module and the same login will produce. What can make it
// stale is a crash between a write of the store and the write of the file,
// or a file from before an update: either costs one correction, never a
// wrong list kept.
//
// **What a file is bound to**, in a header checked before a row is read:
//
//     magic "ARKLV", version 1
//     module    SHA-256 of the module's bytes — they are its closures and
//               its schema, and they are known before anything is decoded
//     view      SHA-256 of the canonical (query name, arguments)
//     account   the user id, and a digest of the roles the login holds
//     position  the confirmed sequence, the last pending intent's id, and
//               a digest of the scope union the replica holds (`partial`)
//
// then the rows, canonical CBOR, then a SHA-256 of everything before it. A
// file whose version, module, view or account differs, whose checksum does
// not verify, or that does not decode, is deleted and ignored: the launch
// takes the ordinary path. The position is compared once the peer is open,
// and a mismatch is counted (`positionMismatches`) — it can only lead to
// the correction above. Each write goes to a temporary name in the same
// directory, is synced, and is renamed over the old file, so a reader
// never sees half of one.
package dev.arkdb.client

import dev.arkdb.Canon
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.Patch
import dev.arkdb.Sha256
import dev.arkdb.Value
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.atomic.AtomicInteger

/** What a launch view's file is bound to: a file under any other binding is not read. */
public data class LaunchBinding(
    /** SHA-256 of the module's bytes. */
    val module: ByteArray,
    /** The signed-in user. */
    val account: String,
    /** The roles the login holds, in any order. */
    val roles: Collection<String>,
) {
    internal val rolesDigest: ByteArray
        get() = Sha256.hash(Canon.encode(Value.VList(roles.sorted().map { Value.VText(it) })))

    override fun equals(other: Any?): Boolean =
        other is LaunchBinding &&
            module.contentEquals(other.module) &&
            account == other.account &&
            roles.sorted() == other.roles.sorted()

    override fun hashCode(): Int = account.hashCode()

    public companion object {
        /** The binding for a module's bytes and a login. */
        public fun of(
            module: ByteArray,
            account: String,
            roles: Collection<String>
        ): LaunchBinding = LaunchBinding(Sha256.hash(module), account, roles)
    }
}

/** Where a replica stood when a launch view was written: compared once the peer is open. */
public data class LaunchPosition(val cursor: Long, val lastPending: Id?, val partial: ByteArray) {
    override fun equals(other: Any?): Boolean =
        other is LaunchPosition &&
            cursor == other.cursor &&
            lastPending == other.lastPending &&
            partial.contentEquals(other.partial)

    override fun hashCode(): Int = cursor.hashCode()

    public companion object {
        /** Where `peer`'s replica stands now. */
        public fun of(peer: Peer): LaunchPosition {
            val r = peer.replica
            val partial =
                Sha256.hash(
                    Canon.encode(
                        r.partial?.let { p ->
                            Value.VStruct(
                                p.mapValues { (_, cs) -> Value.VList(cs.map { Value.VText(it) }) })
                        } ?: Value.VNull))
            return LaunchPosition(r.cursor, r.pending.lastOrNull()?.id, partial)
        }
    }
}

/** The files of an account's launch views: one directory, one file per key. */
public class LaunchViews(public val dir: File, public val binding: LaunchBinding) {
    /** Files read and refused (bound elsewhere, corrupt or torn), and deleted: for diagnostics. */
    public val refused: AtomicInteger = AtomicInteger()

    /** Files whose position was not where the opened replica stood: for diagnostics. */
    public val positionMismatches: AtomicInteger = AtomicInteger()

    /** A launch view of the query `name` with `args`, kept under `key`. */
    public fun view(key: String, name: String, args: Map<String, Value>): LaunchView =
        LaunchView(this, key, name, args)

    internal fun file(key: String): File =
        File(dir, Hex.encode(Sha256.hash(key.toByteArray(Charsets.UTF_8))).take(32) + ".view")

    /** Every file, gone: on signing out, or when the replica goes. */
    public fun clear() {
        dir.listFiles()?.forEach { it.delete() }
    }

    public companion object {
        internal val MAGIC: ByteArray = "ARKLV".toByteArray(Charsets.US_ASCII)
        internal const val VERSION: Int = 1

        /** The directory of `account`'s launch views beside a replica's directory. */
        public fun directory(replica: File, account: String): File =
            File(
                replica.parentFile,
                "launch/" + Hex.encode(Sha256.hash(account.toByteArray(Charsets.UTF_8))).take(32))

        /** SHA-256 of a view's identity: its query's name and its arguments, canonically. */
        public fun viewHash(name: String, args: Map<String, Value>): ByteArray =
            Sha256.hash(
                Canon.encode(
                    Value.record("query" to Value.VText(name), "args" to Value.VStruct(args))))

        /** A file's bytes: the header, the rows, the checksum. */
        internal fun encode(
            binding: LaunchBinding,
            view: ByteArray,
            at: LaunchPosition,
            rows: List<Value>
        ): ByteArray {
            val header =
                Canon.encode(
                    Value.record(
                        "module" to Value.VBytes(binding.module),
                        "view" to Value.VBytes(view),
                        "account" to Value.VText(binding.account),
                        "roles" to Value.VBytes(binding.rolesDigest),
                        "cursor" to Value.VInt(at.cursor),
                        "pending" to (at.lastPending?.let { Value.VId(it) } ?: Value.VNull),
                        "partial" to Value.VBytes(at.partial)))
            val body = Canon.encode(Value.VList(rows))
            val out = ByteArrayOutputStream(header.size + body.size + 64)
            out.write(MAGIC)
            out.write(VERSION)
            int(out, header.size)
            out.write(header)
            int(out, body.size)
            out.write(body)
            out.write(Sha256.hash(out.toByteArray()))
            return out.toByteArray()
        }

        private fun int(out: ByteArrayOutputStream, n: Int) {
            out.write(n ushr 24)
            out.write(n ushr 16)
            out.write(n ushr 8)
            out.write(n)
        }

        /**
         * The rows of a file and where it was written, when it is whole and bound to `binding` and
         * `view`; null otherwise. Never throws on what it reads.
         */
        internal fun decode(
            bytes: ByteArray,
            binding: LaunchBinding,
            view: ByteArray
        ): Pair<LaunchPosition, List<Value>>? {
            try {
                val m = MAGIC.size
                if (bytes.size < m + 1 + 4 + 4 + 32) return null
                if (!bytes.copyOfRange(0, m).contentEquals(MAGIC)) return null
                if (bytes[m].toInt() != VERSION) return null
                val end = bytes.size - 32
                val md = java.security.MessageDigest.getInstance("SHA-256")
                md.update(bytes, 0, end)
                val sum = md.digest()
                for (i in 0 until 32) if (sum[i] != bytes[end + i]) return null
                var at = m + 1
                fun int(): Int {
                    val n =
                        ((bytes[at].toInt() and 0xff) shl 24) or
                            ((bytes[at + 1].toInt() and 0xff) shl 16) or
                            ((bytes[at + 2].toInt() and 0xff) shl 8) or
                            (bytes[at + 3].toInt() and 0xff)
                    at += 4
                    return n
                }
                val hn = int()
                if (hn < 0 || at + hn > end) return null
                val h =
                    (Canon.decode(bytes.copyOfRange(at, at + hn)) as? Value.VStruct)?.fields
                        ?: return null
                at += hn
                fun bytesOf(k: String): ByteArray? = (h[k] as? Value.VBytes)?.value
                if (!(bytesOf("module") ?: return null).contentEquals(binding.module)) return null
                if (!(bytesOf("view") ?: return null).contentEquals(view)) return null
                if (h["account"] != Value.VText(binding.account)) return null
                if (!(bytesOf("roles") ?: return null).contentEquals(binding.rolesDigest))
                    return null
                val cursor = (h["cursor"] as? Value.VInt)?.value ?: return null
                val pending = (h["pending"] as? Value.VId)?.value
                val partial = bytesOf("partial") ?: return null
                val bn = int()
                if (bn < 0 || at + bn != end) return null
                val rows =
                    (Canon.decode(bytes.copyOfRange(at, at + bn)) as? Value.VList)?.items
                        ?: return null
                return LaunchPosition(cursor, pending, partial) to rows
            } catch (e: Exception) {
                return null
            }
        }

        /**
         * Patches that take `from` to `to` (`Views.splice` applies them in order): the common start
         * and end kept, the rows between updated in place, then the extra removed or the missing
         * inserted — a correction that leaves what did not move where it was.
         */
        public fun diff(from: List<Value>, to: List<Value>): List<Patch> {
            var head = 0
            val common = minOf(from.size, to.size)
            while (head < common && from[head] == to[head]) head++
            var tail = 0
            while (tail < common - head &&
                from[from.size - 1 - tail] == to[to.size - 1 - tail]) tail++
            val oldMid = from.size - head - tail
            val newMid = to.size - head - tail
            val out = ArrayList<Patch>()
            for (i in 0 until minOf(oldMid, newMid)) {
                if (from[head + i] != to[head + i]) out.add(Patch.Update(head + i, to[head + i]))
            }
            if (oldMid > newMid) repeat(oldMid - newMid) { out.add(Patch.Remove(head + newMid)) }
            for (i in oldMid until newMid) out.add(Patch.Insert(head + i, to[head + i]))
            return out
        }
    }
}

/**
 * One launch view: its file, read once before the peer opens ([provisional]); then the live view
 * over the peer ([live]), which answers against what was handed out; and its rows written back
 * ([capture], [write]).
 */
public class LaunchView
internal constructor(
    private val views: LaunchViews,
    public val key: String,
    public val name: String,
    public val args: Map<String, Value>,
) {
    private val file = views.file(key)
    private val hash = LaunchViews.viewHash(name, args)
    private var read: Pair<LaunchPosition, List<Value>>? = null

    /** The live view, once [live] has opened it. */
    public var view: ClientView? = null
        private set

    /**
     * The rows kept at the last write, if its file is there, whole, and bound to this module, this
     * view and this login — read and checked, on whatever thread calls it. A file that is not is
     * deleted. Null is the ordinary launch.
     */
    public fun provisional(): List<Value>? {
        val bytes =
            try {
                if (!file.isFile) return null
                file.readBytes()
            } catch (e: Exception) {
                return null
            }
        val got = LaunchViews.decode(bytes, views.binding, hash)
        if (got == null) {
            views.refused.incrementAndGet()
            file.delete()
            return null
        }
        read = got
        return got.second
    }

    /**
     * The live view, opened over `peer`, and what it says against `shown` — the provisional rows
     * the app published, or null if it published none: [Update.Unchanged] when they are the live
     * rows (publish nothing), [Update.Patched] with the difference when they are not, and
     * [Update.Reset] when nothing was shown. On the peer's thread.
     */
    public fun live(peer: Peer, shown: List<Value>?): Update {
        val v = peer.view(name, args)
        view = v
        read?.let { (at, _) ->
            if (at != LaunchPosition.of(peer)) views.positionMismatches.incrementAndGet()
        }
        if (shown == null) return Update.Reset
        if (shown == v.rows) return Update.Unchanged
        return Update.Patched(LaunchViews.diff(shown, v.rows))
    }

    /** What to write, taken on the peer's thread: the rows and where the replica stands. */
    public fun capture(peer: Peer): Captured? {
        val v = view ?: return null
        return Captured(LaunchPosition.of(peer), v.rows)
    }

    /** The rows and position [capture] took. */
    public class Captured
    internal constructor(internal val at: LaunchPosition, internal val rows: List<Value>)

    /** Write what [capture] took — off the UI thread, and off the peer's. Synced, then renamed. */
    public fun write(c: Captured) {
        val bytes = LaunchViews.encode(views.binding, hash, c.at, c.rows)
        val dir = file.parentFile
        if (!dir.isDirectory && !dir.mkdirs()) throw StorageError("cannot make $dir")
        val tmp = File(dir, file.name + ".tmp")
        FileOutputStream(tmp).use {
            it.write(bytes)
            it.fd.sync()
        }
        if (!tmp.renameTo(file)) {
            file.delete()
            if (!tmp.renameTo(file)) throw StorageError("cannot rename $tmp to $file")
        }
    }
}
