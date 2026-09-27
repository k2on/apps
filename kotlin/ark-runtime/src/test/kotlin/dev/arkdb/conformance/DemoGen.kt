// The demo module's two mutators, written exactly as `arkc gen kotlin`
// writes them (the spelling the coordinator quoted), so that the
// conformance runner proves generated code compiles against this runtime
// and agrees with the interpreter on the eval vector.
package dev.arkdb.conformance

import dev.arkdb.Args
import dev.arkdb.CmpOp
import dev.arkdb.Ctx
import dev.arkdb.Dir
import dev.arkdb.Fault
import dev.arkdb.Ops
import dev.arkdb.Plan
import dev.arkdb.Pred
import dev.arkdb.Std
import dev.arkdb.Store
import dev.arkdb.Value

object DemoGen {
    fun createPlaylist(db: Store, ctx: Ctx, autos: Args, args: Args) {
        if ((Std.isEmpty(Std.trim(Ops.arg(args, "name")))).asBool()) {
            throw Fault.refuse(Value.text("a playlist needs a name"))
        } else {
        }
        val v0 = db.exists("playlist", listOf(Ops.arg(autos, "id")))
        if ((v0).asBool()) {
            return
        } else {
        }
        db.put(
            "playlist",
            Value.record(
                listOf(
                    "id" to Ops.arg(autos, "id"),
                    "name" to Std.trim(Ops.arg(args, "name")),
                    "user_id" to Value.text(ctx.user),
                ),
            ),
        )
    }

    fun addToPlaylist(db: Store, ctx: Ctx, autos: Args, args: Args) {
        val v0 = db.exists("playlist", listOf(Ops.arg(args, "playlist_id")))
        if ((Ops.not(v0)).asBool()) {
            return
        } else {
        }
        val v1 = db.exists("playlist_item", listOf(Ops.arg(args, "playlist_id"), Ops.arg(args, "media_id")))
        if ((v1).asBool()) {
            return
        } else {
        }
        val v2 = db.select(
            Plan.from("playlist_item").filter(Pred.cmp("playlist_id", CmpOp.Eq, Ops.arg(args, "playlist_id")))
                .orderBy("pos", Dir.Desc).orderBy("playlist_id", Dir.Asc).orderBy("media_id", Dir.Asc).limit(1),
        )
        val v4 = Std.unwrapOr(Ops.match(Std.first(v2), { v3 -> (v3).field("pos") }, { Value.`null`() }), Value.int(0L))
        db.put(
            "playlist_item",
            Value.record(
                listOf(
                    "added_ms" to Ops.arg(autos, "added_ms"),
                    "media_id" to Ops.arg(args, "media_id"),
                    "playlist_id" to Ops.arg(args, "playlist_id"),
                    "pos" to Ops.add(v4, Value.int(1L)),
                    "user_id" to Value.text(ctx.user),
                ),
            ),
        )
    }

    fun apply(fnHashHex: String, db: Store, ctx: Ctx, autos: Args, args: Args, functions: Map<String, String>) {
        when (fnHashHex) {
            functions["create_playlist"] -> createPlaylist(db, ctx, autos, args)
            functions["add_to_playlist"] -> addToPlaylist(db, ctx, autos, args)
            else -> throw Fault.bug("unknown function $fnHashHex")
        }
    }
}
