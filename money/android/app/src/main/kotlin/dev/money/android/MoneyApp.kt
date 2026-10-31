// The app, once there is a budget: four tabs in a navigation bar — Budget,
// Accounts, Transactions and Insights — each a page with a large top app bar
// that collapses as it scrolls, and the pages they open (an account's
// register, the bank's accounts) pushed over them. Navigation is
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

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.text.style.TextOverflow
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.arkdb.ffi.Id
import uniffi.money_domain.TxnLine

/** The four tabs. */
enum class Tab(val route: String, val label: String, val icon: ImageVector, val selected: ImageVector) {
    Budget("budget", "Budget", Glyphs.Budget, Glyphs.BudgetFilled),
    Accounts("accounts", "Accounts", Glyphs.Accounts, Glyphs.AccountsFilled),
    Activity("activity", "Transactions", Glyphs.Activity, Glyphs.ActivityFilled),
    Insights("insights", "Insights", Glyphs.Insights, Glyphs.InsightsFilled),
}

/** What a page can ask of the app: a transaction's form, a page, the snackbar. */
class Shell(
    val book: Book,
    val snackbar: SnackbarHostState,
    val onAdd: (account: Id?) -> Unit,
    val onEdit: (TxnLine) -> Unit,
    val onAccount: (Id) -> Unit,
    val onBanks: () -> Unit,
    val onTab: (Tab) -> Unit,
    val onSignOut: () -> Unit,
)

val LocalShell = staticCompositionLocalOf<Shell> { error("no shell") }

