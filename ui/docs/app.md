# The Whole App in Rust

An Ark app's state, its rules and its screens' layout are written once, in
Rust; every platform draws them natively — SwiftUI on the iPhone, Compose
on Android, iced on the desktop, the real DOM in the browser. Think
multiplatform SwiftUI or Expo, with Rust as the source and nothing
self-drawn or in a web view on a phone.

This is a prototype on `claude/sleepy-faraday-lcbow5-app`. One vertical
slice is moved — money's **Accounts** tab — and everything else is as it
was. How an app is designed before any of this (Mac-first, Apple's
multiplatform samples and the HIG) is ui/docs/design.md; money's design
is ui/docs/apps/money.md. The last section says what is left and in what
order.

## The Layers

iced is the desktop's alone. The browser is drawn by Leptos over the real
DOM (`ui/rust/web`, crate `arkui-web`; money's client is `money/web`);
money's and harken's iced builds for wasm32 are gone, and harken's Pages
demo with them until its Leptos client exists. What has no UI stays wasm-capable for it: ark-peer, ark-auth's
`web`, ark-format, ark-router, ark-app and the modules' `web` impls.

```
 money/app            the app: State, Msg, update, screen records, #[view] layouts
   └ ark-app          the runtime (App, Runtime, Host, Command) and the view vocabulary (ui/rust/app)
   └ ark-module       the device's plumbing (ui/rust/module)
   └ ark-<name>       a device module each: modules/haptics, clipboard, share, camera, …
 ui/rust/arkui        iced: draws a View (feature `app`) — the desktop
 ui/swift             SwiftUI: the parts a generated view calls, by hand
 ui/kotlin            Compose: the same parts, by hand
 ui/rust/gen          writes SwiftUI and Compose from a recorded view (composition only)
 ui/rust/arkui-macros #[view]
```

### State and Logic: `ark-app`

An app is Elm-shaped (`ark_app::App`):

- `State` — what the screen is in the middle of (a form being filled, a
  row being renamed). The domain's rows are **not** copied into it: they
  stay in the view store (`ark_peer::store::Store`), which every shell
  already holds, and the app names the views it needs (`App::holds`).
- `Msg` — everything the screen can be told. Ids travel as `Id::hex`, as
  routes carry them.
- `update(&mut State, Msg, &Data) -> Vec<Command>` — decides everything.
  A `Command` is data: `Send` a typed change (`MoneyChange`), `Open` a
  `Route`, `Ask` a module, `Then` another message, `Native` (an escape
  hatch, below). What became of a change or a module call comes back as a
  message when the update asked for one (`then: fn(..) -> Msg`, a plain
  function pointer, never a closure).
- `screen(&State, &Data) -> Screen` — the record the screen draws, every
  figure and word worked out (Crux's view model): `AccountsPage`.

`Runtime<A>` owns the loop: a message in, the state updated, the commands
carried out by the shell's `Host`, their messages run in turn, and a
`version` that grows — the number a native shell watches, as it watches
the store's. The runtime has no thread or clock of its own (the host says
the time: the browser has no clock `std` can read), so the same code runs
behind the phones' bridge, in the iced window and in wasm.

A `Host` is each shell's: `money/ffi/src/app.rs` (the phones: the bridge's
`Session`, an `AppShell` foreign trait for routes and the time),
`money/iced/src/overview.rs` (the window: its own store and changes, iced's
clipboard), and the tests' (`money/app/src/accounts/tests.rs`, over the
demo household and the modules' fakes).

### Layout: `#[view]`

A view is a function of the screen's record that returns a `View`, built
from ArkUI's part builders, SwiftUI's way:

```rust
#[view]
pub fn accounts(p: &AccountsPage) -> View<AccountsMsg> {
    tab_page(
        "Accounts",
        (
            card_section(list_row("All Accounts").tile("layers").amount(p.all_total).quiet().on_tap(AccountsMsg::OpenAll)),
            for shelf in p.shelves {
                card_section(for a in shelf.accounts {
                    account_row(a)
                })
                .header(shelf.title)
                .amount(shelf.total)
                .shows_amount(shelf.shows_total)
            },
            if let Some(form) = p.adding {
                sheet("Add Account", AccountsMsg::DismissDraft, (
                    name_field("Name", form.name, form.name_problem, |name| AccountsMsg::DraftName { name }),
                    form_button("Add Account", form.can_save, AccountsMsg::SaveDraft),
                ))
            },
        ),
    )
    .top(hero_figure("Net Worth", p.net_worth, "Every open account, less what's owed"))
    .refresh(AccountsMsg::Refresh)
    .into()
}
```

`#[view]` makes the function generic over a `Backend` the author never
names and writes its entry points beside it, in a module of the same name:

| Entry point | Backend | What it is |
|---|---|---|
| `accounts::view(&page)` | `Real` | the `View` of this record: `for` makes a row per item, `if` takes its branch |
| `accounts::iced(&page)` | `Real`, drawn by `arkui::view` | an `Element<'static, Msg>` (crate feature `iced`) |
| `accounts::web(&page)` | `Real`, drawn by `arkui-web` | Leptos over the DOM (feature `web`) |
| `accounts::record()` | `Recorder` | the view over paths: `for` walked once with a symbolic item, `if` both ways |

What it rewrites, and only that: a read of the record's fields (`p.f.g`,
including `for`/`if let` bindings), `for x in p.list`, `if p.flag`, `if let
Some(x) = p.opt`, a message built from fields (`OpenAccount { id: a.id }`,
read through a probe that the recorder marks with the field's path), and a
sub-view handed a binding (`account_row(a)` → `account_row::<B>(a)`).
Anything else over the record is a compile error that says what to do:
a method called on it ("put it in the screen record"), `match` over it
(not supported yet), a condition that is not one of its flags, `while`, a
destructuring pattern. Logic belongs in `screen()`, which is ordinary Rust
and tested; the view only lays out.

iced and the browser are Rust: they run the view function directly. Only
the phones get generated code.

### The Phones' Code: `ark-ui-gen`

`nix run .#update-ui` runs `money-ui` (money/app, feature `gen`), which
records each view and writes:

- `money/ios/Money/Generated/AccountsView.swift`
- `money/android/app/src/main/kotlin/dev/money/android/generated/AccountsScreen.kt`

`checks.ui` fails when what is committed is not what the views make.
Never edit those files.

**The emitters write composition and nothing else.** Every part a view
uses is written by hand in a real `.swift` / `.kt` file
(`ui/swift/Sources/ArkUI/ScreenParts.swift`,
`ui/kotlin/src/dev/arkui/ScreenParts.kt`), where Xcode and Android
Studio preview and check it. A part's name and signature are declared once
in Rust, next to the part (`ark_app::view::Part::call`): the Rust builder
`list_row` is `ListRow` in both languages, its first input unlabelled,
the rest labelled (`subtitle:`, `subtitle =`). An emitter knows only that
grammar — a struct or `@Composable` reading the record from its holder,
calls by name, `ForEach`/`for`, `if`, `if let`/`?.let`, and actions that
send a message (`{ screen.send(.openAccount(id: a.id)) }`, `{
screen.send(AccountsMsg.OpenAccount(id = a.id)) }`). A part that needs
something non-trivial gets it in its hand-written file, never in the
emitter. One small file per language: `ui/rust/gen/src/swift.rs`,
`kotlin.rs`.

The generated view reads its record from a hand-written holder over the
runtime (`AccountsScreen` in `money/ios/Money/Screens.swift`,
`AccountsHolder` in `money/android/.../Screens.kt`): an
`ObservableObject` / `StateFlow` of the record, read again after every
message and whenever the store moved.

### The Device: `modules/`

What an app asks of the device is a module, Expo's way: one package each
under `modules/`, and a small core under `ui/rust/module`.

```
ui/rust/module/       crate ark-module: ModuleCall, ModuleAnswer, ModuleReply, Modules — the plumbing, no module
modules/<name>/
├── Cargo.toml        crate ark-<name>: the trait, its calls, its fake; desktop/web impls behind features
├── src/
├── swift/            the iPhone's and the Mac's implementation (compiled into the app, not a package)
├── kotlin/           Android's implementation (compiled into the app)
├── module.toml       its crate, and what it asks of each platform (Info.plist keys, permissions)
└── README.md         its API, what it does on each platform, the permissions it needs
```

An update never calls a module. It returns `Command::ask(call)`, where
`call` is a `ModuleCall` the module's crate makes (`ark_haptics::perform(
Haptic::Success)`, `ark_clipboard::copy(text)`, `ark_camera::scan_qr()`):
the module's own enum (`HapticsCall`, …) implementing `ark_module::Call`,
which names the trait that carries it out, erased into one type the
runtime holds, compares and prints. The host hands it to the `Modules` it
registered (`Inbox::ask`), the registry finds the module by its trait
(`register::<dyn Haptics>(…)`), and the `ModuleAnswer` — `Done`, `Text`,
`Bytes`, `Unavailable` (none registered: haptics on the desktop),
`Failed` — comes back into the update as a message (`then: fn(
ModuleAnswer) -> Msg`), at once, or later through a `ModuleReply` the platform
answers once (the camera). Events a module sends on its own (a player's
position) go through a listener trait of the module's (`MediaListener`),
which the shell turns into messages. A test registers each module's fake
(`ark_haptics::fake::Fake`, `ark_clipboard::fake::Fake`, `ark_camera::Fake`
…) and looks at what was asked.

Under each crate's `ffi` feature its trait is a UniFFI foreign trait
(`#[uniffi::export(with_foreign)]`, harken's `PhoneSound` pattern), and
`ark-module`'s `ModuleReply` and `ModuleAnswer` UniFFI types: the module's Swift
class implements the protocol UniFFI writes (`Haptics`), its Kotlin class
the interface (`uniffi.ark_haptics.Haptics`), and the app's ffi crate takes
them (`AccountsApp.useModules(haptics:clipboard:share:)`) and registers
them. Each module crate is its own UniFFI namespace in the app's one
library, so its Kotlin package is `uniffi.ark_<name>`.

