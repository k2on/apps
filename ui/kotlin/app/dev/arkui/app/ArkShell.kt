// The shell every Ark app on Android shows once it runs, written once (ui/docs/app.md, "The App
// Library"): the app's shell screen's generated composable (its sections — a navigation bar, a rail
// or a drawer by the window's width — each its top bar, search and inspector, ArkUI's) hosting the
// app's other screens' generated composables by name, each over its own runtime; what goes over
// every section (the app's sheets); Settings, pushed over the shell with the way back; Add a Device;
// who is signed in at the drawer's foot and their account sheet; a page the app opens in a Custom
// Tab; what the app says once, as a snackbar; Light or Dark over the device's; and what the app's
// host asks of its shell (`ArkHost.natives()`). An app adds only its names, generated from app.toml's
// `[shell]` (ui/prebuild.py writes `generated/ArkShell.kt`: the holders, which name hosts which
// screen, the root the activity draws). The iPhone's is ArkApp's `ArkShell` (ui/swift).
//
// Navigation is navigation-compose's, so the system's back gesture is predictive: Settings slides and
// fades back toward the shell beneath it on arkui's shared X axis (`Transitions`), a quick
// cross-fade with motion reduced.
package dev.arkui.app

import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LargeTopAppBar
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.os.ConfigurationCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import dev.arkdb.ffi.ShellAsk
import dev.arkui.Accent
import dev.arkui.AccountFace
import dev.arkui.AccountGroup
import dev.arkui.AccountRow
import dev.arkui.AccountSheet
import dev.arkui.AddDeviceDialog
import dev.arkui.Amounts
import dev.arkui.ArkGlyphs
import dev.arkui.ArkTheme
import dev.arkui.LargeTitles
import dev.arkui.LocalAmounts
import dev.arkui.LocalArkTheme
import dev.arkui.LocalHostedScreens
import dev.arkui.LocalTabFrame
import dev.arkui.LocaleNumbers
import dev.arkui.MaterialYou
import dev.arkui.Ready
import dev.arkui.ShellMenu
import dev.arkui.Transitions
import dev.arkui.arkuiScheme
import dev.arkui.expressiveTypography
import dev.arkui.reducedMotion
import dev.arkui.shellShortcuts
import dev.arkui.tracked
import java.util.Locale
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.ark_format.NumberStyle
import uniffi.ark_format.formatAmount
import uniffi.ark_format.formatAmountEditable
import uniffi.ark_format.parseAmount

/** One screen's holder for the session open, made again with each session: what its composable reads. */
public class ArkSlot<H : ArkScreen<*, *>> internal constructor(private val make: () -> H) {
    private val _holder = MutableStateFlow<H?>(null)

    /** The holder, while a session is open. */
    public val holder: StateFlow<H?> = _holder

    internal fun open(open: Boolean) {
        _holder.value = if (open) make() else null
    }

    internal fun reload() {
        _holder.value?.reload()
    }
}

/** Who is using the app and where (ark_app's `Household`, as the app's library has it). */
public class ArkHousehold(
    public val who: String,
    public val picture: String,
    public val detail: String,
    public val server: String?,
    public val linked: Boolean?,
    public val demo: Boolean,
    /** How many things want somebody in Settings; the sheet's row, the face's mark, the row's line under it. */
    public val wanting: Long,
    public val wantingTitle: String,
    public val wantingSaid: String,
    public val wantingWhere: String,
)

/** What the shared shell reads of the app's shell page: each a field app.toml's `[shell]` names. */
public class ArkShellPage<P, M>(
    /** The section showing: the ready signal's route. */
    public val section: (P) -> String,
    /** Dark, light, or the device's (none). */
    public val dark: (P) -> Boolean?,
    public val household: (P) -> ArkHousehold,
    /** A page to open in a Custom Tab while it is set, and the message that says it was (a tab is not followed). */
    public val browse: (P) -> String?,
    public val closed: (String) -> M,
)

/**
 * The shell over an app's library: its shell screen's holder and every other screen's, each made
 * again with each session, and the names it hosts.
 */
