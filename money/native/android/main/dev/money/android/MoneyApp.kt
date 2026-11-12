// money over the app library (ui/kotlin/app): the generated activity draws [MoneyRoot] — the first
// run every Ark app shares until a session (or the demo) is open, then the app. The app, once there
// is a budget: four tabs in a navigation bar — Budget,
// Accounts, Transactions and Insights — each a page with a large top app bar
// that collapses as it scrolls, and the pages they open (an account's
// register, Settings with the banks) pushed over them. Navigation is
// navigation-compose's, so the system's back gesture is predictive: the
// page under the finger slides and fades back toward the one beneath it.
// Each tab keeps its own place, as Google's apps do, and the tab already
// showing goes back to its top when it is pressed again.
//
// Every screen's runtime is money's host's (`MoneyHost`, money/bridge/src/host.rs), which carries
// out what each asks of another; what it asks of this shell (a page pushed, Settings, Add a Device)
// comes as its asks. What the app says once — a refusal, a sync — is a snackbar on the page
// showing.
package dev.money.android

import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Badge
import androidx.compose.material3.IconButton
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LargeTopAppBar
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.TopAppBarScrollBehavior
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.layout.layout
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import dev.arkui.LocalHostedScreens
import dev.arkui.shellShortcuts
import uniffi.money_bridge.ShellMsg
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.arkui.AccountFace
import dev.arkui.AccountGroup
import dev.arkui.AccountRow
import dev.arkui.AccountSheet
import dev.arkui.ArkGlyphs
import dev.arkui.TabTitle
import dev.arkui.LargeTitles
import dev.arkui.RouteArg
import dev.arkui.Transitions
import dev.arkui.reducedMotion
import uniffi.money_bridge.Route
import uniffi.money_bridge.routeFromUrl
import androidx.compose.material3.Surface
import dev.arkui.app.ArkApp
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.money_bridge.Household
import uniffi.money_bridge.Appearance
import androidx.compose.foundation.isSystemInDarkTheme
import uniffi.money_bridge.MoneyHost
import uniffi.money_bridge.NativeAsk

/** The four tabs. */
enum class Tab(val route: String, val label: String, val icon: ImageVector, val selected: ImageVector, val section: String) {
    Budget("budget", "Budget", Glyphs.Budget, Glyphs.BudgetFilled, "budget"),
    Accounts("accounts", "Accounts", Glyphs.Accounts, Glyphs.AccountsFilled, "accounts"),
    Activity("activity", "Transactions", Glyphs.Activity, Glyphs.ActivityFilled, "transactions"),
    Insights("insights", "Insights", Glyphs.Insights, Glyphs.InsightsFilled, "insights"),
}

/** What a page can ask of the app: a page, the snackbar, who is using the budget. */
class Shell(
    val library: MoneyHost,
    val snackbar: SnackbarHostState,
    /** Who is using the budget and where (money's host's), for the faces and the account sheet. */
    val household: StateFlow<Household>,
    val onSettings: () -> Unit,
    val onSignOut: () -> Unit,
    /** The account sheet: what the face at the end of a tab's title raises. */
    val onAccountSheet: () -> Unit = {},
)

val LocalShell = staticCompositionLocalOf<Shell> { error("no shell") }

/** The first run's pages until the app itself shows, then the app: what the activity draws. */
@Composable
fun MoneyRoot(app: ArkApp, library: MoneyHost) {
    // The shell's appearance over the device's (a launch's `-appearance`, Settings' Light or
    // Dark), as on the iPhone; none (System, the default) follows the device.
    val moves by app.moves.collectAsStateWithLifecycle()
    val appearance = remember(moves) { library.shell()?.page()?.appearance }
    MoneyTheme(dark = appearance?.let { it == Appearance.DARK } ?: isSystemInDarkTheme()) {
        Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
            val shows by app.showsApp.collectAsStateWithLifecycle()
            // The first run's pages every Ark app shares (generated/WelcomeScreen.kt, from
            // ark_app::welcome, money's words).
            val welcome = remember(library) { FirstRunHolder(library.welcome()) }
            LaunchedEffect(moves) { welcome.reload() }
            val page by welcome.page.collectAsStateWithLifecycle()
            // money's pages over the app itself when their route asked for them.
            if (shows && page.intro == null) MoneyApp(app, library) else WelcomeScreen(welcome)
        }
    }
}

