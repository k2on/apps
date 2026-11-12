// What every Ark app on Android is underneath its screens, written once: the process's app host
// (ark-ffi's `ArkHost`, which runs the first run every app shares, opens the session over the
// process's one runtime and watches its link) owned by the app's `Application` for as long as the
// process lives, the activity that shows it, the browser sign-in in a Custom Tab, the app's links,
// the secure store handed to the host, and — only for an app whose app.toml declares
// `[background]` — the notifications' hookup to the process.
//
// An app's part is generated (ui/prebuild.py, from app.toml), and tiny:
//
//     class MoneyApplication : ArkApplication() {
//         lateinit var money: MoneyHost
//         override fun host(launch: Launch): ArkHost =
//             MoneyHost(launch, PhoneSecureStore(this)).also { money = it }.host()
//     }
//     class MainActivity : ArkActivity() {
//         @Composable override fun Content(app: ArkApp) = MoneyRoot(app, (application as MoneyApplication).money)
//     }
//
// It replaces each app's own `ViewModel` holding the sign-in machine and a book over its peer
// (money's `Start.kt` and `Book.kt`): the state is Rust's, and lives as long as the process.
package dev.arkui.app

import android.app.Application
import android.content.Intent
import android.os.Handler
import android.os.Looper
import dev.arkdb.auth.Step
import dev.arkdb.ffi.ArkHost
import dev.arkdb.ffi.HostShell
import dev.arkdb.ffi.Launch
import dev.arkdb.ffi.Refusal
import dev.arkdb.ffi.ShotsTook
import dev.arkui.DeviceLinkShown
import dev.arkui.Ready
import dev.arkui.Shots
import dev.arkui.ShotsWalk
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.ark_notifications.NotificationListener

/**
 * What an app that declares `[background]` in app.toml hands the library: its notifications'
 * hookup (ark-notifications' `ArkNotices`). Every app that declares none has none, and nothing of
 * it runs. The periodic job, the boot receiver and the UnifiedPush receiver are the modules' own,
 * written into the manifest by ui/prebuild.py from the same `[background]`.
 */
public class BackgroundHooks(
    /** Where the notifications tell what they hear: the process's (`ArkProcess.notices`). */
    public val listen: (NotificationListener?) -> Unit = {},
    /** An intent the activity was started or resumed with: a tap on a notice, said. */
    public val opened: (Intent?) -> Unit = {},
)

/** The app's `Application`: the process's app host, made at the first launch and kept. */
public abstract class ArkApplication : Application() {
    /** The app's host as `launch` says: its bridge's (`MoneyHost(launch, store).host()`). */
    public abstract fun host(launch: Launch): ArkHost

    /** What the app declares of the background; none unless its app.toml has `[background]`. */
    public open val background: BackgroundHooks? = null

    @Volatile private var app: ArkApp? = null

    override fun onCreate() {
        super.onCreate()
        // Only an app that declares `[background]`: the notifications' events into the process's
        // one runtime (a push or a notice's button a wake, an endpoint the session's push row).
        background?.listen(dev.arkdb.ffi.ArkProcess.shared().notices())
    }

    /** The app, made at its first launch (`intent`: the launcher's, a link's) and kept. */
    public fun launch(intent: Intent?): ArkApp =
        app ?: synchronized(this) {
            app ?: ArkApp(host(launchOf(intent))).also {
                app = it
                it.start()
            }
        }

    /** How the launch reads: the app's directory, its version, and its `ark.args`. */
    private fun launchOf(intent: Intent?): Launch {
        val args = intent?.getStringArrayExtra("ark.args")?.toList().orEmpty()
        val demo = args.indexOf("-demo").takeIf { it >= 0 }?.let { args.getOrNull(it + 1) ?: "" }
        val version =
            runCatching {
                    val i = packageManager.getPackageInfo(packageName, 0)
                    @Suppress("DEPRECATION") "${i.versionName} (${i.versionCode})"
                }
                .getOrDefault("")
        return Launch(
            root = filesDir.path,
            version = version,
            demo = demo != null,
            demoDay = demo?.takeIf { it.length == 10 },
            shots = Shots.on,
            // A screenshot walk's launch: a plan in the app's own directory, on an emulator.
            walk = ShotsWalk.planned(this) != null,
            // No code is scanned here yet: ark-camera has no QR decoder on Android.
            scan = false,
            args = args,
        )
    }
}

