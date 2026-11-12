// The app, once there is a budget: four tabs in a navigation bar — Budget,
// Accounts, Transactions and Insights — each a page with a large top app bar
// that collapses as it scrolls, and the pages they open (an account's
// register, Settings with the banks) pushed over them. Navigation is
// navigation-compose's, so the system's back gesture is predictive: the
// page under the finger slides and fades back toward the one beneath it.
// Each tab keeps its own place, as Google's apps do, and the tab already
// showing goes back to its top when it is pressed again.
//
// The transaction form is the app's, over whichever page asked for it (the
// FAB on most of them, a row's tap), as a modal bottom sheet; so are the
// sheets each page raises. What the book says — a refusal, a sync — is a
// snackbar on the page showing.
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
import uniffi.money_bridge.MoneyRequest
import uniffi.money_bridge.ShellMsg
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.arkdb.ffi.Id
import dev.arkui.AccountFace
import dev.arkui.AccountGroup
import dev.arkui.AccountRow
import dev.arkui.AccountSheet
import dev.arkui.ArkGlyphs
import dev.arkui.TabTitle
import dev.arkui.LargeTitles
import dev.arkui.Navigator
import dev.arkui.RouteArg
import dev.arkui.SensoryFeedback
import dev.arkui.Transitions
import dev.arkui.reducedMotion
import dev.arkui.modules.CameraPage
import dev.arkui.modules.PhoneCamera
import dev.arkui.modules.PhoneClipboard
import dev.arkui.modules.PhoneHaptics
import uniffi.money_bridge.registerDevices
import uniffi.money_bridge.Auto
import uniffi.money_bridge.Route
import uniffi.money_bridge.TxnLine
import uniffi.money_bridge.SheetsMsg
import uniffi.money_bridge.routeFromUrl

/** The four tabs. */
enum class Tab(val route: String, val label: String, val icon: ImageVector, val selected: ImageVector, val section: String) {
    Budget("budget", "Budget", Glyphs.Budget, Glyphs.BudgetFilled, "budget"),
    Accounts("accounts", "Accounts", Glyphs.Accounts, Glyphs.AccountsFilled, "accounts"),
    Activity("activity", "Transactions", Glyphs.Activity, Glyphs.ActivityFilled, "transactions"),
    Insights("insights", "Insights", Glyphs.Insights, Glyphs.InsightsFilled, "insights"),
}

/** What a page can ask of the app: a transaction's form, a page, the snackbar. */
class Shell(
    val book: Book,
    val snackbar: SnackbarHostState,
    /** An account's register, or every account's (All Accounts) for none. */
    val onAccount: (Id?) -> Unit,
    val onSettings: () -> Unit,
    val onTab: (Tab) -> Unit,
    val onSignOut: () -> Unit,
    /** The account sheet: what the face at the end of a tab's title raises. */
    val onAccountSheet: () -> Unit = {},
    /** "Add a Device": the join link another phone signs in with, made as this login. */
    val addDevice: suspend () -> dev.arkui.DeviceLinkShown = { dev.arkui.DeviceLinkShown(null, 0, emptyList(), "Sign in first.") },
)

val LocalShell = staticCompositionLocalOf<Shell> { error("no shell") }

