// The conformance runner: every vector under spec/vectors, each directory
// through the part of the runtime it holds, and every `falsify/` vector
// asserted to FAIL. A plain `main`, so that Gradle and the kotlinc fallback
// run the very same code.
package dev.arkdb.conformance

import dev.arkdb.Args
import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.Change
import dev.arkdb.Changes
import dev.arkdb.Client
import dev.arkdb.ClientMsg
import dev.arkdb.CmpOp
import dev.arkdb.Column
import dev.arkdb.Ctx
import dev.arkdb.Decode
import dev.arkdb.Dir
import dev.arkdb.Encode
import dev.arkdb.Entry
import dev.arkdb.Eval
import dev.arkdb.Expr
import dev.arkdb.Facts
import dev.arkdb.Fault
import dev.arkdb.FnHash
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.HsShow
import dev.arkdb.Id
import dev.arkdb.Log
import dev.arkdb.MemoryStore
import dev.arkdb.Mode
import dev.arkdb.Module
import dev.arkdb.Op
import dev.arkdb.Ops
import dev.arkdb.Page
import dev.arkdb.Patch
import dev.arkdb.Plan
import dev.arkdb.Pred
import dev.arkdb.Protocol
import dev.arkdb.Refusal
import dev.arkdb.Relation
import dev.arkdb.Replica
import dev.arkdb.Row
import dev.arkdb.SPEC_VERSION
import dev.arkdb.Schema
import dev.arkdb.Scope
import dev.arkdb.Sequenced
import dev.arkdb.ServerMsg
import dev.arkdb.Std
import dev.arkdb.Stmt
import dev.arkdb.Store
import dev.arkdb.Subscription
import dev.arkdb.Table
import dev.arkdb.Ty
import dev.arkdb.Value
import dev.arkdb.ValueOrder
import dev.arkdb.Verify
import dev.arkdb.View
import java.io.File
import kotlin.system.exitProcess

object Conformance {
    class Failed(message: String) : AssertionError(message)

    private fun check(cond: Boolean, what: () -> String) {
        if (!cond) throw Failed(what())
    }

    private fun <T> eq(got: T, want: T, what: String) {
        if (got != want) throw Failed("$what:\n  got  $got\n  want $want")
    }

    private fun eqBytes(got: ByteArray, want: ByteArray, what: String) {
        if (!got.contentEquals(want)) throw Failed("$what:\n  got  ${Hex.encode(got)}\n  want ${Hex.encode(want)}")
    }

    private fun read(f: File): Value = Json.parse(f.readText(Charsets.UTF_8))

    private class Result(val name: String, val error: Throwable?)

    private val results = ArrayList<Result>()
    private val skipped = ArrayList<String>()

    /** The vectors predate spec 2 (see `run`). */
    private var legacy = false

    /** A spec-1 module as spec 2 would say it: one router per scope, each mutator on its scope's. */
    fun upgrade(m: Module): Module {
        if (m.spec >= SPEC_VERSION) return m
        val routers = m.schema.scopes.map { dev.arkdb.Router(it.name, it.name, emptyList()) }
        return m.copy(
            spec = SPEC_VERSION,
            routers = routers,
            functions = m.functions.map { f -> if (f.kind == dev.arkdb.FnKind.Mutator) f.copy(router = f.scope) else f },
        )
    }

    private fun test(name: String, body: () -> Unit) {
        val err = try {
            body()
            null
        } catch (t: Throwable) {
            t
        }
        results.add(Result(name, err))
        if (err == null) {
            println("  ok    $name")
        } else {
            println("  FAIL  $name\n        ${err.toString().lines().joinToString("\n        ")}")
        }
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val root = File(args.getOrNull(0) ?: "../spec/vectors")
        check(root.isDirectory) { "not a directory of vectors: ${root.absolutePath}" }
        println("vectors: ${root.absolutePath}")
        run(root)
        val failed = results.filter { it.error != null }
        println()
        println("${results.size - failed.size} passed, ${failed.size} failed, ${skipped.size} skipped")
        for (s in skipped) println("  skipped: $s")
        if (failed.isNotEmpty()) {
            for (f in failed) println("  failed: ${f.name}")
            exitProcess(1)
        }
    }

    private fun vectors(dir: File): List<File> =
        (dir.listFiles() ?: emptyArray()).filter { it.isFile && it.name.endsWith(".json") }.sortedBy { it.name }

    fun run(root: File) {
        // The demo module every non-codec directory is about; the hash
        // vector carries no schema, so it takes the eval vector's.
        val demoModule = Decode.fromValue(read(File(root, "eval/add-to-playlist.json")).field("module"))
        // Vectors written at spec 1 decode (args as plain fields, put as an
        // upsert on the key) but hash differently at spec 2; until they are
        // regenerated, the checks that compare hashes are skipped and the
        // rest run against the module as read.
        legacy = demoModule.spec < SPEC_VERSION
        if (legacy) skipped.add("vectors are spec ${demoModule.spec}; hash and byte-identity checks of modules skipped until they are regenerated at spec $SPEC_VERSION")

        val kinds: Map<String, (File) -> Unit> = mapOf(
            "codec" to ::codec,
            "order" to ::order,
            "hash" to { f -> hash(f, demoModule.schema) },
            "module" to ::module,
            "protocol" to ::protocol,
            "eval" to ::eval,
            "verify" to ::verify,
            "views" to ::views,
            "rebase" to ::rebase,
        )
        for ((kind, checkOne) in kinds) {
            val dir = File(root, kind)
            for (f in vectors(dir)) test("$kind/${f.name}") { checkOne(f) }
            for (f in vectors(File(dir, "falsify"))) test("$kind/falsify/${f.name} (must fail)") {
                val failure = try {
                    checkOne(f)
                    null
                } catch (t: Throwable) {
                    t
                }
                check(failure != null) { "a deliberately wrong vector passed" }
            }
        }
        test("codec/self: the decoder refuses each non-canonical shape") { codecRefusals() }
        test("store/self: a put may omit nullable columns and nothing else") { storeComplete() }
        test("std/self: unicode goes through the tables, arithmetic is checked") { stdSelf() }
        test("verify/self: a module that must not verify does not") { verifyRefuses(upgrade(demoModule)) }
        test("eval/self: checks, middleware, insert, upsert and update") { EvalSelf.run() }
        Authoring.run(root, ::test, skipped)
    }