| Module | Crate | iPhone, iPad, Mac | Android | Desktop (iced) | Browser (DOM) |
|---|---|---|---|---|---|
| haptics | `ark-haptics` | `PhoneHaptics` (UIFeedbackGenerator; NSHapticFeedbackManager) | `PhoneHaptics` (ArkUI's `View.perform`) | Unavailable | Unavailable |
| clipboard | `ark-clipboard` | `PhoneClipboard` (UIPasteboard; NSPasteboard) | `PhoneClipboard` (ClipboardManager) | `Queued` (feature `desktop`), written by `iced::clipboard::write` | `Browser` (feature `web`) |
| share | `ark-share` | `PhoneShare` (UIActivityViewController; NSSharingServicePicker) | not yet | Unavailable | Unavailable |
| camera | `ark-camera` | not yet (`QrScanner` stays hand-written) | not yet | Unavailable | Unavailable |
| media-player | `ark-media-player` | not yet | not yet | not yet (`harken_player`) | — |
| notifications | `ark-notifications` | not yet | not yet | Unavailable | — |
| secure-store | `ark-secure-store` | not yet | not yet | Unavailable | — |

**MediaPlayer** generalises harken's `PhoneSound` and
`harken_player::Sound` (modules/media-player/README.md says how harken
moves onto it).

#### How an App Opts In

An app lists its modules once, in `<app>/modules.toml`:

```toml
modules = ["haptics", "clipboard", "share"]
```

and every build reads that list, or is held to it:

- **Cargo**: the app's crates depend on the listed modules' crates (money:
  `money/app` on the ones its update asks, `money/ffi` on each, with `ffi`;
  `money/iced` on `ark-clipboard` with `desktop`).
- **Xcode**: `nix run .#update-modules` (`ui/modules.py`) writes
  `<app>/ios/modules.yml`, an XcodeGen file `project.yml` includes; its
  template `ArkModules`, which the app's target uses, adds each listed
  module's `swift/` to the sources and the Info.plist keys its
  `module.toml` declares (the app's own keys win).
- **Gradle**: `android/app/build.gradle.kts` reads `modules.toml` and adds
  each listed module's `kotlin/` to the sources. Android cannot merge a
  manifest from a source directory, so a module's permissions are checked,
  not written: `checks.modules` fails when the app's manifest lacks one.
- **The shell** registers what it built: `PhoneModules(haptics:
  PhoneHaptics(), …)` in `MoneyApp.swift`, `AccountsHolder(…,
  PhoneHaptics(view), PhoneClipboard(context))` in `MoneyApp.kt`, the
  `Queued` clipboard in `money/iced/src/overview.rs`.

`checks.modules` holds it together: every listed module exists, the app's
ffi crate depends on exactly the listed modules' crates, the manifest asks
for their Android permissions, and `modules.yml` is what the script
writes. The iPhone typecheck (`checks.money-ios-typecheck`) compiles the
listed modules' Swift on Linux against the generated bindings, so every
name they use is checked.

**To add a module:** `modules/<name>/` with its crate (trait, a `Call`
enum, the functions that make calls, a fake, tests), `module.toml`,
`README.md`, `swift/` and `kotlin/`; add the crate to `rust/Cargo.toml`'s
members; an app that wants it lists it, depends on its crate, takes it in
its ffi crate's `useModules` and registers the platform's class.

## Escape Hatches

- **`Command::Native(name)`** — something only the shell can do yet:
  Accounts' pull to refresh asks the phone's own model to sync the bank
  (`refresh`), and a row's Reconcile… raises the shell's own reconcile
  sheet or dialog (`reconcile <id>`) until reconciling moves into Rust.
- **App-provided pieces of a part** — a part that wears something of the
  app's takes it from the environment, set once at the root: the face at
  a tab's title (`\.tabFace`; Compose's frame, `LocalTabFrame`), the mark
  of a row of the app's own kind (`\.rowMark`, `LocalRowMark`: money's
  account marks), the app's own glyphs by name (`LocalGlyphs`).