/** money's screens' holders over the runtimes its host made for one session. */
class Holders(library: MoneyHost) {
    val shell = MoneyShellHolder(library.shell()!!)
    val budget = BudgetHolder(library.budget()!!)
    val accounts = AccountsHolder(library.accounts()!!)
    val transactions = TransactionsHolder(library.transactions()!!)
    val insights = InsightsHolder(library.insights()!!)
    val sheets = SheetsHolder(library.sheets()!!)

    /** Every page read again: something moved. */
    fun reload() {
        for (h in listOf(shell, budget, accounts, transactions, insights, sheets)) h.reload()
        // Where the app is, for a screenshot's ready signal: the section.
        dev.arkui.Ready.at(shell.page.value.section)
    }
}

/** The budget, its tabs and its pages, over the runtimes money's host made. */
@Composable
fun MoneyApp(app: ArkApp, library: MoneyHost) {
    val moves by app.moves.collectAsStateWithLifecycle()
    val generation = remember(moves) { library.generation() }
    // A session closed between a move and this: nothing to draw until the first run shows.
    if (generation == 0uL) return
    val holders = remember(generation) { Holders(library) }
    MoneyApp(app, library, holders, moves)
}

@Composable
private fun MoneyApp(app: ArkApp, library: MoneyHost, holders: Holders, moves: Long) {
    val nav = rememberNavController()
    val snackbar = remember { SnackbarHostState() }
    var account by remember { mutableStateOf(false) }
    var addingDevice by remember { mutableStateOf(false) }
    LaunchedEffect(app) { app.notices.collect { snackbar.showSnackbar(it) } }
    val household = remember(library) { MutableStateFlow(library.household()) }
    val context = LocalContext.current
    val sections = holders.shell
    val sheets = holders.sheets
    val shellPage by sections.page.collectAsStateWithLifecycle()
    // Plaid's page, in a Custom Tab, the moment the server answers a connection asked for here
    // (money_app's shell says which: `SettingsPage.plaid`). A Custom Tab is not followed, so it
    // is taken as closed once opened.
    LaunchedEffect(shellPage.settings.plaid) {
        val url = shellPage.settings.plaid ?: return@LaunchedEffect
        runCatching { CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url)) }
            .onFailure { snackbar.showSnackbar("Couldn't open Plaid's page: open it from Settings") }
        sections.send(ShellMsg.PageClosed(url = url))
    }
    val shell =
        remember(library, nav) {
            Shell(
                library = library,
                snackbar = snackbar,
                household = household,
                onSettings = { nav.navigate("settings") { launchSingleTop = true } },
                onSignOut = { app.signOut() },
                onAccountSheet = { account = true },
            )
        }
    // Whatever moved, every page read again, and what money's host asks of this shell carried out:
    // a page pushed, Settings, Add a Device, and a link taken as a launch putting away what this
    // shows of its own (the shell is told the launch by the host).
    LaunchedEffect(moves) {
        holders.reload()
        household.value = library.household()
        for (ask in library.takeAsks()) {
            when (ask) {
                is NativeAsk.Open ->
                    when (val r = ask.route) {
                        is Route.Banks -> {
                            sections.send(ShellMsg.Pane(title = "Connected Banks"))
                            shell.onSettings()
                        }
                        is Route.Bank -> {
                            sections.send(ShellMsg.OpenBank(name = r.name))
                            shell.onSettings()
                        }
                        else -> nav.navigate(register(r))
                    }
                is NativeAsk.Settings -> {
                    if (ask.pane.isNotEmpty()) sections.send(ShellMsg.Pane(title = ask.pane))
                    shell.onSettings()
                }
                is NativeAsk.AddDevice -> addingDevice = true
                is NativeAsk.Reset -> {
                    dev.arkui.Ready.changed()
                    account = false
                    nav.popBackStack("shell", inclusive = false)
                }
            }
        }
    }

    val reduced = reducedMotion()
    val hosted: @Composable (String) -> Unit = { name -> Hosted(name, holders, library, shell) }

    CompositionLocalProvider(LocalShell provides shell, LocalHostedScreens provides hosted, LocalMoves provides app.moves) {
        NavHost(
            nav,
            startDestination = "shell",
            enterTransition = { enter(reduced) },
            exitTransition = { exit(reduced) },
            popEnterTransition = { popEnter(reduced) },
            popExitTransition = { popExit(reduced) },
        ) {
            composable("shell") {
                // The shell, written from Rust (generated/MoneyShell.kt, ui/docs/apps/money.md): the
                // sections Budget, Accounts, Transactions and Insights — a navigation bar, a rail or a
                // drawer by the window's width — each its top bar, search and inspector, hosting
                // money's own screens.
                ScreenFrame {
                    androidx.compose.foundation.layout.Box(Modifier.shellShortcuts(moneyCommands(sections))) { MoneyShell(sections) }
                }
            }
            composable("register/{arg}") { e ->
                // The route names the account by its id, so a renamed one keeps its register; every
                // account's (All Accounts) for any other.
                val route = RouteArg.path(e.arguments?.getString("arg"))?.let(::routeFromUrl)
                val id = (route as? Route.Account)?.id
                RegisterHosted(library, moves, id, "", title = route?.title() ?: "All Accounts", onBack = { nav.popBackStack() })
            }
            // Settings, written from Rust (generated/MoneySettings.kt): the shell's panes.
            composable("settings") {
                Page(title = "Settings", onBack = { nav.popBackStack() }) { pad, _ ->
                    Box(Modifier.padding(top = pad.calculateTopPadding())) { MoneySettings(sections) }
                }
            }
        }
        SheetsScreen(sheets)
        if (addingDevice) dev.arkui.AddDeviceDialog({ app.deviceLink() }, onDismiss = { addingDevice = false })
        if (account) AccountOf(shell, onDismiss = { account = false })
    }
}

