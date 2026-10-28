// The library the screenshots are drawn from: rows taken from harken's demo
// seed (`harken/iced/src/seed.rs`) — six of its records, their titles,
// composers, parts, track numbers, performers and lengths as the seed has
// them — laid out as the module's queries answer them: `library` in the
// order added, `albums` and `artists` by name, `track_details` per song.
// Fixed, so a screenshot changes only when a screen does.
package dev.harken.android

import dev.arkdb.ffi.Id
import uniffi.harken_domain.Playlist

object Demo {
    private class Row(
        val title: String,
        val composer: String,
        val album: String,
        val part: String,
        val track: Long,
        val performer: String,
        val licence: String,
        val ms: Long,
    )

    private val rows =
        listOf(
        Row("Alla Hornpipe", "George Frideric Handel", "Water Music", "Suite No. 2 in D major", 12, "United States Marine Band", "", 229000),
        Row("Minuet", "George Frideric Handel", "Water Music", "Suite No. 2 in D major", 13, "United States Marine Band", "", 195000),
        Row("Lentement", "George Frideric Handel", "Water Music", "Suite No. 2 in D major", 14, "United States Marine Band", "", 136000),
        Row("Bourrée", "George Frideric Handel", "Water Music", "Suite No. 2 in D major", 15, "United States Marine Band", "", 76000),
        Row("Sarabande", "George Frideric Handel", "Water Music", "Suite No. 3 in G major", 16, "United States Marine Band", "", 168000),
        Row("Rigaudon", "George Frideric Handel", "Water Music", "Suite No. 3 in G major", 17, "United States Marine Band", "", 156000),
        Row("Menuet", "George Frideric Handel", "Water Music", "Suite No. 3 in G major", 19, "United States Marine Band", "", 227000),
        Row("Gigue", "George Frideric Handel", "Water Music", "Suite No. 3 in G major", 21, "United States Marine Band", "", 85000),
        Row("Overture", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 1, "", "CC BY-SA 3.0", 226847),
        Row("Adagio e staccato", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 2, "", "CC BY-SA 3.0", 160758),
        Row("Allegro – Andante – Allegro", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 3, "", "CC BY-SA 3.0", 541806),
        Row("Presto", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 4, "", "CC BY-SA 3.0", 161226),
        Row("Air", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 5, "", "CC BY-SA 3.0", 251246),
        Row("Minuet", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 6, "", "CC BY-SA 3.0", 174315),
        Row("Bourrée – Hornpipe", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 7, "", "CC BY-SA 3.0", 87510),
        Row("Allegro moderato", "George Frideric Handel", "Water Music", "Suite No. 1 in F major", 8, "", "CC BY-SA 3.0", 201273),
        Row("I. Allegro", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 1 in E major, RV 269 (Spring)", 1, "The Modena Chamber Orchestra", "", 214013),
        Row("II. Largo", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 1 in E major, RV 269 (Spring)", 2, "The Modena Chamber Orchestra", "", 178777),
        Row("III. Allegro", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 1 in E major, RV 269 (Spring)", 3, "The Modena Chamber Orchestra", "", 258151),
        Row("I. Allegro non molto", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 2 in G minor, RV 315 (Summer)", 4, "The Modena Chamber Orchestra", "", 328064),
        Row("II. Adagio", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 2 in G minor, RV 315 (Summer)", 5, "The Modena Chamber Orchestra", "", 124823),
        Row("III. Presto", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 2 in G minor, RV 315 (Summer)", 6, "The Modena Chamber Orchestra", "", 176622),
        Row("I. Allegro", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 3 in F major, RV 293 (Autumn)", 7, "The Modena Chamber Orchestra", "", 329337),
        Row("II. Adagio molto", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 3 in F major, RV 293 (Autumn)", 8, "The Modena Chamber Orchestra", "", 200703),
        Row("III. Allegro", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 3 in F major, RV 293 (Autumn)", 9, "The Modena Chamber Orchestra", "", 202379),
        Row("I. Allegro non molto", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 4 in F minor, RV 297 (Winter)", 10, "The Modena Chamber Orchestra", "", 205514),
        Row("II. Largo", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 4 in F minor, RV 297 (Winter)", 11, "The Modena Chamber Orchestra", "", 163847),
        Row("III. Allegro", "Antonio Vivaldi", "The Four Seasons", "Concerto No. 4 in F minor, RV 297 (Winter)", 12, "The Modena Chamber Orchestra", "", 178571),
        Row("Aria", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 1, "Kimiko Ishizaka", "", 300000),
        Row("Variatio 1 a 1 Clav", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 2, "Kimiko Ishizaka", "", 115000),
        Row("Variatio 2 a 1 Clav", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 3, "Kimiko Ishizaka", "", 124000),
        Row("Variatio 3 a 1 Clav Canone allUnisuono", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 4, "Kimiko Ishizaka", "", 117000),
        Row("Variatio 4 a 1 Clav", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 5, "Kimiko Ishizaka", "", 69000),
        Row("Variatio 5 a 1 ovvero 2 Clav", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 6, "Kimiko Ishizaka", "", 94000),
        Row("Variatio 6 a 1 Clav Canone alla Seconda", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 7, "Kimiko Ishizaka", "", 98000),
        Row("Variatio 7 a 1 ovvero 2 Clav", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 8, "Kimiko Ishizaka", "", 132000),
        Row("Variatio 8 a 2 Clav", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 9, "Kimiko Ishizaka", "", 117000),
        Row("Variatio 9 a 1 Clav Canone alla Terza", "Johann Sebastian Bach", "Goldberg Variations, BWV 988", "", 10, "Kimiko Ishizaka", "", 126000),
        Row("I. Overture", "George Frideric Handel", "Music for the Royal Fireworks", "", 1, "Paul Ayres", "", 135941),
        Row("II. Bourrée", "George Frideric Handel", "Music for the Royal Fireworks", "", 2, "Paul Ayres", "", 84088),
        Row("III. La Paix", "George Frideric Handel", "Music for the Royal Fireworks", "", 3, "Paul Ayres", "", 163605),
        Row("IV. La Réjouissance", "George Frideric Handel", "Music for the Royal Fireworks", "", 4, "Paul Ayres", "", 98717),
        Row("V. Menuet", "George Frideric Handel", "Music for the Royal Fireworks", "", 5, "Paul Ayres", "", 115827),
        Row("I. Allegro", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 1 in F major, BWV 1046", 1, "Busch Chamber Players", "CC BY 3.0", 276452),
        Row("II. Adagio", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 1 in F major, BWV 1046", 2, "Busch Chamber Players", "CC BY-SA 3.0", 281967),
        Row("III. Allegro", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 1 in F major, BWV 1046", 3, "Busch Chamber Players", "CC BY-SA 3.0", 272877),
        Row("IV. Menuetto - Trio - Polacca", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 1 in F major, BWV 1046", 4, "Busch Chamber Players", "CC BY-SA 3.0", 480184),
        Row("I. Allegro", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 2 in F major, BWV 1047", 1, "Vince DiMartino and the Lexington Bach Choir Orchestra", "CC BY 3.0", 317100),
        Row("II. Andante", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 2 in F major, BWV 1047", 2, "Vince DiMartino and the Lexington Bach Choir Orchestra", "CC BY 3.0", 197695),
        Row("III. Allegro assai", "Johann Sebastian Bach", "Brandenburg Concertos", "Concerto No. 2 in F major, BWV 1047", 3, "Vince DiMartino and the Lexington Bach Choir Orchestra", "CC BY 3.0", 224052),
        Row("Sinfony", "George Frideric Handel", "Messiah", "Part I", 1, "London Symphony Orchestra, Hermann Scherchen", "", 317006),
        Row("Comfort ye my people", "George Frideric Handel", "Messiah", "Part I", 2, "London Symphony Orchestra, Hermann Scherchen", "", 238006),
        Row("Ev'ry valley shall be exalted", "George Frideric Handel", "Messiah", "Part I", 3, "London Symphony Orchestra, Hermann Scherchen", "", 210014),
        Row("And the glory of the Lord", "George Frideric Handel", "Messiah", "Part I", 4, "London Symphony Orchestra, Hermann Scherchen", "", 167014),
        Row("Thus saith the Lord of hosts", "George Frideric Handel", "Messiah", "Part I", 5, "London Symphony Orchestra, Hermann Scherchen", "", 120005),
        Row("But who may abide the day of His coming", "George Frideric Handel", "Messiah", "Part I", 6, "London Symphony Orchestra, Hermann Scherchen", "", 275007),
        Row("And he shall purify the sons of Levi", "George Frideric Handel", "Messiah", "Part I", 7, "London Symphony Orchestra, Hermann Scherchen", "", 124016),
        Row("Behold, a virgin shall conceive", "George Frideric Handel", "Messiah", "Part I", 8, "London Symphony Orchestra, Hermann Scherchen", "", 46012),
        )

    private fun id(n: Int): Id = Id("%032x".format(n + 1))

    /** Every song, in the order added. */
    val tracks: List<Track> =
        rows.mapIndexed { i, r ->
            Track(id(i), r.title, r.composer, "song", r.ms, "music/" + r.album + "/" + r.title + ".mp3", null, 1_700_000_000_000 + i * 60_000L)
        }

    val details: Map<Id, TrackDetail> =
        // A track in a part is a movement of the work its album names, as `add_song` keys it.
        rows.mapIndexed { i, r ->
            val work = if (r.part.isNotEmpty()) r.album else ""
            id(i) to TrackDetail(id(i), r.album, key(r.album), r.part, r.track, r.performer, r.licence, if (work.isEmpty()) "" else r.composer, work)
        }.toMap()

    val albums: List<Album> =
        rows.groupBy { it.album }
            .map { (name, rs) ->
                // Who is credited on the most of it, as `albums` says.
                val performer = rs.filter { it.performer.isNotEmpty() }.groupingBy { it.performer }.eachCount().maxByOrNull { it.value }?.key ?: ""
                Album(key(name), name, rs.first().composer, "", rs.size.toLong(), performer = performer, label = if (name == "Messiah") "Westminster" else "", released = if (name == "Messiah") 1959 else 0)
            }
            .sortedBy { it.name }

    val artists: List<Artist> =
        rows.groupBy { it.composer }.map { (name, rs) -> Artist(name, "", rs.size.toLong()) }.sortedBy { it.name }

    /** Everyone a work is by. */
    val composers: List<Composer> =
        rows.filter { it.part.isNotEmpty() }.groupBy { it.composer }.map { (name, rs) -> Composer(name, "", "", 0, 0, rs.map { it.album }.distinct().size.toLong(), rs.size.toLong()) }.sortedBy { it.name }

    /** The demo's two shows, as `shows` lists them: a series of sermons and a podcast. */
    val shows: List<Show> =
        listOf(
            Show("podcast/full-time-nix", "podcast", "Full Time Nix", "Shahar \"Dawn\" Or", "Real chats with brilliant Nixers", "", 4),
            Show("sermon/timothy-keller-sermons-podcast-by-gospel-in-life", "sermon", "Timothy Keller Sermons Podcast by Gospel in Life", "Tim Keller", "Sermons by Tim Keller, founder of Redeemer Presbyterian Church in NYC.", "", 3),
        )

    /** Gospel in Life's newest episodes, newest first, as `show` reads them. */
    val sermons: List<Track> =
        listOf("The Dream of the Kingdom" to 2_937_000L, "Love and the Future of God" to 2_725_000L, "Living Grace" to 2_824_000L).mapIndexed { i, (t, ms) ->
            Track(id(900 + i), t, shows[1].title, "sermon", ms, ".feeds/$i.mp3", null)
        }

    /** A composer's works as `works` lists them, and as many performances of each as the seed has. */
    fun works(composer: String): List<dev.harken.android.gen.WorkSummary> =
        rows.filter { it.composer == composer && it.part.isNotEmpty() }.groupBy { it.album }.map { (album, rs) ->
            dev.harken.android.gen.WorkSummary("", "", composer, "", "", composer.lowercase().replace(' ', '-') + "/" + album.lowercase().replace(' ', '-'), "", rs.map { it.performer }.distinct().size.toLong(), album, rs.size.toLong())
        }

    /** A work's performances, the most complete first, as `recordings` answers. */
    fun recordings(work: String): List<dev.harken.android.gen.RecordingsEntry> =
        rows.filter { it.part.isNotEmpty() && it.album == work }.groupBy { it.performer }.map { (who, rs) ->
            dev.harken.android.gen.RecordingsEntry("", rs.sumOf { it.ms }, "w@" + who.lowercase().replace(' ', '-'), "", rs.first().licence, who, if (who.isEmpty()) 0 else 1959, rs.size.toLong())
        }.sortedByDescending { it.tracks }

    val playlists: List<Playlist> =
        listOf("Favorites", "Sunday morning", "Baroque for working").mapIndexed { i, n ->
            Playlist(Id("%032x".format(1000 + i)), n, i.toLong(), 1_700_000_000_000 + i, "ana")
        }

    /** What is on "Sunday morning". */
    val sunday: List<Track> = listOf(0, 16, 28, 33, 12, 4).map { tracks[it] }

    fun albumOf(t: Track): Album? = details[t.id]?.albumId?.let { a -> albums.firstOrNull { it.id == a } }

    /** An album's key, as `keys::album_id` makes it of the folder its tracks are in and its name. */
    fun key(album: String): String = "music/$album/" + album.map { if (it.isLetterOrDigit()) it.lowercaseChar() else ' ' }.joinToString("").trim().replace(' ', '-')

    /** The songs of the album called `name`. */
    fun album(name: String): List<Track> = tracks.filter { details[it.id]?.albumId == key(name) }

    fun artist(name: String): List<Track> = tracks.filter { it.creator == name }

    fun playing(t: Track): Playing = Playing(t.id.hex, t.title, t.creator, albumOf(t)?.name ?: "", "")
}
