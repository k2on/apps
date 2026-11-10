// The shell's parts (ark_app::shell, ui/docs/design.md): each has the name
// and inputs its Rust builder declares (`Part::call`), so a generated shell
// (`MoneyShell`, `MoneySettings`, `moneyCommands`) is only calls to these.
// Material's adaptive shape, decided here by the window's width against
// the theme's size classes (`LayoutTokens`, ui/theme.toml `[layout]`) and
// nowhere in an app: a navigation bar, a rail or a drawer for the sections
// (what NavigationSuiteScaffold picks), a list and its detail side by side
// or one at a time (ListDetailPaneScaffold's panes), a top app bar with its
// actions and its overflow, a search bar, the inspector a supporting pane
// or a bottom sheet, a hardware keyboard's shortcuts. What a section hosts
// is the app's (`LocalHostedScreens`), set once at its root.
//
// Built on material3 alone: material3-adaptive and Jetpack WindowManager
// (the foldable's posture and hinge) wait for the Gradle graph
// (`deps.json`) to be regenerated with them (ui/docs/app.md, What Is Left).
package dev.arkui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.PermanentDrawerSheet
import androidx.compose.material3.PermanentNavigationDrawer
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SearchBar
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

// * What the app provides

/** The screens a shell hosts, by name: the app's own, until they move into Rust. */
public val LocalHostedScreens = compositionLocalOf<@Composable (String) -> Unit> { { } }

/** A screen the app draws itself, by name. */
@Composable
public fun HostedScreen(name: String) {
    LocalHostedScreens.current(name)
}

private fun shellGlyph(name: String, lookup: (String) -> ImageVector?): ImageVector =
    lookup(name) ?: ArkGlyphs.named(name) ?: ArkGlyphs.Chevron

// * The size classes

/** How wide a window is, as the parts that adapt read it (ui/theme.toml `[layout]`). */
public enum class SizeClass { Compact, Medium, Expanded }

public fun sizeClassOf(width: Dp): SizeClass = when {
    width < LayoutTokens.COMPACT_WIDTH.dp -> SizeClass.Compact
    width < LayoutTokens.EXPANDED_WIDTH.dp -> SizeClass.Medium
    else -> SizeClass.Expanded
}

/** The size class the shell measured its window at; none outside a shell. */
internal val LocalSizeClass = compositionLocalOf<SizeClass?> { null }

/**
 * The size class a part adapts to: the shell's, or, drawn outside one (a
 * screen on its own, a screenshot's), the window's own width's — so a
 * tablet's Budget has its columns wherever it is drawn.
 */
@Composable
public fun currentSizeClass(): SizeClass =
    LocalSizeClass.current ?: sizeClassOf(androidx.compose.ui.platform.LocalConfiguration.current.screenWidthDp.dp)

// * The sections

/** One section, as `SectionTab` registers it: in a group (`SectionGroup`) when `group` is said. */
public class SectionTabItem internal constructor(
    public val title: String,
    public val glyph: String,
    public val tag: String,
    public val badge: Long?,
    public val group: String?,
    public val content: @Composable () -> Unit,
)

/** What `AdaptiveTabs`' sections are said in. */
public class TabsScope internal constructor() {
    internal val tabs = mutableListOf<SectionTabItem>()
    internal var group: String? = null
}

/** One section: its title and glyph, its tag, what waits there, what it shows. */
public fun TabsScope.SectionTab(title: String, glyph: String, tag: String, badge: Long? = null, content: @Composable () -> Unit) {
    tabs += SectionTabItem(title, glyph, tag, badge, group, content)
}

/**
 * Sections under a heading in the drawer alone (Mail's mailboxes): never on
 * the navigation bar or the rail. A child's tag is `parent/child`.
 */
public fun TabsScope.SectionGroup(title: String, tabs: TabsScope.() -> Unit) {
    val was = group
    group = title
    tabs()
    group = was
}

/** The section a tag is shown in when there is no drawer: a child's parent. */
public fun compactTag(tag: String): String = tag.substringBefore('/')

/**
 * The app's sections: a navigation bar when compact, a rail when medium, a
 * permanent drawer when expanded (NavigationSuiteScaffold's choice, by the
 * theme's widths).
 */
