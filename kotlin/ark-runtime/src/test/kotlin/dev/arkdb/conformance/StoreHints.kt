// A store's hints held to the definition (`rust/ark/src/store.rs`, "an
// indexed read is the scan filtered"): `scanWhereEq`, `scanWhereText` and
// `scanOrdered` are told which columns a filter holds equal, between
// bounds, or to contain a needle, and read through the store's indexes —
// and must answer exactly what scanning every row, keeping what the filter
// admits and sorting, answers. Over a `MemoryStore` and over an `Overlay`
// on one, through seeded random rows and random reads. A plain `main`, run
// by `gradle check` beside the conformance runner.
package dev.arkdb.conformance

import dev.arkdb.Bound
import dev.arkdb.Change
import dev.arkdb.CmpOp
import dev.arkdb.Column
import dev.arkdb.Dir
import dev.arkdb.Index
import dev.arkdb.Keep
import dev.arkdb.MemoryStore
import dev.arkdb.Overlay
import dev.arkdb.Ref
import dev.arkdb.Row
import dev.arkdb.Rules
import dev.arkdb.Schema
import dev.arkdb.Span
import dev.arkdb.Store
import dev.arkdb.Table
import dev.arkdb.Ty
import dev.arkdb.Value
import dev.arkdb.cmp
import dev.arkdb.fold
import kotlin.system.exitProcess

object StoreHints {
    private val parent =
        Table("p", listOf(Column("id", Ty.TInt, false)), listOf("id"), emptyList(), emptyList())
    private val item =
        Table(
            "t",
            listOf(
                Column("id", Ty.TInt, false),
                Column("a", Ty.TInt, false),
                Column("c", Ty.TInt, true),
                Column("b", Ty.TText, false),
                Column("p", Ty.TInt, true),
            ),
            listOf("id"),
            listOf(Index(listOf("a", "c"), false), Index(listOf("c"), false)),
            listOf(Ref("p", "p")),
            listOf("b"),
        )
    private val schema = Schema(listOf(parent, item))

    private var rng = 0x9E3779B97F4A7C15uL.toLong()

    private fun next(n: Int): Int {
        rng = rng * 6364136223846793005L + 1442695040888963407L
        return ((rng ushr 33) % n).toInt()
    }

    private val words =
        listOf(
            "Água", "agua", "Bach", "BACH partita", "water music", "Wassermusik", "ab", "", "aguas")

    private fun row(id: Int): Row =
        Value.VStruct(
            mapOf(
                "id" to Value.VInt(id.toLong()),
                "a" to Value.VInt(next(4).toLong()),
                "c" to if (next(4) == 0) Value.VNull else Value.VInt(next(5).toLong()),
                "b" to Value.VText(words[next(words.size)]),
                "p" to if (next(3) == 0) Value.VNull else Value.VInt(next(3).toLong()),
            ))

    private fun write(st: Store) {
        val id = next(40)
        val old = st.get("t", listOf(Value.VInt(id.toLong())))
        st.applyChange(
            if (old != null && next(4) == 0) Change.Remove("t", old) else Change.Add("t", row(id)))
    }

    private fun intOrNull(): Value =
        if (next(6) == 0) Value.VNull else Value.VInt(next(5).toLong() - 1)

    private fun bound(): Bound =
        when (next(3)) {
            0 -> Bound.Unbounded
            1 -> Bound.Included(intOrNull())
            else -> Bound.Excluded(intOrNull())
        }

    private fun inSpan(s: Span, v: Value): Boolean {
        val lo = s.lo
        val hi = s.hi
        val okLo =
            when (lo) {
                is Bound.Unbounded -> true
                is Bound.Included -> cmp(CmpOp.Ge, v, lo.v)
                is Bound.Excluded -> cmp(CmpOp.Gt, v, lo.v)
            }
        val okHi =
            when (hi) {
                is Bound.Unbounded -> true
                is Bound.Included -> cmp(CmpOp.Le, v, hi.v)
                is Bound.Excluded -> cmp(CmpOp.Lt, v, hi.v)
            }
        return okLo && okHi
    }

    private fun check(st: Store, what: String) {
        val all = st.scan("t")
        val cols = listOf("id", "a", "c", "p")
        val eq = cols.filter { next(3) == 0 }.map { it to intOrNull() }
        val spans =
            (if (next(2) == 0) listOf(Span(cols[next(cols.size)], bound(), bound()))
            else emptyList())
        val needle = if (next(3) == 0) fold(words[next(words.size)].take(1 + next(5))) else null
        val keep: Keep = { r ->
            eq.all { (c, v) -> cmp(CmpOp.Eq, r.fields[c] ?: Value.VNull, v) } &&
                spans.all { inSpan(it, r.fields[it.column] ?: Value.VNull) } &&
                (needle == null ||
                    ((r.fields["b"] as? Value.VText)?.value?.let { fold(it).contains(needle) }
                        ?: false))
        }
        val want = all.filter(keep)
        val got =
            if (needle != null)
                st.scanWhereText("t", eq, spans, listOf(listOf("b" to needle)), keep)
            else st.scanWhereEq("t", eq, spans, keep)
        if (got != want)
            fail(
                "$what: a read through the hints with eq $eq, spans $spans, needle $needle answered ${got.size} rows, the scan ${want.size}")
        val orderCols = listOf("a", "c", "id")
        val order =
            (0..next(2))
                .map { orderCols[next(orderCols.size)] to if (next(2) == 0) Dir.Asc else Dir.Desc }
                .distinctBy { it.first }
        val limit = next(6)
        val ordered = st.scanOrdered("t", eq, spans, order, keep, limit)
        if (ordered != null) {
            val sorted =
                want.sortedWith { x, y -> Rules.compareRows(item, order, x, y) }.take(limit)
            if (ordered != sorted)
                fail(
                    "$what: an ordered read by $order, eq $eq, spans $spans, limit $limit answered ${ordered.size} rows, the sort ${sorted.size}")
        }
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val st = MemoryStore.empty(schema)
        for (k in 0..2) st.applyChange(
            Change.Add("p", Value.VStruct(mapOf("id" to Value.VInt(k.toLong())))))
        var reads = 0
        for (round in 0 until 300) {
            write(st)
            check(st, "store, round $round")
            val overlay = Overlay(st)
            repeat(next(4)) { write(overlay) }
            check(overlay, "overlay, round $round")
            reads += 2
        }
        println("store hints: $reads reads through the indexes, each the scan filtered and sorted")
    }

    private fun fail(why: String): Nothing {
        println("store hints: FAILED: $why")
        exitProcess(1)
    }
}