/** The budget, its tabs and its pages; `onSignOut` leaves it. */
@Composable
fun MoneyApp(
    book: Book,
    onSignOut: () -> Unit,
    addDevice: suspend () -> dev.arkui.DeviceLinkShown = { dev.arkui.DeviceLinkShown(null, 0, emptyList(), "Sign in first.") },
    links: kotlinx.coroutines.flow.StateFlow<Int> = kotlinx.coroutines.flow.MutableStateFlow(0),
    takeLink: () -> String? = { null },
) {
    val nav = rememberNavController()
    val ledger by book.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    var account by remember { mutableStateOf(false) }
    LaunchedEffect(book) { book.notes.collect { snackbar.showSnackbar(it) } }
    // What the hand feels of a change the book has taken, on the frame it shows: the iPhone's
    // `.sensoryFeedback` on the same events (`Book.felt`).
    val felt by book.felt.collectAsStateWithLifecycle()
    SensoryFeedback(felt.kind, felt.count)
    val context = LocalContext.current
    val sections =
        remember(book) { MoneyShellHolder(book.session, { book.sync() }, { book.state.value.today }) }
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
    // Who is signed in where, for Settings (money_app's SessionFacts).
    val version = remember(context) {
        runCatching {
            val i = context.packageManager.getPackageInfo(context.packageName, 0)
            @Suppress("DEPRECATION") "${i.versionName} (${i.versionCode})"
        }.getOrDefault("")
    }
    LaunchedEffect(ledger.who, ledger.demo, ledger.server, ledger.link, ledger.ready) {
        val facts = uniffi.money_bridge.SessionFacts(
            who = ledger.who,
            login = "",
            demo = ledger.demo,
            server = ledger.server.substringAfter("://"),
            linked = if (ledger.ready) ledger.link.linked else null,
            pending = ledger.link.pending.toLong(),
            version = version,
            leaves = true,
            addsDevices = !ledger.demo,
        )
        sections.send(ShellMsg.Session(facts = facts))
    }
    var addingDevice by remember { mutableStateOf(false) }
    // The device modules (money/app.toml): the camera's page is drawn at the root, below.
    val hapticView = androidx.compose.ui.platform.LocalView.current
    val camera = remember { PhoneCamera() }
    // Registered as the process's (`ArkProcess`'s modules): every screen's runtime asks through them.
    remember(hapticView) { registerDevices(PhoneHaptics(hapticView), PhoneClipboard(hapticView.context), null, camera) }
    // The sheets over the app, written from Rust (generated/SheetsScreen.kt): its month the book's.
    val sheets = remember(book) { SheetsHolder(book.session, { book.sync() }, { book.state.value.today }) }
    camera.onAnswered = { sheets.answered() }
    val shell =
        remember(book, nav) {
            Shell(
                book = book,
                snackbar = snackbar,
                onAccount = { id -> nav.navigate(register(book.state.value.accounts.firstOrNull { it.id == id }?.let { Route.Account(it.id.hex, it.name) } ?: Route.Accounts)) },
                onSettings = { nav.navigate("settings") { launchSingleTop = true } },
                onTab = { t -> sections.send(ShellMsg.Select(tag = t.section)) },
                onSignOut = onSignOut,
                onAccountSheet = { account = true },
                addDevice = addDevice,
            )
        }

    // A link the app was opened with or sent, taken as a launch as the app runs: the sheets put
    // away and the pages popped, then the shell told as a launch tells it, which puts back what
    // it showed itself — a launch link (`money://?screen=…`, ui/docs/shots.md) by its arguments,
    // any other by its address (`money:///budget` selects the Budget; nothing is pushed). A
    // sign-in link is the onboarding's, and the shell passes it over. The first run keeps every
    // link (`FirstRun::open_link`), so one that came while its pages or the sign-in showed is
    // taken here, once the app shows.
    val linked by links.collectAsStateWithLifecycle()
    LaunchedEffect(linked) {
        val link = takeLink() ?: return@LaunchedEffect
        dev.arkui.Ready.changed()
        sheets.send(SheetsMsg.Dismiss)
        account = false
        nav.popBackStack("shell", inclusive = false)
        sections.send(Navigator.launchArgsOf(link, "money")?.let { ShellMsg.Launch(args = it) } ?: ShellMsg.Address(address = link))
    }

    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route
    val tab = Tab.entries.firstOrNull { it.route == route }
    val reduced = reducedMotion()

    // The shell, written from Rust (generated/MoneyShell.kt, ui/docs/apps/money.md): the sections
    // Budget, Accounts, Transactions and Insights — a navigation bar, a rail or a drawer by the
    // window's width — each its top bar, search and inspector, hosting money's own screens.
    LaunchedEffect(ledger) { sections.reload() }
    // Budget, written from Rust (generated/BudgetScreen.kt), over its runtime: its month the book's.
    val budget = remember(book) { BudgetHolder(book.session, { book.sync() }, { book.state.value.today }) }
    LaunchedEffect(budget, ledger.month) { budget.send(uniffi.money_bridge.BudgetMsg.Month(month = ledger.month)) }
    LaunchedEffect(ledger) { budget.reload() }
    // Accounts, written from Rust (generated/AccountsScreen.kt), over its runtime.
    val accounts = remember(book) { AccountsHolder(book.session, { book.sync() }, { book.state.value.today }) }
    LaunchedEffect(ledger) { accounts.reload() }
    // Transactions, written from Rust (generated/TransactionsScreen.kt), over its runtime.
    val transactions = remember(book) { TransactionsHolder(book.session, { book.sync() }, { book.state.value.today }) }
    LaunchedEffect(ledger) { sheets.reload() }
    LaunchedEffect(sheets, ledger.month) { sheets.send(SheetsMsg.Month(month = ledger.month, today = ledger.today)) }
    // What any screen's runtime asks of the app (`MoneyRequest`).
    val carry: (MoneyRequest) -> Unit = { r ->
        when (r) {
            is MoneyRequest.Sheet -> sheets.send(r.msg)
            // Transaction Details in the inspector while it shows; its form otherwise.
            is MoneyRequest.OpenTransaction ->
                if (sections.page.value.inspector) sections.send(ShellMsg.InspectTransaction(id = r.key)) else sheets.send(SheetsMsg.Open(key = r.key))
            is MoneyRequest.Accounts -> {
                sections.send(ShellMsg.Select(tag = "accounts"))
                accounts.send(r.msg)
            }
            // What the bank brought, to look over: Transactions' To Review.
            is MoneyRequest.Review -> {
                sections.send(ShellMsg.Select(tag = "transactions"))
                transactions.send(uniffi.money_bridge.TransactionsMsg.Show(review = true))
            }
            is MoneyRequest.StepMonth -> book.stepMonth(r.by)
            is MoneyRequest.ThisMonth -> book.stepMonth(0)
            is MoneyRequest.Budget -> budget.send(r.msg)
            is MoneyRequest.Refresh -> book.sync()
            is MoneyRequest.Settings -> nav.navigate("settings") { launchSingleTop = true }
            is MoneyRequest.SignOut -> onSignOut()
            is MoneyRequest.AddDevice -> addingDevice = true
            else -> {}
        }
    }
    sections.request = carry
    accounts.requested = carry
    transactions.requested = carry
    // Where the window opens (ark_app::shell::Launch): the launch intent's `ark.args`
    // (`am start --esa ark.args -screen,budget`), once for the activity.
    val launchArgs = (LocalContext.current as? android.app.Activity)?.intent?.getStringArrayExtra("ark.args")
    LaunchedEffect(sections) {
        if (launchArgs != null) sections.send(ShellMsg.Launch(args = launchArgs.toList()))
    }
    // The month is the budget's title in the shell's top bar.
    LaunchedEffect(ledger.month) { sections.send(uniffi.money_bridge.ShellMsg.Month(month = ledger.month)) }
    val hosted: @Composable (String) -> Unit = { name -> Hosted(name, book, ledger, shell, nav, sections, budget, accounts, transactions, sheets, carry) }

    CompositionLocalProvider(LocalShell provides shell, LocalHostedScreens provides hosted) {
        // The camera's page, while a module asks for it (a receipt scanned).
        CameraPage(camera)
        NavHost(
            nav,
            startDestination = "shell",
            enterTransition = { enter(reduced) },
            exitTransition = { exit(reduced) },
            popEnterTransition = { popEnter(reduced) },
            popExitTransition = { popExit(reduced) },
        ) {
            composable("shell") {
                ScreenFrame {
                    androidx.compose.foundation.layout.Box(Modifier.shellShortcuts(moneyCommands(sections))) { MoneyShell(sections) }
                }
            }
            composable("register/{arg}") { e ->
                // The route names the account by its id, so a renamed one keeps its register; every
                // account's (All Accounts) for any other.
                val route = RouteArg.path(e.arguments?.getString("arg"))?.let(::routeFromUrl)
                val id = (route as? Route.Account)?.id
                RegisterHosted(book, ledger, shell, id, "", carry, title = route?.title() ?: "All Accounts", onBack = { nav.popBackStack() })
            }
            // Settings, written from Rust (generated/MoneySettings.kt): the shell's panes.
            composable("settings") {
                Page(title = "Settings", onBack = { nav.popBackStack() }) { pad, _ ->
                    Box(Modifier.padding(top = pad.calculateTopPadding())) { MoneySettings(sections) }
                }
            }
        }
        SheetsScreen(sheets)
        if (addingDevice) dev.arkui.AddDeviceDialog(addDevice, onDismiss = { addingDevice = false })
        if (account) AccountOf(ledger, shell, onDismiss = { account = false })
    }
}

