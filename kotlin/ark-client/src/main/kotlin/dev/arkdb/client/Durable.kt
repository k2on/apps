// What a replica survives a restart in: one canonical-CBOR file per scope
// holding the confirmed store, the cursor, the pending intents and — for a
// peer that is its own authority — the log it sequenced, so the authority
// can be rebuilt by adoption (§11.8) and proven against the store it left.
//
// java.io only: `File`, `FileOutputStream`, `FileInputStream`, a temp file
// and a rename, which is what Android offers and what a JVM has too.
package dev.arkdb.client

import dev.arkdb.Canon
import dev.arkdb.Entry
import dev.arkdb.Facts
import dev.arkdb.Log
import dev.arkdb.MemoryStore
import dev.arkdb.Protocol
import dev.arkdb.Schema
import dev.arkdb.Seq
import dev.arkdb.Value
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream

/** One scope's durable state. */
public class DurableScope(
    public val scope: String,
    public val cursor: Seq,
    public val confirmed: MemoryStore,
    public val pending: List<Entry>,
    /** The log this peer sequenced alone, or null when a server does. */
    public val log: List<Triple<Seq, Entry, Facts>>?,
) {
    public fun toValue(): Value = Value.record(
        "t" to Value.text("replica"),
        "scope" to Value.text(scope),
        "cursor" to Value.int(cursor),
        "confirmed" to confirmed.toValue(),
        "pending" to Value.list(pending.map { Protocol.entryValue(it) }),
        "log" to (
            log?.let { l ->
                Value.list(
                    l.map { (n, e, f) ->
                        Value.record("seq" to Value.int(n), "entry" to Protocol.entryValue(e), "facts" to Protocol.factsValue(f))
                    },
                )
            } ?: Value.VNull
            ),
    )

    /** The log as a `Log`, for `Authority.adopt`. */
    public fun asLog(schema: Schema): Log {
        val l = Log(schema)
        for ((n, e, f) in log ?: emptyList()) {
            val got = l.append(e, f)
            if (got != n) throw IllegalStateException("durable log of $scope is not contiguous at $n")
        }
        return l
    }

    public companion object {
        public fun fromValue(schema: Schema, v: Value): DurableScope {
            val s = v.asStruct()
            if (s["t"] != Value.text("replica")) throw IllegalStateException("not a replica file")
            val rows = s["confirmed"].asStruct().fields.mapValues { it.value.asList() }
            val log = s["log"].let { l ->
                if (l.isNull()) {
                    null
                } else {
                    l.asList().map { item ->
                        Triple(
                            item.field("seq").asInt(),
                            Protocol.entryFromValue(item.field("entry")),
                            item.field("facts").asList().map { Protocol.changeFromValue(it) },
                        )
                    }
                }
            }
            return DurableScope(
                s["scope"].asText(),
                s["cursor"].asInt(),
                MemoryStore.of(schema, rows),
                s["pending"].asList().map { Protocol.entryFromValue(it) },
                log,
            )
        }
    }
}

/** Files under one directory, written whole and renamed into place. */
public object Durable {
    public fun scopeFile(dir: File, scope: String): File = File(dir, "$scope.replica")

    public fun deviceFile(dir: File): File = File(dir, "device")

    public fun readScope(schema: Schema, dir: File, scope: String): DurableScope? {
        val f = scopeFile(dir, scope)
        if (!f.isFile) return null
        return DurableScope.fromValue(schema, Canon.decode(readAll(f)))
    }

    public fun writeScope(dir: File, d: DurableScope) {
        writeAtomically(scopeFile(dir, d.scope), Canon.encode(d.toValue()))
    }

    public fun readAll(f: File): ByteArray = FileInputStream(f).use { it.readBytes() }

    /** Write to a temp file beside the target, sync it, and rename it over the target. */
    public fun writeAtomically(target: File, bytes: ByteArray) {
        target.parentFile?.mkdirs()
        val tmp = File(target.parentFile, target.name + ".tmp")
        FileOutputStream(tmp).use { out ->
            out.write(bytes)
            out.flush()
            out.fd.sync()
        }
        if (!tmp.renameTo(target)) {
            // A platform that refuses to rename over an existing file.
            if (target.exists() && !target.delete()) throw java.io.IOException("could not replace ${target.path}")
            if (!tmp.renameTo(target)) throw java.io.IOException("could not rename ${tmp.path} to ${target.path}")
        }
    }
}
