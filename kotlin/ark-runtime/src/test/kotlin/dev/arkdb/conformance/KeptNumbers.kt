// R9's kept numbers held to what a fresh read answers, beyond the views
// vectors: those exercise a group's members kept as a count and a sum and
// a related list kept as a count or a maximum, one level down, and nothing
// deeper. This runs a plan whose kept lists have kept lists of their own —
// a composer's works counted and their songs summed, each work's songs
// summed over its movements, each movement's songs counted — over the
// schema and store of views/related-three-deep.json, through seeded random
// batches of changes to every table it reads, and after each batch holds
// the view to its contract (a fresh hydrate), to the answer `View.read`
// gives by folding the lists, and to the splice of its patches. A plain
// `main`, run by `gradle check` beside the conformance runner.
package dev.arkdb.conformance

import dev.arkdb.Change
import dev.arkdb.CmpOp
import dev.arkdb.Ctx
import dev.arkdb.Dir
import dev.arkdb.Expr
import dev.arkdb.Key
import dev.arkdb.MemoryStore
import dev.arkdb.Op
import dev.arkdb.Plan
import dev.arkdb.Related
import dev.arkdb.Row
import dev.arkdb.StdFn
import dev.arkdb.Value
import dev.arkdb.View
import dev.arkdb.ViewEnv
import dev.arkdb.Views
import java.io.File
import kotlin.system.exitProcess

object KeptNumbers {
    private fun v(s: Long) = Expr.Var(s)

    private fun lit(n: Long) = Expr.Lit(Value.VInt(n))

    private fun field(e: Expr, n: String) = Expr.FieldE(e, n)

    private fun len(s: Long) = Expr.StdE(StdFn.Len, listOf(v(s)))

    // `fold(list, 0, |acc, x| acc + x.f)`: a sum the view keeps.
    private fun sum(list: Long, acc: Long, x: Long, f: String) =
        Expr.Fold(v(list), lit(0), acc, x, Expr.OpE(Op.Add, listOf(v(acc), field(v(x), f))))

    private fun struct(vararg fs: Pair<String, Expr>) = Expr.StructE(fs.toMap())

    private fun by(c: String) = listOf(Key.Column(c) to Dir.Asc)