/**
 * The account sheet: who is using the budget, the server and whether it answers, the banks that
 * want somebody in Settings, Settings, and the way out — arkui's, as harken's is.
 */
@Composable
private fun AccountOf(ledger: Ledger, shell: Shell, onDismiss: () -> Unit) {
    AccountSheet(
        name = ledger.who,
        picture = ledger.picture,
        detail = if (ledger.demo) "The demo household, on this phone only" else "",
        server = if (ledger.demo) null else ledger.server.substringAfter("://"),
        linked = ledger.link.linked,
        onSettings = shell.onSettings,
        onSignOut = shell.onSignOut,
        onDismiss = onDismiss,
        signOut = if (ledger.demo) "Leave the Demo" else "Sign Out",
    ) { leave ->
        val wanting = ledger.banksWanting
        if (wanting > 0) {
            AccountGroup {
                val red = MaterialTheme.colorScheme.error
                AccountRow(
                    ArkGlyphs.Attention,
                    if (wanting == 1) "A Bank Needs a Look" else "$wanting Banks Need a Look",
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
 * shows: the account of this hex id's, or every account's for none, searched by `search`.
 */
@Composable
private fun RegisterHosted(
    book: Book,
    ledger: Ledger,
    shell: Shell,
    account: String?,
    search: String,
    carry: (MoneyRequest) -> Unit,
    title: String? = null,
    onBack: (() -> Unit)? = null,
) {
    val holder = remember(book, account) { RegisterHolder(book.session, { book.sync() }, { book.state.value.today }, account) }
    holder.requested = carry
    LaunchedEffect(ledger) { holder.reload() }
    LaunchedEffect(holder, search) { holder.send(uniffi.money_bridge.RegisterMsg.Search(text = search)) }
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
    val ledger by shell.book.state.collectAsStateWithLifecycle()
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
                            val wanting = ledger.banksWanting
                            AccountFace(
                                ledger.who,
                                ledger.picture,
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
                isRefreshing = ledger.syncing,
                onRefresh = { shell.book.sync() },
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
 * A screen the shell hosts (ark_app::shell::hosted): money's own, until each moves into Rust.
 * Accounts' list is the generated Accounts slice, over its runtime.
 */
@Composable
private fun Hosted(
    name: String,
    book: Book,
    ledger: Ledger,
    shell: Shell,
    nav: NavHostController,
    sections: MoneyShellHolder,
    budget: BudgetHolder,
    accounts: AccountsHolder,
    transactions: TransactionsHolder,
    sheets: SheetsHolder,
    carry: (MoneyRequest) -> Unit,
) {
    when (name) {
        "budget" -> {
            // A category's month: Category Details, the shell's inspector (written from Rust) —
            // a supporting pane when expanded, a bottom sheet otherwise.
            budget.opened = { r ->
                if (r is Route.Category) {
                    sections.send(ShellMsg.InspectCategory(id = r.id))
                    sections.send(ShellMsg.Inspector(shown = true))
                }
            }
            ScreenFrame { BudgetScreen(budget) }
        }
        "accounts" -> {
            // Written from Rust (generated/AccountsScreen.kt), over its runtime.
            accounts.opened = { r ->
                if (r is Route.Banks) {
                    sections.send(ShellMsg.Pane(title = "Connected Banks"))
                    shell.onSettings()
                } else {
                    nav.navigate(register(r))
                }
            }
            AccountsScreen(accounts)
        }
        "transactions" -> {
            // Written from Rust (generated/TransactionsScreen.kt), over its runtime: the shell's
            // search bar is its search, the book's month its categories'.
            val holder = transactions
            val page by sections.page.collectAsStateWithLifecycle()
            LaunchedEffect(ledger) { holder.reload() }
            LaunchedEffect(ledger.month) { holder.send(uniffi.money_bridge.TransactionsMsg.Month(month = ledger.month)) }
            LaunchedEffect(page.search) { holder.send(uniffi.money_bridge.TransactionsMsg.Search(text = page.search)) }
            ScreenFrame { TransactionsScreen(holder) }
        }
        "insights" -> {
            // Written from Rust (generated/InsightsScreen.kt), over its runtime: its month the book's.
            val holder = remember(book) { InsightsHolder(book.session, { book.sync() }, { book.state.value.today }) }
            LaunchedEffect(ledger) { holder.reload() }
            LaunchedEffect(ledger.month) { holder.send(uniffi.money_bridge.InsightsMsg.Month(month = ledger.month)) }
            ScreenFrame { InsightsScreen(holder) }
        }
        "accounts.register" -> {
            // The account the shell shows (its place's tag, `accounts/<id>`), under the shell's search.
            val page by sections.page.collectAsStateWithLifecycle()
            val id = page.section.removePrefix("accounts/").takeIf { page.section.startsWith("accounts/") }
            if (id != null) RegisterHosted(book, ledger, shell, id, page.search, carry) else Text("No Account Selected", modifier = Modifier.padding(16.dp))
        }
        // Who is signed in: their face, as the tabs' own title rows draw it.
        "household" -> AccountFace(
            ledger.who,
            ledger.picture,
            onClick = shell.onAccountSheet,
            attention = if (ledger.banksWanting > 0) "Banks need a look" else null,
        )
        // What a platform adds to Settings' General: the desktop's alone.
        "settings.more" -> {}
        else -> Text("Nothing Selected", modifier = Modifier.padding(16.dp))
    }
}
