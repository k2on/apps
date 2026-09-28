// The authoring vocabulary against the spec: the demo of AUTHORING.md
// Appendix B, authored in Kotlin (src/test/kotlin/demo), emitted and held to
// the module vector; and every procedure of it — and of harken's printed
// domain — run natively and by the interpreter over the emitted IR, which
// must agree on every answer, refusals included.
package dev.arkdb.conformance

import dev.arkdb.Args
import dev.arkdb.Canon
import dev.arkdb.Ctx
import dev.arkdb.Decode
import dev.arkdb.Encode
import dev.arkdb.Eval
import dev.arkdb.FnKind
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.MemoryStore
import dev.arkdb.Procedure
import dev.arkdb.SPEC_VERSION
import dev.arkdb.Value
import dev.arkdb.Verify
import java.io.File

object Authoring {
    class Failed(message: String) : AssertionError(message)

    private fun check(cond: Boolean, what: () -> String) {
        if (!cond) throw Failed(what())
    }

    private fun <T> eq(got: T, want: T, what: String) {
        if (got != want) throw Failed("$what:\n  got  $got\n  want $want")
    }

    fun run(root: File, test: (String, () -> Unit) -> Unit, skipped: MutableList<String>) {
        val demo = demo.module()
        test("authoring/demo: emits a module that verifies as it is") {
            val bytes = demo.emit()
            val back = Decode.fromValue(Canon.decode(bytes))
            eq(back, demo.ir, "decode of emit")
            eq(Encode.toValue(Verify.verify(back)), Encode.toValue(back), "emit is already the verified form")
            eq(back.spec, SPEC_VERSION, "spec version")
            eq(back.functions.map { it.name }, listOf("create_playlist", "add_to_playlist", "items"), "function order")
            eq(back.routers.map { it.name to it.uses }, listOf("demo" to emptyList<String>()), "routers")
            File(System.getProperty("java.io.tmpdir"), "ark-kotlin-demo.json").writeText(Encode.toValue(back).show())
        }
        val vector = File(root, "module/demo.json")
        val v = Json.parse(vector.readText(Charsets.UTF_8))
        val vspec = (v.field("module").field("spec") as Value.VInt).value
        if (vspec < SPEC_VERSION) {
            skipped.add("authoring/demo: emit == module/demo.json's bytes — the vector is spec $vspec, awaiting regeneration at spec $SPEC_VERSION")
        } else {
            test("authoring/demo: emit is module/demo.json's bytes") {
                eq(Hex.encode(demo.emit()), v.field("bytes").asText(), "module bytes")
                eq(Hex.encode(demo.hash), v.field("hash").asText(), "module hash")
            }
        }
        test("authoring/demo: native agrees with the interpreter on every procedure") { demoAgreement(demo) }
        test("authoring/harken: the printed domain builds, emits and verifies") {
            val h = harken.gen.module()
            val ir = Decode.fromValue(Canon.decode(h.emit()))
            eq(ir.functions.map { it.name to it.kind }, listOf(
                "library" to FnKind.Query,
                "signed_in" to FnKind.Guard,
                "owned" to FnKind.Provide,
                "create_playlist" to FnKind.Mutator,
                "add_to_playlist" to FnKind.Mutator,
                "remove_from_playlist" to FnKind.Mutator,
                "playlists" to FnKind.Query,
                "playlist_items" to FnKind.Query,
            ), "functions")
            eq(ir.functions.associate { it.name to it.uses }["add_to_playlist"], listOf("signed_in", "owned"), "add_to_playlist's chain")
            eq(ir.functions.associate { it.name to it.uses }["create_playlist"], listOf("signed_in"), "create_playlist's chain")
            File(System.getProperty("java.io.tmpdir"), "ark-kotlin-harken.json").writeText(Encode.toValue(ir).show())
        }
        test("authoring/harken: native agrees with the interpreter on every procedure") { harkenAgreement(harken.gen.module()) }
        test("authoring/self: a repeated auto, a statement in an expression, .on after something else") { selfRefusals() }
    }

