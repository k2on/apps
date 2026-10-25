// What a catalogue's pages say, as the desktop's say it (`harken/iced/src/
// view.rs`, `main.rs`'s `known`): which of a composer's works are best known
// and which one is set apart, the lines under a work and a performance, who
// is on a recording, and where an album's tracks fall under its works. Only
// the sorting and the joining of what the views already say — the counting
// is the domain's — and nothing of Compose, so the tests read it plainly.
package dev.harken.android

import dev.harken.android.gen.CreditsEntry
import dev.harken.android.gen.RecordingsEntry
import dev.harken.android.gen.WorkSummary

object Catalogue {
    /** How many works a composer's page lists before its "See All". */
    const val KNOWN_WORKS = 6

    /** What a catalogue's lines are joined with. */
    const val DOT = " · "

    private fun facts(vararg parts: String?): String = parts.filterNotNull().filter { it.isNotEmpty() }.joinToString(DOT)

    /**
     * Works best known first: the most recorded, then the most tracks, the catalogue's order (the
     * `works` view's) between equals.
     */
    fun known(works: List<WorkSummary>): List<WorkSummary> = works.sortedWith(compareByDescending<WorkSummary> { it.recordings }.thenByDescending { it.tracks })

    /**
     * The works a composer's page lists, in the order it lists them: every one by catalogue when
     * there are few or "See All" was pressed (`all`), and otherwise the best known up to
     * [KNOWN_WORKS].
     */
    fun listed(works: List<WorkSummary>, all: Boolean): List<WorkSummary> = if (all || works.size <= KNOWN_WORKS) works else known(works).take(KNOWN_WORKS)

    /** Whether a page listing `listed` of `works` is short of them all, and has a "See All". */
    fun short(listed: List<WorkSummary>, works: List<WorkSummary>): Boolean = listed.size < works.size

    /**
     * The most recorded work, set apart on a composer's page — when one is: a tie is no answer to
     * "which is best known", and one work alone is not set apart from anything.
     */
    fun mostRecorded(works: List<WorkSummary>): WorkSummary? {
        val ranked = known(works)
        if (ranked.size < 2) return null
        return ranked[0].takeIf { it.recordings > ranked[1].recordings }
    }

    /** The set-apart work's line: its catalogue number and how many performances of it there are. */
    fun recordedFacts(w: WorkSummary): String = facts(w.catalogue, Media.count(w.recordings.toInt(), "recording"))

    /** What a work is, at its page's head: catalogue number (unless its title ends with it), form, period. */
    fun workFacts(w: WorkSummary): String = facts(w.catalogue.takeUnless { it.isNotEmpty() && w.title.contains(it) }, w.form, w.period)

    /** When a person lived, as a catalogue heads them: `1685–1750`, `1685–`, `–1750`, or nothing. */
    fun span(born: Long, died: Long): String =
        when {
            born == 0L && died == 0L -> ""
            born == 0L -> "–$died"
            died == 0L -> "$born–"
            else -> "$born–$died"
        }

    /**
     * How a catalogue writes who a work is by over its title: every name but the last as an
     * initial, a particle kept — `A. Vivaldi`, `J. S. Bach`, `L. van Beethoven`.
     */
    fun initials(name: String): String {
        val words = name.split(' ').filter { it.isNotEmpty() }
        if (words.isEmpty()) return ""
        val first = words.dropLast(1).map { w -> if (w[0].isUpperCase()) "${w[0]}." else w }
        return (first + words.last()).joinToString(" ")
    }

    /** Who played a performance — or, where nobody is credited, that nobody is: never a guess. */
    fun who(r: RecordingsEntry): String = r.performers.ifEmpty { "Performer not named" }

    /** A performance's line: when, how much, how long, under what licence — `2016 · 4 tracks · 22 min`. */
    fun takeFacts(r: RecordingsEntry): String =
        facts(if (r.recorded > 0) r.recorded.toString() else null, Media.count(r.tracks.toInt(), "track"), if (r.durationMs > 0) Media.spell(r.durationMs) else null, r.licence)

    /** What a work's page sets its first performance apart as: the fullest of several, or the only one. */
    fun featuredLabel(recordings: Int): String = if (recordings > 1) "Most Complete" else "The Recording"

    /** The work a performance is of: everything before the `@` of its key, or none. */
    fun workOf(recording: String): String? = recording.substringBefore('@', "").takeIf { it.isNotEmpty() }

    /** A recording's title: the work's, and its catalogue number after a comma. */
    fun recordingTitle(w: WorkSummary?, fallback: String): String =
        when {
            w == null -> fallback
            w.catalogue.isEmpty() -> w.title
            else -> "${w.title}, ${w.catalogue}"
        }

    /** Who played a track, with its licence when it has one: `Busch Chamber Players (CC BY 3.0)`. */
    fun credit(performer: String, licence: String): String =
        when {
            licence.isEmpty() -> performer
            performer.isEmpty() -> "($licence)"
            else -> "$performer ($licence)"
        }

    /** Everybody on a recording: the composer, then the credits in billing order, each once with every role they had. */
    fun people(composer: String, credits: List<CreditsEntry>): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        if (composer.isNotEmpty()) out.add(composer to "Composer")
        for (c in credits) {
            var role = c.role
            if (c.instrument.isNotEmpty() && c.instrument != c.role) role = if (role.isEmpty()) c.instrument else "$role, ${c.instrument}"
            val at = out.indexOfFirst { it.first == c.name }
            if (at >= 0) out[at] = c.name to "${out[at].second}, $role" else out.add(c.name to role)
        }
        return out
    }

    /**
     * An album's tracks as runs of one work: where each run starts and ends, the work and whose
     * it is, and how long the run lasts. A song is of no work, and its run has no heading.
     */
    data class Run(val from: Int, val until: Int, val composer: String, val work: String, val ms: Long)

    fun runs(tracks: List<Track>, detail: (Track) -> TrackDetail?): List<Run> {
        val out = ArrayList<Run>()
        var at = 0
        while (at < tracks.size) {
            val d = detail(tracks[at])
            val of = (d?.composer ?: "") to (d?.work ?: "")
            var end = at + 1
            while (end < tracks.size && detail(tracks[end]).let { (it?.composer ?: "") to (it?.work ?: "") } == of) end++
            out.add(Run(at, end, of.first, of.second, tracks.subList(at, end).sumOf { it.durationMs }))
            at = end
        }
        return out
    }

    /** The records a list of tracks is on, each once, in the order they first come. */
    fun albumsOf(tracks: List<Track>, detail: (Track) -> TrackDetail?, albums: Map<String, Album>): List<Album> =
        tracks.mapNotNull { t -> detail(t)?.album?.let(albums::get) }.distinctBy { it.name }

    /**
     * Who an album is by, as its page says it: who played it in the accent — whoever is credited
     * on the most of it, which on a record by one orchestra is the orchestra — or, nobody
     * credited, whoever made it; and under it, whose it is when that is somebody else, and the year.
     */
    fun albumBy(album: Album?, creator: String): Pair<String, String> {
        val performer = album?.performer?.takeIf { it.isNotEmpty() && it != creator }
        val small = facts(if (performer != null) creator.uppercase() else null, album?.released?.takeIf { it > 0 }?.toString())
        return (performer ?: creator) to small
    }
}