public class ArkShell<H : ArkScreen<P, M>, P, M>(
    public val app: ArkApp,
    make: () -> H,
    /** The shell's page as it is now, read straight from its runtime: the theme, before any holder is. */
    internal val read: () -> P,
    /** The hosted name drawn as who is signed in (app.toml's `[shell] account`). */
    internal val account: String,
    internal val page: ArkShellPage<P, M>,
) {
    private val slots = mutableListOf<ArkSlot<*>>()

    /** The shell screen's own holder. */
    public val shell: ArkSlot<H> = slot(make)

    /** A screen's slot, its holder made by `make` with each session. */
    public fun <X : ArkScreen<*, *>> slot(make: () -> X): ArkSlot<X> = ArkSlot(make).also { slots.add(it) }

    /** Every holder made again over the session open (none: let go). */
    internal fun open(open: Boolean) {
        for (s in slots) s.open(open)
    }

    /** Every page read again; where the app is told the ready signal; what the host asks, taken. */
    internal fun moved(): List<ShellAsk> {
        for (s in slots) s.reload()
        shell.holder.value?.let { Ready.at(page.section(it.page.value)) }
        return app.host.natives()
    }
}

/** A screen the shell hosts: its generated composable over its slot's holder, while a session is open. */
@Composable
public fun <X : ArkScreen<*, *>> ArkHosted(slot: ArkSlot<X>, content: @Composable (X) -> Unit) {
    val h by slot.holder.collectAsStateWithLifecycle()
    h?.let { content(it) }
}

/**
 * A name the shell hosts that is no screen's: who is signed in (the account's name: their face,
 * raising the account sheet), else an inspector with nothing chosen.
 */
@Composable
public fun <H : ArkScreen<P, M>, P, M> ArkShellPlace(shell: ArkShell<H, P, M>, name: String) {
    val frame = LocalShellFrame.current
    if (name == shell.account) {
        val holder by shell.shell.holder.collectAsStateWithLifecycle()
        val screen = holder ?: return
        val p by screen.page.collectAsStateWithLifecycle()
        val h = shell.page.household(p)
        AccountFace(h.who, h.picture, onClick = frame.onAccount, attention = if (h.wanting > 0) h.wantingSaid else null)
    } else {
        Text("Nothing Selected", modifier = Modifier.padding(16.dp))
    }
}

/** What a page under the shell can ask of it: the account sheet, and the snackbar. */
private class ShellFrame(val onAccount: () -> Unit, val snackbar: SnackbarHostState)

private val LocalShellFrame = androidx.compose.runtime.staticCompositionLocalOf { ShellFrame({}, SnackbarHostState()) }

/**
 * Every Ark app's root once the activity draws: arkui's theme in its accent over the device's or
 * the shell's Light or Dark, the first run's pages (`welcome`) until the app itself shows, then the
 * app's shell.
 */
@Composable
public fun <H : ArkScreen<P, M>, P, M> ArkShellRoot(
    shell: ArkShell<H, P, M>,
    accent: Accent,
    welcome: Boolean,
    welcomeScreen: @Composable () -> Unit,
    main: @Composable (H) -> Unit,
    settings: @Composable (H) -> Unit,
    over: @Composable () -> Unit,
    hosted: @Composable (String) -> Unit,
    commands: (H) -> List<ShellMenu>,
) {
    val app = shell.app
    // The shell's appearance over the device's (a launch's `-appearance`, Settings' Light or
    // Dark), as on the iPhone; none (System, the default) follows the device.
    val moves by app.moves.collectAsStateWithLifecycle()
    val dark = remember(moves) { shell.page.dark(shell.read()) }
    ArkAppTheme(accent, dark = dark ?: isSystemInDarkTheme()) {
        Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
            val shows by app.showsApp.collectAsStateWithLifecycle()
            if (shows && !welcome) ShellApp(shell, moves, main, settings, over, hosted, commands) else welcomeScreen()
        }
    }
}