    // Agreement ------------------------------------------------------------------

    private class Pair2(val m: dev.arkdb.authoring.Module) {
        val ir = m.ir
        val natives: Map<String, Procedure> = m.procedures().associate { it.second.name to it.second }
        var checked = 0

        /** Both ways, compared; the store after, for the next step. */
        fun step(name: String, ctx: Ctx, autos: Args, args: Args, st: MemoryStore): MemoryStore {
            val fn = ir.lookupFunction(name) ?: throw Failed("no $name")
            val c = Hash.closure(ir, fn)
            val native = natives.getValue(name)
            checked++
            return when (fn.kind) {
                FnKind.Mutator -> {
                    val byEval = Eval.applyClosure(ir.schema, c, ctx, autos, args, st)
                    val byNative = native.apply(ir.schema, ctx, autos, args, st)
                    when {
                        byEval is Eval.Applied.Ok && byNative is Eval.Applied.Ok -> {
                            eq(byNative.changes, byEval.changes, "$name $args: changes")
                            eq(byNative.store, byEval.store, "$name $args: store")
                            byEval.store
                        }
                        else -> {
                            eq(byNative, byEval, "$name $args: answer")
                            st
                        }
                    }
                }
                else -> {
                    eq(native.query(ir.schema, ctx, args, st), Eval.queryClosure(ir.schema, c, ctx, args, st), "$name $args: answer")
                    st
                }
            }
        }

        fun answer(name: String, ctx: Ctx, args: Args, st: MemoryStore): Eval.Answer =
            natives.getValue(name).query(ir.schema, ctx, args, st)
    }

    private fun idN(k: kotlin.Int): Value = Value.id(Id(ByteArray(16).also { it[15] = k.toByte() }))

    private fun demoAgreement(m: dev.arkdb.authoring.Module) {
        val p = Pair2(m)
        val alice = Ctx("alice", "a1")
        val bob = Ctx("bob", "b1")
        var st = MemoryStore(p.ir.schema)
        fun create(ctx: Ctx, id: kotlin.Int, name: String) {
            st = p.step("create_playlist", ctx, mapOf("id" to idN(id)), mapOf("name" to Value.text(name)), st)
        }
        fun add(ctx: Ctx, pl: kotlin.Int, track: String) {
            st = p.step("add_to_playlist", ctx, emptyMap(), mapOf("playlist_id" to idN(pl), "track_id" to Value.text(track)), st)
        }
        create(alice, 1, "  Road  ")
        create(alice, 2, "   ")            // refused: trimmed, then min 1
        create(alice, 3, "Road")           // a no-op: (user_id, name) matches
        create(bob, 4, "Road")             // another person's
        add(alice, 1, "t1")
        add(alice, 1, "t2")
        add(bob, 4, "t1")
        add(alice, 1, "t1")                // the key matches: a no-op
        add(alice, 9, "t3")                // refused: no such playlist (default message)
        add(alice, 1, "")                  // refused: track_id: at least 1 characters
        for (pl in listOf(1, 4, 9)) st = p.step("items", alice, emptyMap(), mapOf("playlist_id" to idN(pl)), st)
        // The answers themselves, not only their agreement.
        eq(st.scan("playlist").map { it["name"] }, listOf(Value.text("Road"), Value.text("Road")), "two playlists, one each")
        val items = p.answer("items", alice, mapOf("playlist_id" to idN(1)), st)
        eq((items as Eval.Answer.Ok).value.asList().map { it.field("track_id") to it.field("pos") }, listOf(Value.text("t1") to Value.int(1), Value.text("t2") to Value.int(2)), "items in pos order")
        val refusedExists = Eval.applyClosure(p.ir.schema, Hash.closure(p.ir, p.ir.lookupFunction("add_to_playlist")!!), alice, emptyMap(), mapOf("playlist_id" to idN(9), "track_id" to Value.text("x")), st)
        eq(refusedExists, Eval.Applied.Refused(dev.arkdb.Refusal.Refused("playlist_id: no such playlist")) as Eval.Applied, "the default message of exists")
        eq(p.checked, 13, "every step compared")
    }