@Composable
public fun AdaptiveTabs(selection: String, onSelect: (String) -> Unit, footer: @Composable () -> Unit = {}, tabs: TabsScope.() -> Unit) {
    val items = TabsScope().apply(tabs).tabs
    val lookup = LocalGlyphs.current
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val size = sizeClassOf(maxWidth)
        val drawer = maxWidth >= LayoutTokens.SIDEBAR_MIN_WIDTH.dp
        // Short of a drawer, a child is shown in its parent's tab, which pushes it.
        val shownTag = if (drawer) selection else compactTag(selection)
        val shown = items.firstOrNull { it.tag == shownTag } ?: items.firstOrNull() ?: return@BoxWithConstraints
        val sections = items.filter { it.group == null }
        val icon: @Composable (SectionTabItem) -> Unit = { t ->
            BadgedBox(badge = { t.badge?.let { Badge { Text(it.toString()) } } }) {
                Icon(shellGlyph(t.glyph, lookup), contentDescription = null)
            }
        }
        CompositionLocalProvider(LocalSizeClass provides size, LocalShellFooter provides if (drawer) null else footer) {
            when {
                size == SizeClass.Compact -> Scaffold(bottomBar = {
                    NavigationBar {
                        sections.forEach { t ->
                            NavigationBarItem(selected = t.tag == shown.tag, onClick = { onSelect(t.tag) }, icon = { icon(t) }, label = { Text(t.title) })
                        }
                    }
                }) { pad -> Box(Modifier.padding(pad)) { shown.content() } }
                !drawer -> Row(Modifier.fillMaxSize()) {
                    NavigationRail {
                        sections.forEach { t ->
                            NavigationRailItem(selected = t.tag == shown.tag, onClick = { onSelect(t.tag) }, icon = { icon(t) }, label = { Text(t.title) })
                        }
                    }
                    Box(Modifier.weight(1f)) { shown.content() }
                }
                else -> PermanentNavigationDrawer(drawerContent = {
                    PermanentDrawerSheet(Modifier.width(LayoutTokens.SIDEBAR_WIDTH.dp)) {
                        Column(Modifier.weight(1f)) {
                            var heading: String? = null
                            items.forEach { t ->
                                if (t.group != null && t.group != heading) {
                                    Text(
                                        t.group,
                                        style = MaterialTheme.typography.labelMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.padding(start = 28.dp, top = 16.dp, bottom = 4.dp),
                                    )
                                }
                                heading = t.group
                                NavigationDrawerItem(
                                    label = { Text(t.title) },
                                    icon = { Icon(shellGlyph(t.glyph, lookup), contentDescription = null) },
                                    badge = { t.badge?.let { Text(it.toString()) } },
                                    selected = t.tag == shown.tag,
                                    onClick = { onSelect(t.tag) },
                                    modifier = Modifier.padding(horizontal = 12.dp),
                                )
                            }
                        }
                        Box(Modifier.padding(12.dp)) { footer() }
                    }
                }) { shown.content() }
            }
        }
    }
}

/** The drawer's foot, which a top app bar carries at its end where there is no drawer. */
private val LocalShellFooter = compositionLocalOf<(@Composable () -> Unit)?> { null }

// * Columns

/**
 * A list beside what it opens: side by side when expanded (the list
 * `LayoutTokens.CONTENT_WIDTH` wide), one at a time otherwise, the detail
 * shown while `showing` and back sending `onBack`.
 */
@Composable
public fun SplitColumns(showing: Boolean, onBack: () -> Unit, detail: @Composable () -> Unit, content: @Composable () -> Unit) {
    if (currentSizeClass() == SizeClass.Expanded) {
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.width(LayoutTokens.CONTENT_WIDTH.dp).fillMaxHeight()) { content() }
            VerticalDivider()
            Box(Modifier.weight(1f).fillMaxHeight()) { detail() }
        }
    } else if (showing) {
        BackHandler(onBack = onBack)
        detail()
    } else {
        content()
    }
}

/** What a list opens, pushed over it while `showing`, at every width; back sends `onBack`. */
@Composable
public fun PushedDetail(showing: Boolean, onBack: () -> Unit, detail: @Composable () -> Unit, content: @Composable () -> Unit) {
    if (showing) {
        BackHandler(onBack = onBack)
        detail()
    } else {
        content()
    }
}

// * The toolbar

/** Where a toolbar's button goes. */
public enum class BarPlacement { Navigation, Principal, Primary, Secondary }

/** A toolbar's button, as `BarButton` registers it. */
public class BarItem internal constructor(
    public val title: String,
    public val glyph: String,
    public val placement: BarPlacement,
    public val enabled: Boolean,
    public val onTap: () -> Unit,
)

