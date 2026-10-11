// The conformance runner (spec/RUNNERS.md): every vector
// spec/vectors/MANIFEST.json names, walked from the manifest. A vector of a
// kind this runtime claims is run, and must pass — or, marked
// `"expect": "fail"`, must fail its check; a vector the manifest marks
// `rust-only`, or of a kind not yet implemented, is skipped, by name, with
// the reason. It prints, per kind, what it ran and what it skipped, and
// exits non-zero on any failure, on a manifest out of step with the files,
// and on a claimed kind of which nothing ran. A plain `main`, so that it
// needs no test framework. `rust/ark/tests/vectors.rs` is the reference
// reading of each check.
package dev.arkdb.conformance

import dev.arkdb.Canon
import dev.arkdb.Hex
import dev.arkdb.ModuleError
import dev.arkdb.Value
import java.io.File
import kotlin.system.exitProcess

/**
 * A claim a check makes that does not hold: what `ensure!` returns as an `Err` in the reference.
 */
class Failed(message: String) : Exception(message)

fun ensure(ok: Boolean, why: () -> String) {
    if (!ok) throw Failed(why())
}

fun <T> ensureEq(got: T, want: T, why: String) {
    if (got != want) throw Failed("$why: ${show(got)} != ${show(want)}")
}

fun ensureBytes(got: ByteArray, want: ByteArray, why: String) {
    if (!got.contentEquals(want)) throw Failed("$why: ${Hex.encode(got)} != ${Hex.encode(want)}")
}

private fun show(x: Any?): String =
    when (x) {
        is Value -> x.show()
        is ByteArray -> Hex.encode(x)
        is List<*> -> x.joinToString(",", "[", "]") { show(it) }
        else -> x.toString()
    }

/** One vector as the manifest lists it. */
class Vector(
    val path: String,
    val kind: String,
    val form: String,
    val side: String,
    val expect: String
)

object Conformance {
    /** The kinds this runtime runs, in the manifest's order of kinds, each with its check. */
    private val claimed: Map<String, (Vector, Value) -> Unit> =
        linkedMapOf(
            "codec" to { _, v -> Checks.codec(v) },
            "order" to { _, v -> Checks.order(v) },
            "hash" to { _, v -> Checks.hash(v) },
        )

    /** The kinds a later round of docs/plan-phones.md turns on, and which. */
    private val notYet: Map<String, String> =
        linkedMapOf(
            "module" to "round 2, modules",
            "verify" to "round 2, modules",
            "eval" to "round 2, eval",
            "views" to "round 2, views",
            "protocol" to "round 3, the client",
            "rebase" to "round 3, the client",
            "trace" to "round 3, the client",
        )

    lateinit var root: File

    fun read(f: File): Value = Json.parse(f.readText(Charsets.UTF_8))

    /** A vector by its path under spec/vectors, parsed. */
    fun vector(path: String): Value = read(File(root, path))

    @JvmStatic
    fun main(args: Array<String>) {
        root = File(args.getOrElse(0) { "../spec/vectors" })
        val manifest = read(File(root, "MANIFEST.json"))
        val vectors =
            manifest.field("vectors").asList().map {
                Vector(
                    it.field("path").asText(),
                    it.field("kind").asText(),
                    it.field("form").asText(),
                    it.field("side").asText(),
                    it.field("expect").asText(),
                )
            }
        val problems = ArrayList<String>()
        // The manifest is the list: a file it names must be there, and a
        // `.json` it does not name is a manifest out of date.
        val named = vectors.map { it.path }.toSet()
        if (named.size != vectors.size) problems.add("MANIFEST.json lists a path twice")
        for (p in named) if (!File(root, p).isFile) problems.add("$p is listed and is not there")
        val onDisk =
            root
                .walkTopDown()
                .filter { it.isFile && it.name.endsWith(".json") }
                .map { it.relativeTo(root).invariantSeparatorsPath }
                .filter { it != "MANIFEST.json" }
                .toList()
        for (p in onDisk) if (p !in named) problems.add("$p is a vector the manifest does not name")

        val ran = LinkedHashMap<String, MutableList<String>>()
        val skipped = LinkedHashMap<String, MutableList<String>>()
        for (v in vectors) {
            val check = claimed[v.kind]
            when {
                v.side != "client" ->
                    skipped
                        .getOrPut(v.kind, ::ArrayList)
                        .add("${v.path} (${v.side}: Rust's simulator and its scheduler)")
                check == null ->
                    skipped
                        .getOrPut(v.kind, ::ArrayList)
                        .add(
                            "${v.path} (kind not implemented: ${notYet[v.kind] ?: "unknown kind"})")
                else -> {
                    val (failure, bug) = run(v, check)
                    val line =
                        when {
                            bug -> "${v.path}: BUG: $failure".also { problems.add(it) }
                            v.expect == "pass" && failure == null -> "${v.path}: pass"
                            v.expect == "fail" && failure != null ->
                                "${v.path}: fails as it must (${failure.take(160)})"
                            v.expect == "pass" ->
                                "${v.path}: FAILED: $failure".also { problems.add(it) }
                            else ->
                                "${v.path}: FAILED: a falsify vector passed"
                                    .also { problems.add(it) }
                        }
                    ran.getOrPut(v.kind, ::ArrayList).add(line)
                }
            }
        }
        val kinds = (vectors.map { it.kind }).distinct()
        for (k in kinds) {
            val r = ran[k].orEmpty()
            val s = skipped[k].orEmpty()
            println("$k: ran ${r.size}, skipped ${s.size}")
            for (line in r) println("  ran     $line")
            for (line in s) println("  skipped $line")
        }
        for (k in claimed.keys) if (ran[k].isNullOrEmpty())
            problems.add("$k is claimed and ran no vector")
        val total = ran.values.sumOf { it.size }
        val skippedTotal = skipped.values.sumOf { it.size }
        println(
            "conformance: ${vectors.size} vectors, $total run, $skippedTotal skipped; claimed: ${claimed.keys.joinToString(" ")}")
        if (problems.isNotEmpty()) {
            println("conformance: ${problems.size} problem(s)")
            for (p in problems) println("  $p")
            exitProcess(1)
        }
        println("conformance: every claimed vector passed, and every falsify vector failed")
    }

    // A check's first unmet claim, or null. What the runtime itself rejects —
    // bytes that are not canonical, a module of the wrong shape — is a claim
    // unmet too; anything else thrown is a bug in this runtime and is
    // reported as one, so that a falsify vector cannot pass by crashing.
    private fun run(v: Vector, check: (Vector, Value) -> Unit): Pair<String?, Boolean> =
        try {
            check(v, vector(v.path))
            null to false
        } catch (e: Failed) {
            (e.message ?: "failed") to false
        } catch (e: Canon.DecodeError) {
            "decode: ${e.why}" to false
        } catch (e: ModuleError) {
            "module: ${e.message}" to false
        } catch (e: Throwable) {
            ("${e.javaClass.simpleName}: ${e.message}\n" +
                e.stackTrace.take(12).joinToString("\n") { "      at $it" }) to true
        }
}