/**
 * The account sheet: who is using the budget, the server and whether it answers, the banks that
 * want somebody in Settings, Settings, and the way out — arkui's, as harken's is.
 */
@Composable
private fun AccountOf(shell: Shell, onDismiss: () -> Unit) {
    val h by shell.household.collectAsStateWithLifecycle()
    AccountSheet(
        name = h.who,
        picture = h.picture,
        detail = h.detail,
        server = h.server,
        linked = h.linked ?: false,
        onSettings = shell.onSettings,
        onSignOut = shell.onSignOut,
        onDismiss = onDismiss,
        signOut = if (h.demo) "Leave the Demo" else "Sign Out",
    ) { leave ->
        val wanting = h.banksWanting
        if (wanting > 0) {
            AccountGroup {
                val red = MaterialTheme.colorScheme.error
                AccountRow(
                    ArkGlyphs.Attention,
                    if (wanting == 1L) "A Bank Needs a Look" else "$wanting Banks Need a Look",
                    onClick = { leave(shell.onSettings) },
                    under = "In Settings, under Banks",
                    tint = red,
                    chevron = true,
                )
            }
        }
    }
}

/**
 * A register, written from Rust (generated/RegisterScreen.kt), over a runtime of its own while it
 * shows: the account of this hex id's, or every account's for none, searched by `search`, its row
 * the one the shell's inspector shows.
 */
@Composable
private fun RegisterHosted(
    library: MoneyHost,
    moves: Long,
    account: String?,
    search: String,
    selected: String? = null,
    title: String? = null,
    onBack: (() -> Unit)? = null,
) {
    val holder = remember(library, account) { library.register(account)?.let { RegisterHolder(it) } } ?: return
    LaunchedEffect(moves) { holder.reload() }
    LaunchedEffect(holder, search) { holder.send(uniffi.money_bridge.RegisterMsg.Search(text = search)) }
    LaunchedEffect(holder, selected) { holder.send(uniffi.money_bridge.RegisterMsg.Select(id = selected)) }
    ScreenFrame(title = title, onBack = onBack) { RegisterScreen(holder) }
}

/** Where the back stack goes for a register's route: an account's, or All Accounts. */
private fun register(route: Route): String = "register/" + RouteArg.of(route.url())