/** What a `WithToolbar`'s items are said in. */
public class ToolbarScope internal constructor() {
    internal val items = mutableListOf<BarItem>()
}

public fun ToolbarScope.BarButton(title: String, glyph: String, placement: BarPlacement, enabled: Boolean, onTap: () -> Unit) {
    items += BarItem(title, glyph, placement, enabled, onTap)
}

/**
 * A section's top app bar over its content: the navigation item at its
 * start, the title (or the principal items), the primary items as its
 * actions, the secondary ones in its overflow menu.
 */
/**
 * Whether a screen is under the shell's top bar (`WithToolbar`): its own large title, its month
 * pager and its add button are the bar's there, and it draws none of them.
 */
public val LocalUnderToolbar = compositionLocalOf { false }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun WithToolbar(title: String, items: ToolbarScope.() -> Unit, content: @Composable () -> Unit) {
    val all = ToolbarScope().apply(items).items
    val lookup = LocalGlyphs.current
    var overflow by remember { mutableStateOf(false) }
    val button: @Composable (BarItem) -> Unit = { b ->
        IconButton(onClick = b.onTap, enabled = b.enabled) { Icon(shellGlyph(b.glyph, lookup), contentDescription = b.title) }
    }
    Scaffold(topBar = {
        TopAppBar(
            title = {
                val principal = all.filter { it.placement == BarPlacement.Principal }
                if (principal.isEmpty()) {
                    Text(title, fontWeight = FontWeight.SemiBold)
                } else {
                    Row { principal.forEach { button(it) } }
                }
            },
            // Side by side: the slot is a box, where two would lie one over the other.
            navigationIcon = { Row { all.filter { it.placement == BarPlacement.Navigation }.forEach { button(it) } } },
            actions = {
                all.filter { it.placement == BarPlacement.Primary }.forEach { button(it) }
                LocalInspectorToggle.current?.let { t ->
                    IconButton(onClick = t.second) { Icon(ArkGlyphs.PanelRight, contentDescription = t.first) }
                }
                LocalShellFooter.current?.invoke()
                val secondary = all.filter { it.placement == BarPlacement.Secondary }
                if (secondary.isNotEmpty()) {
                    IconButton(onClick = { overflow = true }) { Icon(ArkGlyphs.named("ellipsis-vertical") ?: ArkGlyphs.Chevron, contentDescription = "More") }
                    DropdownMenu(expanded = overflow, onDismissRequest = { overflow = false }) {
                        secondary.forEach { b ->
                            DropdownMenuItem(text = { Text(b.title) }, enabled = b.enabled, onClick = {
                                overflow = false
                                b.onTap()
                            })
                        }
                    }
                }
            },
        )
    }) { pad -> Box(Modifier.padding(pad)) { CompositionLocalProvider(LocalUnderToolbar provides true) { content() } } }
}

// * Search and the inspector

/** `content` under a search bar: what is typed sent as it changes. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun WithSearch(prompt: String, text: String, onChange: (String) -> Unit, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        SearchBar(
            inputField = {
                SearchBarDefaults.InputField(
                    query = text,
                    onQueryChange = onChange,
                    onSearch = onChange,
                    expanded = false,
                    onExpandedChange = {},
                    placeholder = { Text(prompt) },
                    leadingIcon = { Icon(ArkGlyphs.named("search") ?: ArkGlyphs.Chevron, contentDescription = null) },
                )
            },
            expanded = false,
            onExpandedChange = {},
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {}
        Box(Modifier.weight(1f)) { content() }
    }
}

/**
 * `content` with an inspector: a supporting pane at the trailing edge when
 * expanded, a bottom sheet otherwise; shown while `isShown`.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun WithInspector(title: String, isShown: Boolean, onToggle: (Boolean) -> Unit, inspector: @Composable () -> Unit, content: @Composable () -> Unit) {
    // Its toggle goes in the section's top app bar (`WithToolbar`'s actions).
    CompositionLocalProvider(LocalInspectorToggle provides (title to { onToggle(!isShown) })) {
        if (currentSizeClass() == SizeClass.Expanded) {
            Row(Modifier.fillMaxSize()) {
                Box(Modifier.weight(1f)) { content() }
                if (isShown) {
                    VerticalDivider()
                    Column(Modifier.width(LayoutTokens.INSPECTOR_WIDTH.dp).fillMaxHeight()) { inspector() }
                }
            }
        } else {
            content()
            if (isShown) {
                ModalBottomSheet(onDismissRequest = { onToggle(false) }) {
                    Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 16.dp))
                    inspector()
                }
            }
        }
    }
}

/** The inspector's title and its toggle, for the top app bar over it. */
internal val LocalInspectorToggle = compositionLocalOf<Pair<String, () -> Unit>?> { null }

