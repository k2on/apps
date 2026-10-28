// harken in a car (Car.kt, CoverProvider.kt, PlaybackService): the browse
// tree read from the replica, a cover by its URI, search and voice, and the
// one rule that is the car's — a choice made in the car plays on this
// phone, wherever the sound was. Over harken's module and a library of the
// demo's records and more (RestorationTests' `Fixture`), read through the
// library's own views and the domain's own queries; the session's server is
// the test's (`Hall`), its devices real Kotlin peers. No car and no Desktop
// Head Unit runs here: what is held is what the service hands Media3.
//
// Each was falsified once; the comment beside each says how, and what failed.
package dev.harken.android

import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.Looper
import android.provider.MediaStore
import androidx.media3.common.MediaItem
import androidx.media3.session.MediaConstants
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import dev.harken.android.gen.Harken
import dev.harken.android.onboarding.ArkSession
import dev.harken.ffi.Hall
import dev.harken.android.ui.Art
import dev.harken.android.gen.Output
import java.io.File
import kotlin.random.Random
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class CarTests {
    // For layoutlib: a Bundle, a Uri, a Bitmap, a Looper.
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar")

    private val server = "https://music.example.com"

    /** The fixture's library, read as the car reads it. */
    private fun car(random: Random = Random(7)): Pair<Car, Harken> {
        val peer = Fixture.peer()
        val session = ArkSession.inline(peer, server, testLogin("library", roles = listOf("library")))
        return Car(Kept.of(session, "Phone"), server, random) to peer
    }

    private fun titles(items: List<MediaItem>): List<String> = items.map { it.mediaMetadata.title.toString() }

    private fun tracks(of: () -> dev.arkdb.ffi.View<uniffi.harken_domain.LibraryEntry>): List<Track> = Phone.once(of, Track::of)

    /**
     * The root is four tabs, a fifth only for a car that draws five; Home ends with every song when
     * Songs is not a tab; albums and artists are grids, a list of tracks a list; an album opens to
     * the domain's own `album` query, a playlist to its `playlist` query, page by page; and choosing
     * a track plays the list it was in, from it.
     *
     * Falsified by paging from the page number rather than from page × size: "the second page of
     * Water Music" began at its second track.
     */
    @Test
    fun theBrowseTreeIsTheReplicas() {
        val (car, peer) = car()
        runBlocking {
            val root = car.children(Car.ROOT, 0, 100, Car.TABS)!!
            assertEquals(listOf("Home", "Playlists", "Albums", "Artists"), titles(root))
            assertTrue("every tab browsable", root.all { it.mediaMetadata.isBrowsable == true && it.mediaMetadata.isPlayable == false })
            assertEquals(listOf("Home", "Playlists", "Albums", "Artists", "Songs"), titles(car.children(Car.ROOT, 0, 100, 5)!!))
            val grid = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM
            assertEquals("albums are a grid", grid, root[2].mediaMetadata.extras!!.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE))
            assertEquals("artists are a grid", grid, root[3].mediaMetadata.extras!!.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE))
            assertEquals(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM, Car.rootItem().mediaMetadata.extras!!.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE))

            val home = car.children(Car.HOME, 0, 100, Car.TABS)!!
            assertEquals("four tabs: every song at the end of Home", Car.SONGS, home.last().mediaId)
            assertTrue(car.children(Car.HOME, 0, 100, 5)!!.none { it.mediaId == Car.SONGS })

            // The albums: every one, as the `albums` view has them, each a page.
            val albums = car.children(Car.ALBUMS, 0, 1000, Car.TABS)!!
            assertEquals(Phone.once({ peer.albums() }) { it.name }, titles(albums))
            val water = albums.first { it.mediaMetadata.title == "Water Music" }
            assertEquals(Page.ForAlbum("Water Music").route, water.mediaId)

            // An album's tracks: the domain's `album` query, in its order, playable.
            val want = tracks { Phone.album(peer, "Water Music") }
            val all = car.children(water.mediaId, 0, 1000, Car.TABS)!!
            assertEquals(want.map { it.title }, titles(all))
            assertTrue(all.all { it.mediaMetadata.isPlayable == true && it.mediaMetadata.isBrowsable == false })
            // Paged: five at a time, the pages one after another, and nothing past the end.
            val first = car.children(water.mediaId, 0, 5, Car.TABS)!!
            val second = car.children(water.mediaId, 1, 5, Car.TABS)!!
            assertEquals("the first page of Water Music", titles(all).take(5), titles(first))
            assertEquals("the second page of Water Music", titles(all).drop(5).take(5), titles(second))
            assertEquals(emptyList<MediaItem>(), car.children(water.mediaId, 100, 5, Car.TABS)!!)

            // A playlist's tracks: the domain's `playlist` query, thirty of them.
            val list = car.children(Car.PLAYLISTS, 0, 100, Car.TABS)!!.first { it.mediaMetadata.title == Fixture.playlists[0] }
            val onIt = car.children(list.mediaId, 0, 100, Car.TABS)!!
            assertEquals(tracks { Phone.playlist(peer, Fixture.first!!) }.map { it.title }, titles(onIt))
            assertEquals(30, onIt.size)

            // Choosing the third track of Water Music plays Water Music from it.
            val pick = car.resolve(all[2].mediaId)!!
            assertEquals(want.map { it.id }, pick.queue.map { it.id })
            assertEquals(2, pick.at)
            assertEquals("#album/Water%20Music", pick.from)
            assertNull("an id that is not one of ours is nothing", car.children("nowhere", 0, 10, Car.TABS))
        }
        peer.peer.close()
    }

    /**
     * Every picture a car is given is a `content://` URI the provider answers: an album with a
     * cover is that cover, cropped square, the size the car asked for; one with none is the derived
     * square, in the name's tint — never an empty tile. The playing track's is one too, whether or
     * not it has a cover: a car draws it beside a map from the URI alone, and an `http` one it will
     * not load.
     *
     * Falsified by the provider ignoring the size asked for: a 96-pixel ask came back 120 across;
     * and by the playing track carrying its cover's `http` URL, which Android Auto's card left blank.
     */
    @Test
    fun aCoverIsAContentUriTheProviderAnswers() {
        val context = paparazzi.context
        val url = "$server/media/covers/water.png"
        // The cover's original, as the screens' cache keeps it: 200 by 120, red, with a blue
        // band down each side that a square crop of its middle leaves out.
        val dir = Covers.folder(context)
        dir.mkdirs()
        val file = File(dir, "%08x".format(Art.hash(url)))
        val red = android.graphics.Color.rgb(200, 30, 30)
        val original = android.graphics.Bitmap.createBitmap(200, 120, android.graphics.Bitmap.Config.ARGB_8888)
        original.eraseColor(red)
        android.graphics.Canvas(original).apply {
            val blue = android.graphics.Paint().apply { color = android.graphics.Color.BLUE }
            drawRect(0f, 0f, 30f, 120f, blue)
            drawRect(170f, 0f, 200f, 120f, blue)
        }
        file.outputStream().use { original.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
        try {
            val uri = CarArt.uri(url, "Water Music", 96)
            assertEquals("content", uri.scheme)
            assertEquals(CarArt.AUTHORITY, uri.authority)
            val provider = CoverProvider()
            val info = android.content.pm.ProviderInfo().apply { authority = CarArt.AUTHORITY }
            provider.attachInfo(context, info)
            val opened = CarArt.file(context, uri)!!
            val cover = BitmapFactory.decodeFile(opened.path)
            assertEquals("the size asked for", 96, cover.width)
            assertEquals(96, cover.height)
            assertEquals("the cover's own colour, in the middle", red, cover.getPixel(48, 48))
            assertEquals("cropped square: no band at its edge", red, cover.getPixel(2, 48))

            // No cover: the derived square, in the tint the name picks.
            val square = BitmapFactory.decodeFile(CarArt.file(context, CarArt.uri(null, "Messiah", 64))!!.path)
            assertEquals(64, square.width)
            val tint = Art.pick(Palette.DarkArt, "Messiah").first
            val corner = square.getPixel(1, 1)
            assertEquals("the name's tint, at full strength in its corner", tint.red, android.graphics.Color.red(corner) / 255f, 0.02f)
            assertNull("a URI that is not one of ours", CarArt.file(context, android.net.Uri.parse("content://elsewhere/1/2/3")))

            // And what the car is given for an album is such a URI.
            val (car, peer) = car()
            val water = runBlocking { car.children(Car.ALBUMS, 0, 1000, Car.TABS)!! }.first { it.mediaMetadata.title == "Water Music" }
            assertEquals(CarArt.AUTHORITY, water.mediaMetadata.artworkUri!!.authority)
            assertEquals("Water Music", CarArt.parse(water.mediaMetadata.artworkUri!!)!!.seed)
            peer.peer.close()

            // And the playing track's: its cover behind the provider, never the server's URL.
            val playing = Items.of(queuedTrack("Alla Hornpipe", 1), server, "covers/water.png", "#album/Water%20Music").mediaMetadata.artworkUri!!
            assertEquals("content", playing.scheme)
            val named = CarArt.parse(playing)!!
            assertEquals("$server/media/covers/water.png", named.url)
            assertEquals("Water Music", named.seed)
            assertEquals(CarArt.PLAYING, named.px)
            val bare = CarArt.parse(Items.of(queuedTrack("Alla Hornpipe", 1), server, "", "#library").mediaMetadata.artworkUri!!)!!
            assertNull("no cover: the derived square", bare.url)
            assertEquals("Water Music", bare.seed)
        } finally {
            file.delete()
            File(context.cacheDir, "car-art").deleteRecursively()
        }
    }

    /**
     * Search is the phone's own, grouped: albums, artists, songs. "Play Water Music" plays the album
     * in its order; "play Antonio Vivaldi" shuffles every track of his; a song's title plays that
     * song among the songs the domain's `search` found; the assistant's guess at what was asked
     * (`EXTRA_MEDIA_FOCUS`) is taken when it gives one; nothing at all carries on.
     *
     * Falsified by an artist played in its order rather than shuffled: "Vivaldi, shuffled" was the
     * artist's own order. And by asking the songs `search` finds before a name asked for exactly:
     * "Vivaldi, shuffled" again — the search's songs, in its order, where the artist was named.
     */
    @Test
    fun searchAndVoicePlayWhatWasAskedFor() {
        val (car, peer) = car(Random(11))
        runBlocking {
            val found = car.search("water")
            val groups = found.map { it.mediaMetadata.extras?.getString(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_GROUP_TITLE) }
            assertEquals("Albums", groups.first())
            assertTrue(found.first().mediaMetadata.title == "Water Music")

            val album = car.voice("Water Music", null)!!
            assertEquals("play <album>: that album, in its order", tracks { Phone.album(peer, "Water Music") }.map { it.id }, album.queue.map { it.id })
            assertEquals(0, album.at)
            assertEquals("#album/Water%20Music", album.from)

            val vivaldi = tracks { Phone.artist(peer, "Antonio Vivaldi") }
            val artist = car.voice("antonio vivaldi", null)!!
            assertEquals("every track of the artist's", vivaldi.map { it.id }.toSet(), artist.queue.map { it.id }.toSet())
            assertEquals(vivaldi.size, artist.queue.size)
            assertNotEquals("Vivaldi, shuffled", vivaldi.map { it.id }, artist.queue.map { it.id })

            val song = car.voice("Alla Hornpipe", null)!!
            assertEquals("Alla Hornpipe", song.queue[song.at].title)

            val focus = Bundle().apply {
                putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Albums.ENTRY_CONTENT_TYPE)
                putString(MediaStore.EXTRA_MEDIA_ALBUM, "Messiah")
            }
            assertEquals("the assistant's guess: the album", "Messiah", car.voice("messiah by handel", focus)!!.queue.first().album)
            assertNull("nothing matches: nothing", car.voice("zzzz nothing like it", null))
            assertNotNull("nothing asked: carrying on", car.voice("", null))
        }
        peer.peer.close()
    }

    /**
     * The car's rule: with the session playing on the laptop, a track chosen in the car starts
     * the queue and hands the sound to this phone; chosen anywhere else — the lock screen, a watch
     * — it starts the queue where the sound was. Play from the car, the sound elsewhere, brings it
     * here; pause from the car does not.
     *
     * Falsified twice, by the session's player not asking who was asking: for a choice, "and
     * handed to this phone" — it played on the laptop; for play, "play in the car brings the sound
     * here" — it played on the laptop.
     */
    @Test
    fun aChoiceInTheCarPlaysOnThisPhone() {
        fun rig(car: Boolean): Triple<Hall, RemotePlayer, Listen> {
            val hall = Hall()
            val phonePeer = hall.device("phone", true)
            val laptopPeer = hall.device("laptop", true, "computer")
            val phone = Listen(ArkSession.inline(phonePeer, server, testLogin("alice", "phone")), "Phone").also { hall.carry() }
            val laptop = Presser("laptop")
            assertNull(laptop.press(laptopPeer, Press.Start(listOf(queuedTrack("a", 1), queuedTrack("b", 2)), 0, 0, "")))
            hall.carry()
            phone.poke()
            assertTrue("playing on the laptop", hall.state().outputs("laptop") && hall.state().sounding)
            val player = RemotePlayer(Looper.myLooper() ?: Looper.getMainLooper())
            player.listen = phone
            player.fromCar = { car }
            player.show(phone.heard.value)
            return Triple(hall, player, phone)
        }
        val chosen = listOf(Items.of(queuedTrack("c", 3), server, "", "#library"), Items.of(queuedTrack("d", 4), server, "", "#library"))

        val (inCar, carPlayer, _) = rig(car = true)
        carPlayer.setMediaItems(chosen, 1, 0)
        inCar.carry()
        assertEquals("the car's choice is started", listOf("c", "d"), inCar.state().active!!.queue.map { it.title })
        assertEquals(1L, inCar.state().active!!.at)
        val out = inCar.state().active!!.output
        assertTrue("and handed to this phone: $out", out is Output.Device && out.id == "phone")

        val (elsewhere, player, _) = rig(car = false)
        player.setMediaItems(chosen, 1, 0)
        elsewhere.carry()
        assertEquals(listOf("c", "d"), elsewhere.state().active!!.queue.map { it.title })
        assertTrue("from anywhere else, where the sound was", elsewhere.state().outputs("laptop"))

        // Pause from the car leaves the sound where it is; play from the car brings it here.
        val (hall, p, listen) = rig(car = true)
        p.pause()
        hall.carry()
        assertTrue("a pause in the car is a pause", hall.state().outputs("laptop") && !hall.state().active!!.clock.playing)
        p.show(listen.heard.value)
        p.play()
        hall.carry()
        val now = hall.state().active!!
        assertTrue("play in the car brings the sound here: ${now.output}", (now.output as? Output.Device)?.id == "phone" && now.clock.playing)
    }
}
