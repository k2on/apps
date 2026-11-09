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
import androidx.compose.material3.SearchBarDefaults
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

private val LocalSizeClass = compositionLocalOf { SizeClass.Compact }

// * The sections

/** One section, as `SectionTab` registers it. */
public class SectionTabItem internal constructor(
    public val title: String,
    public val glyph: String,
    public val tag: String,
    public val badge: Long?,
    public val content: @Composable () -> Unit,
)

/** What `AdaptiveTabs`' sections are said in. */
public class TabsScope internal constructor() {
    internal val tabs = mutableListOf<SectionTabItem>()
}

/** One section: its title and glyph, its tag, what waits there, what it shows. */
public fun TabsScope.SectionTab(title: String, glyph: String, tag: String, badge: Long? = null, content: @Composable () -> Unit) {
    tabs += SectionTabItem(title, glyph, tag, badge, content)
}

/**
 * The app's sections: a navigation bar when compact, a rail when medium, a
 * permanent drawer when expanded (NavigationSuiteScaffold's choice, by the
 * theme's widths).
 */
@Composable
public fun AdaptiveTabs(selection: String, onSelect: (String) -> Unit, tabs: TabsScope.() -> Unit) {
    val items = TabsScope().apply(tabs).tabs
    val lookup = LocalGlyphs.current
    val shown = items.firstOrNull { it.tag == selection } ?: items.firstOrNull() ?: return
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val size = sizeClassOf(maxWidth)
        val icon: @Composable (SectionTabItem) -> Unit = { t ->
            BadgedBox(badge = { t.badge?.let { Badge { Text(it.toString()) } } }) {
                Icon(shellGlyph(t.glyph, lookup), contentDescription = null)
            }
        }
        CompositionLocalProvider(LocalSizeClass provides size) {
            when (size) {
                SizeClass.Compact -> Scaffold(bottomBar = {
                    NavigationBar {
                        items.forEach { t ->
                            NavigationBarItem(selected = t.tag == shown.tag, onClick = { onSelect(t.tag) }, icon = { icon(t) }, label = { Text(t.title) })
                        }
                    }
                }) { pad -> Box(Modifier.padding(pad)) { shown.content() } }
                SizeClass.Medium -> Row(Modifier.fillMaxSize()) {
                    NavigationRail {
                        items.forEach { t ->
                            NavigationRailItem(selected = t.tag == shown.tag, onClick = { onSelect(t.tag) }, icon = { icon(t) }, label = { Text(t.title) })
                        }
                    }
                    Box(Modifier.weight(1f)) { shown.content() }
                }
                SizeClass.Expanded -> PermanentNavigationDrawer(drawerContent = {
                    PermanentDrawerSheet(Modifier.width(LayoutTokens.SIDEBAR_WIDTH.dp)) {
                        items.forEach { t ->
                            NavigationDrawerItem(
                                label = { Text(t.title) },
                                icon = { icon(t) },
                                selected = t.tag == shown.tag,
                                onClick = { onSelect(t.tag) },
                                modifier = Modifier.padding(horizontal = 12.dp),
                            )
                        }
                    }
                }) { shown.content() }
            }
        }
    }
}

// * Columns

/**
 * A list beside what it opens: side by side when expanded (the list
 * `LayoutTokens.CONTENT_WIDTH` wide), one at a time otherwise, the detail
 * shown while `showing` and back sending `onBack`.
 */
@Composable
public fun SplitColumns(showing: Boolean, onBack: () -> Unit, detail: @Composable () -> Unit, content: @Composable () -> Unit) {
    if (LocalSizeClass.current == SizeClass.Expanded) {
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
            navigationIcon = { all.filter { it.placement == BarPlacement.Navigation }.forEach { button(it) } },
            actions = {
                all.filter { it.placement == BarPlacement.Primary }.forEach { button(it) }
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
    }) { pad -> Box(Modifier.padding(pad)) { content() } }
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
    if (LocalSizeClass.current == SizeClass.Expanded) {
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f)) { content() }
            if (isShown) {
                VerticalDivider()
                Column(Modifier.width(LayoutTokens.INSPECTOR_WIDTH.dp).fillMaxHeight()) {
                    Text(title, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(16.dp))
                    HorizontalDivider()
                    inspector()
                }
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

/** Settings: its panes, one after another (Android's Settings screen). */
@Composable
public fun SettingsPanes(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize()) { content() }
}

/** One pane of Settings: its heading over what it holds. */
@Composable
public fun SettingsPane(title: String, glyph: String, content: @Composable () -> Unit) {
    Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(16.dp))
    content()
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
