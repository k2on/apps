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
        test("protocol/server: a push is held to its login and every refusal says why") { heldToItsLogin(demo) }
        test("protocol/server: work done before signing in becomes the signer's") { signedInLater(demo) }
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
        }
        test("authoring/harken: native agrees with the interpreter on every procedure") { harkenAgreement(harken.gen.module()) }
        // The phone's print against the whole module the Rust domain emits:
        // every procedure it carries must hash as that module's does, or the
        // phone's entries name functions the server has never seen.
        val ark = File(root, "../../harken/domain/harken.ark")
        val whole = if (ark.isFile) Decode.fromValue(Canon.decode(ark.readBytes())) else null
        if (whole == null || whole.spec < SPEC_VERSION) {
            skipped.add("authoring/harken: harken.ark is ${if (whole == null) "missing" else "spec ${whole.spec}"}; the phone's hashes are not compared with it")
        } else {
            test("authoring/harken: every procedure the phone carries hashes as harken.ark's") {
                val phone = harken.gen.module().ir
                eq(phone.schema, whole.schema, "the schema is whole")
                for (fn in phone.functions) {
                    val theirs = whole.lookupFunction(fn.name) ?: throw Failed("harken.ark has no ${fn.name}")
                    eq(Hash.functionHash(Hash.closure(phone, fn)).hex, Hash.functionHash(Hash.closure(whole, theirs)).hex, "${fn.name}'s hash")
                }
            }
        }
        test("authoring/self: an auto name of two kinds, a statement in an expression, .on after something else") { selfRefusals() }
        test("authoring/vocab: helpers, records, option comparisons and a repeated auto") { vocabulary() }
        test("authoring/vocab: each helper's closure is harken.ark's") { helpersAsHarken(root, skipped) }
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

    /**
     * §12.3 and §12.5 on a real authority: an entry is held to the login
     * that pushed it unless the server knows the user owns the older one,
     * another user's entry is refused whatever the server knows, and every
     * refusal reaches the author as a sentence a screen can show.
     */
    private fun heldToItsLogin(m: dev.arkdb.authoring.Module) {
        val ir = m.ir
        val bodies = Hash.closures(ir)
        val create = bodies.entries.single { it.value.fn.name == "create_playlist" }.key
        // A token is "user:session".
        val auth = dev.arkdb.Authenticate { tok ->
            tok?.split(':')?.takeIf { it.size == 2 }?.let { (u, s) -> dev.arkdb.Identity(u, s) }
        }
        fun serve(owns: Boolean): dev.arkdb.Server {
            val sv = dev.arkdb.Server(auth, { true }, dev.arkdb.Authority(ir.schema, bodies))
            return if (owns) sv.withOwns { user, session -> user == "alice" && session == "old" } else sv
        }
        fun entry(k: kotlin.Int, actor: String, session: String, name: String) = dev.arkdb.Entry(
            (idN(k) as Value.VId).value, actor, session, create, mapOf("name" to Value.text(name)), mapOf("id" to idN(k)),
        )
        // The verdict each entry got, in order: its sequence, or the reason.
        fun push(owns: Boolean, entries: List<dev.arkdb.Entry>): List<Any> {
            val sv = serve(owns)
            sv.recv(1, dev.arkdb.ClientMsg.Hello(dev.arkdb.Subscription(0, dev.arkdb.Mode.Whole), "alice:new", SPEC_VERSION))
            sv.takeOutgoing()
            sv.recv(1, dev.arkdb.ClientMsg.Push(entries))
            return sv.takeOutgoing().flatMap { (_, f) ->
                when (f) {
                    is dev.arkdb.ServerMsg.Ack -> f.seqs
                    is dev.arkdb.ServerMsg.Reject -> listOf(f.reason)
                    else -> emptyList()
                }
            }
        }
        // Authored offline under an older login, pushed after signing in again.
        eq(push(false, listOf(entry(1, "alice", "old", "Mix"))), listOf<Any>("not yours"), "an older login, unowned")
        eq(push(true, listOf(entry(1, "alice", "old", "Mix"))), listOf<Any>(1L), "an older login the user owns")
        // Owning a session is only ever about the connection's own user.
        eq(push(true, listOf(entry(1, "bob", "old", "Mix"))), listOf<Any>("not yours"), "another user's entry")
        // The login pushing is always its own.
        eq(push(false, listOf(entry(1, "alice", "new", "Mix"))), listOf<Any>(1L), "the connection's own login")
        // A refusal is the author's own sentence; a constraint's is named in one.
        eq(
            push(false, listOf(entry(1, "alice", "new", "   "), entry(2, "alice", "new", "Mix"), entry(3, "alice", "new", "Mix"))),
            listOf<Any>("a playlist needs a name", 1L, 2L),
            "a duplicate under insert .on writes nothing and is still sequenced, not refused",
        )
        eq(
            dev.arkdb.Protocol.refusalText(dev.arkdb.Refusal.UniqueViolation("playlist", listOf("user_id", "name"))),
            "playlist: another row has the same user_id, name",
            "a constraint, as a sentence",
        )
        eq(dev.arkdb.Refusal.MissingParent("item", "playlist_id", "playlist").text, "item.playlist_id names no playlist", "a refusal's text is that sentence")
        // A hello that proves nothing, and a push before any hello.
        val sv = serve(false)
        sv.recv(2, dev.arkdb.ClientMsg.Hello(dev.arkdb.Subscription(0, dev.arkdb.Mode.Whole), null, SPEC_VERSION))
        sv.recv(3, dev.arkdb.ClientMsg.Push(listOf(entry(1, "alice", "new", "Mix"))))
        eq(sv.takeOutgoing(), listOf<kotlin.Pair<Long, dev.arkdb.ServerMsg>>(2L to dev.arkdb.ServerMsg.Denied("not signed in"), 3L to dev.arkdb.ServerMsg.Denied("hello first")), "denied")
        eq(sv.authority.log.headSeq, 0L, "and nothing sequenced")
    }

    /**
     * §11.2b A peer used for a while with no account, then signed in: all of
     * it is pushed as the person who signed in and accepted, and the rows
     * say whose they are. Pushed without signing in, every entry is refused.
     */
    private fun signedInLater(m: dev.arkdb.authoring.Module) {
        val ir = m.ir
        val sch = ir.schema
        val bodies = Hash.closures(ir)
        fun hashOf(name: String) = bodies.entries.single { it.value.fn.name == name }.key
        val pid = idN(30)
        fun raw(k: kotlin.Int) = (idN(k) as Value.VId).value
        fun authored(): dev.arkdb.Replica {
            val local = dev.arkdb.Replica.open(sch, bodies, MemoryStore(sch), 0, emptyList())
            local.mutate(raw(31), Ctx.nobody, hashOf("create_playlist"), mapOf("id" to pid), mapOf("name" to Value.text("Offline")))
            for (k in 0 until 10) {
                local.mutate(raw(32 + k), Ctx.nobody, hashOf("add_to_playlist"), emptyMap(), mapOf("playlist_id" to pid, "track_id" to Value.text("t$k")))
            }
            eq(local.pending.size, 11, "eleven pending")
            return local
        }
        // Everything the client says reaches the server; what came back.
        fun run(client: dev.arkdb.Client): Triple<kotlin.Int, kotlin.collections.List<String>, dev.arkdb.Server> {
            val sv = dev.arkdb.Server(dev.arkdb.Authenticate.trusting, { true }, dev.arkdb.Authority(sch, bodies))
            client.connected()
            for (f in client.takeOutgoing()) sv.recv(7, f)
            val out = sv.takeOutgoing().map { it.second }
            val acked = out.sumOf { (it as? dev.arkdb.ServerMsg.Ack)?.ids?.size ?: 0 }
            val refused = out.mapNotNull { (it as? dev.arkdb.ServerMsg.Reject)?.reason }
            return Triple(acked, refused, sv)
        }
        val signed = dev.arkdb.Client(authored(), dev.arkdb.Mode.Whole, null)
        signed.signIn(Ctx("alice", "dev"), "alice")
        val owner = (signed.replica.view.get("playlist", listOf(pid)) as Value.VStruct)["user_id"]
        eq(owner, Value.text("alice") as Value, "the optimistic view already says whose it is")
        check(signed.replica.pending.all { it.actor == "alice" && it.session == "dev" }) { "every intent re-stamped" }
        val (acked, refused, sv) = run(signed)
        eq(acked to refused, 11 to emptyList<String>(), "all eleven accepted")
        eq(sv.authority.store.scan("playlist").map { it["user_id"] }, listOf(Value.text("alice") as Value), "the playlist is alice's")

        val (acked2, refused2, _) = run(dev.arkdb.Client(authored(), dev.arkdb.Mode.Whole, "alice"))
        eq(acked2 to refused2.size, 0 to 11, "unsigned, all eleven are refused")
        check(refused2.all { it == "not yours" }) { "each one not yours: $refused2" }

        // A hello that proves nobody is not signed in.
        val nobodyServer = dev.arkdb.Server({ _ -> dev.arkdb.Identity("", "") }, { true }, dev.arkdb.Authority(sch, bodies))
        nobodyServer.recv(1, dev.arkdb.ClientMsg.Hello(dev.arkdb.Subscription(0, dev.arkdb.Mode.Whole), "x", SPEC_VERSION))
        eq(nobodyServer.takeOutgoing(), listOf<kotlin.Pair<Long, dev.arkdb.ServerMsg>>(1L to dev.arkdb.ServerMsg.Denied("not signed in")), "nobody is denied")
    }

    private fun vocabulary() {
        val m = selfdemo.vocabModule()
        val ir = m.ir
        eq(
            ir.functions.map { it.name to it.kind },
            listOf(
                "work_title", "slug", "key_part", "work_key", "work_id", "credited_as", "movement_key", "recording_key", "recording_id",
            ).map { it to FnKind.Helper } + listOf("keys" to FnKind.Query, "compare" to FnKind.Query, "twice" to FnKind.Mutator),
            "each helper once, before the first function that calls it, a helper a helper calls first",
        )
        eq(Decode.fromValue(Canon.decode(m.emit())), ir, "emit decodes to the IR")
        eq(Encode.toValue(Verify.verify(ir)), Encode.toValue(ir), "and verifies as it is")
        val rid = ir.lookupFunction("recording_id")!!
        eq(rid.input.map { it.first to it.second.ty }, listOf(
            "work_id" to dev.arkdb.Ty.TOption(dev.arkdb.Ty.TText), "album" to dev.arkdb.Ty.TText, "title" to dev.arkdb.Ty.TText,
            "artist" to dev.arkdb.Ty.TText, "performer" to dev.arkdb.Ty.TText,
        ), "a helper's parameters are its names and its Kotlin types")
        eq(rid.ret, dev.arkdb.Ty.TText as dev.arkdb.Ty?, "and its result's")
        val keysTy = dev.arkdb.Ty.TStruct(mapOf("credited" to dev.arkdb.Ty.TText, "movement" to dev.arkdb.Ty.TText, "recording" to dev.arkdb.Ty.TText, "work" to dev.arkdb.Ty.TOption(dev.arkdb.Ty.TText)))
        eq(ir.lookupFunction("keys")!!.ret, dev.arkdb.Ty.TList(keysTy) as dev.arkdb.Ty?, "a record is a TStruct of its fields")
        eq(ir.lookupFunction("compare")!!.ret, dev.arkdb.Ty.TStruct(mapOf("eq" to dev.arkdb.Ty.TBool, "ne" to dev.arkdb.Ty.TBool, "keys" to dev.arkdb.Ty.TOption(keysTy))) as dev.arkdb.Ty?, "a record holding a record")
        check(ir.lookupFunction("keys")!!.body.toString().contains("Call(fn=work_title")) { "a helper is called by ECall" }
        val compare = ir.lookupFunction("compare")!!.body.toString()
        for (op in listOf(dev.arkdb.CmpOp.Eq, dev.arkdb.CmpOp.Ne)) {
            val want = dev.arkdb.Expr.Cmp(op, dev.arkdb.Expr.Arg("a"), dev.arkdb.Expr.Arg("b")).toString()
            check(compare.contains(want)) { "options compare by ECmp $op: $compare" }
        }
        val twice = ir.lookupFunction("twice")!!
        eq(twice.autos, listOf("id" to dev.arkdb.Auto.NewId("playlist"), "at" to (dev.arkdb.Auto.Now as dev.arkdb.Auto)), "a name drawn twice is one auto")

        // Natively and by the interpreter, the same answers.
        val p = Pair2(m)
        val st = MemoryStore(ir.schema)
        val alice = Ctx("alice", "a1")
        fun track(artist: String, catalogue: String, workTitle: String, album: String, title: String, performer: String, part: String, no: Long) = mapOf(
            "artist" to Value.text(artist), "catalogue" to Value.text(catalogue), "work_title" to Value.text(workTitle), "album" to Value.text(album),
            "title" to Value.text(title), "performer" to Value.text(performer), "part" to Value.text(part), "no" to Value.int(no),
        )
        val tracks = listOf(
            track("Johann Sebastian Bach", "BWV 988", "Goldberg Variations", "Goldberg Variations", "Aria", "Kimiko Ishizaka", "", 1),
            track("Antonín Dvořák", "", "", "Symphony No. 9", "Largo", "", "II", 2),
            track("!!!", "", "", "Myth Takes", "Heart of Hearts", "", "", 3),
            track("Sergei Rachmaninoff", "Op. 23", "Preludes", "Preludes", "No. 5", "", "", 5),
            track("", "", "", "", "", "", "", 0),
        )
        for (t in tracks) p.step("keys", alice, emptyMap(), t, st)
        val answer = (p.answer("keys", alice, tracks[0], st) as Eval.Answer.Ok).value.asList().single()
        eq(answer.field("work"), Value.text("johann-sebastian-bach/bwv-988") as Value, "a work's key")
        eq(answer.field("movement"), Value.text("johann-sebastian-bach/bwv-988#1") as Value, "a movement's")
        eq(answer.field("recording"), Value.text("johann-sebastian-bach/bwv-988@kimiko-ishizaka") as Value, "a recording's")
        eq(answer.field("credited"), Value.text("Kimiko Ishizaka") as Value, "who is credited")
        val bang = (p.answer("keys", alice, tracks[2], st) as Eval.Answer.Ok).value.asList().single()
        check(bang.field("recording").asText().startsWith("myth-takes/heart-of-hearts@x")) { "a name with no letters keys by its hash: $bang" }
        eq(dev.arkdb.authoring.evaluate { selfdemo.slug(dev.arkdb.authoring.lit("  Dvořák -- Op. 23 ")) }, Eval.Answer.Ok(Value.text("dvořák-op-23")) as Eval.Answer, "evaluate runs a helper natively")

        fun opt(s: String?): Value = s?.let { Value.text(it) } ?: Value.VNull
        for ((a, b) in listOf(null to null, "x" to null, null to "x", "x" to "x", "x" to "y")) {
            val args = mapOf("a" to opt(a), "b" to opt(b))
            p.step("compare", alice, emptyMap(), args, st)
            val c = (p.answer("compare", alice, args, st) as Eval.Answer.Ok).value
            eq(c.field("eq") to c.field("ne"), Value.bool(a == b) to Value.bool(a != b), "$a against $b")
            eq(c.field("keys").isNull(), a == null, "a record in an option, filtered on one of its fields ($a)")
        }

        // One name, one value: the id and the clock read twice are the entry's one each.
        val id = Value.id(dev.arkdb.Id(ByteArray(16) { 3 }))
        val after = p.step("twice", alice, mapOf("id" to id, "at" to Value.int(42)), emptyMap(), st)
        val row = after.scan("playlist").single()
        eq(row["id"] to row["name"], id to dev.arkdb.Std.textOfId(id), "the id, twice")
        eq(after.scan("item").single()["track_id"], Value.text("42") as Value, "the clock, twice")
    }

    /**
     * The helpers above are harken's, as library.rs writes them in Rust:
     * each one's closure hash is the one harken.ark carries under its name,
     * which holds the Kotlin emission to Rust's byte for byte.
     */
    private fun helpersAsHarken(root: File, skipped: MutableList<String>) {
        val ark = File(root, "../../harken/domain/harken.ark")
        val whole = if (ark.isFile) Decode.fromValue(Canon.decode(ark.readBytes())) else null
        val ir = selfdemo.vocabModule().ir
        val helpers = ir.functions.filter { it.kind == FnKind.Helper }
        val theirs = helpers.mapNotNull { f -> whole?.lookupFunction(f.name)?.let { f to it } }
        if (whole == null || theirs.isEmpty()) {
            skipped.add("authoring/vocab: harken.ark carries none of the helpers; their hashes are not compared")
            return
        }
        for ((f, t) in theirs) {
            eq(Hex.encode(Hash.functionHash(Hash.closure(ir, f)).bytes), Hex.encode(Hash.functionHash(Hash.closure(whole, t)).bytes), "${f.name}: the closure hash")
        }
        eq(theirs.size, helpers.size, "every helper is in harken.ark")
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
        fails("one auto name drawn as two kinds") { selfdemo.twiceDifferently().ir }
        fails("a read inside map's closure") { selfdemo.readInMap().ir }
        fails(".on after another statement") { selfdemo.onLate().ir }
    }
}
