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
import uniffi.money_app.ShellMsg
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
import dev.arkui.Links
import dev.arkui.Navigator
import dev.arkui.RouteArg
import dev.arkui.SensoryFeedback
import dev.arkui.Transitions
import dev.arkui.reducedMotion
import dev.arkui.modules.PhoneClipboard
import dev.arkui.modules.PhoneHaptics
import uniffi.money_domain.Auto
import uniffi.money_domain.Route
import uniffi.money_domain.TxnLine
import uniffi.money_domain.newDraft
import uniffi.money_domain.routeFromPath

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
    val onAdd: (account: Id?) -> Unit,
    val onEdit: (TxnLine) -> Unit,
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
fun MoneyApp(book: Book, onSignOut: () -> Unit, addDevice: suspend () -> dev.arkui.DeviceLinkShown = { dev.arkui.DeviceLinkShown(null, 0, emptyList(), "Sign in first.") }) {
    val nav = rememberNavController()
    val ledger by book.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    var form by remember { mutableStateOf<TxnForm?>(null) }
    var account by remember { mutableStateOf(false) }
    LaunchedEffect(book) { book.notes.collect { snackbar.showSnackbar(it) } }
    // What the hand feels of a change the book has taken, on the frame it shows: the iPhone's
    // `.sensoryFeedback` on the same events (`Book.felt`).
    val felt by book.felt.collectAsStateWithLifecycle()
    SensoryFeedback(felt.kind, felt.count)
    // Plaid's page, in a Custom Tab, the moment the server answers a connection asked for here.
    val context = LocalContext.current
    LaunchedEffect(book) {
        book.pages.collect { url ->
            runCatching { CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url)) }
                .onFailure { snackbar.showSnackbar("Couldn't open Plaid's page: open it from Settings") }
        }
    }

    val sections =
        remember(book) { MoneyShellHolder(book.session, { book.sync() }, { book.state.value.today }) }
    val shell =
        remember(book, nav) {
            Shell(
                book = book,
                snackbar = snackbar,
                onAdd = { account -> form = newForm(book.state.value, account) },
                onEdit = { t -> form = TxnForm.of(t) },
                onAccount = { id -> nav.navigate(register(book.state.value.accounts.firstOrNull { it.id == id }?.let { Route.Account(it.id.hex, it.name) } ?: Route.Accounts)) },
                onSettings = { nav.navigate("settings") { launchSingleTop = true } },
                onTab = { t -> sections.send(ShellMsg.Select(tag = t.section)) },
                onSignOut = onSignOut,
                onAccountSheet = { account = true },
                addDevice = addDevice,
            )
        }

    // A link the app was opened with (`money://account/Checking`): its route, navigated to. The
    // tabs' own routes switch to the tab; a sign-in link (`money://join?…`) is the onboarding's.
    Links("money") { link ->
        Navigator(nav, ::destination).handle(link, "money", ::routeFromPath) { r ->
            when (r) {
                is Route.Join -> true
                is Route.Budget, is Route.Category -> true.also { sections.send(ShellMsg.Select(tag = "budget")) }
                is Route.Reflect -> true.also { sections.send(ShellMsg.Select(tag = "insights")) }
                is Route.Inbox -> true.also { sections.send(ShellMsg.Select(tag = "transactions")) }
                else -> false
            }
        }
    }

    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route
    val tab = Tab.entries.firstOrNull { it.route == route }
    val reduced = reducedMotion()

    // The shell, written from Rust (generated/MoneyShell.kt, ui/docs/apps/money.md): the sections
    // Budget, Accounts, Transactions and Insights — a navigation bar, a rail or a drawer by the
    // window's width — each its top bar, search and inspector, hosting money's own screens.
    LaunchedEffect(ledger) { sections.reload() }
    sections.native = { name ->
        when (name) {
            "new-transaction" -> form = newForm(book.state.value, null)
            "new-account" -> shell.onAccountSheet()
            "previous-month" -> book.stepMonth(-1)
            "next-month" -> book.stepMonth(1)
            "auto-assign" -> book.autoAssign(Auto.UNDERFUNDED)
            "refresh" -> book.sync()
            "review" -> {}
            else -> if (name.startsWith("settings")) nav.navigate("settings") { launchSingleTop = true }
        }
    }
    // Where the window opens (ark_app::shell::Launch): the launch intent's `ark.args`
    // (`am start --esa ark.args -screen,budget`), once for the activity.
    val launchArgs = (LocalContext.current as? android.app.Activity)?.intent?.getStringArrayExtra("ark.args")
    LaunchedEffect(sections) {
        if (launchArgs != null) sections.send(ShellMsg.Launch(args = launchArgs.toList()))
    }
    // The month is the budget's title in the shell's top bar.
    LaunchedEffect(ledger.month) { sections.send(uniffi.money_app.ShellMsg.Month(month = ledger.month)) }
    val hosted: @Composable (String) -> Unit = { name -> Hosted(name, book, ledger, shell, nav, sections) }

    CompositionLocalProvider(LocalShell provides shell, LocalHostedScreens provides hosted) {
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
                // The route names the account by its id (an older link's by its name, `names`), so a
                // renamed one keeps its register; one that is gone is the route's fallback, All Accounts.
                val route = RouteArg.path(e.arguments?.getString("arg"))?.let(::routeFromPath)
                val id = (route as? Route.Account)?.let { a -> ledger.accounts.firstOrNull { a.names(it.id.hex, it.name) }?.id }
                RegisterScreen(ledger, id, onBack = { nav.popBackStack() })
            }
            composable("settings") { SettingsScreen(ledger, onBack = { nav.popBackStack() }) }
        }
        form?.let { f -> TxnSheet(ledger, f, onDismiss = { form = null }) }
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

