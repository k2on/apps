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
DOM (`ui/rust/web`, not written yet); money's and harken's iced builds for
wasm32 are gone, and with them their Pages demos until the Leptos client
exists. What has no UI stays wasm-capable for it: ark-peer, ark-auth's
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
| `accounts::web(&page)` | `Real`, drawn by `arkui-web` | Leptos over the DOM (feature `web`; the crate is not written yet) |
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

## The iced Fork (Assessed, Not Taken)

The owner asked for the whole workspace on COSMIC's iced
(`github.com/pop-os/iced`) and its context menu. Assessed on 2026-11-09
against its `master` (d5d638c3):

- **Its context menu is not in the fork.** COSMIC's `context_menu` and
  menu trees are libcosmic's (`cosmic::widget::menu`, MPL-2.0, adapted
  from iced_aw), built on libcosmic's own theme and widget set. Taking the
  fork brings no menu; taking the menu means libcosmic, which brings its
  design system, which ArkUI keeps its own of.
- **Its graph is seven git forks**: pop-os/iced, pop-os/winit (a winit 0.31
  pre-release, rev 9567503), pop-os/window_clipboard, smithay-clipboard,
  softbuffer, wash2/accesskit and iced-rs/cryoglyph — 40 git packages in a
  453-package lock. Each needs a pinned fetch in rust/Cargo.nix, and the
  winit fork is a newer winit API than iced 0.14's, so arkui's window,
  touchpad (the Wayland hold handling) and renderer selection move with it.
- **It does not build for the browser.** A probe crate on the fork's
  `iced` (no default features; `tiny-skia`, `svg`, `image`, `advanced`,
  `winit`, `webgl`) fails `cargo check --target wasm32-unknown-unknown`
  in the fork's own `iced_winit`: `winit::platform::web::{
  WindowAttributesExtWebSys, WindowExtWebSys, EventLoopExtWebSys}` are
  gone from its winit fork, and its compositor future is not `'static`
  for `spawn_local`. money's and harken's web builds are iced in wasm, so
  the switch stops here, as the owner's condition says; nothing of it was
  committed.

So the menu was built in arkui (option c): `arkui::context_menu`, ArkUI's
menu panel at the pointer, flipped at the window's edges
(`Menu::origin_for`), every item, `↑`/`↓`/`j`/`k`, `Enter`, `Esc`, a press
outside closes it and goes no further, a long press on touch, the menu key
or `Shift-F10`. The browser draws the same menu when it has a DOM client.

## What Is Left

In the order the owner asked for it, with the design rules first
(ui/docs/design.md: every app designed as a standard macOS SwiftUI app,
then written in this vocabulary, then its domain underneath;
ui/docs/apps/money.md for money):

1. **The shell's vocabulary**: scenes (`WindowGroup`, `Settings`,
   `Commands`), the sidebar-adaptable tabs, the split view (collapsing to
   a stack), toolbar placements, the inspector, `searchable`,
   `confirmationDialog`, swipe actions — each a part that adapts by size
   class inside itself (design.md's table: SwiftUI, Compose with
   NavigationSuiteScaffold/ListDetailPaneScaffold and WindowManager's
   postures, iced, Leptos), and the keyboard model (vim layer, keys panel,
   command box, pane focus) as ark-app's, every app's.
2. **money in that shell**: the sidebar sections, Accounts as content +
   register detail + inspector; then Budget, Transactions, Insights,
   Settings, sign-in and receipts ported in that order (ui/docs/apps/
   money.md). harken's design doc and port after it; revelation is not an
   Ark app (plain Kotlin) and stays as it is.
3. **The browser**: `ui/rust/web` drawing a `View` with Leptos over the
   real DOM, the theme as CSS variables generated from ui/theme.toml, the
   same context menu in the DOM, and `money/web` running the demo
   household in wasm.
4. **macOS, iPad, Android tablets and foldables**: money's and harken's
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