/** The budget, its tabs and its pages; `onSignOut` leaves it. */
@Composable
fun MoneyApp(book: Book, onSignOut: () -> Unit) {
    val nav = rememberNavController()
    val ledger by book.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    var form by remember { mutableStateOf<TxnForm?>(null) }
    LaunchedEffect(book) { book.notes.collect { snackbar.showSnackbar(it) } }

    val shell =
        remember(book, nav) {
            Shell(
                book = book,
                snackbar = snackbar,
                onAdd = { account -> form = newForm(book.state.value, account) },
                onEdit = { t -> form = formOf(t, book.state.value) },
                onAccount = { id -> nav.navigate("register/${id.hex}") },
                onBanks = { nav.navigate("banks") },
                onTab = { t -> nav.go(t) },
                onSignOut = onSignOut,
            )
        }

    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route
    val tab = Tab.entries.firstOrNull { it.route == route }

    CompositionLocalProvider(LocalShell provides shell) {
        Scaffold(
            contentWindowInsets = WindowInsets(0),
            bottomBar = {
                AnimatedVisibility(
                    visible = tab != null || route == null,
                    enter = slideInVertically { it } + fadeIn(),
                    exit = slideOutVertically { it } + fadeOut()) {
                        NavigationBar {
                            for (t in Tab.entries) {
                                val on = t == tab
                                NavigationBarItem(
                                    selected = on,
                                    onClick = { nav.go(t) },
                                    icon = {
                                        val count = if (t == Tab.Activity) ledger.inbox.size else 0
                                        BadgedBox(badge = { if (count > 0) Badge { Text("$count") } }) {
                                            Icon(if (on) t.selected else t.icon, contentDescription = null)
                                        }
                                    },
                                    label = { Text(t.label) },
                                )
                            }
                        }
                    }
            },
        ) { pad ->
            NavHost(
                nav,
                startDestination = Tab.Budget.route,
                modifier = Modifier.padding(bottom = pad.calculateBottomPadding()),
                enterTransition = { enter() },
                exitTransition = { exit() },
                popEnterTransition = { popEnter() },
                popExitTransition = { popExit() },
            ) {
                composable(Tab.Budget.route) { BudgetScreen(ledger) }
                composable(Tab.Accounts.route) { AccountsScreen(ledger) }
                composable(Tab.Activity.route) { ActivityScreen(ledger) }
                composable(Tab.Insights.route) { InsightsScreen(ledger) }
                composable("register/{account}") { e ->
                    val id = Id(e.arguments?.getString("account").orEmpty())
                    RegisterScreen(ledger, id, onBack = { nav.popBackStack() })
                }
                composable("banks") { BanksScreen(ledger, onBack = { nav.popBackStack() }) }
            }
        }
        form?.let { f -> TxnSheet(ledger, f, onDismiss = { form = null }) }
    }
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

// Between tabs, Material's fade through: the old page fades out quickly, the new one fades in
// and grows the last few percent. Into a page and back out, the shared X axis: a short slide
// with a fade, which the back gesture drives in reverse.
private fun AnimatedContentTransitionScope<NavBackStackEntry>.enter(): EnterTransition =
    if (isTab(initialState) && isTab(targetState)) {
        fadeIn(tween(210, delayMillis = 90, easing = LinearOutSlowInEasing)) +
            scaleIn(tween(210, delayMillis = 90, easing = LinearOutSlowInEasing), initialScale = 0.92f)
    } else {
        slideInHorizontally(tween(300, easing = FastOutSlowInEasing)) { it / 5 } + fadeIn(tween(300))
    }

private fun AnimatedContentTransitionScope<NavBackStackEntry>.exit(): ExitTransition =
    if (isTab(initialState) && isTab(targetState)) fadeOut(tween(90))
    else slideOutHorizontally(tween(300, easing = FastOutSlowInEasing)) { -it / 5 } + fadeOut(tween(300))

private fun AnimatedContentTransitionScope<NavBackStackEntry>.popEnter(): EnterTransition =
    slideInHorizontally(tween(300, easing = FastOutSlowInEasing)) { -it / 5 } + fadeIn(tween(300))

private fun AnimatedContentTransitionScope<NavBackStackEntry>.popExit(): ExitTransition =
    slideOutHorizontally(tween(300, easing = FastOutSlowInEasing)) { it / 5 } + fadeOut(tween(300))

/** A new transaction: today, in `account` or the first open budget account, money going out. */
fun newForm(l: Ledger, account: Id?): TxnForm =
    TxnForm(
        account = account ?: l.accounts.firstOrNull { it.onBudget && !it.closed }?.id,
        date = l.today,
    )

/** A transaction as the form corrects it. */
fun formOf(t: TxnLine, l: Ledger): TxnForm =
    TxnForm(
        editing = t.id,
        account = t.accountId,
        date = t.date,
        payee = if (t.transfer) "" else t.payee,
        category = t.categoryId,
        transferTo = if (t.transfer) l.accounts.firstOrNull { "Transfer : ${it.name}" == t.payee }?.id else null,
        memo = t.memo,
        amount = t.amount,
        cleared = t.cleared != uniffi.money_domain.Cleared.UNCLEARED,
        by = t.userId,
        bankSaid = bankSaid(t),
    )

/**
 * The bank's own description of `t` when it was imported and that says something the payee does not;
 * otherwise empty.
 */
fun bankSaid(t: TxnLine): String {
    val said = t.importedPayee.trim()
    return if (t.imported && said.isNotEmpty() && !said.equals(t.payee.trim(), ignoreCase = true)) said else ""
}

/**
 * A page as every screen here is laid out: a large top app bar that collapses as the content
 * scrolls, its `actions`, a back arrow when `onBack` is given, the FAB, the snackbar, and pull to
 * refresh asking the bank for anything new. `content` gets the padding to keep clear of the bar.
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
    Scaffold(
        modifier = modifier.nestedScroll(bar.nestedScrollConnection),
        // The navigation bar below keeps the bottom clear; a pushed page has none, and keeps it itself.
        contentWindowInsets =
            if (onBack == null) WindowInsets.systemBars.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)
            else WindowInsets.systemBars,
        topBar = {
            LargeTopAppBar(
                title = { titleContent?.invoke() ?: Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                        }
                    }
                },
                actions = actions,
                scrollBehavior = bar,
                colors = TopAppBarDefaults.largeTopAppBarColors(scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer),
            )
        },
        floatingActionButton = fab,
        snackbarHost = { SnackbarHost(shell.snackbar) },
    ) { pad ->
        if (refresh) {
            PullToRefreshBox(
                isRefreshing = ledger.syncing,
                onRefresh = { shell.book.sync() },
                modifier = Modifier.fillMaxSize().padding(top = pad.calculateTopPadding()),
            ) {
                content(PaddingValues(bottom = pad.calculateBottomPadding()), bar)
            }
        } else {
            Box(Modifier.fillMaxSize().padding(top = pad.calculateTopPadding())) {
                content(PaddingValues(bottom = pad.calculateBottomPadding()), bar)
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
