// The operators native procedures call (Ark.Eval §6.4, §6.5): checked
// arithmetic, comparison under the total order, the list combinators.
package dev.arkdb

public object Ops {
    private fun int(v: Value): Long = (v as? Value.VInt)?.value
        ?: throw Fault.bug("TypeError ${HsShow.text("expected Int, got " + v.show().take(60))}")

    private fun bool(v: Value): Boolean = (v as? Value.VBool)?.value
        ?: throw Fault.bug("TypeError ${HsShow.text("expected Bool, got " + v.show().take(60))}")

    private fun list(v: Value): List<Value> = (v as? Value.VList)?.items
        ?: throw Fault.bug("TypeError ${HsShow.text("expected List, got " + v.show().take(60))}")

    private fun overflow(): Nothing = throw Fault.refuse("integer overflow")

    public fun add(a: Value, b: Value): Value {
        val x = int(a)
        val y = int(b)
        val r = x + y
        if (((x xor r) and (y xor r)) < 0) overflow()
        return Value.VInt(r)
    }

    public fun sub(a: Value, b: Value): Value {
        val x = int(a)
        val y = int(b)
        val r = x - y
        if (((x xor y) and (x xor r)) < 0) overflow()
        return Value.VInt(r)
    }

    public fun mul(a: Value, b: Value): Value {
        val x = int(a)
        val y = int(b)
        val hi = Math.multiplyHigh(x, y)
        val lo = x * y
        if ((hi != 0L || lo < 0) && (hi != -1L || lo >= 0)) overflow()
        return Value.VInt(lo)
    }

    /** Truncating toward zero; `MIN / -1` is an overflow. */
    public fun div(a: Value, b: Value): Value {
        val x = int(a)
        val y = int(b)
        if (y == 0L) throw Fault.refuse("division by zero")
        if (x == Long.MIN_VALUE && y == -1L) overflow()
        return Value.VInt(x / y)
    }

    /** The remainder with the dividend's sign; `MIN % -1` is an overflow. */
    public fun mod(a: Value, b: Value): Value {
        val x = int(a)
        val y = int(b)
        if (y == 0L) throw Fault.refuse("division by zero")
        if (x == Long.MIN_VALUE && y == -1L) overflow()
        return Value.VInt(x % y)
    }

    public fun neg(a: Value): Value {
        val x = int(a)
        if (x == Long.MIN_VALUE) overflow()
        return Value.VInt(-x)
    }

    /** Under the total order, so `NULL = NULL` is true and `NULL < 0` is true. */
    public fun cmp(op: CmpOp, a: Value, b: Value): Value = Value.VBool(op.holds(compareValue(a, b)))

    public fun not(a: Value): Value = Value.VBool(!bool(a))

    /** The argument (or auto) by name; a bug if missing. */
    public fun arg(args: Args, name: String): Value =
        args[name] ?: throw Fault.bug("MissingArg ${HsShow.text(name)}")

    /** `match opt { Some x -> some(x); None -> none() }`. */
    public fun match(opt: Value, some: (Value) -> Value, none: () -> Value): Value =
        if (opt.isNull()) none() else some(opt)

    public fun map(xs: Value, f: (Value) -> Value): Value = Value.VList(list(xs).map(f))

    public fun filter(xs: Value, f: (Value) -> Value): Value = Value.VList(list(xs).filter { bool(f(it)) })

    public fun any(xs: Value, f: (Value) -> Value): Value {
        for (x in list(xs)) if (bool(f(x))) return Value.VBool(true)
        return Value.VBool(false)
    }

    public fun all(xs: Value, f: (Value) -> Value): Value {
        for (x in list(xs)) if (!bool(f(x))) return Value.VBool(false)
        return Value.VBool(true)
    }

    /** Stable, under `compareValue` of the key. */
    public fun sortBy(xs: Value, key: (Value) -> Value): Value {
        val keyed = list(xs).map { it to key(it) }
        return Value.VList(keyed.sortedWith { p, q -> compareValue(p.second, q.second) }.map { it.first })
    }

    public fun fold(xs: Value, init: Value, f: (Value, Value) -> Value): Value {
        var acc = init
        for (x in list(xs)) acc = f(acc, x)
        return acc
    }
}