    // codec/ -------------------------------------------------------------------

    private fun codec(f: File) {
        val v = read(f)
        val value = v.field("value")
        val bytes = Hex.decode(v.field("bytes").asText())
        eqBytes(Canon.encode(value), bytes, "encode")
        eq(Canon.decode(bytes), value, "decode")
        check(Canon.roundTrip(bytes)) { "roundTrip" }
    }

    private fun codecRefusals() {
        fun refuses(hex: String, why: Canon.Why) {
            val got = try {
                Canon.decode(Hex.decode(hex))
                null
            } catch (e: Canon.DecodeError) {
                e.why
            }
            eq(got, why, "decode $hex")
        }
        refuses("1801", Canon.Why.NonCanonicalHead) // 1 with a one-byte argument
        refuses("1900ff", Canon.Why.NonCanonicalHead) // 255 with a two-byte argument
        refuses("1c", Canon.Why.NonCanonicalHead) // reserved additional information
        refuses("9fff", Canon.Why.IndefiniteLength)
        refuses("5f", Canon.Why.IndefiniteLength)
        refuses("ff", Canon.Why.IndefiniteLength) // a stray break
        refuses("a2616201616102", Canon.Why.UnsortedKeys)
        refuses("a2616101616102", Canon.Why.DuplicateKey)
        refuses("a26161016162", Canon.Why.Truncated)
        refuses("a10102", Canon.Why.NonTextKey)
        refuses("a2616101626161", Canon.Why.Truncated)
        refuses("a262616101616102", Canon.Why.UnsortedKeys) // "aa" before "b": length-first order
        refuses("d82650" + "00".repeat(16), Canon.Why.BadTag)
        refuses("d9002550" + "00".repeat(16), Canon.Why.NonCanonicalHead)
        refuses("d8254100", Canon.Why.BadId)
        refuses("d82501", Canon.Why.BadId)
        refuses("f90000", Canon.Why.Float)
        refuses("fa00000000", Canon.Why.Float)
        refuses("fb0000000000000000", Canon.Why.Float)
        refuses("f7", Canon.Why.BadSimple)
        refuses("f820", Canon.Why.BadSimple)
        refuses("61ff", Canon.Why.BadUtf8)
        refuses("63eda080", Canon.Why.BadUtf8) // an encoded surrogate
        refuses("62c080", Canon.Why.BadUtf8) // overlong
        refuses("1b8000000000000000", Canon.Why.IntOutOfRange)
        refuses("3b8000000000000000", Canon.Why.IntOutOfRange)
        refuses("0102", Canon.Why.Trailing)
        refuses("18", Canon.Why.Truncated)
        refuses("", Canon.Why.Truncated)
        eq(Canon.decode(Hex.decode("3b7fffffffffffffff")), Value.VInt(Long.MIN_VALUE), "min int")
        eq(Canon.decode(Hex.decode("1b7fffffffffffffff")), Value.VInt(Long.MAX_VALUE), "max int")
    }

    // order/ -------------------------------------------------------------------

    private fun order(f: File) {
        val v = read(f)
        val input = v.field("input").asList()
        val sorted = v.field("sorted").asList()
        eq(input.sortedWith(ValueOrder), sorted, "sorted")
        for (a in input) for (b in input) {
            val ab = ValueOrder.compare(a, b)
            val ba = ValueOrder.compare(b, a)
            check(Integer.signum(ab) == -Integer.signum(ba)) { "antisymmetry of ${a.show()} and ${b.show()}" }
            check((ab == 0) == (a == b)) { "equality agrees with the order for ${a.show()} and ${b.show()}" }
        }
    }

    // hash/ --------------------------------------------------------------------

    private fun storeOf(schema: Schema, v: Value): MemoryStore =
        MemoryStore.of(schema, v.asStruct().fields.mapValues { it.value.asList() })

    private fun hash(f: File, schema: Schema) {
        val v = read(f)
        val st = storeOf(schema, v.field("store"))
        eq(Hex.encode(Hash.stateHash(st)), v.field("hash").asText(), "state hash")
    }

    // module/ ------------------------------------------------------------------

    private fun module(f: File) {
        val v = read(f)
        val mv = v.field("module")
        val bytes = Hex.decode(v.field("bytes").asText())
        val m = Decode.fromValue(mv)
        eqBytes(Canon.encode(mv), bytes, "module bytes")
        eq(Canon.decode(bytes), mv, "module from bytes")
        if (legacy) return
        eq(Encode.toValue(m), mv, "decode then encode")
        eq(Hex.encode(Hash.moduleHash(m)), v.field("hash").asText(), "module hash")
        eq(Decode.fromValue(Encode.toValue(m)), m, "decode of encode is the identity")
    }

    // protocol/ ----------------------------------------------------------------

    private fun protocol(f: File) {
        val v = read(f)
        val frame = v.field("frame")
        val bytes = Hex.decode(v.field("bytes").asText())
        eqBytes(Canon.encode(frame), bytes, "frame bytes")
        eq(Canon.decode(bytes), frame, "frame from bytes")
        when {
            f.name.startsWith("client-") -> {
                val m = Protocol.clientFromValue(frame)
                eq(m.toValue(), frame, "client frame: fromValue then toValue")
                eq(Protocol.clientFromValue(m.toValue()), m, "client frame: decode of encode")
            }
            f.name.startsWith("server-") -> {
                val m = Protocol.serverFromValue(frame)
                eq(m.toValue(), frame, "server frame: fromValue then toValue")
                if (m !is ServerMsg.Closures) eq(Protocol.serverFromValue(m.toValue()), m, "server frame: decode of encode")
            }
            else -> throw Failed("neither a client nor a server frame: ${f.name}")
        }
    }

    // eval/ --------------------------------------------------------------------

    private fun ctxOf(v: Value): Ctx = Ctx(v.field("user").asText(), v.field("session").asText())

    private fun argsOf(v: Value): Args = v.asStruct().fields