    private fun harkenAgreement(m: dev.arkdb.authoring.Module) {
        val p = Pair2(m)
        val alice = Ctx("alice", "a1")
        val bob = Ctx("bob", "b1")
        val nobody = Ctx("", "n1")
        var st = MemoryStore(p.ir.schema)
        // Tracks arrive as facts; the phone's module has no add_track.
        for (k in 1..3) {
            st.applyChange(dev.arkdb.Change.Add("track", Value.record(
                "id" to idN(100 + k), "title" to Value.text("T$k"), "artist" to Value.text(if (k == 2) "A" else "B"),
                "album" to (if (k == 3) Value.VNull else Value.text("L")), "duration_ms" to Value.int(1000L * k),
                "file" to Value.text("f$k"), "added_ms" to Value.int(5), "user_id" to Value.text("scanner"),
            ) as Value.VStruct))
        }
        fun create(ctx: Ctx, id: kotlin.Int, name: String) {
            st = p.step("create_playlist", ctx, mapOf("id" to idN(id), "created_ms" to Value.int(7)), mapOf("name" to Value.text(name)), st)
        }
        fun on(verb: String, ctx: Ctx, pl: kotlin.Int, track: kotlin.Int) {
            val autos = if (verb == "add_to_playlist") mapOf("added_ms" to Value.int(8)) else emptyMap()
            st = p.step(verb, ctx, autos, mapOf("playlist_id" to idN(pl), "track_id" to idN(100 + track)), st)
        }
        create(nobody, 1, "Mine")          // refused: sign in first
        create(alice, 1, " Mix ")
        create(alice, 2, "x".repeat(121))  // refused: at most 120
        create(bob, 3, "Mix")
        on("add_to_playlist", alice, 1, 1)
        on("add_to_playlist", alice, 1, 2)
        on("add_to_playlist", bob, 1, 3)   // refused: not your playlist
        on("add_to_playlist", alice, 9, 3) // refused: no such playlist
        on("remove_from_playlist", alice, 1, 1)
        on("add_to_playlist", alice, 1, 1)
        for (ctx in listOf(alice, bob, nobody)) st = p.step("playlists", ctx, emptyMap(), emptyMap(), st)
        st = p.step("library", alice, emptyMap(), emptyMap(), st)
        for ((ctx, pl) in listOf(alice to 1, bob to 1, alice to 9)) st = p.step("playlist_items", ctx, emptyMap(), mapOf("playlist_id" to idN(pl)), st)
        val items = (p.answer("playlist_items", alice, mapOf("playlist_id" to idN(1)), st) as Eval.Answer.Ok).value
        eq(items.asList().map { it.field("track_id") to it.field("pos") }, listOf(idN(102) to Value.int(2), idN(101) to Value.int(3)), "after a remove, the next lands at the end")
        eq(p.answer("playlists", nobody, emptyMap(), st), Eval.Answer.Refused(dev.arkdb.Refusal.Refused("sign in first")) as Eval.Answer, "a query refused by its guard")
        val lib = (p.answer("library", alice, emptyMap(), st) as Eval.Answer.Ok).value
        eq(lib.asList().map { it.field("title") }, listOf(Value.text("T2"), Value.text("T3"), Value.text("T1")), "the library by artist, then album (null first)")
    }

    private fun selfRefusals() {
        fun fails(what: String, f: () -> Unit) {
            val ok = try {
                f()
                true
            } catch (e: dev.arkdb.Fault.Bug) {
                false
            }
            check(!ok) { "should have been refused: $what" }
        }
        fails("an auto drawn twice") { selfdemo.twice().ir }
        fails("a read inside map's closure") { selfdemo.readInMap().ir }
    }
}
