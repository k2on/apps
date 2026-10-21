// harken's peers as the JVM tests make them: the real kernel — the library
// built for this machine (`harken-android-lib`'s `host/`), loaded through
// JNA — each on a disk of its own, signed in, and alone or in a [Hall] (a
// harken server in the same process, `harken/ffi`).
package dev.harken.android

import dev.arkdb.ffi.Login
import dev.harken.android.gen.AddSong
import dev.harken.android.gen.Device as Declared
import dev.harken.android.gen.Harken
import dev.harken.ffi.Hall
import dev.harken.ffi.harkenOpen
import java.nio.file.Files

/** A login of `user` on `session`, holding `roles`. */
fun testLogin(user: String, session: String = "s1", roles: List<String> = emptyList(), name: String = "", email: String = ""): Login =
    Login("t", session, user, name, email, roles, 0)

/** What a phone says it is in the listening session. */
fun declared(name: String = "Phone", canPlay: Boolean = true, kind: String = "phone"): Declared = Declared(canPlay, "", kind, name, "")

/** A disk of its own. */
fun scratch(): String = Files.createTempDirectory("harken").toFile().path

/** A peer alone — its own authority — signed in as `user` holding `roles`: the library's role by default. */
fun alone(user: String = "library", roles: List<String> = listOf("library")): Harken {
    val p = harkenOpen(scratch(), testLogin(user, "s-$user", roles), declared().json())
    p.alone()
    return Harken(p)
}

/** A device of alice's in `hall`, its login's session — its id in the listening session — `session`. */
fun Hall.device(session: String, canPlay: Boolean = true, kind: String = "phone"): Harken =
    Harken(device(scratch(), "alice", session, declared(session, canPlay, kind).json()))

/** Its socket goes: not a decision about the music. */
fun Hall.drop(h: Harken) {
    h.peer.setOnline(false)
    carry()
}

/** Back, on a connection of its own. */
fun Hall.relink(h: Harken) {
    h.peer.setOnline(true)
    carry()
}

/** Alice's listening session as the hall holds it. */
fun Hall.state(): Listening = Listening.parse(listening("alice"))

/** Whether a device's socket is up. */
val Harken.linked: Boolean
    get() = peer.status().linked

/** A song of the seed's shape: `i`'s title and file, on album `album` (none for ""). */
fun song(i: Int, album: String = "Album ${i / 7}", artist: String = "Artist ${i / 10}", ms: Long = 180_000): AddSong =
    AddSong(
        title = "Song $i",
        artist = artist,
        album = album,
        durationMs = ms,
        file = "music/$i.flac",
        track = (i % 7 + 1).toLong(),
        part = "",
        catalogue = "",
        performer = "",
        bpm = 0,
        albumArt = "covers/${i / 7}.jpg",
        artistArt = "",
        disc = 1,
        workTitle = "",
        movementNo = 0,
    )
