// §2.2 The type section, and a value held to a type (`rust/ark/src/types.rs`,
// `docs/plan-live.md`). A live state is made of records and sums the module
// declares by name (`Module.types`). Three things are asked of them, and all
// three are here: what a named type stands for (`resolve`), whether a value
// is one (`conforms`) — asked of an input, a snapshot and a transition's
// result, wherever a value crosses a boundary — and which of a state's
// fields are ephemeral, which the state's hash and what a server keeps leave
// out (`ephemeralFields`, `orderedPart`).
//
// A value of each type, exactly:
//
// | type | its values |
// |---|---|
// | `Bool`, `Int`, `Text`, `Bytes`, `Id t` | the value of that kind |
// | `Enum vs` | a `Text` that is one of `vs` |
// | `Option t` | `Null`, or a value of `t` |
// | `List t` | a list of values of `t` |
// | `NonEmpty t` | a list of values of `t` with at least one |
// | `Struct fs` | a struct of exactly the fields `fs`, each of its type |
// | `Sum vs` | a struct whose `t` names a variant of `vs`, and whose other fields are exactly that
// variant's |
// | `PerDevice t` | a struct keyed by device ids, each value of `t` |
// | `Ephemeral t` | a value of `t` |
// | `Named n` | a value of what `n` is declared as |
package dev.arkdb

public object Types {
    /** The tag a sum's value carries its variant's name under, as the IR's own nodes do. */
    public const val TAG: String = "t"

    // The most a type may nest named types: deeper is a cycle.
    private const val DEPTH: Int = 64

    /** Why a type does not resolve or a value is not of it: the reference's sentence. */
    public class TypeError(public val why: String) : Exception(why, null, false, false)

    /**
     * `resolve`: `t` with every named type replaced by its declaration, at any depth. A `TypeError`
     * names a type the section does not declare, or one that refers to itself.
     */
    public fun resolve(types: Map<String, Ty>, t: Ty): Ty = resolveAt(types, t, ArrayList())

    private fun resolveAt(types: Map<String, Ty>, t: Ty, seen: MutableList<String>): Ty =
        when (t) {
            is Ty.TNamed -> {
                if (t.name in seen || seen.size > DEPTH)
                    throw TypeError("the type ${t.name} refers to itself")
                val d = types[t.name] ?: throw TypeError("no type ${t.name}")
                seen.add(t.name)
                val r = resolveAt(types, d, seen)
                seen.removeAt(seen.size - 1)
                r
            }
            is Ty.TOption -> Ty.TOption(resolveAt(types, t.of, seen))
            is Ty.TList -> Ty.TList(resolveAt(types, t.of, seen))
            is Ty.TNonEmpty -> Ty.TNonEmpty(resolveAt(types, t.of, seen))
            is Ty.TPerDevice -> Ty.TPerDevice(resolveAt(types, t.of, seen))
            is Ty.TEphemeral -> Ty.TEphemeral(resolveAt(types, t.of, seen))
            is Ty.TStruct -> Ty.TStruct(t.fields.mapValues { resolveAt(types, it.value, seen) })
            is Ty.TSum ->
                Ty.TSum(
                    t.variants.mapValues { (_, fs) ->
                        fs.mapValues { resolveAt(types, it.value, seen) }
                    })
            else -> t
        }

    /**
     * `unmarked`: a resolved type with every `Ephemeral` marker taken off — what the checker reads
     * a state's fields as.
     */
    public fun unmarked(t: Ty): Ty =
        when (t) {
            is Ty.TEphemeral -> unmarked(t.of)
            is Ty.TOption -> Ty.TOption(unmarked(t.of))
            is Ty.TList -> Ty.TList(unmarked(t.of))
            is Ty.TNonEmpty -> Ty.TNonEmpty(unmarked(t.of))
            is Ty.TPerDevice -> Ty.TPerDevice(unmarked(t.of))
            is Ty.TStruct -> Ty.TStruct(t.fields.mapValues { unmarked(it.value) })
            is Ty.TSum ->
                Ty.TSum(t.variants.mapValues { (_, fs) -> fs.mapValues { unmarked(it.value) } })
            else -> t
        }

    /**
     * `ephemeral_fields`: the fields of a state of this resolved type marked `Ephemeral`; none for
     * a state that is not a record.
     */
    public fun ephemeralFields(state: Ty): Set<String> =
        if (state is Ty.TStruct)
            state.fields.filterValues { it is Ty.TEphemeral }.keys.toSortedSet(CodePointOrder)
        else emptySet()

    /** `ordered_part`: a state's value with its ephemeral fields taken out. */
    public fun orderedPart(ephemeral: Set<String>, v: Value): Value =
        if (v is Value.VStruct && ephemeral.isNotEmpty())
            Value.VStruct(v.fields.filterKeys { it !in ephemeral })
        else v