    private fun eval(f: File) {
        val v = read(f)
        val m = Decode.fromValue(v.field("module"))
        val name = v.field("function").asText()
        val fn = m.lookupFunction(name) ?: throw Failed("no function $name")
        val closure = Hash.closure(m, fn)
        val hash = Hash.functionHash(closure)
        eq(Hash.closures(m)[hash]?.fn?.name, name, "closures(module) holds the function under its hash")
        if (!legacy) {
            // The module's function hashes to what every entry in the protocol
            // and rebase vectors names it by.
            val pushed = Protocol.clientFromValue(read(File(f.parentFile.parentFile, "protocol/client-push.json")).field("frame")) as ClientMsg.Push
            eq(hash.hex, pushed.entries.single().fn.hex, "the function's hash is the one the protocol vectors' entries carry")
            eq(hash.hex, v.field("function_hash").asText(), "function hash")
        }
        val ctx = ctxOf(v.field("ctx"))
        val autos = argsOf(v.field("autos"))
        var st = storeOf(m.schema, v.field("store_before"))
        for ((i, step) in v.field("steps").asList().withIndex()) {
            val args = argsOf(step.field("args"))
            // Through the interpreter, by closure (as an entry replays)…
            val applied = Eval.applyClosure(m.schema, closure, ctx, autos, args, st)
            check(applied is Eval.Applied.Ok) { "step $i refused: $applied" }
            applied as Eval.Applied.Ok
            eq(Value.VList(applied.changes.map { Protocol.changeValue(it) }), step.field("changes"), "step $i changes")
            eq(applied.store.toValue(), step.field("store_after"), "step $i store")
            eq(Hex.encode(Hash.stateHash(applied.store)), step.field("hash_after").asText(), "step $i hash")
            // …and by name.
            val byName = Eval.apply(m, name, ctx, autos, args, st)
            eq(byName, applied, "step $i by name")
            st = applied.store
        }
    }

    // verify/ ------------------------------------------------------------------

    private fun verify(f: File) {
        val v = read(f)
        val mv = v.field("module")
        val m = Decode.fromValue(mv)
        if (legacy) {
            skipped.add("verify/${f.name}: a spec-1 module does not verify at spec $SPEC_VERSION")
            return
        }
        val want = v.field("verifies").asBool()
        val verified = try {
            Verify.verify(m)
        } catch (e: Verify.VerifyFailed) {
            check(!want) { "does not verify: ${e.errors}" }
            return
        }
        check(want) { "verified a module that must not" }
        // The vector's module is already verified (orders completed,
        // symbols renumbered), so verifying it again is the identity.
        eq(Encode.toValue(verified), mv, "verify is the identity on a verified module")
    }

    private fun verifyRefuses(m: Module) {
        fun refused(what: String, m2: Module) {
            val ok = try {
                Verify.verify(m2)
                true
            } catch (e: Verify.VerifyFailed) {
                false
            }
            check(!ok) { "verified: $what" }
        }
        refused("wrong spec version", m.copy(spec = m.spec + 1))
        refused("duplicate function", m.copy(functions = m.functions + m.functions.first()))
        // A put whose `pos` is text: `add_to_playlist`'s row no longer types.
        val add = m.lookupFunction("add_to_playlist")!!
        val badBody = add.body.map { s ->
            val row = (s as? Stmt.Upsert)?.row
            if (s is Stmt.Upsert && row is Expr.Struct) {
                Stmt.Upsert(s.table, Expr.Struct(row.fields + ("pos" to Expr.Lit(Value.VText("nine")))), s.on)
            } else {
                s
            }
        }
        refused("a put with a column of the wrong type", m.copy(functions = m.functions.map { if (it.name == add.name) it.copy(body = badBody) else it }))
        // An unknown column in a put.
        val extraBody = add.body.map { s ->
            val row = (s as? Stmt.Upsert)?.row
            if (s is Stmt.Upsert && row is Expr.Struct) {
                Stmt.Upsert(s.table, Expr.Struct(row.fields + ("bogus" to Expr.Lit(Value.VInt(1)))), s.on)
            } else {
                s
            }
        }
        refused("a put with an unknown column", m.copy(functions = m.functions.map { if (it.name == add.name) it.copy(body = extraBody) else it }))
        // A read inside a helper.
        val helper = add.copy(name = "peek", kind = dev.arkdb.FnKind.Helper, scope = null, router = null, autos = emptyList(), ret = Ty.TBool,
            body = listOf(Stmt.Let(0, Expr.Exists("playlist", listOf(Expr.Arg("playlist_id")))), Stmt.Return(Expr.Var(0))))
        refused("a read in a helper", m.copy(functions = listOf(helper) + m.functions))
        // A refuse outside a mutator.
        val q = add.copy(name = "q", kind = dev.arkdb.FnKind.Query, autos = emptyList(), ret = Ty.TBool,
            body = listOf(Stmt.Refuse(Expr.Lit(Value.VText("no")))))
        refused("a refuse in a query", m.copy(functions = m.functions + q))
        // And the demo module with its orders stripped verifies back to the vector's form.
        val stripped = m.copy(functions = m.functions.map { fn ->
            fn.copy(body = fn.body.map { s ->
                val e = (s as? Stmt.Let)?.e
                if (s is Stmt.Let && e is Expr.Select) Stmt.Let(s.sym, Expr.Select(e.plan.copy(order = e.plan.order.take(1)))) else s
            })
        })
        eq(Encode.toValue(Verify.verify(stripped)), Encode.toValue(m), "verify completes the orders")
    }

    // views/ -------------------------------------------------------------------

    private val pid = Id(ByteArray(16).also { it[15] = 1 })

    // The two plans the vectors were made from, rebuilt by hand; the vector's
    // `plan` string (Haskell's `show`) pins that the rebuild is the right one.
    private fun viewPlan(name: String): Plan = when (name) {
        "top-two-by-pos" -> Plan.from("playlist_item")
            .filter(Pred.cmp("playlist_id", CmpOp.Eq, Value.id(pid)))
            .orderBy("pos", Dir.Asc)
            .limit(2)
        "playlist-with-items" -> Plan.from("playlist")
            .orderBy("name", Dir.Asc)
            .related("items", "playlist", "playlist_item", "playlist_id", Plan.from("playlist_item").orderBy("pos", Dir.Desc).limit(3))
        else -> throw Failed("no hand-built plan for the view vector $name")
    }

