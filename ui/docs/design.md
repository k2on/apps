# Designing an Ark App

Every Ark app is designed the same way, in this order:

1. **As a standard macOS SwiftUI app**, the way Apple's multiplatform
   samples and the Human Interface Guidelines lay one out: its scenes, a
   sidebar of sections, a split view, a toolbar with semantic placements,
   an inspector, a Settings scene, Commands in the menu bar, search,
   context menus, sheets and alerts, a table that becomes a list on a
   narrow screen. Nothing about a phone first, and nothing from an app's
   earlier screens unless the design keeps it on its merits.
2. **In ark-app's Rust vocabulary** (`ui/rust/app`, ui/docs/app.md): one
   app description whose parts adapt themselves, as SwiftUI's do, so the
   app almost never says "if iPhone" or "if wider than…".
3. **Over its arkdb domain**: the records, the views the store keeps and
   the typed changes, plugged in underneath the screens' records.

The references are Apple's own:

- *Food Truck: Building a SwiftUI multiplatform app* (WWDC22) —
  <https://developer.apple.com/documentation/swiftui/food_truck_building_a_swiftui_multiplatform_app>:
  one `WindowGroup`, a `NavigationSplitView` whose sidebar is the app's
  sections, a `Table` on the Mac that is a `List` on the iPhone, toolbar
  items by placement, `Settings`.
- *Backyard Birds: Building an app with SwiftData and widgets* —
  <https://developer.apple.com/documentation/swiftui/backyard-birds-sample>:
  the same shape over a data layer, inspectors and sheets.
- The HIG: [Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars),
  [Tab bars](https://developer.apple.com/design/human-interface-guidelines/tab-bars),
  [Split views](https://developer.apple.com/design/human-interface-guidelines/split-views),
  [Toolbars](https://developer.apple.com/design/human-interface-guidelines/toolbars),
  [The menu bar](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar),
  [Context menus](https://developer.apple.com/design/human-interface-guidelines/context-menus),
  [Lists and tables](https://developer.apple.com/design/human-interface-guidelines/lists-and-tables),
  [Search fields](https://developer.apple.com/design/human-interface-guidelines/search-fields),
  [Sheets](https://developer.apple.com/design/human-interface-guidelines/sheets),
  [Alerts](https://developer.apple.com/design/human-interface-guidelines/alerts),
  [Settings](https://developer.apple.com/design/human-interface-guidelines/settings),
  [Keyboards](https://developer.apple.com/design/human-interface-guidelines/keyboards),
  [Layout](https://developer.apple.com/design/human-interface-guidelines/layout)
  (size classes).

## One Description, Parts That Adapt

The app says *what* it is — its sections, what each lists, what a row
opens, what the toolbar offers, what a row's menu offers, which keys do
what — once. The parts decide *how*, from the size class, the platform's
conventions and its input, in one place each (the width token is the
parts', never an app's: SwiftUI's compact and regular).

| SwiftUI (the design) | Compose (Android) | iced (desktop) | Leptos (the DOM) |
|---|---|---|---|
| `App` + `WindowGroup` | the activity's root | the main window (`arkui::window`) | the page |
| `Settings` scene | a settings screen | a second window | a route, shown as a dialog over the page |
| `Commands`, `.keyboardShortcut` | hardware-keyboard shortcuts; the app bar's overflow menu | key bindings (the vim layer and chords), an in-window menu (the command box, `arkui::command`) | key listeners on the document, the same command box |
| `TabView` + `.tabViewStyle(.sidebarAdaptable)` | `NavigationSuiteScaffold` (bar, rail, drawer by window size class) | a sidebar (`arkui::sidebar`), bottom tabs below the compact width | a sidebar, bottom tabs below the compact width |
| `NavigationSplitView(sidebar:content:detail:)`, a stack when compact | `ListDetailPaneScaffold` (material3-adaptive; hinge-aware panes on a foldable through Jetpack WindowManager) | panes side by side, one at a time when narrow | the same, in CSS grid |
| `NavigationStack` | `NavHost` | `arkui::navigator` | the router's history |
| `.toolbar` (`.primaryAction`, `.navigation`, `.principal`, `.secondaryAction`) | `TopAppBar` actions and title, overflow for secondary | a toolbar row over the content (`arkui::parts`) | a toolbar row |
| `.inspector` | a supporting pane when expanded, a bottom sheet when compact | a pane at the trailing edge | a pane at the trailing edge |
| `.searchable` | `SearchBar` | `parts::search_field` in the toolbar, `/` | a search field in the toolbar, `/` |
| `Table` (Mac, iPad regular) / `List` (compact) | a list (a table's columns folded into a row) | `arkui::table` / rows | a table / rows |
| `.contextMenu` | long press: `DropdownMenu`, lit (`Press.Highlight`) | right click, long press or the menu key: `arkui::context_menu` | `contextmenu` event: the same ArkUI menu in the DOM |
| `.sheet`, `.alert`, `.confirmationDialog` | `ModalBottomSheet`, `AlertDialog` | `arkui::sheet`, `arkui::dialog` | `<dialog>` |
| `.refreshable` | pull to refresh | ⌘R / the command | ⌘R / the command |
| `.swipeActions` | swipe to dismiss/reveal (`arkui` `swipe`) | right click (no swipe with a pointer) | right click |

Width checks live inside the parts that adapt (the tabs, the split view,
the inspector, the table); a foldable's posture (book, tabletop) reaches
only the Compose parts, through WindowManager. A phone that folds open to
a tablet's width (the rumoured iPhone Fold) needs no code: its size class
changes and the parts follow. There is no simulator for one; nothing was
tested.

## The Keyboard, for Every App

Every Ark app gets the same keyboard, from ArkUI, not from the app
(`ui/rust/arkui/src/vim.rs`, `which.rs`, `command.rs`; its rules were
money's and harken's desktop apps'): on the desktop, in the browser, on a
Mac and with a hardware keyboard on an iPad or an Android tablet.

| Keys | What |
|---|---|
| `j` / `k`, `↓` / `↑` | down / up the focused pane |
| `h` / `l` | left / right, or the pane that way (sidebar ↔ list ↔ detail) |
| `Tab` / `Shift-Tab` | the next / previous pane |
| `gg` / `G`, `{n}G` | first / last / the n-th row |
| `zz` / `zt` / `zb` | scroll: the cursor in the middle / at the top / at the foot |
| `Ctrl-d` / `Ctrl-u` | a page down / up |
| `Ctrl-o` / `Ctrl-i` | back / forward |
| `Enter`, `o` | open |
| `/`, `n` / `N` | search, next / previous match |
| `Shift-F10`, the menu key | the row's context menu |
| `:` or ⌘K | the command box: every command, by name |
| `?` (and a held prefix) | the keys panel (which-key): what can be pressed now |

The rows of a pane carry a focus the way a Mac list does: one row is
selected, the focus moves with the keys and the pointer, and a pane that
has the keyboard says so. An app adds its own commands (a month forward,
a transaction added) as `Commands`; the parts show them in the keys panel,
the command box and, on the Mac, the menu bar.

## Where Things Live

What does not depend on an app is in `ui/`: the parts, the shell's
structure (scenes, the adaptive tabs, the split view, the toolbar, the
inspector), the keyboard, the commands, the context menu, the theme.
What an app is about is in the app: its sections and screens, its records,
its domain. A part an app needs and lacks is added to ui/ first.

Each app's design is a short document under `ui/docs/apps/`:
`money.md` now; harken next (What Is Left, ui/docs/app.md).