/** Where the back stack goes for a register's route: an account's, or All Accounts. */
private fun register(route: Route): String = "register/" + RouteArg.of(route.path())

/**
 * Where the back stack goes for a route this phone pushes: a register (a saved filter's is every
 * account's here), or Settings, which is where the banks are.
 */
private fun destination(route: Route): String =
    when (route) {
        is Route.Settings, is Route.Banks, is Route.Bank -> "settings"
        is Route.Account -> register(route)
        else -> register(Route.Accounts)
    }

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

/** A new transaction: today, in `account` or the usual one (the core's), money going out. */
fun newForm(l: Ledger, account: Id?): TxnForm = TxnForm(newDraft(l.accounts, account, l.today))

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
    // Under the shell's top bar (ArkUI's): the bar has the title, the actions and the add button.
    val hosted = dev.arkui.LocalUnderToolbar.current && onBack == null
    Scaffold(
        modifier = modifier.nestedScroll(bar.nestedScrollConnection),
        // The navigation bar below keeps the bottom clear; a pushed page has none, and keeps it itself.
        contentWindowInsets =
            if (onBack == null) WindowInsets.systemBars.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)
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
private fun Hosted(name: String, book: Book, ledger: Ledger, shell: Shell, nav: NavHostController, sections: MoneyShellHolder) {
    when (name) {
        "budget" -> BudgetScreen(ledger)
        "accounts" -> {
            // Written from Rust (generated/AccountsScreen.kt), over its runtime.
            val view = androidx.compose.ui.platform.LocalView.current
            val holder = remember(book) {
                AccountsHolder(book.session, { book.sync() }, { book.state.value.today }, PhoneHaptics(view), PhoneClipboard(view.context))
            }
            var reconciling by remember { mutableStateOf<Id?>(null) }
            holder.opened = { r -> if (r is Route.Banks) shell.onSettings() else nav.navigate(register(r)) }
            holder.reconciling = { reconciling = it }
            LaunchedEffect(ledger) { holder.reload() }
            AccountsScreen(holder)
            reconciling?.let { id ->
                ledger.accounts.firstOrNull { it.id == id }?.let { acc ->
                    ReconcileDialog(acc.cleared, acc.name, onReconcile = { book.reconcile(id, it); reconciling = null }, onDismiss = { reconciling = null })
                }
            }
        }
        "transactions" -> ActivityScreen(ledger)
        "insights" -> InsightsScreen(ledger)
        "accounts.register" -> {
            // The account the shell shows: its place's tag, `accounts/<id>`.
            val page by sections.page.collectAsStateWithLifecycle()
            val id = ledger.accounts.firstOrNull { "accounts/${it.id.hex}" == page.section }?.id
            if (id != null) RegisterScreen(ledger, id, onBack = { sections.send(uniffi.money_app.ShellMsg.Back) }) else Text("No Account Selected", modifier = Modifier.padding(16.dp))
        }
        // Who is signed in, at the drawer's foot (or the top bar's end): the account sheet.
        "household" -> androidx.compose.material3.TextButton(onClick = shell.onAccountSheet) { Text(ledger.who) }
        "settings.general" -> SettingsScreen(ledger, onBack = {})
        else -> Text("Nothing Selected", modifier = Modifier.padding(16.dp))
    }
}
