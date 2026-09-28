// The vocabulary's helpers, records, option comparisons and repeated autos,
// over the demo's tables. The helpers are harken's key derivations as
// harken/domain/src/library.rs writes them in Rust, spelt in Kotlin, so
// that their emitted closures can be held to the ones harken.ark carries.
package selfdemo

import demo.Demo
import demo.Item
import demo.Playlist
import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

fun workTitle(workTitle: Text, album: Text, catalogue: Text, part: Text): Text =
    helper("work_title", "work_title" to workTitle, "album" to album, "catalogue" to catalogue, "part" to part) { workTitle, album, catalogue, part ->
        pick(workTitle.isEmpty().and(catalogue.isEmpty().not().or(part.isEmpty().not())), album, workTitle)
    }

fun slug(text: Text): Text = helper("slug", "text" to text) { text ->
    concat(text.chars().map { x -> pick(x.isAlnum(), x.lower(), lit(" ")) })
        .trim()
        .chars()
        .fold("") { acc, x ->
            pick(
                x.eq(" ").and(acc.chars().last().mapOr(false) { x2 -> x2.eq("-") }),
                acc,
                concat(list(acc, pick(x.eq(" "), lit("-"), x))),
            )
        }
}

fun keyPart(text: Text): Text = helper("key_part", "text" to text) { text ->
    pick(slug(text).isEmpty(), concat(list(lit("x"), text.trim().fnv1a64().toText())), slug(text))
}

fun workKey(composer: Text, catalogue: Text, title: Text): Text =
    helper("work_key", "composer" to composer, "catalogue" to catalogue, "title" to title) { composer, catalogue, title ->
        concat(list(keyPart(composer), lit("/"), keyPart(pick(catalogue.isEmpty(), title, catalogue))))
    }

fun workId(artist: Text, catalogue: Text, workTitle: Text): Opt<Text> =
    helper("work_id", "artist" to artist, "catalogue" to catalogue, "work_title" to workTitle) { artist, catalogue, workTitle ->
        pick(workTitle.isEmpty().or(artist.isEmpty()), none<Text>(), some(workKey(artist, catalogue, workTitle)))
    }

fun movementKey(workId: Text, no: Int): Text = helper("movement_key", "work_id" to workId, "no" to no) { workId, no ->
    concat(list(workId, lit("#"), no.toText()))
}

fun recordingKey(of: Text, who: Text): Text = helper("recording_key", "of" to of, "who" to who) { of, who ->
    concat(list(of, lit("@"), keyPart(who)))
}

fun recordingId(workId: Opt<Text>, album: Text, title: Text, artist: Text, performer: Text): Text = helper(
    "recording_id",
    "work_id" to workId,
    "album" to album,
    "title" to title,
    "artist" to artist,
    "performer" to performer,
) { workId, album, title, artist, performer ->
    recordingKey(workId.unwrapOr(concat(list(keyPart(album), lit("/"), keyPart(title)))), pick(performer.isEmpty(), artist, performer))
}

fun creditedAs(workId: Opt<Text>, artist: Text, performer: Text): Text =
    helper("credited_as", "work_id" to workId, "artist" to artist, "performer" to performer) { workId, artist, performer ->
        pick(performer.isEmpty(), pick(workId.isSome(), lit(""), artist), performer)
    }

/** What one track's keys come to: a record, not a row. */
class Keys(
    val credited: Text,
    val movement: Text,
    val recording: Text,
    val work: Opt<Text>,
) : Record {
    companion object : Record.Of<Keys> {
        override fun fields(): Fields<Keys> =
            fields<Keys>().field("credited", text()).field("movement", text()).field("recording", text()).field("work", opt(text()))
    }
}

class Track(
    val artist: Text,
    val catalogue: Text,
    val workTitle: Text,
    val album: Text,
    val title: Text,
    val performer: Text,
    val part: Text,
    val no: Int,
) : Input {
    companion object : Input.Of<Track> {
        override fun schema(): Schema<Track> = obj(
            field("artist", text()),
            field("catalogue", text()),
            field("work_title", text()),
            field("album", text()),
            field("title", text()),
            field("performer", text()),
            field("part", text()),
            field("no", int()),
        )
    }
}

/** Two options and how they compare, as a record holding a record. */
class Compared(
    val eq: Bool,
    val ne: Bool,
    val keys: Opt<Keys>,
) : Record {
    companion object : Record.Of<Compared> {
        override fun fields(): Fields<Compared> = fields<Compared>().field("eq", bool()).field("ne", bool()).field("keys", opt(record<Keys>()))
    }
}

class TwoOpts(
    val a: Opt<Text>,
    val b: Opt<Text>,
) : Input {
    companion object : Input.Of<TwoOpts> {
        override fun schema(): Schema<TwoOpts> = obj(field("a", opt(text())), field("b", opt(text())))
    }
}

fun vocab(): Router<Demo> {
    val r = router<Demo>("vocab")
    return r.routes(
        r.input<Track>().query("keys") { _, _, t ->
            val wt = workTitle(t.workTitle, t.album, t.catalogue, t.part)
            val w = workId(t.artist, t.catalogue, wt)
            list(
                Keys(
                    credited = creditedAs(w, t.artist, t.performer),
                    movement = movementKey(w.unwrapOr(lit("")), t.no),
                    recording = recordingId(w, t.album, t.title, t.artist, t.performer),
                    work = w,
                ),
            )
        },
        r.input<TwoOpts>().query("compare") { _, _, i ->
            // A record handed on whole, and read back field by field.
            val k = Keys(credited = lit("c"), movement = lit("m"), recording = lit("r"), work = i.a)
            Compared(eq = i.a.eq(i.b), ne = i.a.ne(i.b), keys = some(k).filter { x -> x.work.ne(none<Text>()) })
        },
        // One auto named in two places is one auto: one id, one clock.
        r.mutation("twice") { ctx, db, _ ->
            db.playlist.insert(Playlist(id = ctx.newId("id"), name = ctx.newId<Playlist>("id").toText(), userId = ctx.user))
            db.item.insert(Item(playlistId = ctx.newId("id"), trackId = ctx.now("at").toText(), pos = ctx.now("at")))
        },
    )
}

fun vocabModule(): Module = Module(vocab())

/** The same name drawn as two kinds of auto. */
fun twiceDifferently(): Module {
    val r = router<Demo>("demo")
    return Module(
        r.routes(
            r.mutation("m") { ctx, db, _ ->
                db.playlist.insert(Playlist(id = ctx.newId("id"), name = ctx.now("id").toText(), userId = ctx.user))
            },
        ),
    )
}