@Composable
private fun <H : ArkScreen<P, M>, P, M> ShellApp(
    shell: ArkShell<H, P, M>,
    moves: Long,
    main: @Composable (H) -> Unit,
    settings: @Composable (H) -> Unit,
    over: @Composable () -> Unit,
    hosted: @Composable (String) -> Unit,
    commands: (H) -> List<ShellMenu>,
) {
    val app = shell.app
    val generation = remember(moves) { app.host.generation() }
    // A session closed between a move and this: nothing to draw until the first run shows.
    if (generation == 0uL) return
    remember(generation) { shell.open(true) }
    val holder by shell.shell.holder.collectAsStateWithLifecycle()
    val screen = holder ?: return
    val nav = rememberNavController()
    val snackbar = remember { SnackbarHostState() }
    var account by remember { mutableStateOf(false) }
    var addingDevice by remember { mutableStateOf(false) }
    LaunchedEffect(app) { app.notices.collect { snackbar.showSnackbar(it) } }
    val context = LocalContext.current
    val page by screen.page.collectAsStateWithLifecycle()
    val toSettings = { nav.navigate("settings") { launchSingleTop = true } }
    // A page the app opens (a bank's sign-in), in a Custom Tab the moment its server answers. A
    // Custom Tab is not followed, so it is taken as closed once opened.
    val browse = shell.page.browse(page)
    LaunchedEffect(browse) {
        val url = browse ?: return@LaunchedEffect
        runCatching { CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url)) }
            .onFailure { snackbar.showSnackbar("Couldn't open the page: open it from Settings") }
        screen.send(shell.page.closed(url))
    }
    // Whatever moved, every page read again, and what the app's host asks of this shell carried
    // out: Settings, Add a Device, and a link taken as a launch putting away what this shows of
    // its own.
    LaunchedEffect(moves) {
        for (ask in shell.moved()) {
            when (ask) {
                is ShellAsk.Settings -> toSettings()
                is ShellAsk.AddDevice -> addingDevice = true
                is ShellAsk.Reset -> {
                    Ready.changed()
                    account = false
                    nav.popBackStack("shell", inclusive = false)
                }
                // The app host signs out itself: never handed to a shell.
                is ShellAsk.SignOut -> {}
            }
        }
    }
    val reduced = reducedMotion()
    val frame = remember(snackbar) { ShellFrame({ account = true }, snackbar) }
    CompositionLocalProvider(LocalShellFrame provides frame, LocalHostedScreens provides hosted) {
        NavHost(
            nav,
            startDestination = "shell",
            enterTransition = { Transitions.pushIn(reduced) },
            exitTransition = { Transitions.pushOut(reduced) },
            popEnterTransition = { Transitions.popIn(reduced) },
            popExitTransition = { Transitions.popOut(reduced) },
        ) {
            composable("shell") {
                // Every tab's page in the shell's frame: under the shell's top bar, its pull to
                // refresh, the snackbar.
                CompositionLocalProvider(LocalTabFrame provides { _, onRefresh, inner -> TabFrame(snackbar, onRefresh, inner) }) {
                    Box(Modifier.shellShortcuts(commands(screen))) { main(screen) }
                }
            }
            // Settings, the shell's panes, pushed with the way back.
            composable("settings") {
                SettingsPage(snackbar, onBack = { nav.popBackStack() }) { settings(screen) }
            }
        }
        over()
        if (addingDevice) AddDeviceDialog({ app.deviceLink() }, onDismiss = { addingDevice = false })
        if (account) AccountOf(shell.page.household(page), onSettings = toSettings, onSignOut = { app.signOut() }, onDismiss = { account = false })
    }
}