    private fun views(f: File) {
        val v = read(f)
        val m = Decode.fromValue(v.field("module"))
        val sch = m.schema
        val plan = viewPlan(f.name.removeSuffix(".json"))
        eq(hsPlan(plan), v.field("plan").asText(), "the hand-built plan is the vector's")
        val changes = v.field("changes").asList().map { step -> step.asList().map { Protocol.changeFromValue(it) } }
        val steps = v.field("steps").asList()
        eq(changes.size, steps.size, "one step per change list")
        val st = MemoryStore(sch)
        var view = View.hydrate(sch, plan, st)
        for ((i, facts) in changes.withIndex()) {
            val before = view.rows
            val patches = ArrayList<Patch>()
            for (ch in facts) {
                st.applyChange(ch)
                val (v2, ps) = view.push(sch, st, ch)
                view = v2
                patches.addAll(ps)
            }
            val step = steps[i]
            eq(Value.VList(patches.map { it.toValue() }), step.field("patches"), "step $i patches")
            eq(Value.VList(view.rows), step.field("rows"), "step $i rows")
            check(view.contract(sch, st)) { "step $i: the contract" }
            eq(View.splice(patches, before), view.rows, "step $i: splicing the patches gives the rows")
        }
    }

    // Haskell's `show` of a ViewPlan, for the check above.
    private fun hsPlan(p: Plan): String = "ViewPlan {vpTable = ${HsShow.text(p.table)}, vpFilter = ${hsMaybe(p.filter) { hsFilter(it) }}, " +
        "vpOrder = [${p.order.joinToString(",") { (c, d) -> "(${HsShow.text(c)},$d)" }}], vpLimit = ${hsMaybe(p.limit) { it.toString() }}, " +
        "vpRelated = [${p.related.joinToString(",") { r -> "(${HsShow.text(r.name)},${hsRelation(r.relation)},${hsPlan(r.plan)})" }}]}"

    private fun hsRelation(r: Relation): String =
        "Relation {relParent = ${HsShow.text(r.parent)}, relChild = ${HsShow.text(r.child)}, relColumn = ${HsShow.text(r.column)}}"

    private fun <A> hsMaybe(x: A?, show: (A) -> String): String {
        if (x == null) return "Nothing"
        val s = show(x)
        return if (s.contains(' ') && !s.startsWith("(") && !s.startsWith("[") && !s.startsWith("\"")) "Just ($s)" else "Just $s"
    }

    private fun hsFilter(f: Pred): String = when (f) {
        is Pred.Cmp -> "FCmp ${HsShow.text(f.column)} ${f.op} ${hsValueP(f.value)}"
        is Pred.In -> "FIn ${HsShow.text(f.column)} [${f.values.joinToString(",") { hsValue(it) }}]"
        is Pred.All -> "FAll [${f.items.joinToString(",") { hsFilter(it) }}]"
        is Pred.Any -> "FAny [${f.items.joinToString(",") { hsFilter(it) }}]"
        is Pred.Not -> "FNot (${hsFilter(f.p)})"
    }

    private fun hsValueP(v: Value): String = if (v is Value.VNull) "VNull" else "(${hsValue(v)})"

    private fun hsValue(v: Value): String = when (v) {
        is Value.VNull -> "VNull"
        is Value.VBool -> "VBool ${if (v.value) "True" else "False"}"
        is Value.VInt -> if (v.value < 0) "VInt (${v.value})" else "VInt ${v.value}"
        is Value.VText -> "VText ${HsShow.text(v.value)}"
        is Value.VBytes -> "VBytes ${HsShow.bytes(v.value)}"
        is Value.VId -> "VId (IdBytes ${HsShow.bytes(v.value.bytes)})"
        is Value.VList -> "VList [${v.items.joinToString(",") { hsValue(it) }}]"
        is Value.VStruct -> "VStruct (fromList [${v.fields.entries.joinToString(",") { "(${HsShow.text(it.key)},${hsValue(it.value)})" }}])"
    }

    // rebase/ ------------------------------------------------------------------

    private fun rebase(f: File) {
        when (f.name) {
            "three-peers.json" -> threePeers(f)
            else -> {
                skipped.add("rebase/${f.name}: a seeded fleet needs the server machine and Ark.Sim, which this runtime does not carry")
            }
        }
    }

    private fun posOf(st: Store, k: Int): Value {
        val row = st.get("playlist_item", listOf(Value.id(pid), Value.bytesHex("%02x".format(k))))
        return if (row is Row) row["pos"] else Value.VNull
    }

    private fun idN(k: Int): Id = Id(ByteArray(16).also { it[15] = k.toByte() })

