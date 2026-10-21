// Every screen, in every state worth looking at, in light and in dark, at
// a phone's size: what `packages.harken-android-screens` is made of.
//
// Paparazzi renders each on the JVM through layoutlib, the renderer
// Android Studio's previews use, so nothing here needs a phone or an
// emulator. Each screen is drawn the way the app draws it — inside
// `Chrome` where the app has the tab bar and the mini player, from the
// demo library (`Demo`) — with nothing behind it: no peer, no network,
// so every cover is the derived square. Two things are framed rather than
// raised, because a dialog and a bottom sheet are windows of their own and
// layoutlib draws one window: the sheets sit in `Sheet`, the new-playlist
// card over a scrim, each drawing the same composable the app raises.
package dev.harken.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import dev.arkdb.ffi.Id
import dev.arkdb.ffi.Status
import dev.harken.android.onboarding.AppIdentity
import dev.harken.android.onboarding.Opening
import dev.harken.android.onboarding.Permission
import dev.harken.android.onboarding.PermissionStep
import dev.harken.android.onboarding.ServerStep
import dev.harken.android.onboarding.SignInStep
import dev.harken.android.onboarding.Step
import dev.harken.android.ui.Space
import dev.harken.android.gen.Active
import dev.harken.android.gen.Clock
import dev.harken.android.gen.Output
import dev.harken.android.gen.Present
import dev.harken.android.gen.Track as Queued
import org.junit.Rule
import org.junit.Test