/** A tab's page under the shell's top bar (which has its title and actions): pull to refresh, the snackbar. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TabFrame(snackbar: SnackbarHostState, onRefresh: (() -> Unit)?, content: @Composable () -> Unit) {
    // The bar has taken the status bar's inset: the page takes none again.
    Scaffold(contentWindowInsets = WindowInsets(0), snackbarHost = { SnackbarHost(snackbar) }) { pad ->
        val fill = Modifier.fillMaxSize().padding(top = pad.calculateTopPadding())
        if (onRefresh != null) PullToRefreshBox(isRefreshing = false, onRefresh = onRefresh, modifier = fill) { Box { content() } }
        else Box(fill) { content() }
    }
}

/** Settings pushed over the shell: a large top app bar that collapses as it scrolls, the way back. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsPage(snackbar: SnackbarHostState, onBack: () -> Unit, content: @Composable () -> Unit) {
    val bar = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(bar.nestedScrollConnection),
        contentWindowInsets = WindowInsets.systemBars,
        topBar = {
            // The expanded title at the iPhone's large title's weight; collapsed, Material's own.
            LargeTitles {
                LargeTopAppBar(
                    title = { Text("Settings", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") } },
                    scrollBehavior = bar,
                    colors = TopAppBarDefaults.largeTopAppBarColors(scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer),
                )
            }
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { pad ->
        // Nothing of its own in General: only the desktop adds there.
        CompositionLocalProvider(LocalHostedScreens provides NothingHosted) {
            Box(Modifier.fillMaxSize().padding(top = pad.calculateTopPadding())) { content() }
        }
    }
}

/** What Settings hosts: nothing of the app's (only the desktop adds to its General). */
private val NothingHosted: @Composable (String) -> Unit = {}

/**
 * The account sheet: who is using the app, the server and whether it answers, what wants somebody
 * in Settings, Settings, and the way out — arkui's.
 */
@Composable
private fun AccountOf(h: ArkHousehold, onSettings: () -> Unit, onSignOut: () -> Unit, onDismiss: () -> Unit) {
    AccountSheet(
        name = h.who,
        picture = h.picture,
        detail = h.detail,
        server = h.server,
        linked = h.linked ?: false,
        onSettings = onSettings,
        onSignOut = onSignOut,
        onDismiss = onDismiss,
        signOut = if (h.demo) "Leave the Demo" else "Sign Out",
    ) { leave ->
        if (h.wanting > 0) {
            AccountGroup {
                AccountRow(
                    ArkGlyphs.Attention,
                    h.wantingTitle,
                    onClick = { leave(onSettings) },
                    under = h.wantingWhere,
                    tint = MaterialTheme.colorScheme.error,
                    chevron = true,
                )
            }
        }
    }
}

/**
 * Every Ark app's look: Material You (arkui's `MaterialYou`) in arkui's theme (ui/theme.toml) — the
 * neutrals every app shares and the app's accent (app.toml's), the shared caution in the tertiary
 * slot — Material's expressive type scale tracked for each size as the iPhone's is, and money written
 * by the library (ark_format) in the phone's locale. The wallpaper's colours are not taken: an app
 * is its accent on every phone, as it is on the iPhone and the desktop.
 */
@Composable
public fun ArkAppTheme(accent: Accent, dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val locale = ConfigurationCompat.getLocales(LocalConfiguration.current)[0] ?: Locale.getDefault()
    val amounts = remember(locale) { LibraryAmounts(LocaleNumbers.of(locale)) }
    val schemes = remember(accent) { arkuiScheme(accent, dark = false) to arkuiScheme(accent, dark = true) }
    MaterialYou(light = schemes.first, dark = schemes.second, darkTheme = dark, dynamic = false, typography = ArkType) {
        CompositionLocalProvider(LocalAmounts provides amounts, LocalArkTheme provides ArkTheme(accent.art(dark)), content = content)
    }
}

/** arkui's expressive type scale, tracked for each size. */
private val ArkType = expressiveTypography().tracked()

/** Money as the library writes it (ark_format's, every client's): in [numbers], what java.text says of the phone's locale. */
public class LibraryAmounts(numbers: LocaleNumbers = LocaleNumbers.of()) : Amounts {
    public val style: NumberStyle =
        NumberStyle(
            numbers.currency, numbers.symbol, numbers.symbolFirst, numbers.symbolSpaced,
            numbers.decimal, numbers.grouping, numbers.minus, numbers.minusFirst)

    override fun format(minor: Long): String = formatAmount(minor, false, style)

    override fun signed(minor: Long): String = formatAmount(minor, true, style)

    override val symbol: String = numbers.symbol

    override fun editable(minor: Long): String = formatAmountEditable(minor, style)

    override fun parse(typed: String): Long? = parseAmount(typed, style)
}