    private fun threePeers(f: File) {
        val v = read(f)
        val m = Decode.fromValue(v.field("module"))
        val sch = m.schema
        val scope = "playlists"
        val bodies = Hash.closures(m)
        // A spec-1 vector's entries name spec-1 hashes: renamed to this
        // module's, by which function's input their arguments are.
        val byArgs = bodies.entries.associate { (h, c) -> c.fn.input.map { it.first }.toSet() to h }
        val entries = v.field("entries").asList().map { ev ->
            val seq = ev.field("seq").asInt()
            val fields = ev.asStruct().fields.filterKeys { it != "seq" }
            val e = Protocol.entryFromValue(Value.VStruct(fields))
            seq to if (legacy) e.copy(fn = byArgs.getValue(e.args.keys)) else e
        }
        val facts: List<Facts> = v.field("facts").asList().map { fl -> fl.asList().map { Protocol.changeFromValue(it) } }
        val finalHash = v.field("final_hash").asText()
        val finalStore = v.field("final_store")
        eq(entries.size, facts.size, "facts per entry")
        fun hashOf(r: Replica) = Hex.encode(r.verifyAt().second)

        // (a) An authority replays the entries in order and lands on the final hash.
        val auth = Authority(scope, sch, bodies)
        for ((i, se) in entries.withIndex()) {
            val (n, e) = se
            val s = auth.sequenceEntry(e)
            check(s is Sequenced.Appended) { "entry $n was not appended: $s" }
            s as Sequenced.Appended
            eq(s.seq, n, "entry $n sequence")
            eq(s.facts, facts[i], "entry $n facts")
            eq(auth.sequenceEntry(e), Sequenced.Duplicate(n) as Sequenced, "a re-pushed entry is a duplicate")
        }
        eq(Hex.encode(Hash.stateHash(auth.store)), finalHash, "authority's final hash")
        eq(auth.store.toValue(), finalStore, "authority's final store")
        eq(auth.log.stateAt(auth.log.headSeq)?.let { Hex.encode(Hash.stateHash(it)) }, finalHash, "the state at the head, from facts")
        check(auth.log.contiguous) { "the log is contiguous" }

        // (b) A replica with no closures at all applies by facts.
        val carol = Replica.open(sch, scope, emptyMap(), MemoryStore(sch), 0, emptyList())
        eq(carol.takeChanges(), Changes.Rebuilt as Changes, "a freshly opened replica reports a rebuild once")
        for ((n, e) in entries) carol.receive(n, e)
        eq(carol.needs(), entries.map { it.first }, "without closures carol asks for every entry's facts")
        eq(carol.cursor, 0L, "nothing applied without facts")
        for ((i, se) in entries.withIndex()) carol.receiveFacts(se.first, facts[i])
        eq(carol.cursor, entries.last().first, "carol's cursor")
        eq(hashOf(carol), finalHash, "by facts alone carol reaches the final hash")
        check(carol.diverged.isEmpty()) { "carol did not diverge" }
        eq(carol.takeChanges(), Changes.Applied(facts.flatten()) as Changes, "with nothing pending the changes are the facts")

        // (c) A replica with the closures replays by intent, out of order, and agrees.
        val frank = Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList())
        for ((n, e) in entries.reversed()) frank.receive(n, e)
        eq(hashOf(frank), finalHash, "frank replays by intent, delivered in reverse")
        check(frank.needs().isEmpty() && frank.diverged.isEmpty()) { "frank needs nothing" }
        val grace = Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList())
        for ((i, se) in entries.withIndex()) grace.receiveWith(se.first, se.second, facts[i])
        eq(hashOf(grace), finalHash, "grace replays and compares")
        check(grace.diverged.isEmpty()) { "grace's replay agreed with the facts" }

        // (d) The scenario the vector was written from: alice, bob, a rebase.
        val e1 = entries[0].second
        val e2 = entries[1].second
        val e3 = entries[2].second
        val e9 = entries[3].second
        check(e1.actor == "alice" && e2.actor == "bob" && e3.actor == "bob" && e9.actor == "alice") { "the scenario's authors" }
        val auth2 = Authority(scope, sch, bodies)
        fun push(e: Entry): Pair<Long, Facts> = when (val s = auth2.sequenceEntry(e)) {
            is Sequenced.Appended -> s.seq to s.facts
            else -> throw Failed("push: $s")
        }
        val alice = Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList())
        val bob = Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList())
        val a1 = alice.mutate(e1.id, Ctx(e1.actor, e1.session), e1.fn, e1.autos, e1.args)
        eq(a1, e1, "alice authors the vector's first entry")
        val (s1, _) = push(a1)
        eq(s1, 1L, "the playlist was sequenced first")
        alice.ack(e1.id, s1)
        bob.receive(s1, e1)
        eq(alice.view.get("playlist", listOf(Value.id(pid))).field("name"), Value.text("Favorites") as Value, "alice's name was trimmed")
        eq(hashOf(alice), hashOf(bob), "alice and bob agree after step 1")
        eq(alice.takeChanges(), Changes.Rebuilt as Changes, "the ack of the first intent, with nothing else, rebuilt from open")
        // alice goes dark; bob adds two tracks; alice adds one alone
        for (e in listOf(e2, e3)) {
            val b = bob.mutate(e.id, Ctx(e.actor, e.session), e.fn, e.autos, e.args)
            eq(b, e, "bob authors the vector's entry")
            val (s, _) = push(b)
            bob.ack(e.id, s)
        }
        val a9 = alice.mutate(e9.id, Ctx(e9.actor, e9.session), e9.fn, e9.autos, e9.args)
        eq(a9, e9, "alice authors the vector's last entry")
        eq(posOf(alice.view, 9), v.field("alice_alone_pos_of_9"), "alone, alice's track is first on her view")
        val chA = alice.takeChanges()
        check(chA is Changes.Applied && chA.changes.size == 1) { "a local mutation reports its changes, not a rebuild: $chA" }
        eq(posOf(bob.view, 1), Value.int(1), "bob's first track")
        eq(posOf(bob.view, 2), Value.int(2), "bob's second track")
        // alice comes back: bob's entries land, her pending replays on top
        alice.receive(2, e2)
        alice.receive(3, e3)
        eq(alice.takeChanges(), Changes.Rebuilt as Changes, "the rebase is reported as a rebuild")
        eq(posOf(alice.view, 9), v.field("alice_after_rebase_pos_of_9"), "after the rebase alice's track is third")
        eq(hashOf(alice), hashOf(bob), "alice's confirmed state is bob's")
        val (s9, f9) = push(a9)
        alice.receiveFacts(s9, f9)
        alice.ack(e9.id, s9)
        bob.receive(s9, e9)
        check(alice.pending.isEmpty()) { "nothing is pending on alice once acked" }
        check(alice.takeChanges() is Changes.Applied) { "with nothing pending the ack costs no rebuild" }
        eq(alice.view, alice.confirmed, "alice's view is her confirmed store")
        check(f9.any { it is Change.Add && it.row["pos"] == Value.int(3) }) { "the authority's facts say pos 3 too" }
        eq(hashOf(alice), finalHash, "alice: one hash")
        eq(hashOf(bob), finalHash, "bob: one hash")
        eq(Hex.encode(Hash.stateHash(auth2.store)), finalHash, "the authority: one hash")
        val bobBefore = bob.verifyAt()
        bob.receive(2, e2)
        eq(bob.verifyAt().first, bobBefore.first, "a duplicate delivery is a no-op (cursor)")
        eqBytes(bob.verifyAt().second, bobBefore.second, "a duplicate delivery is a no-op (hash)")
        check(bob.pending.isEmpty() && bob.inbox.isEmpty()) { "a duplicate delivery leaves nothing behind" }

        // (e) dave's build of add_to_playlist steps by two; the facts catch it.
        val hAdd = bodies.entries.first { it.value.fn.name == "add_to_playlist" }.key
        val addClosure = bodies.getValue(hAdd)
        val wrongBody = addClosure.fn.body.map { s ->
            val row = (s as? Stmt.Upsert)?.row
            if (s is Stmt.Upsert && row is Expr.Struct) {
                val fs = row.fields.toMutableMap()
                fs["pos"] = Expr.Op(Op.Add, listOf(Expr.Var(4), Expr.Lit(Value.VInt(2))))
                Stmt.Upsert(s.table, Expr.Struct(fs), s.on)
            } else {
                s
            }
        }
        val wrong = addClosure.copy(fn = addClosure.fn.copy(body = wrongBody))
        val dave = Replica.open(sch, scope, bodies + (hAdd to wrong), MemoryStore(sch), 0, emptyList())
        for ((i, se) in entries.withIndex()) dave.receiveWith(se.first, se.second, facts[i])
        eq(dave.diverged.toList(), listOf(2L, 3L, 4L), "a divergent runtime is detected")
        eq(hashOf(dave), finalHash, "and healed by the facts")

        // (f) eve has no server: her own authority, then adoption.
        val eve = Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList())
        val eveAuth = Authority(scope, sch, bodies)
        val hCreate = bodies.entries.first { it.value.fn.name == "create_playlist" }.key
        val eveCtx = Ctx("eve", "eve-session")
        eve.mutate(idN(201), eveCtx, hCreate, mapOf("id" to Value.id(idN(2))), mapOf("name" to Value.text("Road")))
        eve.mutate(idN(202), eveCtx, hAdd, e9.autos, mapOf("playlist_id" to Value.id(idN(2)), "media_id" to Value.bytesHex("05")))
        Authority.localCommit(eveAuth, eve)
        check(eve.cursor == 2L && eve.pending.isEmpty() && eve.view == eve.confirmed) { "alone, eve confirms her own intents" }
        eqBytes(eve.verifyAt().second, Hash.stateHash(eveAuth.store), "and her state is her authority's")
        val adopted = Authority.adopt(sch, scope, bodies, eveAuth.log)
        eqBytes(Hash.stateHash(adopted.store), eve.verifyAt().second, "a server adopts her scope by replaying it")
        val tampered = Log(sch)
        for ((n, ef) in eveAuth.log.entries) {
            val (e, fs) = ef
            tampered.append(e, if (n == 2L) fs.map { c -> if (c is Change.Add) Change.Add(c.table, Value.VStruct(c.row.fields + ("pos" to Value.int(99)))) else c } else fs)
        }
        val adoptErr = try {
            Authority.adopt(sch, scope, bodies, tampered)
            null
        } catch (e: dev.arkdb.AdoptError) {
            e
        }
        eq(adoptErr, dev.arkdb.AdoptError.FactsDiffer(2) as dev.arkdb.AdoptError?, "a log whose facts were touched is refused")
        // a refused intent is dropped and kept for the app
        val refusedBefore = eve.rejections.size
        val refused = try {
            eve.mutate(idN(203), eveCtx, hCreate, mapOf("id" to Value.id(idN(3))), mapOf("name" to Value.text("   ")))
            null
        } catch (e: Fault.Refuse) {
            e.refusal
        }
        eq(refused, Refusal.Refused("a playlist needs a name") as Refusal?, "a refused intent is a verdict")
        check(eve.pending.isEmpty() && eve.rejections.size == refusedBefore) { "a refusal changes nothing and records nothing" }

        // (g) compaction: the horizon moves to 2
        check(auth.compact(2)) { "compact to 2" }
        val below = auth.page(0, 10)
        check(below is Page.BelowHorizon && below.snapshot.seq == 2L) { "a peer at 0 is sent the snapshot" }
        val tail = auth.page(2, 10)
        check(tail is Page.Entries && !tail.hasMore && tail.items.map { it.first } == listOf(3L, 4L)) { "a peer at 2 is sent the tail" }
        eq(auth.log.stateAt(4)?.let { Hex.encode(Hash.stateHash(it)) }, finalHash, "the state at the head, from facts, after compaction")
        check(auth.log.seqOf(e1.id) == 1L) { "ids are kept below the horizon" }

        // (h) the client machine over the same entries
        clientMachine(sch, scope, bodies, entries, facts, finalHash, below as Page.BelowHorizon)
    }

    private fun clientMachine(
        sch: Schema,
        scope: String,
        bodies: Map<FnHash, dev.arkdb.Closure>,
        entries: List<Pair<Long, Entry>>,
        facts: List<Facts>,
        finalHash: String,
        snapshot: Page.BelowHorizon,
    ) {
        // A whole-scope client: hello, a batch without facts, and it replays.
        val whole = Client(sch, "tok")
        whole.subscribe(Mode.Whole, Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList()))
        check(whole.takeOutgoing().isEmpty()) { "nothing is queued while unlinked" }
        whole.connected()
        eq(whole.takeOutgoing(), listOf<ClientMsg>(ClientMsg.Hello(listOf(Subscription(scope, 0, Mode.Whole)), "tok", SPEC_VERSION)), "hello on connect")
        whole.recv(ServerMsg.Batch(scope, entries.map { (n, e) -> Triple(n, e, null) }, false))
        check(whole.takeOutgoing().isEmpty()) { "a whole-scope client needs no facts" }
        eq(Hex.encode(whole.replica(scope)!!.verifyAt().second), finalHash, "the whole-scope client's hash")
        whole.verifyAll()
        val out = whole.takeOutgoing()
        check(out.size == 1 && out[0] is ClientMsg.Verify && (out[0] as ClientMsg.Verify).seq == 4L) { "verifyAll asks about the cursor: $out" }
        whole.recv(ServerMsg.Agree(scope, 4, Hex.decode(finalHash), true))
        eq(whole.agreed.toList(), listOf(Triple(scope, 4L, true)), "agree is recorded")
        whole.recv(ServerMsg.Heard(byteArrayOf(1, 2)))
        eq(whole.takeHeard().map { Hex.encode(it) }, listOf("0102"), "heard frames are taken in order")
        whole.say(byteArrayOf(9))
        check(whole.takeOutgoing().single() is ClientMsg.Say) { "say is queued while linked" }
        whole.disconnected()
        whole.say(byteArrayOf(9))
        check(whole.takeOutgoing().isEmpty()) { "say is dropped while unlinked" }

        // A facts-mode client with no closures asks for facts and reaches the same state.
        val byFacts = Client(sch, null)
        byFacts.subscribe(Mode.ByFacts, Replica.open(sch, scope, emptyMap(), MemoryStore(sch), 0, emptyList()))
        byFacts.connected()
        byFacts.takeOutgoing()
        byFacts.recv(ServerMsg.Batch(scope, entries.map { (n, e) -> Triple(n, e, null) }, true))
        val asked = byFacts.takeOutgoing()
        eq(asked, listOf(ClientMsg.NeedFacts(scope, entries.map { it.first }), ClientMsg.Hello(listOf(Subscription(scope, 0, Mode.ByFacts)), null, SPEC_VERSION)), "need facts, then hello for the rest")
        byFacts.recv(ServerMsg.FactsFor(scope, entries.mapIndexed { i, (n, _) -> n to facts[i] }))
        eq(Hex.encode(byFacts.replica(scope)!!.verifyAt().second), finalHash, "the facts-mode client's hash")

        // A client below the horizon is sent the snapshot and continues from it.
        val late = Client(sch, null)
        late.subscribe(Mode.Whole, Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList()))
        late.connected()
        late.takeOutgoing()
        val sn = snapshot.snapshot
        val rows = sn.store.tableNames.associateWith { t -> sn.store.scan(t).map { it as Value } }
        late.recv(ServerMsg.SnapshotOf(scope, sn.seq, sn.hash, rows))
        eq(late.replica(scope)!!.cursor, 2L, "the cursor moves to the snapshot")
        late.recv(ServerMsg.Batch(scope, entries.filter { it.first > 2 }.map { (n, e) -> Triple(n, e, null) }, false))
        eq(Hex.encode(late.replica(scope)!!.verifyAt().second), finalHash, "from the snapshot to the head")

        // A client that pushes: its own entry comes back as an ack; a reject drops one.
        val pusher = Client(sch, null)
        pusher.subscribe(Mode.Whole, Replica.open(sch, scope, bodies, MemoryStore(sch), 0, emptyList()))
        pusher.connected()
        pusher.takeOutgoing()
        val e1 = entries[0].second
        val mine = pusher.mutate(scope, e1.id, Ctx(e1.actor, e1.session), e1.fn, e1.autos, e1.args)
        eq(pusher.takeOutgoing(), listOf<ClientMsg>(ClientMsg.Push(scope, listOf(mine))), "a mutation is pushed")
        val e2 = entries[1].second
        pusher.mutate(scope, e2.id, Ctx(e2.actor, e2.session), e2.fn, e2.autos, e2.args)
        pusher.recv(ServerMsg.Reject(scope, e2.id, "not yours"))
        eq(pusher.replica(scope)!!.rejections.map { it.second }, listOf<Refusal>(Refusal.Refused("not yours")), "a reject is kept for the app")
        eq(pusher.replica(scope)!!.pending.map { it.id }, listOf(e1.id), "the rejected intent is dropped")
        pusher.recv(ServerMsg.Ack(scope, listOf(e1.id), listOf(1)))
        eq(pusher.replica(scope)!!.cursor, 1L, "an ack confirms the intent")
        check(pusher.replica(scope)!!.pending.isEmpty()) { "nothing pending after the ack" }
        pusher.disconnected()
        pusher.connected()
        eq(pusher.takeOutgoing(), listOf<ClientMsg>(ClientMsg.Hello(listOf(Subscription(scope, 1, Mode.Whole)), null, SPEC_VERSION)), "reconnect says hello at the cursor")
        pusher.recv(ServerMsg.Denied("not signed in"))
        eq(pusher.denied, "not signed in", "denied is recorded")
        check(!pusher.linked) { "denied unlinks" }

        // Closures arriving unblock an inbox.
        val learner = Client(sch, null)
        learner.subscribe(Mode.Whole, Replica.open(sch, scope, emptyMap(), MemoryStore(sch), 0, emptyList()))
        learner.connected()
        learner.takeOutgoing()
        learner.recv(ServerMsg.Batch(scope, entries.map { (n, e) -> Triple(n, e, null) }, false))
        eq(learner.replica(scope)!!.cursor, 0L, "without closures nothing applies")
        learner.recv(ServerMsg.Closures(bodies.toList()))
        eq(Hex.encode(learner.replica(scope)!!.verifyAt().second), finalHash, "closures unblock the inbox")
    }

    // self checks -------------------------------------------------------------

    private fun storeComplete() {
        val sch = Schema(
            listOf(
                Scope(
                    "s",
                    listOf(
                        Table(
                            "t",
                            listOf(Column("id", Ty.TInt, false), Column("name", Ty.TText, false), Column("note", Ty.TText, true)),
                            listOf("id"),
                            listOf(dev.arkdb.Index(listOf("note"), true)),
                            emptyList(),
                        ),
                    ),
                ),
            ),
        )
        val st = MemoryStore(sch)
        st.put("t", Value.record("id" to Value.int(1), "name" to Value.text("a")))
        eq(st.get("t", listOf(Value.int(1))), Value.record("id" to Value.int(1), "name" to Value.text("a"), "note" to Value.VNull), "an omitted nullable column is Null")
        eq(st.changes.size, 1, "one add")
        st.put("t", Value.record("id" to Value.int(1), "name" to Value.text("a")))
        eq(st.changes.size, 1, "the same row again is no change")
        st.put("t", Value.record("id" to Value.int(2), "name" to Value.text("b")))
        eq(st.changes.size, 2, "two nulls in a unique column do not clash")
        fun refusal(row: Value): Refusal? = try {
            st.put("t", row)
            null
        } catch (e: Fault.Refuse) {
            e.refusal
        }
        check(refusal(Value.record("id" to Value.int(3))) is Refusal.MalformedRow) { "omitting a non-nullable column is malformed" }
        check(refusal(Value.record("id" to Value.int(3), "name" to Value.text("c"), "bogus" to Value.int(0))) is Refusal.MalformedRow) { "an unknown column is malformed" }
        check(refusal(Value.record("id" to Value.int(3), "name" to Value.VNull)) is Refusal.NotNull) { "null in a non-nullable column" }
        check(refusal(Value.record("id" to Value.int(3), "name" to Value.int(3))) is Refusal.MalformedRow) { "a column of the wrong type" }
        st.put("t", Value.record("id" to Value.int(3), "name" to Value.text("c"), "note" to Value.text("n")))
        eq(refusal(Value.record("id" to Value.int(4), "name" to Value.text("d"), "note" to Value.text("n"))), Refusal.UniqueViolation("t", listOf("note")) as Refusal?, "a unique clash")
        check(refusal(Value.record("id" to Value.int(3), "name" to Value.text("c"), "note" to Value.text("n2"))) == null) { "an edit" }
        check(st.changes.last() is Change.Edit) { "an edit reports both versions" }
        eq(st.select(Plan.from("t").filter(Pred.cmp("note", CmpOp.Eq, Value.VNull)).orderBy("id", Dir.Desc)).asList().map { it.field("id") }, listOf(Value.int(2), Value.int(1)), "select with NULL = NULL")
    }

    private fun stdSelf() {
        eq(Std.trim(Value.text("　 a b \u0085")), Value.text("a b"), "trim strips White_Space, not only ASCII")
        eq(Std.lower(Value.text("ÀÉİ")), Value.text("àéi̇".take(2) + Std.lower(Value.text("İ")).asText()), "lower is the simple mapping")
        eq(Std.textLen(Value.text("🎵")), Value.int(1), "text_len counts code points")
        eq(Std.chars(Value.text("a🎵")), Value.list(Value.text("a"), Value.text("🎵")), "chars are code points")
        eq(Std.isAlnum(Value.text("")), Value.bool(false), "is_alnum of nothing")
        eq(Std.isAlnum(Value.text("a1٣")), Value.bool(true), "is_alnum over Alphabetic and numeric")
        eq(Std.splitOnce(Value.text("a=b=c"), Value.text("=")), Value.record("before" to Value.text("a"), "after" to Value.text("b=c")), "split_once")
        eq(Std.splitOnce(Value.text("abc"), Value.text("")), Value.VNull, "split_once on an empty separator")
        eq(Std.fnv1a64(Value.text("")), Value.int(-3750763034362895579L), "fnv1a64 offset basis")
        eq(Std.fnv1a64(Value.text("a")), Value.int("af63dc4c8601ec8c".toULong(16).toLong()), "fnv1a64 of a")
        eq(Hex.encode(Std.sha256(Value.bytesHex("")).asBytes()), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "sha256 of nothing")
        eq(Std.idOfText(Value.text("0102030405060708090A0B0C0D0E0F10")), Value.VNull as Value, "id_of_text wants dashes")
        eq(Std.idOfText(Value.text("01020304-0506-0708-090A-0B0C0D0E0F10")), Value.id(Id.ofHex("0102030405060708090a0b0c0d0e0f10")), "id_of_text either case")
        eq(Std.textOfId(Value.id(Id.ofHex("0102030405060708090a0b0c0d0e0f10"))), Value.text("01020304-0506-0708-090a-0b0c0d0e0f10"), "text_of_id lowercase")
        fun refusal(f: () -> Value): String? = try {
            f()
            null
        } catch (e: Fault.Refuse) {
            e.refusal.text
        }
        eq(refusal { Ops.add(Value.int(Long.MAX_VALUE), Value.int(1)) }, "integer overflow", "add overflow")
        eq(refusal { Ops.sub(Value.int(Long.MIN_VALUE), Value.int(1)) }, "integer overflow", "sub overflow")
        eq(refusal { Ops.mul(Value.int(Long.MIN_VALUE), Value.int(-1)) }, "integer overflow", "mul overflow")
        eq(refusal { Ops.mul(Value.int(3037000500L), Value.int(3037000500L)) }, "integer overflow", "mul overflow, positive")
        eq(Ops.mul(Value.int(-3037000499L), Value.int(3037000499L)), Value.int(-9223372030926249001L), "mul near the edge")
        eq(refusal { Ops.div(Value.int(1), Value.int(0)) }, "division by zero", "div by zero")
        eq(refusal { Ops.div(Value.int(Long.MIN_VALUE), Value.int(-1)) }, "integer overflow", "div overflow")
        eq(refusal { Ops.mod(Value.int(Long.MIN_VALUE), Value.int(-1)) }, "integer overflow", "mod overflow")
        eq(Ops.div(Value.int(-7), Value.int(2)), Value.int(-3), "div truncates toward zero")
        eq(Ops.mod(Value.int(-7), Value.int(2)), Value.int(-1), "mod takes the dividend's sign")
        eq(refusal { Ops.neg(Value.int(Long.MIN_VALUE)) }, "integer overflow", "neg overflow")
        eq(refusal { Std.abs(Value.int(Long.MIN_VALUE)) }, "integer overflow", "abs overflow")
        eq(refusal { Std.clamp(Value.int(1), Value.int(5), Value.int(2)) }, "clamp: lower bound above upper bound", "clamp fault")
        eq(Ops.cmp(CmpOp.Lt, Value.VNull, Value.int(0)), Value.bool(true), "NULL < 0")
        eq(Ops.cmp(CmpOp.Eq, Value.VNull, Value.VNull), Value.bool(true), "NULL = NULL")
        eq(Ops.cmp(CmpOp.Lt, Value.text("～"), Value.text("🎵")), Value.bool(true), "U+FF5E before U+1F3B5 by code point")
        eq(Ops.sortBy(Value.list(Value.text("b"), Value.text("a"), Value.text("B"))) { Value.int(0) }, Value.list(Value.text("b"), Value.text("a"), Value.text("B")), "sort_by is stable")
        eq(Ops.fold(Value.list(Value.int(1), Value.int(2)), Value.int(0)) { acc, x -> Ops.add(acc, x) }, Value.int(3), "fold")
        val bug = try {
            Value.int(1).asText()
            null
        } catch (e: IllegalStateException) {
            e
        }
        check(bug != null) { "an accessor mismatch is fatal" }
    }
}