    /** person → work → movement → song, every list beneath the person kept as numbers. */
    fun plan(): Plan {
        val song = Plan.from("song").copy(order = by("id"))
        val movement =
            Plan.from("movement")
                .copy(
                    row = 2,
                    order = by("id"),
                    related =
                        listOf(
                            Related(
                                "song",
                                3,
                                listOf("movement_id" to Expr.SomeE(field(v(2), "id"))),
                                song)),
                    project = struct("no" to field(v(2), "no"), "n" to len(3)),
                )
        val work =
            Plan.from("work")
                .copy(
                    row = 1,
                    order = by("id"),
                    related =
                        listOf(
                            Related(
                                "movement", 4, listOf("work_id" to field(v(1), "id")), movement)),
                    project = struct("songs" to sum(4, 20, 21, "n"), "movements" to len(4)),
                    having = Expr.CmpE(CmpOp.Ge, len(4), lit(0)),
                )
        return Plan.from("person")
            .copy(
                row = 0,
                order = by("name"),
                related =
                    listOf(Related("work", 5, listOf("composer" to field(v(0), "name")), work)),
                project =
                    struct(
                        "name" to field(v(0), "name"),
                        "works" to len(5),
                        "songs" to sum(5, 22, 23, "songs"),
                        "movements" to sum(5, 24, 25, "movements"),
                    ),
            )
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val root = File(args.getOrElse(0) { "../spec/vectors" })
        val vec = Json.parse(File(root, "views/related-three-deep.json").readText(Charsets.UTF_8))
        val sch = Checks.moduleOf(vec.field("module")).schema
        val plan = plan()
        val shape = Views.shape(sch, plan, emptyList())
        if (shape.kept.size != 3)
            fail("the plan keeps ${shape.kept.size} lists as numbers, not 3: $shape")
        var rng = 0x2545F4914F6CDD1DL
        fun next(n: Int): Int {
            rng = rng * 6364136223846793005L + 1442695040888963407L
            return ((rng ushr 33) % n).toInt()
        }
        val persons = listOf("bach", "handel", "vivaldi", "anon")
        val works = (1..6).map { "w$it" }
        val movements = (1..10).map { "m$it" }
        val songs = (1..14).map { "s$it" }
        fun row(vararg fs: Pair<String, Value>): Row = Value.VStruct(fs.toMap())
        fun t(s: String) = Value.VText(s)
        val st = MemoryStore.fromValue(sch, vec.field("store_before"))
        val env = ViewEnv(emptyList(), Ctx("alice", "session-1"), emptyMap(), emptyMap())
        val view = Views.hydrate(sch, plan, env, st)
        var patches = 0
        val steps = 400
        for (step in 0 until steps) {
            val batch = ArrayList<Change>()
            repeat(1 + next(3)) {
                val change: Change? =
                    when (next(4)) {
                        0 -> {
                            val k = persons[next(persons.size)]
                            val old = st.get("person", listOf(t(k)))
                            if (old != null && next(3) == 0) Change.Remove("person", old)
                            else
                                Change.Add(
                                    "person",
                                    row("name" to t(k), "born" to Value.VInt(next(100).toLong())))
                        }
                        1 -> {
                            val k = works[next(works.size)]
                            val old = st.get("work", listOf(t(k)))
                            val new =
                                row(
                                    "id" to t(k),
                                    "composer" to t(persons[next(persons.size)]),
                                    "title" to t("t${next(9)}"))
                            when {
                                old == null -> Change.Add("work", new)
                                next(3) == 0 -> Change.Remove("work", old)
                                else -> Change.Edit("work", old, new)
                            }
                        }
                        2 -> {
                            val k = movements[next(movements.size)]
                            val old = st.get("movement", listOf(t(k)))
                            val new =
                                row(
                                    "id" to t(k),
                                    "work_id" to t(works[next(works.size)]),
                                    "no" to Value.VInt(next(5).toLong()),
                                    "part" to t("p"))
                            when {
                                old == null -> Change.Add("movement", new)
                                next(3) == 0 -> Change.Remove("movement", old)
                                else -> Change.Edit("movement", old, new)
                            }
                        }
                        else -> {
                            val k = songs[next(songs.size)]
                            val old = st.get("song", listOf(t(k)))
                            val mv =
                                if (next(5) == 0) Value.VNull
                                else t(movements[next(movements.size)])
                            val new =
                                row(
                                    "id" to t(k),
                                    "title" to t("x"),
                                    "creator" to t("c"),
                                    "movement_id" to mv,
                                    "pos" to Value.VInt(next(9).toLong()))
                            when {
                                old == null -> Change.Add("song", new)
                                next(3) == 0 -> Change.Remove("song", old)
                                else -> Change.Edit("song", old, new)
                            }
                        }
                    }
                if (change != null) {
                    st.applyChange(change)
                    batch.add(change)
                }
            }
            val before = view.rows()
            val ps = Views.pushAll(sch, st, batch, view)
            patches += ps.size
            if (!Views.contract(sch, st, view)) fail("step $step: the view is not a fresh hydrate")
            val read = View.read(sch, plan, env.scope(sch), st)
            if (view.rows() != read)
                fail(
                    "step $step: the view answers ${Value.VList(view.rows()).show()}, a fold ${Value.VList(read).show()}")
            if (Views.splice(ps, before) != view.rows())
                fail("step $step: the patches do not splice")
        }
        println(
            "kept numbers: $steps batches over three kept levels, $patches patches; every one a fresh hydrate, a fold's answer and a splice")
    }

    private fun fail(why: String): Nothing {
        println("kept numbers: FAILED: $why")
        exitProcess(1)
    }
}