- **A hand-written screen beside a generated one** — every other money
  screen still is, inside the same shells.

## Rules

- The source of a layout is the Rust view; the source of a part is its
  hand-written file per toolkit; generated files are composition and
  never edited.
- Every colour, measure and spring a part uses is the theme's
  (`ui/theme.toml`, `ui/docs/motion.md`); a generated view has no
  literals but words.
- Exported names avoid C, Swift and Kotlin keywords (`register` is why the
  runtime's call is `useModules`).
- UI macros are `arkui-macros`'; `ark-macros` keeps the database's.

## What Is Verified

- Rust: `cargo test` and `clippy --all-targets -D warnings` on ark-app,
  ark-module and every module crate (ark-clipboard with `desktop`, and
  with `web` for wasm32), arkui-macros, ark-ui-gen, arkui (the context
  menu's tests drive it in iced's simulator: every item runs its own
  message, the keys move and run, a press outside closes it), money-app
  (Accounts over the demo household with the fakes, Reconcile, the amount
  in cents, the row's menu in order), money-iced and harken-iced with
  `demo`, money-ffi (`--no-default-features`: the session's day).
- nix: `checks.theme`, `checks.ui`, `checks.modules`,
  `checks.money-ios-typecheck` (now with the listed modules' Swift, and
  the screens' glue: three name clashes it found are fixed — the UniFFI
  vtable's `open` in the C header (now `openRoute`), ark-ffi's `Reply`
  against ark-module's (now `ModuleReply`), money's `Shelf` protocol
  against the page's record (now `AccountsShelf`) — and `assumeIsolated`,
  which Linux's Swift has not, is a main-actor `Task`),
  `checks.harken-ios-typecheck`; the UniFFI Kotlin bindings' names
  (`uniffi.ark_haptics.Haptics`, `AppShell.openRoute`/`today`,
  `AccountsMsg.Reconcile`/`DraftCents`) read against what the glue calls.
- Not compiled here: SwiftUI views (`ScreenParts.swift`, the generated
  view, the modules' UIKit/AppKit branches), Compose (the Android build was
  not run); nothing seen on a device; no screenshot compared.

## The Browser: `arkui-web`

`ui/rust/web` draws an `ark_app` `View` in the DOM, client-side, with
Leptos 0.8 (`csr`; no server functions, no hydration), the way
`arkui::view` draws it in iced:

- **`render(view)`** a page's view (`#[view]`'s `web(&record)`);
  **`shell(view, hosts)`** a shell's: the sections a sidebar from
  `sidebar-min-width`, a capsule of tabs in the toolbar short of it, tabs
  at the foot when compact; each section's toolbar (the way back and the
  navigation items, the title inline or large, the actions, the
  inspector's toggle and the search field at its trailing end, under the
  title when compact); its inspector a pane at the trailing edge or a
  sheet from the foot; a pushed detail; questions and sheets over it.
  The size class is the window's width against `[layout]`'s tokens read
  from the theme's CSS variables, so it is written once.
- **`run(app, accent, css)`** mounts a **`WebApp`** (its `view`, `update`,
  `commands`, what the address asks for in `arrive`, what a field
  `suggest`s); a message is **`send(m)`**: the app updates and the page is
  drawn again, Leptos keeping the elements that are the same (a focused
  field keeps its keys and its caret).
- **The transient parts** (`menu`, `fields`): a row's context menu at the
  pointer (right click, the menu key, Shift-F10) and a control's menu
  under it (`menu::button`), each with its title, its entries, a check, a
  shortcut and a submenu hanging off an entry after arkui's dwell (200 ms)
  or on `→`; `↑`/`↓`/`j`/`k`, `←` out, `Enter`, `Esc` one level, a press
  outside every level; flipped at the window's edges as AppKit does, in
  arkui's `panel` measures. A search field's suggestions, a date field's
  calendar (arrows a day or a week, PageUp/PageDown a month), a picker in
  a form's row as a menu with its choice checked; a modal centred, a sheet
  from the foot when compact.
- **`table`**: a Mac's `Table` (headings, rows a grid of cells, striped,
  the selected and the cursor's row lit, a row's menu), and when compact a
  `List` of two-line rows from each column's `Slot`.
- **`figures`**: arkui's figures, the charts as inline SVG in the `[chart]`
  palette (`line_svg` is iced's line, point for point).
- **`keys`**: the shortcuts of the app's `CommandMenu`s
  (`ark_app::shell::command_for`), the command box (`:`, ⌘K), the keys
  panel (`?`), and the vim layer over the rows drawn: `j`/`k`, counts,
  `gg`/`G`/`{n}G`, `^d`/`^u`, `zz`/`zt`/`zb`, `h`/`l`/Tab between the
  panes, Enter/`o`, `/`, `n`/`N`. A field keeps its keys. iced's grammar
  (`arkui::vim`) reads iced's `Key`, so this is its twin in the DOM's
  words, not the same code: moving the grammar into a toolkit-free crate
  is what is left.
- **The theme**: `theme.css` is ui/theme.py's (`checks.theme` holds it):
  every role a variable, dark by the system unless the page says
  (`data-theme`), an app's accent by `data-accent`, the window's measures,
  the springs as CSS `linear()` curves sampled from the same stiffness and
  damping as every toolkit's, the size classes' container queries;
  `arkui.css` is the parts' own over those variables. The glyphs are
  `arkui-glyphs`, the table iced draws too.
- **Motion** (ui/docs/motion.md): a menu or dialog opened by the pointer
  grows from 0.95 on the move spring, a sheet rises; by a key nothing
  moves; reduced (`data-motion="reduced"`), only the fade. `ALWAYS_ANIMATE`
  is true as in the other three, `?reduce-motion=1` forces the gentle path
  as `ARK_REDUCE_MOTION=1` does.
- **The address** opens any screen with no steps: the launch arguments
  every shell reads (`?screen=account:checking&inspector=1&pane=banks`,
  `ark_app::shell::Launch`), and arkui-web's `?context=<row>`,
  `?menu=<id>`, `?popover=<id>`, `?command=`, `?keys=1`, `?theme=dark`.

## The iced Fork: COSMIC's

The desktop draws with COSMIC's iced (`github.com/pop-os/iced`), the
whole workspace and the docs site, at the revision libcosmic pins as its
`iced` submodule: `d5d638c3a3022396fcf5b9b588b3446820dc11ad` (2026-11-05).
It is iced 0.14 with COSMIC's desktop stack under it; libcosmic itself
(its widgets, its theme, its menus) is not taken, and ArkUI keeps its
look.

- **Its git dependencies**, pinned by `rust/Cargo.lock` and fetched by
  `rust/Cargo.nix` with their hashes (`crate-hashes.json`): pop-os/iced,
  pop-os/winit (a winit 0.31 pre-release, `9567503d`), pop-os/
  window_clipboard (`sctk-0.20`, `f68595ee`), pop-os/softbuffer
  (`cosmic-4.0`, `c2b2c19d`), pop-os/cosmic-protocols (`c0cff4d`),
  wash2/accesskit (`6c20249d`) and iced-rs/cryoglyph (`e429a025`).
- **Its features**: none of its defaults (COSMIC's: tokio, no wgpu, no
  winit shell); upstream iced 0.14's defaults instead, the winit shell,
  `a11y` (accesskit: arkui's wrappers pass their content's tree on, and a
  cut line, `arkui::line`, says its whole text) and what every window
  asks for (`rust/Cargo.toml` says each).
- **What moved**: `Widget::diff` takes `&mut self`; the space bar is a
  character (`" "`), not a named key; a drop is every file at once; wgpu 28
  lists its adapters asynchronously (`renderer.rs` still pins
  `WGPU_BACKEND`, and `ICED_BACKEND` still picks tiny-skia: both are read
  by the fork's compositor as before); a text's style carries its
  selection's colours, a slider's its breakpoints'. The Wayland hold
  gesture (`touchpad.rs`) binds on the same wayland-client 0.31 as the
  fork's winit. smithay-client-toolkit links libxkbcommon (pkg-config at
  build time: `desktopOverrides` in flake.nix).
- **What it would have changed, and does not**: its text is 1.4 times
  its size where upstream's is 1.3, so every line of every window was a
  tenth taller. `arkui::text` and `arkui::text_input` set ArkUI's
  (`ui/theme.toml`'s `[layout] line-height`), and every window's text
  goes through them. money's desktop shots are pixel for pixel what they
  were on upstream iced; harken's table (`arkui::line` cells under
  `arkui::table` headings) sits about one logical pixel higher.
- **Not taken from it**: its browser build (iced is the desktop's alone
  now), its tokio executor, libcosmic's context menu (ArkUI's own is
  `arkui::context_menu`).

## What Is Left

In the order the owner asked for it, with the design rules first
(ui/docs/design.md: every app designed as a standard macOS SwiftUI app,
then written in this vocabulary, then its domain underneath;
ui/docs/apps/money.md for money):

1. **The shell's vocabulary** — written (`ui/rust/app/src/shell.rs`):
   `adaptive_tabs`/`section_tab` (with a badge; a tag `parent/child` is a
   child of `parent`, its own place in the sidebar and the parent's pushed
   page when compact) and its `footer` (`.tabViewSidebarBottomBar`; the
   toolbar's end where there is no sidebar), `tab_group` (`TabSection`,
   sidebar only, Mail's mailboxes), `pushes` (`navigationDestination`),
   `split_view`, `toolbar`
   with `bar_item`s by `Placement`, `searchable`, `inspector`, `hosted`,
   `confirmation_dialog`, `alert`, `settings_panes` (one shown, by title),
   and the menu bar's
   `CommandMenu`s; `View::map` hosts a shell in another app's messages.
   The size classes are `ui/theme.toml`'s `[layout]` (`LayoutTokens`,
   `arkui::layout_tokens`), decided by the parts alone (`arkui::shell::
   SizeClass`; a sidebar from `sidebar-min-width`, a tab bar at the top
   short of it, as an iPad upright). The inspector puts its own toggle at
   the toolbar's trailing end and the search field sits after it, as on a
   Mac; nothing (no account button) floats over the toolbar. The figures'
   parts are ui/docs/parts.md's "Figures". Parts by hand: `ShellParts.swift` (TabView
   `.sidebarAdaptable`, `.toolbar`, `.searchable`, `.inspector`, the
   `Settings` scene as `ArkSettings`, `ArkCommands`), `ShellParts.kt`
   (bar/rail/drawer by width, side-by-side panes or one at a time,
   `TopAppBar` with overflow, `SearchBar`, a supporting pane or a bottom
   sheet, hardware-keyboard shortcuts), `arkui::view::shell` (iced). The
   keyboard is `arkui::shell::Keyboard`, every desktop app's. Left: swipe
   actions; Compose on material3-adaptive (`NavigationSuiteScaffold`,
   `ListDetailPaneScaffold`) and WindowManager's postures, which need the
   Gradle graph (`deps.json`) regenerated with them — the parts are
   material3's own until then; a split view that is SwiftUI's
   `NavigationSplitView` (it is columns in the section's one stack, so the
   toolbar and search stay the section's); Settings as a second iced
   window (it is the shell's panes in the window); the commands in the
   iced command box and keys panel (they are shortcuts).
2. **money in that shell** — started: the sections, toolbars, search,
   inspector, Settings and commands on iced, the iPhone/iPad/Mac and
   Android, hosting the existing screens (ui/docs/apps/money.md, The
   Shell). Next: the inspectors' and details' content, then Budget,
   Transactions, Insights, Settings, sign-in and receipts ported in that
   order. harken's design doc and port after it; revelation is not an Ark
   app (plain Kotlin) and stays as it is.
3. **The browser** — written: `ui/rust/web` (crate `arkui-web`) draws a
   `View` with Leptos over the real DOM ("The Browser", below) and
   `money/web` runs money's shell and the Accounts slice over the demo
   household in wasm, its other sections hosted by its own screens as
   money/iced hosts its; `nix build .#money-web` is the page
   (`checks.money-web`), `.github/screens/app-web.sh` photographs it.
   Left: harken's client, an SSR or pre-rendered page for the docs, the
   vim grammar shared with iced (below), the page's own history.
4. **macOS, iPad, Android tablets and foldables**: money's target is one
   multiplatform target with a macOS destination (`project.yml`,
   `supportedDestinations`), its kernel's Mac slice and `money-macos`
   (flake.nix) built by nix; ArkUI builds for the Mac (its colours
   through `Color.dynamic`, what only the iPhone has behind
   `Platform.swift`'s shims, the document scanner in ArkUI). Never built
   here: no Mac. money's and harken's
   iPhone targets take the iPad (`TARGETED_DEVICE_FAMILY` 1,2, every
   orientation on an iPad, no `UIRequiresFullScreen`), not yet built on a
   Mac; Android's activities have no fixed orientation and are resizeable
   already. Left: the generated SwiftUI compiled for the Mac (a macOS
   destination on money's target: today its hand-written screens use
   UIKit, so the destination waits for the shell of item 1), the adaptive
   scaffolds, and the shots in the existing Mac and Android jobs (an iPad
   Pro 13" in both orientations, a Pixel Tablet landscape, a Pixel Fold
   folded and open). The rumoured iPhone Fold needs no code (size
   classes) and has no simulator to try.
5. **The parity check**: the same screens with the same demo data at one
   size, shot in iced (arkui-shots), the web (Playwright's Chromium) and
   macOS, side by side in the PR's comment; iced against web gated by a
   perceptual diff, a row's context menu open among the shots.
6. **Modules' native implementations**: Camera QR on iOS (wrapping
   `QrScanner`) and Android (a decoder dependency and deps.json
   regenerated), Share on Android, and media-player, notifications and
   secure-store everywhere.
7. **Primitives**: the hand-written layer shrinks to primitives (layout,
   content, token-only surfaces, interaction, the native containers and
   controls, motion), and ArkUI's components become `#[view]`s emitted into
   ui/swift and ui/kotlin. Today's `ScreenParts` are the component layer
   written by hand, the step before that.