/** A tab, kept where it was; the tab showing goes back to its top. */
private fun NavHostController.go(t: Tab) {
    navigate(t.route) {
        popUpTo(graph.findStartDestination().id) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
}

private fun isTab(e: NavBackStackEntry): Boolean = Tab.entries.any { it.route == e.destination.route }

// Between tabs, nothing moves: a tab is a peer, switched too often for motion to read as anything
// but lag, and is there at once, as harken's are. Into a page and back out, arkui's shared X axis
// (`Transitions`): a short slide with a fade, which the back gesture drives in reverse, along the
// same path it came in by — on its critically damped springs, so a navigation interrupted
// part-way carries on from where it is. With motion reduced it is a quick cross-fade.
private fun AnimatedContentTransitionScope<NavBackStackEntry>.between(): Boolean =
    isTab(initialState) && isTab(targetState)

private fun AnimatedContentTransitionScope<NavBackStackEntry>.enter(reduced: Boolean): EnterTransition =
    if (between()) EnterTransition.None else Transitions.pushIn(reduced)

private fun AnimatedContentTransitionScope<NavBackStackEntry>.exit(reduced: Boolean): ExitTransition =
    if (between()) ExitTransition.None else Transitions.pushOut(reduced)

private fun AnimatedContentTransitionScope<NavBackStackEntry>.popEnter(reduced: Boolean): EnterTransition =
    if (between()) EnterTransition.None else Transitions.popIn(reduced)

private fun AnimatedContentTransitionScope<NavBackStackEntry>.popExit(reduced: Boolean): ExitTransition =
    if (between()) ExitTransition.None else Transitions.popOut(reduced)

/**
 * A page as every screen here is laid out: a large top app bar that collapses as the content
 * scrolls, its `actions`, a back arrow when `onBack` is given, the FAB, the snackbar, and pull to
 * refresh asking the bank for anything new. `content` gets the padding to keep clear of the bar.
 * A tab's root (no `onBack`) has arkui's [TabTitle] instead of the app bar — the one large title row
 * harken's tabs have too, from the status bar's foot, with no padding of money's own — its
 * `actions` (Budget's menu) and the account's face at the end of its line, raising the account
 * sheet. It goes up with the page as the content scrolls, as harken's and the iPhone's do, and
 * comes back once the content is back at its top; a page pushed over a tab has the large top app
 * bar, and no face. Under a tab's title the padding `content` gets starts with arkui's
 * [TabTitle.BELOW], the one gap every tab keeps between its title and its first content: a list
 * puts it in its `contentPadding`, so it scrolls away with the list rather than cutting it off.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun Page(
    title: String,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
    actions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {},
    fab: @Composable () -> Unit = {},
    refresh: Boolean = true,
    titleContent: (@Composable () -> Unit)? = null,
    content: @Composable (PaddingValues, TopAppBarScrollBehavior) -> Unit,
) {
    val shell = LocalShell.current
    val household by shell.household.collectAsStateWithLifecycle()
    val bar = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    // Under the shell's top bar (ArkUI's): the bar has the title, the back button and the
    // actions — a pushed page's too, whose bar replaces its section's — so the page draws none.
    val hosted = dev.arkui.LocalUnderToolbar.current
    Scaffold(
        // Under the shell's bar the bar's own behaviour moves the title: this one, laid out
        // nowhere, has no limit, and would take every scroll up for itself.
        modifier = if (hosted) modifier else modifier.nestedScroll(bar.nestedScrollConnection),
        // The navigation bar below keeps the bottom clear; a pushed page has none, and keeps it itself.
        // Under the shell's top bar the bar has taken the status bar's inset: the page
        // takes none again (an account's register drew an empty band over its title).
        contentWindowInsets =
            if (hosted) WindowInsets(0)
            else if (onBack == null) WindowInsets.systemBars.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)
            else WindowInsets.systemBars,
        topBar = {
            if (hosted) {
                // The shell's bar is over it.
            } else if (onBack == null) {
                // The tab's large title row, from the status bar's foot, going up under it as the
                // page scrolls: the scroll behaviour's offset, read as it is laid out, not composed.
                Box(
                    Modifier.windowInsetsPadding(WindowInsets.statusBars).clipToBounds().layout { m, c ->
                        val p = m.measure(c.copy(minHeight = 0, maxHeight = Constraints.Infinity))
                        val limit = -p.height.toFloat()
                        if (bar.state.heightOffsetLimit != limit) bar.state.heightOffsetLimit = limit
                        val off = bar.state.heightOffset.roundToInt().coerceIn(-p.height, 0)
                        layout(p.width, p.height + off) { p.place(0, off) }
                    }
                ) {
                    TabTitle(
                        title,
                        account = {
                            val wanting = household.banksWanting
                            AccountFace(
                                household.who,
                                household.picture,
                                onClick = shell.onAccountSheet,
                                attention = if (wanting > 0) "Banks need a look" else null,
                            )
                        },
                        actions = actions,
                    )
                }
            } else {
                // The expanded title at the iPhone's large title's weight; collapsed, Material's own.
                LargeTitles {
                    LargeTopAppBar(
                        title = { titleContent?.invoke() ?: Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                        navigationIcon = {
                            IconButton(onClick = onBack) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                            }
                        },
                        actions = actions,
                        scrollBehavior = bar,
                        colors = TopAppBarDefaults.largeTopAppBarColors(scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer),
                    )
                }
            }
        },
        floatingActionButton = { if (!hosted) fab() },
        snackbarHost = { SnackbarHost(shell.snackbar) },
    ) { pad ->
        val under = if (onBack == null && !hosted) TabTitle.BELOW else 0.dp
        if (refresh) {
            PullToRefreshBox(
                isRefreshing = household.syncing,
                onRefresh = { shell.library.syncBank() },
                modifier = Modifier.fillMaxSize().padding(top = pad.calculateTopPadding()),
            ) {
                content(PaddingValues(top = under, bottom = pad.calculateBottomPadding()), bar)
            }
        } else {
            Box(Modifier.fillMaxSize().padding(top = pad.calculateTopPadding())) {
                content(PaddingValues(top = under, bottom = pad.calculateBottomPadding()), bar)
            }
        }
    }
}

/** Kept per page across a recomposition: a sheet's subject, a filter. */
@Composable
fun <T : Any> rememberSheet(): androidx.compose.runtime.MutableState<T?> = remember { mutableStateOf(null) }

/** Whether a bool survives rotation. */
@Composable
fun rememberFlag(initial: Boolean = false): androidx.compose.runtime.MutableState<Boolean> = rememberSaveable { mutableStateOf(initial) }

/**
 * A screen the shell hosts (ark_app::shell::hosted): money's runtimes' generated screens. What
 * each opens is money's host's to carry out, and what of it is this shell's comes as its asks.
 */
@Composable
private fun Hosted(name: String, holders: Holders, library: MoneyHost, shell: Shell) {
    val moves by LocalMoves.current.collectAsStateWithLifecycle()
    when (name) {
        // Written from Rust (generated/BudgetScreen.kt), over its runtime.
        "budget" -> ScreenFrame { BudgetScreen(holders.budget) }
        // Written from Rust (generated/AccountsScreen.kt), over its runtime.
        "accounts" -> AccountsScreen(holders.accounts)
        // Written from Rust (generated/TransactionsScreen.kt), over its runtime: the shell's
        // search bar is its search (money's host's).
        "transactions" -> ScreenFrame { TransactionsScreen(holders.transactions) }
        // Written from Rust (generated/InsightsScreen.kt), over its runtime.
        "insights" -> ScreenFrame { InsightsScreen(holders.insights) }
        "accounts.register" -> {
            // The account the shell shows (its place's tag, `accounts/<id>`), under the shell's search.
            val page by holders.shell.page.collectAsStateWithLifecycle()
            val id = page.section.removePrefix("accounts/").takeIf { page.section.startsWith("accounts/") }
            if (id != null) RegisterHosted(library, moves, id, page.search, page.transaction?.id)
            else Text("No Account Selected", modifier = Modifier.padding(16.dp))
        }
        // Who is signed in: their face, as the tabs' own title rows draw it.
        "household" -> {
            val h by shell.household.collectAsStateWithLifecycle()
            AccountFace(h.who, h.picture, onClick = shell.onAccountSheet, attention = if (h.banksWanting > 0) "Banks need a look" else null)
        }
        // What a platform adds to Settings' General: the desktop's alone.
        "settings.more" -> {}
        else -> Text("Nothing Selected", modifier = Modifier.padding(16.dp))
    }
}

/** How many times anything moved: what a register hosted in a section reads again on. */
val LocalMoves = staticCompositionLocalOf<StateFlow<Long>> { MutableStateFlow(0L) }