/**
 * The app host as Compose reads it: where the first run stands, whether the app itself shows,
 * the sentence to say once, the sign-in to open, and a count that grows whenever anything the app
 * shows moved — the app's screens read again on it ([ArkScreen.reload]).
 */
public class ArkApp internal constructor(public val host: ArkHost) {
    private val main = Handler(Looper.getMainLooper())

    private val _step = MutableStateFlow(host.step())
    public val step: StateFlow<Step> = _step.asStateFlow()

    private val _shows = MutableStateFlow(false)
    /** The app itself shows: a session or the demo, no page of the first run over it. */
    public val showsApp: StateFlow<Boolean> = _shows.asStateFlow()

    private val _moves = MutableStateFlow(0L)
    /** Grows whenever anything the app shows moved. */
    public val moves: StateFlow<Long> = _moves.asStateFlow()

    private val _notices = MutableSharedFlow<String>(extraBufferCapacity = 16)
    /** Sentences to show once each: a refusal, what the bank brought. */
    public val notices: SharedFlow<String> = _notices.asSharedFlow()

    private val _browse = MutableStateFlow<String?>(null)
    /** The sign-in to open in a Custom Tab, once. */
    public val browse: StateFlow<String?> = _browse.asStateFlow()

    private val shell =
        object : HostShell {
            override fun changed() {
                main.post { refresh() }
            }

            override fun browse(url: String) {
                main.post { _browse.value = url }
            }
        }

    /** The version a screenshot's ready signal last saw. */
    private var seen = ULong.MAX_VALUE

    internal fun start() {
        // A screenshot is ready once nothing is on its way and the version has stood still.
        Ready.probe("app") {
            val v = host.version()
            if (v != seen) {
                seen = v
                Ready.changed()
                false
            } else host.idle()
        }
        // A screenshot walk's steps, Rust's: each reaches what it names in the views on the screen
        // and sends the message a tap on it sends (ark_app::shots).
        ShotsWalk.steps =
            object : ShotsWalk.Steps {
                override fun step(json: String): ShotsWalk.Took =
                    when (val t = host.shotsStep(json)) {
                        is ShotsTook.Done -> ShotsWalk.Took.Done
                        is ShotsTook.NotYet -> ShotsWalk.Took.NotYet(t.labels)
                        is ShotsTook.Failed -> ShotsWalk.Took.Failed(t.why)
                    }

                override fun tree(): String = host.shotsTree()
            }
        host.start(shell)
    }

    private fun refresh() {
        _step.value = host.step()
        _shows.value = host.showsApp()
        host.takeNotice()?.let { _notices.tryEmit(it) }
        _moves.value += 1
        Ready.changed()
    }

    /** Whether the first screen has what it draws: what the splash waits for. */
    public fun ready(): Boolean =
        when (step.value) {
            is Step.Running, is Step.Demo -> showsApp.value
            else -> true
        }

    /** A link the app was opened with, or sent. */
    public fun open(url: String): Unit = host.openUrl(url)

    /** The sign-in's browser opened: taken. */
    public fun browsed() {
        _browse.value = null
    }

    /**
     * The activity's window is made: what the launch's arguments open is opened from now (at once,
     * or once the session is). The iPhone and the Mac say so once the app's content has appeared, as
     * a sheet asked for before the Mac's window showed kept it hidden; here the window is there
     * before anything draws.
     */
    public fun shown(): Unit = host.shown()

    /** Back in front after it was away: the link dialled now, a sign-in tried again. */
    public fun foreground(): Unit = host.foreground()

    /** Signed out, or out of the demo. */
    public fun signOut(): Unit = host.signOut()

    /** "Add a Device": a join link another device signs in with, made as this login; or why not. */
    public suspend fun deviceLink(): DeviceLinkShown =
        withContext(Dispatchers.IO) {
            try {
                val d = host.deviceLink()
                DeviceLinkShown(d.link, d.qr.size.toInt(), d.qr.dark, null)
            } catch (x: Refusal) {
                DeviceLinkShown(null, 0, emptyList(), (x as? Refusal.Refused)?.why ?: x.message ?: "no answer")
            }
        }
}