    /** `with_empty_slots`: a value restored from its ordered part, every ephemeral field empty. */
    public fun withEmptySlots(ephemeral: Set<String>, v: Value): Value {
        if (v !is Value.VStruct || ephemeral.isEmpty()) return v
        val m = java.util.TreeMap<String, Value>(CodePointOrder)
        m.putAll(v.fields)
        for (f in ephemeral) m[f] = Value.VStruct(emptyMap())
        return Value.VStruct(m)
    }

    /**
     * `conforms`: whether `v` is a value of `t`; a `TypeError` says where it is not, as a path and
     * a sentence: `queue: an empty list where a non-empty one is declared`.
     */
    public fun conforms(types: Map<String, Ty>, t: Ty, v: Value) {
        conformAt(types, t, v, ArrayList(), 0)
    }

    /** [conforms] as a sentence, or null where the value is one. */
    public fun whyNot(types: Map<String, Ty>, t: Ty, v: Value): String? =
        try {
            conforms(types, t, v)
            null
        } catch (e: TypeError) {
            e.why
        }

    private fun at(path: List<String>, what: String): String =
        if (path.isEmpty()) what else "${path.joinToString(".")}: $what"

    private fun conformAt(
        types: Map<String, Ty>,
        t: Ty,
        v: Value,
        path: MutableList<String>,
        depth: Int
    ) {
        fun kind(want: String): Nothing =
            throw TypeError(at(path, "expected $want, found ${short(v)}"))
        when (t) {
            is Ty.TNamed -> {
                if (depth > DEPTH) throw TypeError(at(path, "the type ${t.name} refers to itself"))
                val d = types[t.name] ?: throw TypeError(at(path, "no type ${t.name}"))
                conformAt(types, d, v, path, depth + 1)
            }
            is Ty.TEphemeral -> conformAt(types, t.of, v, path, depth)
            is Ty.TBool -> if (v !is Value.VBool) kind("a bool")
            is Ty.TInt -> if (v !is Value.VInt) kind("an int")
            is Ty.TText -> if (v !is Value.VText) kind("text")
            is Ty.TBytes -> if (v !is Value.VBytes) kind("bytes")
            is Ty.TId -> if (v !is Value.VId) kind("an id")
            is Ty.TEnum ->
                if (v !is Value.VText || v.value !in t.variants)
                    throw TypeError(
                        at(
                            path,
                            "expected one of ${t.variants.joinToString(", ")}, found ${short(v)}"))
            is Ty.TOption -> if (v !is Value.VNull) conformAt(types, t.of, v, path, depth)
            is Ty.TList ->
                if (v is Value.VList) elems(types, t.of, v.items, path, depth) else kind("a list")
            is Ty.TNonEmpty -> {
                if (v !is Value.VList) kind("a list")
                if (v.items.isEmpty())
                    throw TypeError(at(path, "an empty list where a non-empty one is declared"))
                elems(types, t.of, v.items, path, depth)
            }
            is Ty.TStruct ->
                if (v is Value.VStruct) fields(types, t.fields, v.fields, false, path, depth)
                else kind("a record")
            is Ty.TSum -> {
                if (v !is Value.VStruct) kind("a variant")
                val tag =
                    v.fields[TAG] as? Value.VText
                        ?: throw TypeError(at(path, "a sum's value names its variant under t"))
                val fs =
                    t.variants[tag.value] ?: throw TypeError(at(path, "no variant ${tag.value}"))
                path.add(tag.value)
                fields(types, fs, v.fields, true, path, depth)
                path.removeAt(path.size - 1)
            }
            is Ty.TPerDevice -> {
                if (v !is Value.VStruct) kind("a map of devices")
                for ((k, e) in v.fields) {
                    path.add("[$k]")
                    conformAt(types, t.of, e, path, depth)
                    path.removeAt(path.size - 1)
                }
            }
        }
    }

    private fun elems(
        types: Map<String, Ty>,
        t: Ty,
        xs: List<Value>,
        path: MutableList<String>,
        depth: Int
    ) {
        for ((i, x) in xs.withIndex()) {
            path.add("[$i]")
            conformAt(types, t, x, path, depth)
            path.removeAt(path.size - 1)
        }
    }

    private fun fields(
        types: Map<String, Ty>,
        fs: Map<String, Ty>,
        m: Map<String, Value>,
        tagged: Boolean,
        path: MutableList<String>,
        depth: Int
    ) {
        for (k in m.keys) {
            if (!fs.containsKey(k) && !(tagged && k == TAG))
                throw TypeError(at(path, "no field $k"))
        }
        for ((k, t) in fs) {
            val x = m[k] ?: throw TypeError(at(path, "missing field $k"))
            path.add(k)
            conformAt(types, t, x, path, depth)
            path.removeAt(path.size - 1)
        }
    }

    // A value named briefly, for a sentence.
    private fun short(v: Value): String =
        when (v) {
            is Value.VNull -> "nothing"
            is Value.VBool -> "a bool"
            is Value.VInt -> "an int"
            is Value.VText -> "text"
            is Value.VBytes -> "bytes"
            is Value.VId -> "an id"
            is Value.VList -> "a list"
            is Value.VStruct -> "a struct"
        }
}