// * A question and a notice

/** One action of a `ConfirmDialog`. */
public class DialogAction(public val title: String, public val destructive: Boolean, public val onTap: () -> Unit)

/** A question before something that cannot be undone: up while it is composed. */
@Composable
public fun ConfirmDialog(title: String, message: String, actions: List<DialogAction>, onCancel: () -> Unit) {
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(title) },
        text = { Text(message) },
        confirmButton = {
            Row {
                actions.forEach { a ->
                    TextButton(onClick = a.onTap) {
                        Text(a.title, color = if (a.destructive) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
                    }
                }
            }
        },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}

/** Something the person has to know, and OK: up while it is composed. */
@Composable
public fun NoticeAlert(title: String, message: String, onOk: () -> Unit) {
    AlertDialog(
        onDismissRequest = onOk,
        title = { Text(title) },
        text = { Text(message) },
        confirmButton = { TextButton(onClick = onOk) { Text("OK") } },
    )
}

// * Settings

/** One pane of Settings, as `SettingsPane` registers it. */
public class SettingsPaneItem internal constructor(public val title: String, public val glyph: String, public val content: @Composable () -> Unit)

/** What `SettingsPanes`' panes are said in. */
public class PanesScope internal constructor() {
    internal val panes = mutableListOf<SettingsPaneItem>()
}

/** One pane of Settings: its title, its glyph, what it holds. */
public fun PanesScope.SettingsPane(title: String, glyph: String, content: @Composable () -> Unit) {
    panes += SettingsPaneItem(title, glyph, content)
}

/**
 * Settings: its panes' tabs across the top, the one titled `selection`
 * under them (choosing another sends `onSelect`).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun SettingsPanes(selection: String, onSelect: (String) -> Unit, panes: PanesScope.() -> Unit) {
    val all = PanesScope().apply(panes).panes
    val lookup = LocalGlyphs.current
    val shown = all.firstOrNull { it.title == selection } ?: all.firstOrNull() ?: return
    Column(Modifier.fillMaxSize()) {
        PrimaryTabRow(selectedTabIndex = all.indexOf(shown)) {
            all.forEach { p ->
                Tab(
                    selected = p == shown,
                    onClick = { onSelect(p.title) },
                    text = { Text(p.title) },
                    icon = { Icon(shellGlyph(p.glyph, lookup), contentDescription = null) },
                )
            }
        }
        Box(Modifier.weight(1f)) { shown.content() }
    }
}

// * Commands

/** Where a menu of commands goes: under its heading in the app bar's menu. */
public enum class MenuPlace { File, View, Go }

/** One command: its title, its key with Ctrl (or ⌘), and Shift or Alt with it. */
public class ShellCommand(
    public val title: String,
    public val key: String? = null,
    public val shift: Boolean = false,
    public val option: Boolean = false,
    public val run: () -> Unit,
)

/** A menu of commands. */
public class ShellMenu(public val place: MenuPlace, public val commands: List<ShellCommand>)

/**
 * The commands' shortcuts on a hardware keyboard (a tablet's, a Chromebook's,
 * a phone's on a desk): Ctrl or Meta and the key, Shift and Alt as said.
 */
public fun Modifier.shellShortcuts(menus: List<ShellMenu>): Modifier = onPreviewKeyEvent { e ->
    if (e.type != KeyEventType.KeyDown || !(e.isCtrlPressed || e.isMetaPressed)) return@onPreviewKeyEvent false
    val typed = when (e.key) {
        Key.LeftBracket -> "["
        Key.RightBracket -> "]"
        else -> e.utf16CodePoint.takeIf { it > 0 }?.toChar()?.lowercase()
    } ?: return@onPreviewKeyEvent false
    val c = menus.flatMap { it.commands }.firstOrNull {
        it.key.equals(typed, ignoreCase = true) && it.shift == e.isShiftPressed && it.option == e.isAltPressed
    } ?: return@onPreviewKeyEvent false
    c.run()
    true
}
