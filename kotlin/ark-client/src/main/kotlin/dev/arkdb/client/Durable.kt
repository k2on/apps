// What a replica survives a restart in: one canonical-CBOR file holding the
// confirmed store, the cursor, the pending intents, the verdicts on this
// peer's own intents and — for a peer that is its own authority — the log
// it sequenced, so the authority can be rebuilt by adoption (§11.8) and
// proven against the store it left.
//
// java.io only: `File`, `FileOutputStream`, `FileInputStream`, a temp file
// and a rename, which is what Android offers and what a JVM has too.
package dev.arkdb.client

import dev.arkdb.Canon
import dev.arkdb.Entry
import dev.arkdb.Facts
import dev.arkdb.Id
import dev.arkdb.Log
import dev.arkdb.MemoryStore
import dev.arkdb.Protocol
import dev.arkdb.Schema
import dev.arkdb.Seq
import dev.arkdb.Value
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream

/** What became of an intent this peer authored, once the log answered it. */
public sealed class Verdict {
    /** Sequenced at `seq`. */
    public data class Confirmed(val seq: Seq) : Verdict()

    /** Refused, and why: the authority's sentence, or this peer's own on replay. */
    public data class Rejected(val reason: String) : Verdict()

    public fun toValue(): Value = when (this) {
        is Confirmed -> Value.record("t" to Value.text("confirmed"), "seq" to Value.int(seq))
        is Rejected -> Value.record("t" to Value.text("rejected"), "reason" to Value.text(reason))
    }

    public companion object {
        public fun fromValue(v: Value): Verdict = when (val t = v.field("t").asText()) {
            "confirmed" -> Confirmed(v.field("seq").asInt())
            "rejected" -> Rejected(v.field("reason").asText())
            else -> throw IllegalStateException("unknown verdict $t")
        }
    }
}

/** The replica's durable state. */
public class DurableReplica(
    public val cursor: Seq,
    public val confirmed: MemoryStore,
    public val pending: List<Entry>,
    /** The log this peer sequenced alone, or null when a server does. */
    public val log: List<Triple<Seq, Entry, Facts>>?,
    /** The verdicts on this peer's own intents, by entry id, oldest first. */
    public val verdicts: List<Pair<Id, Verdict>> = emptyList(),
) {
    public fun toValue(): Value = Value.record(
        "t" to Value.text("replica"),
        "cursor" to Value.int(cursor),
        "confirmed" to confirmed.toValue(),
        "pending" to Value.list(pending.map { Protocol.entryValue(it) }),
        "verdicts" to Value.list(verdicts.map { (i, v) -> Value.record("id" to Value.id(i), "verdict" to v.toValue()) }),
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
            if (got != n) throw IllegalStateException("durable log is not contiguous at $n")
        }
        return l
    }

    public companion object {
        public fun fromValue(schema: Schema, v: Value): DurableReplica {
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
            val verdicts = s.fields["verdicts"]?.asList()?.map { it.field("id").asId() to Verdict.fromValue(it.field("verdict")) }
            return DurableReplica(
                s["cursor"].asInt(),
                MemoryStore.of(schema, rows),
                s["pending"].asList().map { Protocol.entryFromValue(it) },
                log,
                verdicts ?: emptyList(),
            )
        }
    }
}

/** Files under one directory, written whole and renamed into place. */
public object Durable {
    public fun replicaFile(dir: File): File = File(dir, "log.replica")

    public fun deviceFile(dir: File): File = File(dir, "device")

    public fun readReplica(schema: Schema, dir: File): DurableReplica? {
        val f = replicaFile(dir)
        if (!f.isFile) return null
        return DurableReplica.fromValue(schema, Canon.decode(readAll(f)))
    }

    public fun writeReplica(dir: File, d: DurableReplica) {
        writeAtomically(replicaFile(dir), Canon.encode(d.toValue()))
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
