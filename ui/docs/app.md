# The Whole App in Rust

An Ark app's state, its rules and its screens' layout are written once, in
Rust; every platform draws them natively — SwiftUI on the iPhone, Compose
on Android, iced on the desktop, the real DOM in the browser. Think
multiplatform SwiftUI or Expo, with Rust as the source and nothing
self-drawn or in a web view on a phone.

This is a prototype on `claude/sleepy-faraday-lcbow5-app`. One vertical
slice is moved — money's **Accounts** tab — and everything else is as it
was. The last section says what is left and in what order.

## The Layers

```
 money/app            the app: State, Msg, update, screen records, #[view] layouts
   └ ark-app          the runtime (App, Runtime, Host, Command) and the view vocabulary
   └ ark-modules      the device: Haptics, Clipboard, Share, Camera, MediaPlayer, …
 ui/rust/arkui           iced: draws a View (feature `app`) — the desktop
 ui/swift          SwiftUI: the parts a generated view calls, by hand
 ui/kotlin         Compose: the same parts, by hand
 ui/rust/gen      writes SwiftUI and Compose from a recorded view (composition only)
 ui/rust/arkui-macros    #[view]
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

### The Device: `ark-modules`

A module is one Rust trait, implemented natively per platform and
registered at startup (`Modules`). An update never calls one: it returns a
`ModuleCall` (`Haptic`, `Copy`, `Share`, `ScanQr`, `TakePhoto`, `Notify`,
`SecretGet`, …), the runtime hands it to the module, and the
`ModuleAnswer` comes back as a message — at once, or later for the camera
(a `Reply` object the platform answers once). So an update is testable
with `ark_modules::fake::Fakes`, which records what was asked and lets a
test answer a scan.

Under the `ffi` feature each trait is a UniFFI foreign trait
(`#[uniffi::export(with_foreign)]`, harken's `PhoneSound` pattern): Swift
and Kotlin classes implement the protocol / interface UniFFI writes.

| Module | Trait | iPhone | Android | Desktop (iced) | Browser |
|---|---|---|---|---|---|
| Haptics | `perform(haptic)` | `PhoneHaptics` (UIFeedbackGenerator) | `PhoneHaptics` (`View.perform`) | none (Unavailable) | none |
| Clipboard | `copy_text(text)` | `PhoneClipboard` (UIPasteboard) | `PhoneClipboard` (ClipboardManager) | `iced::clipboard::write` | — |
| Share | `share(text, url)` | `PhoneShare` (UIActivityViewController) | — | — | — |
| Camera | `scan_qr`, `take_photo` (answer later) | API only: `QrScanner` stays hand-written | API only | — | — |
| MediaPlayer | `load`, `play`, `pause`, `seek`, `set_rate`, `set_volume`, `set_now_playing`, events (`MediaListener`) | API only | API only | API only | — |
| Notifications | `schedule`, `cancel` | API only | API only | — | — |
| SecureStore | `get`, `put`, `remove` | API only | API only | — | — |

**MediaPlayer** generalises harken's `PhoneSound` and
`harken_player::Sound`: one sound at a time, loaded from a URL at a
position, played and paused, sought, at a rate and a volume, with the lock
screen's now-playing record; what it did (`Loaded`, `Position`, `Playing`,
`Paused`, `Ended`, `Remote` from the lock screen or a headset, `Failed`)
arrives through the `MediaListener` the app registers and becomes the
app's messages. Moving harken onto it is: implement the trait over
AVPlayer (iPhone), Media3 (Android) and `harken_player` (desktop), register
it, and move the player's state machine from the shells into harken's
`update`.

**To add a module:** add its trait and its calls/answers to `ark-modules`
(exported under `ffi`), a fake in `fake.rs`, a case in `Modules::call`;
implement it per platform; register it where the app registers the others
(`MoneyApp.swift`, `MoneyApp.kt` / `Screens.kt`, the iced host's `ask`).
The owner's direction is one package per module, Expo's way
(`modules/<name>/` with its crate, its `swift/`, its `kotlin/`, its README
and the permissions it declares, and a small shared `ark-module` crate for
the plumbing); that split is not done yet (below).

## Escape Hatches

- **`Command::Native(name)`** — something only the shell can do yet:
  Accounts' pull to refresh asks the phone's own model to sync the bank.
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

- Rust: `cargo test` / `clippy -D warnings` on ark-app, ark-modules,
  arkui-macros (expansion and error tests), ark-ui-gen, money-app
  (Accounts over the demo household with the fakes), money-iced (a test
  drives the Accounts page in the window), arkui; money-ffi builds;
  `checks.ui` built with nix.
- The UniFFI Swift and Kotlin bindings were generated from the built
  library and every name the glue uses was checked against them.
- Not compiled here: the SwiftUI and Compose (no Xcode; the Android
  build was not run); nothing seen on a device; no screenshot compared.

## What Is Left

In the order the owner asked for it:

1. **The browser**: `rust/arkui-web` drawing a `View` with Leptos over the
   real DOM, theme tokens as CSS variables generated beside the others, and
   a `money/web` crate running the demo household in wasm.
2. **Primitives**: the hand-written layer shrinks to primitives — layout
   (`Stack`, `Spacer`, padding/frame), content (`Text`, `Glyph`, `Image`),
   token-only surface modifiers, interaction (`Button`/press, menus,
   swipe), the containers that must stay native (`Scroll`, `List`/`Section`,
   `NavigationStack`, the adaptive tabs, `Sheet`, `Menu`, `Alert`), the
   native controls (`TextField`, `Toggle`, `Picker`, `Slider`,
   `DatePicker`, `ProgressView`) and motion (`animate(Springs.x)`) — and
   ArkUI's components (`ListRow`, `Badge`, `Tile`, `GroupCard`,
   `HeroFigure`, `SectionHeader`, …) are themselves `#[view]`s in Rust,
   emitted into a generated directory inside ui/swift and ui/kotlin.
   Each primitive is one that cannot be composed: a platform list's swipe,
   separators and laziness; a native text field's input methods; a sheet's
   system gesture; the system's springs. Today's `ScreenParts` are the
   component layer written by hand, the step before that.
3. **Modules as packages**, one directory each (crate, `swift/`,
   `kotlin/`, README with permissions), an app opting in per module;
   Camera QR on iOS (wrapping `QrScanner`) and Android (needs a decoder
   dependency and a deps.json regeneration).
4. **Adaptive layout**: the app's tabs declared once in Rust, the size
   class decided in the core from one breakpoint token, bottom tabs when
   compact and a sidebar when regular on every target, per-tab back stacks
   on the Navigator.
5. **macOS**: the generated SwiftUI and ArkUI compiled for the Mac (one
   multiplatform target), modules' Mac behaviour, commands and keyboard
   shortcuts declared beside the messages.
6. **The rest of money and harken**: each screen moves the way Accounts
   did — its state and update into the app crate, its record into
   `screen()`, its layout into a `#[view]`, the hand-written screen
   deleted. Budget (drag to reorder, the month stepper) needs gesture and
   motion primitives first; harken's player needs MediaPlayer.