class Screens {
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true)

    // One screen, light then dark.
    private fun shot(name: String, content: @Composable () -> Unit) {
        for (dark in listOf(false, true)) {
            paparazzi.snapshot(if (dark) "$name-dark" else "$name-light") {
                HarkenTheme(dark) {
                    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) { content() }
                }
            }
        }
    }

    @Composable
    private fun identity() =
        AppIdentity(
            name = "Harken",
            scheme = "harken",
            mark = painterResource(R.drawable.mark),
            light = Palette.LightScheme,
            dark = Palette.DarkScheme,
            typography = HarkenType,
            device = declared(),
        )

    private val server = "https://music.example.com"
    private val login = testLogin("ana", "s", name = "Ana Lima", email = "ana@example.com")
    private val hornpipe = Demo.tracks[0]
    private val player =
        PlayerState(
            now = Demo.playing(hornpipe),
            playing = true,
            durationMs = hornpipe.durationMs,
            upNext = Demo.tracks.subList(1, 9).map(Demo::playing),
        )
    private val at = still(82_000)
    private val last = LastPlayed(Demo.playing(Demo.tracks[17]), Page.ForAlbum("The Four Seasons"))

    @Composable
    private fun tab(t: Tab, state: PlayerState = player, page: @Composable () -> Unit) = Chrome(t, state, Still, onTab = {}, position = at) { page() }

    // The onboarding ---------------------------------------------------------

    @Test
    fun a1_server() {
        shot("empty") { ServerStep(identity(), Step.Server(), {}, {}) }
        shot("checking") { ServerStep(identity(), Step.Server("music.example.com", checking = true), {}, {}) }
        shot("unreachable") {
            ServerStep(
                identity(),
                Step.Server("192.168.1.20:8787", error = "Couldn't reach 192.168.1.20:8787 over https or http: nothing answered there."),
                {},
                {},
            )
        }
        shot("other-app") {
            ServerStep(
                identity(),
                Step.Server(
                    "apps.example.com",
                    error = "apps.example.com is an Ark server, but it serves an app this Harken doesn't know — another app, or a newer Harken than this one.",
                ),
                {},
                {},
            )
        }
    }

    @Test
    fun a2_sign_in() {
        shot("idle") { SignInStep(identity(), Step.SignIn(server), {}, {}) }
        shot("browser") { SignInStep(identity(), Step.SignIn(server, Step.Phase.Browser), {}, {}) }
        shot("exchanging") { SignInStep(identity(), Step.SignIn(server, Step.Phase.Exchanging), {}, {}) }
        shot("denied") {
            SignInStep(
                identity(),
                Step.SignIn(
                    server,
                    error = "The server signed you out (the session was revoked). Sign in again — what's on this phone is kept, and syncs once you do.",
                ),
                {},
                {},
            )
        }
    }

    @Test
    fun a3_permission() {
        shot("notifications") { PermissionStep(identity(), Step.Ask(server, login, Permission.Notifications(WHY_NOTIFY)), {}, {}) }
    }

    // Signed in just now, the server's snapshot on its way, after the page's beat: the one
    // place the onboarding says it is opening, and never on a remembered launch.
    @Test
    fun a4_opening() {
        shot("first-sign-in") { Opening(identity(), server, delayMs = 0) }
    }

    // The tabs ---------------------------------------------------------------

    @Test
    fun b1_home() {
        shot("full") {
            tab(Tab.Home) { HomeScreen(HomeData(true, last, Demo.tracks, Demo.playlists, Demo.albums, null), Demo::albumOf, Idle) }
        }
        shot("offline") {
            tab(Tab.Home) {
                HomeScreen(HomeData(true, last, Demo.tracks, Demo.playlists, Demo.albums, "music.example.com"), Demo::albumOf, Idle)
            }
        }
        shot("empty") {
            tab(Tab.Home, PlayerState()) { HomeScreen(HomeData(true, null, emptyList(), emptyList(), emptyList(), null), Demo::albumOf, Idle) }
        }
        // The library not here yet — the server's snapshot on its way — as it reads after its
        // beat. A remembered launch with the library on the phone never draws this: the splash
        // is up until Home has its lists.
        shot("loading") {
            tab(Tab.Home, PlayerState()) {
                HomeScreen(HomeData(true, null, emptyList(), emptyList(), emptyList(), null, fetching = "music.example.com"), Demo::albumOf, Idle, placeholderDelayMs = 0)
            }
        }
    }

    @Test
    fun b2_search() {
        shot("recent") {
            tab(Tab.Search) {
                SearchScreen("", null, listOf("Vivaldi", "hornpipe", "Goldberg"), {}, {}, {}, Demo::albumOf, null, null, Idle)
            }
        }
        shot("nothing-yet") { tab(Tab.Search) { SearchScreen("", null, emptyList(), {}, {}, {}, Demo::albumOf, null, null, Idle) } }
        val needle = "alle"
        val songs = Demo.tracks.filter { it.title.contains(needle, true) || it.creator.contains(needle, true) }
        shot("typing") {
            tab(Tab.Search) {
                SearchScreen(needle, Results.of(needle, songs, Demo.albums, Demo.artists), emptyList(), {}, {}, {}, Demo::albumOf, hornpipe.id.hex, null, Idle)
            }
        }
        shot("results") {
            tab(Tab.Search) {
                SearchScreen("handel", Results.of("handel", Demo.artist("George Frideric Handel"), Demo.albums, Demo.artists), emptyList(), {}, {}, {}, Demo::albumOf, hornpipe.id.hex, null, Idle)
            }
        }
        shot("no-results") {
            tab(Tab.Search) { SearchScreen("zither", Results.of("zither", emptyList(), Demo.albums, Demo.artists), emptyList(), {}, {}, {}, Demo::albumOf, null, null, Idle) }
        }
    }

    private val data = LibraryData(true, Demo.playlists, Demo.albums, Demo.artists, Demo.tracks, null)

    @Test
    fun b3_library() {
        for (t in LibraryTab.entries) {
            shot(t.name.lowercase()) { tab(Tab.Library) { LibraryScreen(t, {}, data, Demo::albumOf, hornpipe.id.hex, Idle) } }
        }
        shot("playlists-none") {
            tab(Tab.Library, PlayerState()) { LibraryScreen(LibraryTab.Playlists, {}, data.copy(playlists = emptyList()), Demo::albumOf, null, Idle) }
        }
    }

    // The pages --------------------------------------------------------------

    @Test
    fun c1_album() {
        val name = "Water Music"
        shot("water-music") {
            tab(Tab.Library) { AlbumPage(name, Demo.albums.first { it.name == name }, Demo.album(name), Demo.details, hornpipe.id.hex, Idle) }
        }
        val seasons = "The Four Seasons"
        shot("four-seasons") { tab(Tab.Library) { AlbumPage(seasons, Demo.albums.first { it.name == seasons }, Demo.album(seasons), Demo.details, null, Idle) } }
    }

    @Test
    fun c2_artist() {
        val name = "George Frideric Handel"
        val tracks = Demo.artist(name)
        val albums = tracks.mapNotNull { Demo.details[it.id]?.album }.distinct().mapNotNull { a -> Demo.albums.firstOrNull { it.name == a } }
        shot("handel") { tab(Tab.Library) { ArtistPage(name, Demo.artists.first { it.name == name }, tracks, albums, Demo::albumOf, hornpipe.id.hex, Idle) } }
    }

    @Test
    fun c3_playlist() {
        val p = Demo.playlists[1]
        shot("sunday") { tab(Tab.Library) { PlaylistPage(p.id, p, Demo.sunday, Demo::albumOf, null, Idle) } }
        shot("empty") { tab(Tab.Library, PlayerState()) { PlaylistPage(p.id, Demo.playlists[2], emptyList(), Demo::albumOf, null, Idle) } }
        shot("gone") { tab(Tab.Library, PlayerState()) { PlaylistPage(Id("ff".repeat(16)), null, emptyList(), Demo::albumOf, null, Idle) } }
    }

    // The player -------------------------------------------------------------

    @Test
    fun d1_player() {
        shot("mini") {
            Column(Modifier.fillMaxSize()) {
                Spacer(Modifier.weight(1f))
                MiniPlayer(player.copy(playing = false), Still, at)
                Spacer(Modifier.height(Space.xl))
            }
        }
        shot("full") { FullPlayer(player, Still, at) }
        shot("full-shuffle-repeat-one") { FullPlayer(player.copy(playing = false, shuffle = true, repeat = Repeat.One), Still, still(201_000)) }
        shot("queue") { Sheet { QueueSheet(player.copy(shuffle = true), Still) } }
    }

    // The player on its way up from the bar, held at each quarter: the card between the bar's
    // bounds and the screen's, the bar's face gone by a fifth of the way, the full face in over the
    // last two fifths, the art between the two, the tabs sliding away and the page dimming.
    @Test
    fun d2_player_rising() {
        for (p in listOf(0f, 0.25f, 0.5f, 0.75f, 1f)) {
            shot("p" + (p * 100).toInt()) {
                val density = LocalDensity.current
                Chrome(Tab.Home, player, Still, onTab = {}, position = at, sheet = remember { PlayerSheet.held(p, density) }) {
                    HomeScreen(HomeData(true, last, Demo.tracks, Demo.playlists, Demo.albums, null), Demo::albumOf, Idle)
                }
            }
        }
    }

    // What rises over a page -------------------------------------------------

    @Test
    fun e1_sheets() {
        shot("track-menu") { Sheet { TrackMenu(Demo.sunday[1], Demo.albumOf(Demo.sunday[1]), Page.ForPlaylist(Demo.playlists[1].id), object : MenuActions {}) } }
        shot("add-to-playlist") {
            Sheet { AddToPlaylist(hornpipe, Demo.playlists, setOf(Demo.playlists[1].id), {}, {}) }
        }
        shot("new-playlist") {
            Scrimmed { NewPlaylistCard("Sunday morning", null, adding = true, {}, {}, {}) }
        }
        shot("new-playlist-refused") {
            Scrimmed { NewPlaylistCard("   ", "a playlist needs a name", adding = false, {}, {}, {}) }
        }
    }

    // Settings, and what is behind it ----------------------------------------

    @Test
    fun f1_settings() {
        val s = SettingsData("Ana Lima", "ana", "ana@example.com", server, linked = true, version = "0.1 (1)", diagnostics = false)
        shot("linked") { SettingsScreen(s, {}, {}, {}, {}) }
        shot("unreachable-unlocked") { SettingsScreen(s.copy(linked = false, diagnostics = true), {}, {}, {}, {}) }
    }

    @Test
    fun f2_diagnostics() {
        val status =
            Status(
                link = "online",
                linked = true,
                epoch = 3u,
                cursor = 1287,
                pending = 2u,
                joining = false,
                user = "ana",
                session = "s-7f3a",
                alone = false,
                failure = null,
                denied = null,
            )
        val d =
            Diagnostics(
                status,
                listOf("add Alla Hornpipe to Sunday morning" to "not synced yet", "make the playlist Sunday morning" to "saved"),
            )
        shot("numbers") { DiagnosticsScreen(d, {}) }
        shot("listening") { DiagnosticsScreen(d, {}, heard(session(Output.Device("s-web", false), playing = false), scrub = mapOf("s-web" to 201_000L))) }
    }

    // The listening session --------------------------------------------------
    //
    // The account's devices: this phone, a desktop with no speakers, a browser tab and a speaker
    // in the house; and a session of the Water Music, 82 s into the Alla Hornpipe.

    private val devices =
        listOf(
                Present(true, false, null, null, "s-phone", "phone", false, "Pixel 7", "ana", 100),
                Present(false, false, null, null, "s-desktop", "computer", false, "Desktop", "ana", 100),
                Present(true, false, null, null, "s-web", "computer", false, "Browser", "ana", 100),
                Present(true, false, null, null, "media_player.kitchen", "speaker", false, "Kitchen", "ana", 100),
            )
            .associateBy { it.id }

    private fun session(output: Output, playing: Boolean): Active {
        val queue = Demo.album("Water Music").take(8).map { t -> Queued("Water Music", t.creator, t.durationMs, t.file, t.id, t.title) }
        val clock = if (playing) Clock.Playing(82_000, System.currentTimeMillis()) else Clock.Paused(82_000)
        return Active(0, clock, "#album/Water%20Music", output, queue)
    }

    private fun heard(s: Active?, scrub: Map<String, Long> = emptyMap()): Heard =
        Heard(me = "s-phone", joined = true, listening = Listening(devices, scrub, s), linked = true, epoch = 3)

    @Test
    fun g1_listening() {
        val desktop = player.copy(on = Where("Browser", "computer", here = false))
        // The sound on the browser tab: the phone a remote for it, its own speaker silent.
        shot("full-elsewhere") { FullPlayer(desktop, Still, at) }
        shot("mini-elsewhere") {
            Column(Modifier.fillMaxSize()) {
                Spacer(Modifier.weight(1f))
                MiniPlayer(desktop, Still, at)
                Spacer(Modifier.height(Space.xl))
            }
        }
        // The sound here, and the chip lit.
        shot("full-here") { FullPlayer(player.copy(on = Where("Pixel 7", "phone", here = true)), Still, at) }
        // Handed to the speaker, which has not said it has started.
        shot("full-connecting") { FullPlayer(player.copy(on = Where("Kitchen", "speaker", here = false, moving = true)), Still, at) }
        // The browser's scrubber held at 3:21 while the track is at 1:22: the phone's shows 3:21.
        shot("scrub-from-elsewhere") { FullPlayer(desktop, Still, at, scrubbing = nobody(201_000)) }
        // The picker while the browser plays; while nothing does, this phone ticked; and with the
        // laptop the sound belongs to gone.
        val playing = heard(session(Output.Device("s-web", false), playing = true))
        shot("picker") {
            val rows = Picker.rows(playing.listening!!, "s-phone")
            Sheet { DevicesSheet(rows, Picker.cursor(rows), object : PickerActions {}) }
        }
        shot("picker-nothing-playing") {
            val rows = Picker.rows(heard(null).listening!!, "s-phone")
            Sheet { DevicesSheet(rows, Picker.cursor(rows), object : PickerActions {}) }
        }
        shot("picker-not-answering") {
            val rows = Picker.rows(heard(session(Output.Away("s-laptop", "Laptop"), playing = false)).listening!!, "s-phone")
            Sheet { DevicesSheet(rows, Picker.cursor(rows), object : PickerActions {}) }
        }
        // Home, while the browser is playing: last played is the session's track, and where.
        shot("home-last-played") {
            val live = lastPlayed(playing, null, Demo.playlists, { "" }, server, "Pixel 7")
            tab(Tab.Home, desktop) { HomeScreen(HomeData(true, live, Demo.tracks, Demo.playlists, Demo.albums, null), Demo::albumOf, Idle) }
        }
    }

    // Volume, and speakers joined ------------------------------------------
    //
    // The house: this phone, a desktop with no speakers, and three speakers of one family — the
    // kitchen playing, the bedroom joined to it, and the study, which could be.

    private val house =
        listOf(
                Present(true, true, null, null, "s-phone", "phone", false, "Pixel 7", "ana", 60),
                Present(false, false, null, null, "s-desktop", "computer", false, "Desktop", "ana", 100),
                Present(true, true, "sonos", null, "media_player.kitchen", "speaker", false, "Kitchen", "ana", 40),
                Present(true, true, "sonos", "media_player.kitchen", "media_player.bedroom", "speaker", false, "Bedroom", "ana", 60),
                Present(true, true, "sonos", null, "media_player.study", "speaker", true, "Study", "ana", 20),
            )
            .associateBy { it.id }

    @Test
    fun g2_volume() {
        // The sound on the kitchen: the slider is the kitchen's, as the phone's buttons are.
        shot("full-speaker") {
            FullPlayer(player.copy(on = Where("Kitchen", "speaker", here = false), volume = Volume(Level.Device("media_player.kitchen"), 40, false, remote = true)), Still, at)
        }
        // The kitchen leading the bedroom: the group's slider, at the mean of the two.
        shot("full-group") { FullPlayer(player.copy(on = Where("Kitchen + 1", "speaker", here = false), volume = Volume(Level.Group, 50, false, remote = true)), Still, at) }
        // …muted, every member of it.
        shot("full-group-muted") { FullPlayer(player.copy(on = Where("Kitchen + 1", "speaker", here = false), volume = Volume(Level.Group, 50, true, remote = true)), Still, at) }
        // The picker: the group first, every member ticked, a slider and a mute on every row with a
        // volume, the bedroom's `−` and the study's `+` — and the study muted.
        shot("picker-group") {
            val s = Listening(house, emptyMap(), session(Output.Device("media_player.kitchen", false), playing = true))
            val rows = Picker.rows(s, "s-phone")
            Sheet { DevicesSheet(rows, Picker.cursor(rows), object : PickerActions {}) }
        }
        // Nothing grouped: each speaker its own slider, and the two that could join the kitchen.
        shot("picker-joinable") {
            val alone = house.mapValues { (_, d) -> d.copy(follows = null, muted = false) }
            val s = Listening(alone, emptyMap(), session(Output.Device("media_player.kitchen", false), playing = true))
            val rows = Picker.rows(s, "s-phone")
            Sheet { DevicesSheet(rows, Picker.cursor(rows), object : PickerActions {}) }
        }
    }
}

/** A bottom sheet as the app raises it: a scrim over the page, the sheet at its foot. */
@Composable
private fun Sheet(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.32f))) {
        Spacer(Modifier.weight(1f))
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column {
                Box(Modifier.fillMaxWidth().padding(vertical = 22.dp), contentAlignment = Alignment.Center) {
                    Box(Modifier.width(32.dp).height(4.dp).background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f), RoundedCornerShape(2.dp)))
                }
                content()
            }
        }
    }
}

/** A dialog's card, centred over a scrim. */
@Composable
private fun Scrimmed(content: @Composable () -> Unit) {
    Box(
        Modifier.fillMaxSize().background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.32f)).padding(horizontal = 40.dp),
        contentAlignment = Alignment.Center,
    ) {
        content()
    }
}
