# The Whole App in Rust

An Ark app's state, its rules and its screens' layout are written once, in
Rust; every platform draws them natively — SwiftUI on the iPhone, Compose
on Android, iced on the desktop, the real DOM in the browser: one source
in Rust, and nothing self-drawn or in a web view on a phone.

This is a prototype on `claude/sleepy-faraday-lcbow5-app`. One vertical
slice is moved — money's **Accounts** tab — and everything else is as it
was. How an app is designed before any of this (Mac-first, Apple's
multiplatform samples and the HIG) is ui/docs/design.md; money's design
is ui/docs/apps/money.md. The last section says what is left and in what
order.

## The Pieces

iced is the desktop's alone. The browser is drawn by Leptos over the real
DOM (`ui/rust/web`, crate `arkui-web`; an app's page is generated from
its app.toml: "The Browser's Page");
money's and harken's iced builds for wasm32 are gone, and harken's Pages
demo with them until its Leptos client exists. What has no UI stays wasm-capable for it: ark-peer, ark-auth's
`web`, ark-format, ark-router, ark-app and the modules' `web` impls.

```
 money/core           the domain: its rows, rules and views' keys; no UniFFI, no peer
 money/app            the app: State, Msg, update, screen records, #[view] layouts
 money/demo           the demo's household on a peer and a disk, and `money-seed`
   └ ark-app          the runtime (App, Runtime, Host, Command) and the view vocabulary (ui/rust/app)
   └ ark-module       the device's plumbing (ui/rust/module)
   └ ark-<name>       a device module each: modules/ark-haptics, clipboard, share, camera, …
 ui/rust/arkui        iced: draws a View (feature `app`) — the desktop
 ui/swift             SwiftUI: the parts a generated view calls, by hand
 ui/kotlin            Compose: the same parts, by hand
 ui/rust/gen          writes SwiftUI and Compose from a recorded view (composition only)
 ui/rust/arkui-macros #[view]
```

### State and Logic: `ark-app`

An app is a state, its messages and one update over them (`ark_app::App`):

- `State` — what the screen is in the middle of (a form being filled, a
  row being renamed). The domain's rows are **not** copied into it: they
  stay in the views the shell holds, which the app reads through the
  kernel's port (`ark::views::Views`; a peer's `ark_peer::store::Store` is
  one), and the app names the views it needs (`App::holds`), as keys of
  the domain's (`ark::views::ViewKey`).
- `Msg` — everything the screen can be told. Ids travel as `Id::hex`, as
  routes carry them.
- `update(&mut State, Msg, &Data) -> Vec<Command>` — decides everything.
  A `Command` is data: `Send` a typed change (`MoneyChange`), `Open` a
  `Route`, `Back`, `Ask` a module's typed operation, `Request` something
  only the host can do (the app's own typed `App::Request`), `Then`
  another message, `AfterDraw` one once the screen as it is now has been
  drawn (a presentation put away while the place under it still shows,
  before a route moves somewhere else: a phone's host, `draws_between`,
  calls `Runtime::drawn` after it draws, and is not idle until then; the
  desktop and the browser run it next). What became of a change or an operation comes back as
  a message when the update asked for one (`then: fn(..) -> Msg`, a plain
  function pointer, never a closure). Effects and Ports, below, says how.
- `screen(&State, &Data) -> Screen` — the record the screen draws, every
  figure and word worked out (the page record): `AccountsPage`.

`Runtime<A>` owns the loop: a message in, the state updated, the commands
carried out by the shell's `Host`, their messages run in turn, and a
`version` that grows — the number a native shell watches, as it watches
the store's. The runtime has no thread or clock of its own (the host says
the time: the browser has no clock `std` can read), so the same code runs
behind the phones' bridge, in the iced window and in wasm.

A runtime's `Host` is what carries its commands out for one message. No
shell writes one any more: `ark_app::host::Screen` is a runtime with its
host written once over the platform's place (below), and the tests'
(`money/app/src/testing.rs`, over the demo household and the modules'
fakes) is the one other.

### The App's Part of Its Host: `ark_app::host`

An app is more than one runtime: money's shell, Budget, Accounts,
Transactions, Insights and the sheets run side by side over one session,
and what one asks is often another's message (the shell's Previous Month
moves Budget, Transactions, Insights and the sheets; a launch's
`-screen category:groceries` is the shell's inspector over the budget's
line). That part — which runtimes there are, what a route or a request
becomes, what one page says that another follows, a link taken, the
demo's bank behind Sync, who is signed in as Settings says it — is the
app's, the same on every platform, and written once in its app crate
(`money_app::host::Money`). A host is two halves:

- **The place** (`ark_app::host::Place`), the platform's: the views its
  store holds (read, held, released), the time, a change sent to its
  peer, its device modules and what wakes it when one answers later, a
  file fetched or kept, a sentence said, the server dialled, who is
  signed in where (`Session`: the demo, the day, the login, the server as
  shown, the link, what is pending, the version, and whether this host can
  sign out or add a device), and two facts of its toolkit: whether it
  draws between messages (SwiftUI, Compose) and whether a page opened goes
  on a section's stack (a phone) or is one of the window's places (the
  browser, the desktop). Object-safe, and handed in by reference for each
  call, so nothing of it is kept by the app.
- **The app's part** (`HostedApp`): its screens (each an
  `ark_app::host::Screen<A>`, a runtime that holds the views its state
  names through the place, and takes its modules' answers), opened over a
  `Session`; `route`, `request`, `back` for what they asked; `follow`
  after every call; `moved` when the store moved or an answer came later
  (what a launch asked before the household arrived is asked again);
  `link` and `launched` for a launch's arguments or an address; and its
  `Native`s — what only the toolkit's shell can do (money's: a page pushed,
  Settings or Add a Device put up, its own pages put away for a link,
  signed out) — taken by the host after the call.

`Hosting<H>` is the loop between them, the same everywhere: a message in
for one screen (`send::<Accounts>(msg, place)`, `Has<Accounts>` saying
which field it is), the routes and requests it asked carried out in order
once no runtime is mid-message (so a request that reaches another runtime,
or the same one, never runs inside it), `follow`, the app's own views held,
and `idle`/`version` summed over its screens. ark-app stays free of the
world: the place is a trait, and no adapter crate is named.

Each platform has one place and drives the one loop:

| Platform | Place | Driven by |
|---|---|---|
| iPhone, iPad, Mac, Android | ark-ffi's `PhonePlace` (the session's views over the process's peer, the process's modules, the login, the link) | ark-ffi's `Hosted<P>`, the `HostApp` under `ArkHost`; one UniFFI object per screen (`ark_ffi::hosted_screen!`); a register pushed on a stack runs `beside` it |
| Browser | arkui-web's `WebPlace` (the page's replica and store; `Seen` to read) | `arkui_web::host::WebHost<W>`, over the app's `Windowed` |
| Desktop | not yet: the iced window still carries its own requests (`money/iced/src/sections.rs`) | |
| Tests | `money/app/src/host/tests.rs`'s place over the demo household | `Hosting` |

On the phones an app names no code of its own: its library is generated
("The Phones' Library", below), and what `Hosted` asks of it beside its
part (`PhoneApp`) is app.toml's — its name and scheme, the domain its
replica's log is of, its demo's crate. Its part says the rest
(`HostedApp`): what it keeps (`KEEPS`), its first run's words
(`welcome`), how a change is encoded, which native signs out.

**A window its host draws itself** (`ark_app::window::Windowed`): the
browser's page has no shell of its own as a phone does, so the app's part
says the whole window, in the views every toolkit draws, once: its shell's
view (`window`), what each name it hosts shows (`hosted`: money's month in
the budget's inspector with nothing chosen and who is using it at the
sidebar's foot are money-app's views, `money_app::window::view`), its
sheets (`over`), Settings while it shows (`settings`, a window of its own
with Done), the menu bar's commands, what a field suggests, a page it opens
in a tab of its own (`opens`), its appearance and its address (`address`).
One message type reaches every screen (`Windowed::Msg`, money's
`MoneyMsg`), taken inside the loop (`take`); a native the toolkit's shell
would carry out is the part's own in a window (`native`: Settings shown is
its state there). The desktop's window is to be hosted the same way.

### The Phones' Library

Generated, never committed: `ui/prebuild.py ffi <app>` writes `lib.rs`
and `Cargo.toml` of `<id>-ffi` (lib and UniFFI namespace `<id>_ffi`;
Kotlin's `uniffi.money_ffi`, Swift's types in the app's module) from
app.toml's `[ffi]` and `[screens]`, which nix/ark-app.nix builds
(`ffiSrc`, `crates`) and the iPhone's cargo build adds at `<app>/.ffi`.
It is, in order:

- the twins of what crosses (`ark::twins!`): `[ffi.twins]` by crate, and
  without being listed each screen's page and message (beside its `app`
  type), the app's route and native (`[ffi] route`, `native`) and the
  first run's records;
- the first run's object (`ark_ffi::welcome_app!`);
- `Phone`, its `PhoneApp`: `[ffi] app` (the part), `domain` (a function
  giving the domain), `demo` (the demo's crate, whose `seeded_disk(today,
  now_ms)` its `open(disk)` opens), the name and the scheme;
- one object per `[screens]` entry (`ark_ffi::hosted_screen!`, or
  `beside_screen!` for one that says `beside`: a register the phone runs
  for as long as it shows it, made with its first message);
- the object the phone holds (`ark_ffi::phone_host!`, `[ffi] host`):
  `new(launch, store)`, `host()`, `welcome()`, each screen by its name
  (its blank page while no session is open), each beside screen by its
  name (`register(first:)`), `generation()`, `natives()` (the app's
  `Native`s, taken), `today()`; and `routeFromUrl` and the route's
  `url()`.

Its Rust is tested without UniFFI by the app's own tests over the same
pieces (money/app/tests/phone.rs: a demo launch running every screen, the
demo remembered and left, a server that is not there, a launch's
arguments, the replica opened, the demo seeded, a household of two
devices synced through `ark_ffi::hall::Hall`); `checks.ffi` has UniFFI
write its Kotlin and Swift.

### The Browser's Page

Generated, never committed, the phones' library's twin: `ui/prebuild.py
web <app>` writes `<id>-web` (its binary the page's module) — `main.rs`
and `Cargo.toml` — and the page that loads it, `index.html`, with the
app's icon beside it (`favicon.svg`), from app.toml: its name, scheme,
accent and version, and of `[ffi]` its part of its host, its domain and
its demo. nix/ark-app.nix builds it (`webSrc`, `crates`), flake.nix's
`<app>-web` binds it (`wasmPage`), and a cargo build of the page adds it to
the workspace at `<app>/.web` (ignored by git). Its `main.rs` is one
`arkui_web::host::WebHosted` and `arkui_web::host::run`, with the browser's
device of each module its crate depends on whose module.toml says `[web]`
(`device`, `port`: ark-scanner's file input, ark-clipboard's clipboard).

arkui-web's `host` (feature `host`) is the rest, the same for every app:
`WebPlace` (the page's replica and the views it holds; `Seen` to read),
`WebHost<W>` driving `Hosting` over it and drawing what the part says
(`Windowed`), and `live`, the sign-in and the socket: the server is
whoever served the page, the first run is every app's
(`ark_app::welcome`), and `?demo` runs the app's demo in the page with no
server, seeded to `?today=`. The address arrives as a link
(`HostedApp::link`): a launch when its query names a screen
(`?screen=budget&inspector=1`), the address itself otherwise
(`/account/<id>/checking/?inspector=1`). The page's root says where it is
(`data-ark-address`, the part's `address`) beside its ready signal.

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

An app names its screens once, in its app.toml: each `[screens]` entry's
`#[view]` in its crate (`view = "accounts::view::accounts"`) and the names
it is written under in Swift and Kotlin (`swift-view`, `kotlin-view`), and
`[views]` for a screen's further views (money's Settings, Connected Banks
alone) and its menu bar's `commands()`. An app has no generator and no
crate of its own for it: ui/prebuild.py writes the generator's main from
those (`prebuild.py views money`: `ark_ui_gen::app::main(App { … })`, the
first run's screen among them), nix/ark-app.nix links it with the app's
crate as `<id>-gen` and runs it while it
builds the app's native projects, which then hold

- `<Name>.swift` (`AccountsView`, `MoneyShellView`, `MoneySettingsView`,
  `MoneyCommands`), and
- `<Name>.kt` (`AccountsScreen`, `MoneyShell`, `MoneySettings`,
  `moneyCommands`), in the app's Kotlin package,

none of them committed (below). `nix build .#money-ui` is what it writes.
The emitters' own snapshot tests hold what they write for a fixture view
of no app's (`ui/rust/gen/tests/snapshots.rs`; after a deliberate change
to an emitter, `ARK_UPDATE_SNAPSHOTS=1 cargo test -p ark-ui-gen`).

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

The generated view reads its record from a holder over the runtime: the
app library's `ArkScreen` (ui/swift's ArkApp, ui/kotlin/app), an
`ObservableObject` / `StateFlow` of the record, read again after every
message and whenever the store moved, with the after-draw handshake
(`drawn()`). Each app's holders are generated over it from app.toml's
`[screens]` (`typealias AccountsScreen = ArkScreen<AccountsApp>`,
`class AccountsHolder(r: AccountsApp) : ArkScreen<…>`), and the runtimes
are the app's host's ("The App Library").

### The Device: `modules/`

What an app asks of the device is a module: one package each under
`modules/`, and a small core under `ui/rust/module`.

```
ui/rust/module/       crate ark-module: Op, ModuleError, Reply, Modules — the plumbing — and the ports an app asks (haptics, clipboard, share)
modules/<name>/
├── Cargo.toml        crate ark-<name>: the trait, its operations, its fake; desktop/web impls behind features
├── src/
├── swift/            the iPhone's and the Mac's implementation (nix: a module per app over its bindings, `ArkBindings`; Xcode: compiled into the app)
├── kotlin/           Android's implementation (compiled into the app: it implements its bindings' interface)
├── module.toml       its crate, its [ops] and their outputs, and what it asks of each platform
└── README.md         its API, what it does on each platform, the permissions it needs
```

An update never calls a module. It returns `Command::ask(op)` or
`Command::ask_then(op, Msg::Scanned)`, where `op` is one of the module's
operations (`ark_app::modules::haptics::perform(Haptic::Success)`, a
`HapticsOp`; `ark_app::modules::clipboard::copy(text)`;
`ark_camera::scan_qr()`, a `ScanQr`), each
implementing `ark_module::Op` with the trait that carries it out and the
type of what comes of it (`Output`: `()` for a haptic, `Option<String>`
for a scan, `Option<Vec<u8>>` for a photo). Effects and Ports, below,
says how it is carried. **A module an app's update asks is a port of the
runtime's**: its trait, its operations and their outputs are in
`ark-module` (`ark_module::haptics`, `clipboard`, `share`, `secure_store`; re-exported
as `ark_app::modules`), so the app's crate depends on no module crate
(layers.toml), and the module's crate implements the port — its fake,
the desktop's and the browser's, and the phones' foreign trait. A test
registers each module's fake
(`ark_haptics::fake::Fake`, `ark_clipboard::fake::Fake`, `ark_camera::Fake`
…) and looks at what was asked.

Under each crate's `ffi` feature it declares a UniFFI foreign trait of
the port's name (`ark_haptics::phone::Haptics`,
`#[uniffi::export(with_foreign)]`; the media player's is
`NativeMediaPlayer`, apart from Apple's MediaPlayer framework) and
registers what the phone gives as the port (`ark_haptics::phone::port`);
`ark-module` itself has no UniFFI. A
module that answers later hands its implementation a typed reply object
of its own (the camera's `ScanReply.scanned(code)`,
`PhotoReply.taken(jpeg)`, either's `failed(why)`): the module's Swift
class implements the protocol UniFFI writes (`Haptics`), its Kotlin class
the interface (`uniffi.ark_haptics.Haptics`), and the app's bridge takes
them (`AccountsApp.useModules(haptics:clipboard:share:)`) and registers
them. Each module crate is its own UniFFI namespace in the app's one
library, so its Kotlin package is `uniffi.ark_<name>`.

| Module | Crate | iPhone, iPad, Mac | Android | Desktop (iced) | Browser (DOM) |
|---|---|---|---|---|---|
| ark-haptics | `ark-haptics` | `PhoneHaptics` (UIFeedbackGenerator; NSHapticFeedbackManager) | `PhoneHaptics` (ArkUI's `View.perform`) | Unavailable | Unavailable |
| ark-clipboard | `ark-clipboard` | `PhoneClipboard` (UIPasteboard; NSPasteboard) | `PhoneClipboard` (ClipboardManager) | `Queued` (feature `desktop`), written by `iced::clipboard::write` | `Browser` (feature `web`) |
| ark-share | `ark-share` | `PhoneShare` (UIActivityViewController; NSSharingServicePicker) | not yet | Unavailable | Unavailable |
| ark-camera | `ark-camera` | `PhoneCamera` (a QR code by VisionKit's data scanner, a photo, a watched photo by AVFoundation; the Mac fails, said) | `PhoneCamera` (a watched photo by Camera2, its page `CameraPage`; no QR decoder yet) | Unavailable | Unavailable |
| ark-scanner | `ark-scanner` | over the camera (`over_camera`: the eyes, the warp in Rust) | over the camera, with the corner model (`model`) | `Chosen` (feature `desktop`): a file the window's chooser gives | `Browser` (feature `web`): the page's file input |
| ark-media-player | `ark-media-player` | `PhoneMediaPlayer` (AVPlayer, the playback audio session, `MPNowPlayingInfoCenter`, `MPRemoteCommandCenter`) | `PhoneMediaPlayer` in `ArkMediaService` (Media3's ExoPlayer in a `MediaLibraryService`) | `Desktop` (feature `desktop`: rodio and MPRIS) | `Browser` (feature `web`: `<audio>` and the Media Session API) |
| ark-notifications | `ark-notifications` | `PhoneNotifications` (UNUserNotificationCenter; APNs) | `PhoneNotifications` (NotificationManager, AlarmManager; UnifiedPush) | Unavailable | Unavailable |
| ark-background | `ark-background` | `PhoneBackground` (BGTaskScheduler; NSBackgroundActivityScheduler) | `PhoneBackground` (JobScheduler) | Unavailable | Unavailable |
| ark-secure-store | `ark-secure-store` | `PhoneSecureStore` (the keychain: the bundle id's generic passwords, this device only) | `PhoneSecureStore` (an Android keystore AES-GCM key; the ciphertext in private preferences kept out of backups) | `SecretService` (feature `desktop`: the Secret Service over D-Bus; Unavailable without one) | Unavailable |

**MediaPlayer** is one sound at a time, what the system shows of it, its
controls as events (`Remote`) the app decides, and a browse tree for a car
or a watch (modules/ark-media-player/README.md); harken's player is a queue
over it (harken-player's `MediaSound`).

#### How an App Opts In

An app takes a module on by depending on its crate (`cargo add
ark-camera` in its app crate), directly or through another of its path
dependencies, and nowhere else: there is no list of modules in app.toml.
Each module's crate declares itself in its Cargo.toml
(`[package.metadata.ark] module = "module.toml"`), and its `module.toml`
says what it takes on each platform. ui/prebuild.py finds an app's modules
in `cargo metadata` over the workspace (`crate_modules`: the app's crate's
normal and build dependencies, walked; nix runs cargo once and hands it
the JSON as `ARK_CARGO_METADATA`; `prebuild.py modules <app>` prints them),
and nix/ark-app.nix's `modulesOf` reads the same manifests while it
evaluates, for what must be known then (ark-ffi's features, the iPhone's
module builds). ui/prebuild.py writes them into the generated
projects (below): each module's `swift/` and `kotlin/` among the
sources; its `[ios.info]` keys (the app's own win), `[ios] entitlements`
and `background-modes` (`UIBackgroundModes`); its `[android]`
`permissions`, `services` (a `<service>` with its actions and
foreground type), `meta-data` and Maven `dependencies`. A device the phone
hands the process says how in its module.toml's `[phone]` (the ark-ffi
method that registers it, and how Swift and Kotlin make it): the generated
`@main` and activity register each one as the process's
(`ArkProcess.registerHaptics(device: PhoneHaptics())`, … ark-ffi's
`devices`: a feature of ark-ffi per module, which the app's generated
library turns on from the same modules — `<id>-ffi`'s dependency on
ark-ffi — links its crate and carries its UniFFI traits), and
the activity puts a module's page at its root (ark-camera's
`CameraPage`); an app writes nothing native for its modules. An answer
that comes later (the camera's) wakes the app's host, which drains its
runtimes. `checks.prebuild` holds that no app.toml lists modules, that
every module's crate declares itself, and that ark-ffi has a feature for
each device a module's `[phone]` registers. The desktop registers its own (the
`Queued` clipboard in `money/iced/src/overview.rs`).

The iPhone typecheck (`checks.money-ios-typecheck`) compiles the app's
modules' Swift on Linux against the generated bindings, so every name
they use is checked.

**To add a module:** `modules/<name>/` with its crate (trait, its `Op`s
and their outputs, the functions that make them, a fake, tests),
`module.toml` (its `[ops]`: each operation's type and its output's, which
`checks.prebuild` holds to the crate),
`README.md`, `swift/` and `kotlin/`, and its Cargo.toml's
`[package.metadata.ark] module = "module.toml"`; add the crate to
`rust/Cargo.toml`'s members (and to layers.toml's `modules`); an app that
wants it depends on its crate (`nix run .#update-cargo-nix` after), and
everything native follows. A module is a device capability any
app could use, never one app's: what an app makes of it stays in the app.
One app's own native capability that no other would share is a local
module, `<app>/modules/<name>/` in the same shape, which ui/prebuild.py
compiles in as it does the shared ones.

### The First Run: `ark_app::welcome`

Every app's first run is one shared part: its pages of what the app is,
the server's address (with the demo and Scan a Code where the app has
them), signing in on the server's own page, a server from a link or a
scanned code asked about first, and what the server has on its way; and,
signed in, "Add a Device" (`add_device`), the code another device joins
with. The records (`WelcomePage`, `AddDevicePage`), the messages
(`WelcomeMsg`, `AddDeviceMsg`) and the views (`#[view] welcome`,
`add_device`) are ark-app's, over five parts of their own (FirstRunPage,
AddressField, WideButton, StatusLine, CodeImage; ui/docs/parts.md). The
state is the sign-in machine every app already shares
(`ark_auth::onboarding`), and `ark_auth::first_run::FirstRun` works out
its page from where it stands and takes a press back as its events; its
effects are the machine's, data the shell carries out as before ("Scan
a Code" asks the shell's camera, whose text comes back through
`FirstRun::scanned`). An app gives only its words and mark, a
`WelcomeCopy` (`money_app::welcome::copy`, `harken_app::welcome::copy`).
On the phones a bridge writes `FirstRunApp` with `ark_ffi::first_run_app!`
after twinning the records; money's iPhone, iPad, Mac and Android draw the
generated `WelcomeView` / `WelcomeScreen` over it, the browser
`arkui_web::render(welcome::view(..))`, the desktop walks the same machine
under its window (the window is the app from the start, over its replica:
what the machine says is its note). The first page is the app's Welcome on
every platform: the server's page where there is one to choose (the phones),
the app's own page where the server is the platform's (the desktop, the
browser), shown at a first launch (`FirstRun::launch`) until it is passed
(`Asked::seen`, which the shell remembers its own way — the desktop's app
data, the browser's local storage), never on a screenshot's, the demo's or a
link's launch, and by its route (`/welcome/`) always. The app's pages never
stand before the server's. The demo is a launch's (`-demo`, `?demo`), never
offered by the first run. A link that comes while a page shows is kept for
the app (`open_link`, `take_link`). harken's bridge exports it; its phones still draw their own
first run until harken is ported.

### Signing In

Each app's server is its provider's one OpenID Connect client, a
confidential one (`ark_auth::oidc`, `ark_auth::server`). It holds the
secret, runs the Authorization Code flow with PKCE (S256), state and a
nonce, and checks the ID token whole: the signature against the
provider's key set (fetched at every exchange, so a rotated key is
followed), the issuer, the audience, the expiry and the nonce. It accepts
whatever algorithms discovery names, so Kanidm's ES256 works. It then
hands the device a session of its own (`/auth/login` → the provider →
`/auth/callback` → `/auth/exchange`). No device holds a provider token, and
the browser keeps its session as an `HttpOnly` cookie. The audience is
per server: money's session token is refused by harken's server, and
harken's by money's.

What makes the second app one tap is the provider's own session, which
lives in the browser. So every client opens `/auth/login` in a browser
that keeps it:

| Platform | Opens the sign-in in |
|---|---|
| iPhone, iPad, Mac | `ASWebAuthenticationSession` (SwiftUI's `webAuthenticationSession`), `preferredBrowserSession: .shared`, never ephemeral |
| Android | Custom Tabs, never a WebView |
| Desktop | the system browser, back on a loopback port (`ark_auth::client`) |
| Browser | the same tab |

A server is configured under its prefix (`ark_peer::settings`):
`_OIDC_ISSUER`, `_OIDC_CLIENT_ID`, `_OIDC_CLIENT_SECRET_FILE` (a file,
for a systemd credential), `_OIDC_SCOPES` (`openid profile email`), and
two optional settings. `_OIDC_GROUPS` lets in only those whose `groups`
claim names one of the groups (`ark_auth::oidc::Groups`, which matches a
name or Kanidm's `name@domain`; the provider has to send the claim).
`_OIDC_TRUSTED_CLIENTS` names other client ids whose ID tokens are
accepted; the server's own client id is always trusted. The provider's
certificate is checked against the public roots and the machine's own, so
a household CA works. `checks.oidc-kanidm` (nix/oidc-kanidm.nix) signs in
to money and then to harken through a Kanidm VM with one browser session,
and fails if harken asks for the password again or if a server accepts
the other's token.

**Later: one sign-in per device.** Opening money after harken will offer
"Sign in as Ann" with no browser. A device-wide `ark` public client at
the provider is refreshed, and its ID token is exchanged at money's
server, which checks the token's audience and groups. The seams are in
place now:

- **`ark_auth::device_accounts::DeviceAccounts`** (list, save, forget:
  name, picture, provider, the app it came from). The one implementation
  today is `Secured::app_private`, in the secure store's `Scope::Private`,
  so it lists only that app's own account. The same implementation over
  `Scope::Shared` comes once a platform has a shared store, and the
  refresh token goes beside it, kept apart from the list.
- **`ark_auth::server::Proof`**, which `/auth/exchange` dispatches on.
  `Proof::Code` is the only variant today; an ID-token variant comes in
  beside it, checked against `Provider::trusted` (`_OIDC_TRUSTED_CLIENTS`
  names `ark`) and `Groups`.
- **`FirstRun::set_device_accounts`**, which the shell fills at launch.
  It is empty everywhere today and changes no page. The page that offers
  an account, and the field on its record (`SignInPage`), come with that
  screen.

## Effects and Ports

An update decides; it never does. Everything it wants done leaves it as
a `Command` — a value that prints, compares in a test and crosses a
bridge — and the shell's `Host` carries it out:

| Command | What it is | Who carries it out |
|---|---|---|
| `Send { change, then }` | a domain change (`MoneyChange`) | `Host::send`: the peer, its refusal the domain's sentence |
| `Open(route)`, `Back` | the app's `Route` | `Host::open`: the Navigator, the back stack, the address bar |
| `Ask(Ask<Msg>)` | a module's operation and its `then` | `Host::ask`: the `Modules` the shell registered |
| `Request(App::Request)` | what only the host can do yet, typed | `Host::request` |
| `Then(msg)` | the next message | the runtime itself |

**A module's operation is typed both ways.** Each module declares its
operation types and their outputs (`impl Op for ScanQr { type Output =
Option<String>; … }`, listed in its `module.toml`'s `[ops]`). The update
asks `Command::ask_then(ark_camera::scan_qr(), Msg::Scanned)`, where
`Msg::Scanned` takes `Result<Option<String>, ModuleError>`: the compiler
holds the `then` to the operation's output. `Command::Ask` carries an
`Ask<Msg>`, the operation and its `then` with the output's type known
only inside, so it stays data: `Debug` prints the operation
(`Ask(ScanQr, then)`), `==` compares the operation and the very `then`
(`std::ptr::fn_addr_eq`), and `Ask::op::<ScanQr>()` reads it back. The
host runs it over its `Modules` (`Ask::run`, `Inbox::ask`): the registry
finds the module by its trait, the module answers through a `Reply<T>`
once — at once (a haptic), or later (the camera, when the person is
done) — and what came of it is turned into the update's message where
the type is still known. A module that is not registered answers
`ModuleError::Unavailable`, so every operation is answered.

**A request is the app's own port.** What only the host can do yet is an
enum of the app's (`money_app::MoneyRequest`): the bank sync (the demo's
bank, or the server's importer asked); what the host decides between (`OpenTransaction { key }`:
Transaction Details in the inspector while it shows, the transaction's sheet
otherwise); a message for another of the app's runtimes beside it (`Budget {
msg: BudgetMsg::AutoAssign }` from the shell's toolbar, `Sheet { msg:
SheetsMsg::Reconcile { account } }` from a register's Reconcile…, the sheets
any screen raises being one runtime of their own). The app's part of its host matches on it once
(`money_app::host::Money::request`, "The App's Part of Its Host"), for
every platform but the desktop, whose window still has `App::requested`. There is no call into a shell by name, and `ShellMsg` has a case
for each command (`NewTransaction`, `PreviousMonth`, …), not a string.
What a shell used to be told so it could follow (the search typed, the
launch's appearance) is the page's (`ShellPage::search`,
`ShellPage::appearance`), read like any other field.

**The ready signal.** `Runtime::is_idle(&store)` is true when no
operation's answer is awaited, no answer is queued, no message is running
and every view `App::holds` names is held; `Runtime::version()` grows
with every message (and with an answer that runs none). A screenshot
driver waits until a screen is idle and its version has not moved since
it last looked. Each shell exposes them:

- **The phones**: every screen's runtime has `isIdle()` and `version()`
  over UniFFI (`ark_ffi::hosted_screen!`), and the app's host sums them
  (`ArkHost::idle`, `ArkHost::version`, with the session's peer settled);
  the app library probes that once for a screenshot's ready signal
  (ArkUI's `Ready`), ready at a version that stood still.
- **The browser**: the page's root carries `data-ark-idle="1"` and
  `data-ark-version="<n>"` after every draw (arkui-web's
  `WebHost::say_idle`, over `Hosting::idle`/`version`, the sum over the
  app's runtimes); a
  driver waits for `document.documentElement.dataset.arkIdle === "1"`.
- **The desktop**: the window owns its runtimes in process; a driver
  (arkui-shots, iced_test) asks the window's `App::is_idle()` and
  `App::version()` (money/iced/src/sections.rs), over its three runtimes.

**Ports not yet typed.** The peer's disk (`ark_peer::disk::Disk`) is
synchronous; an async `Disk` port, so a browser's IndexedDB and a
phone's file system are adapters of one trait, is left for later.

## Background And Notifications

**Off by default, opt-in per app and per person.** An app runs nothing in
the background unless its app.toml says so, and even then nothing until
the person turns it on in the app.

- *Per app:* only an app whose app.toml has a `[background]` gets any of
  it in its projects. With none, ui/prebuild.py writes no background mode
  (`fetch`, `remote-notification`), no `BGTaskSchedulerPermittedIdentifiers`,
  no `aps-environment`, no job service, no `BOOT_COMPLETED` receiver, no
  UnifiedPush receiver and no Notification Service Extension;
  `checks.prebuild` holds it (`prebuild.py selftest`: every app without
  one, and a fixture with all of it).
- *Per person:* an app that declares it embeds the shared "Notify Me"
  setting (`ark_app::background`: `Notify`, `NotifyMsg`, `update`, the
  `notify_me` view over ArkUI's switch row). Off until they turn it on:
  then they are asked whether the app may notify them, the device is
  registered for push and the periodic sync scheduled; turned off, the
  device is unregistered (its session's push row taken away) and the sync
  stopped. What they chose is read back at launch.
- Local notices (`ark_app::modules::notifications::schedule`, `cancel`)
  are any app's that lists ark-notifications, as before; they run nothing
  in the background.

```toml
[background]
sync = "30m"               # a periodic Woken::Sync (needs ark-background)
push = true                # Woken::Push (needs ark-notifications)
boot = true                # Android: Woken::Boot when the device starts
service-extension = true   # iPhone: the Notification Service Extension (needs push)
```

**One runtime, woken.** The OS may start the process with no screen: a
periodic sync, a push, a button on a notice, the device started. Each is
a message into the process's one runtime — `ark_app::Woken` (`Sync`,
`Push`, `Action { notice, action }`, `Boot`), which the app takes as its
own (`App::woken`) and answers with commands like any other. The
component the OS woke (Android's `ArkSyncJob`, `ArkBootReceiver`,
`ArkPushReceiver`, `ArkNoticeReceiver`; the iPhone's `BGAppRefreshTask`
and the app delegate's remote-notification callback) calls
`ArkProcess::wake(reason, budget, done)` and finishes when it is told:
once the runtime is idle — `Command::Sync` answered, every module's
answer in — or its time is nearly up. A wake is work
(`ArkProcess::work`): the runtime is online for it, and offline again
after when no UI is attached. There is no second runtime of a platform's.

**A push is only a wake-up.** It carries nothing of the app's data: no
name, amount or word from a row, only that there is something, for which
app. What the person is told is built on the device from its own replica
after it syncs, so it is the same whether it came by push, by the
periodic sync or by opening the app.

```
 a device writes ─► session.push (ark::auth::Push: its session's endpoint,
 its endpoint         over its signed-in connection; revoked with the session)
        │
 an entry lands on the hub ─► rows of the app changed?   (sign-in's own wake nobody)
        │                       each live session with a push row,
        │                       NOT connected now (a connected one gets the rows
        │                       over its socket), not woken in the last minute
        ▼
 PushSender (ark_push's port) ── APNs (alert with the app's generic words,
        │                        "New activity in Money"; silent ones are throttled)
        │                     └─ UnifiedPush (sealed to the device, Urgency high,
        │                        shows nothing)
        ▼
 the device: Woken(Push) ─► update: Command::Sync { then }
        │                     the runtime dials, syncs as always
        ▼
 then ─► update reads its replica ─► Command::Ask(notifications::schedule(Notice { … }))
        ▼
 idle ─► the woken component finishes; the runtime goes offline
```

A periodic `Woken::Sync` takes the same path from `Command::Sync` on. On
the iPhone a push is visible by default, with the app's own generic
words, since Apple throttles silent pushes and may drop them; an app with
the Notification Service Extension shows the device's own words in their
place (the last notice its runtime worked out, left in their App Group).
On Android a UnifiedPush wake is sent at high urgency and shows nothing:
the runtime builds the notice.

| Platform | Periodic sync | Push | Local notices |
|---|---|---|---|
| iPhone, iPad | `BGAppRefreshTask` `<bundle id>.sync` | APNs (`aps-environment`, `remote-notification`) | `UNUserNotificationCenter` |
| Mac | `NSBackgroundActivityScheduler` | APNs (its entitlement is not generated yet) | `UNUserNotificationCenter` |
| Android | a persisted `JobScheduler` job (WorkManager is not in the offline Maven graph; it runs on JobScheduler itself) | UnifiedPush, the receiver protocol implemented directly; the person's distributor (FCM through one, if they choose) | `NotificationManager`, `AlarmManager` |
| Desktop, browser | none | none (`Unavailable`) | none (`Unavailable`) |

The server's side is any app's: `ark_peer::Server::push(sender, wake)`,
with `ark_peer::Apns::sender` reading `_APNS_KEY_FILE`, `_APNS_KEY_ID` and
`_APNS_TEAM_ID` (a file and two ids: the key is never in the repository
or the store). An app's server opts in by calling it; neither money nor
harken does. money is the worked example it is written for: its server
would wake with `Wake::of("money").shown("Money", "New activity in
Money")`, and its shell, on `Woken::Sync`/`Push`, would `Command::Sync`
then schedule "N to review" from the accounts' `unapproved` counts,
opening `/inbox`.

**Why the Notification Service Extension is separate.** Apple runs it as
a process of its own for each alert push marked `mutable-content`, with
about 30 seconds and about 24 MB, and shows what it hands back. It cannot
reach the app's process, runtime or replica, so it never runs the app's
`update`; it only swaps words (modules/ark-notifications/README.md).

## The App Library

Everything native that is the same for every Ark app is one shared
library per toolkit, versioned and tested like arkdb, never generated:
**ArkApp** (`ui/swift/Sources/ArkApp`, beside ArkUI and ArkUICore; nix
builds it per app and platform with `swiftModule`, over the app's own
bindings, and Xcode compiles it into the app's target) and
**ark-app-android** (`ui/kotlin/app`, package `dev.arkui.app`, the gradle
project's `:app` beside ArkUI's libraries; nix builds it once,
`packages.ark-app-android`, compiled against the kernel's Kotlin but
carrying none of it, and every Ark app links its jar as `-ParkAppLib`).
Under both is ark-ffi's `ArkHost` (`rust/ark-ffi/src/host.rs`), the part
written once in Rust: the first run every app shares and its effects (the
server's `/healthz`, `ark_auth::probe`; the code traded; what is
remembered — the login in the secure store, the server, the demo and the
pages seen in a file of the app's), the session opened over the process's
one runtime (`ArkProcess`), its link watched (a denial told the machine
once, the server's refusals said, the first sync told), a link taken
(`<scheme>://auth`, a join, the app's pages, or kept for the app until it
shows), and "Add a Device".

| | Swift (`ArkApp`) | Kotlin (`dev.arkui.app`) |
|---|---|---|
| The process's app host, held for the app's life | `ArkApp<Library>` (an `ObservableObject`: `step`, `showsApp`, `notice`, `moves`), attached to `ArkProcess` while the app runs | `ArkApplication` (makes it at the first launch: `host(launch)`), `ArkApp` (`StateFlow`s: `step`, `showsApp`, `moves`, `notices`) |
| The root | `ArkRoot(app) { WelcomeView() } content: { … }`: the first run until the app shows, the code scanner, the toast | `ArkActivity`: the splash held until the first screen has what it draws, `Content(app)` |
| Browser sign-in | `ASWebAuthenticationSession`, `preferredBrowserSession: .shared` | Custom Tabs |
| Links and the app's phase | `.onOpenURL`, the scene's phase | the launch and new intents (`singleTask`), `ON_START` after `ON_STOP` |
| The secure store | the app's `PhoneSecureStore()`, handed to its library | the same, `PhoneSecureStore(this)` |
| The generic screen holder | `ArkScreen<Runtime: ArkRuntime>` (`page`, `send`, `reload`, `attach`, the after-draw `drawn()` handshake) | `ArkScreen<P, M>` (the same over function references) |
| Background, only with `[background]` | `BackgroundHooks` and `ArkAppDelegate` (the task registered before launch finishes, APNs' token, a remote notification) | `BackgroundHooks` (`ArkNotices` told the process's notices; a notice's tap) |
| Ready signal | one probe: `ArkHost.idle()` at a `version()` that stood still | the same, `Ready.probe("app")` |

**What is generated per app is tiny** (ui/prebuild.py, from app.toml):
Swift's `Generated/ArkApp.swift` — a `@main` holding the app's library
(`[ffi] host`: `ArkApp(MoneyHost(launch: .current(scan:), store:
PhoneSecureStore()))`), its accent, the scene it draws (`[apple] scene`),
and a typealias over `ArkScreen` for each of its screens (`[screens]`) —
and, only for an app that declares `[background]`,
`Generated/ArkBackground.swift` (`backgroundHooks()`, the modules' parts
the delegate reaches) with the delegate adaptor in its `@main`; Kotlin's
`generated/ArkApp.kt` (`class MoneyApplication : ArkApplication()`, its
library and, with `[background]`, its `BackgroundHooks`) and
`generated/ArkActivity.kt` (`class MainActivity : ArkActivity()` drawing
`[android] root`, and the holders); the manifest names the Application.
The app's own part of its host is its app crate's (`money_app::host`,
"The App's Part of Its Host"), run under `ArkHost` by ark-ffi's
`Hosted`; its library is generated from app.toml ("The Phones'
Library"). What is left native is what an app draws
itself beside its generated screens: money's shell host (Shell.swift,
MoneyApp.kt), its theme and marks.

**Background stays off by default.** Nothing of `BackgroundHooks` is
written, and no delegate adaptor, for an app without `[background]`;
`checks.prebuild`'s self-test holds it, and its fixture (money's
app.toml with every key) is compiled: its Kotlin by
`packages.ark-app-android` (`:fixture`), its Swift by
`packages.aarch64-darwin.ark-app-fixture-ios` (mac-compile.yaml). Neither
money nor harken declares any.

**harken, later.** harken is not an Ark app yet (no app.toml). To adopt
the library it needs: an app.toml (`[ffi] host`, `[screens]`, `[apple]
scene`, `[android] root`); its part of its host in harken/app (a
`HostedApp`: its screens' runtimes and what one asks of another) and a
`PhoneApp` in harken/ffi (its replica's open, its demo) exporting a
`HarkenHost` with `host()` and `welcome()`, as `MoneyHost` does; its first run drawn as the
generated `WelcomeView` over `WelcomeApp` in place of its own onboarding
(OnboardingViews.swift, OnboardingScreens.kt); and its process player's
work (`ArkMedia`, `BackgroundWork`) kept as it is, the player being its
module's. Its Kotlin package (`dev.harken.ffi`) is app.toml's `[ffi]
kotlin`.

## Layers

Which crate may depend on which is a table, `layers.toml` at the root,
and `checks.layers` holds it (build.yaml's lint group runs it):
`tools/layers.py` reads `cargo metadata
--no-deps` and fails on a workspace member in no layer, on a dependency
(normal or build, optional or not; never a dev-dependency) its layer may
not use, and on an `[allow]` line that is no longer a violation. The
table says nothing of a stack: it would hold another build tool's graph
the same way.

| Layer | Crates | May use |
|---|---|---|
| kernel | `ark`, `ark-macros` | external pure crates only (serde, sha2, postcard, …) |
| core | `money-core`, `harken-domain` | the kernel; serde |
| runtime | `ark-app`, `ark-module`, `ark-router`, `ark-format`, `arkui-macros` | the kernel and the core |
| modules | the device modules (`ark-haptics`, `ark-camera`, …), `ark-push` | the kernel and the runtime; their platforms' crates behind features |
| app | `money-app` | the core, the kernel, the runtime, the modules it asks |
| adapters | `ark-peer`, `ark-auth`, `ark-ffi`, `arkdb`, the toolkits (`arkui*`, `ark-ui-gen`), the modules, the servers, the shells | anything below them |

**Pure code is held inside too.** Beside each crate of the kernel and
the core is a `clippy.toml` whose `disallowed-methods` and
`disallowed-types` are `std::fs`, `std::net`, `std::process`,
`std::env`, `std::thread::spawn`, `SystemTime::now` and `Instant::now`:
what such a crate needs of the world it is handed (the time is `Data::now_ms`'s,
a file's bytes are its caller's). Their integration tests read fixtures
and are allowed to (an `#![allow]` at each test's top); no library code
is. `money-seed`'s command line, which writes the demo's household to
disk, is a crate of its own (money/demo), an adapter.

**Who is signed in is the kernel's.** A domain's schema embeds two
tables, `sign_in` and `role`, and registers the functions over them
(revoke a session, grant a role, who am I); every peer judges a `Hello`
by those rows. They are domain-agnostic and pure — rows and functions over
`ark::db`, no I/O — so they are a kernel extension (`ark::auth`:
`SignIn`, `Role`, `register`, `feed`, `ark::auth_tables!`) rather than a
port: a port would make each domain restate the same two tables to stay
off an adapter, and what varies between hosts is not the rows but how a
person comes to sign in. That part — the provider, the tokens, the
browser's round trip, the server's routes — is `ark-auth`'s, an adapter,
which re-exports the rows as `ark_auth::rows` for its own code.

**What crosses to the phones is written in the adapter.** A domain's or
an app's type that the phones read is a UniFFI record or enum, but not in
its own crate: `#[ark(ffi)]` (on a row, an input or a `Describe` type)
or `#[derive(ark::twin::Twin)]` (on a plain one; `#[ark(ffi_eq)]` for
one that compares and hashes) writes beside it only its shape — each
field's type by position (`ark::twin::Field<V, F>`) and a hidden macro.
The phones' library, generated, calls them (`ark::twins!(money_core::{…},
money_app::{…})`, from app.toml's `[ffi.twins]`; harken/ffi for harken)
and the twins are written there: `AccountLineFfi`, named `AccountLine` in
Swift and Kotlin, with the conversions both ways (`ark_ffi::Twin`). So
money-core and money-app have no `ffi` feature and no UniFFI; their
Kotlin package is the library's (`uniffi.money_ffi`, harken's
`dev.harken.ffi`).

**The allowlist.** `layers.toml`'s `[allow]` lists today's violations,
each `"<crate> -> <dependency>"` with what removes it; a change that
fixes one deletes its line, or the check fails. In order:

Done: who is signed in (`ark::auth`), the view keys and what an app
reads of them (`ark::views`), harken's routes (harken/app), the phones'
records (generated in the phones' library), the phones' bridge (gone:
generated) and
the demo's peer (money/demo) are out of the core and the app; display
text with a figure in it is the app's (`money_app::words`), and
ark-format's UniFFI exports are an adapter's (ark-format-ffi, namespace
`ark_format`); ark-module is the runtime's, and the modules money asks
are ports of it; a view is drawn by the adapter that draws it
(`arkui::view::element(accounts::view(&page))` in money/iced,
`arkui_web::render(…)` by arkui-web's host) and recorded for the phones by the
generator's main ui/prebuild.py writes. The allowlist is empty.

## Generated, Not Committed

Nothing native of an app is committed. An app is

```
<app>/
├── app.toml      what it is: name, id, accent, platforms, modules, what it asks of each platform
├── icon.svg      its icon, every platform's
├── core/         its domain, and (`demo`) its demo's household
├── app/          ark-app: its state, messages, update, #[view]s and `commands()`
├── server/       when it has one
├── demo/         the demo's household (its seed crate), shots.json, server.sh
└── native/       what is not written in Rust yet (going: see below)
```

**`native/` and `iced/` are being removed** (`bridge/` and `web/` are gone).
The end state is `app.toml`, `icon.svg`, `core/`, `app/`, `server/` and
`demo/`, nothing else committed: the layout is this convention, held by
review, and each of the other directories only shrinks until it is gone.
The phones' library is generated from app.toml ("The Phones' Library");
money's part of its host is money-app's; every money screen is Rust, and
the phones' `Model.swift`, `Bridge.swift`, `Book.kt` and `Start.kt` are
gone into the app library and the host ("The App Library"). The
browser's page is generated from app.toml ("The Browser's Page"), and
`iced/` becomes a generated entry point the same way. (`views/`
is gone: the phones' generator's main is written from app.toml's
`[screens]` and `[views]`.)

If you are an agent, or anyone, about to add to one of these:

- **A screen, or what a screen shows:** write it in `app/` (state,
  message, `update`, a `#[view]` from ArkUI's parts), not as SwiftUI or
  Compose in `native/`, and not as an export of the phones' library for
  a native screen to read. A part another app could use goes in ArkUI, in
  every toolkit.
- **A device feature** (camera, keychain, player, sharing…): a port in
  `ark-module`, implemented in a module under `modules/ark-*`, asked for
  with a `Command`; never a call from `native/`.
- **Domain rules or data:** `core/`, pure.
- **Something the phones must call that has no other home** (rare): a
  generic export of ark-ffi's (`phone_host!`) every app's library writes,
  or a key of app.toml's `[ffi]` it reads; never an app's own Rust for
  its library, which has none.
- **Never** grow `native/`, or
  copy a screen into `iced/`; and never commit a `web/`: the page is
  generated, and what it draws of the app's own is a view in `app/`.

and nix generates the rest, from app.toml, each listed module's
`module.toml` and the Rust views (nix/ark-app.nix, ui/prebuild.py):

| Generated | From | Where it is built |
|---|---|---|
| XcodeGen's `project.yml` (one multiplatform target for the iPhone, the iPad and the Mac), its Info.plist keys, entitlements, the asset catalog (AccentColor from the theme, AppIcon rendered from icon.svg) | app.toml's `[apple]`, the modules' `[ios]` | `<app>-ios-simulator`, `<app>-ios-archive`, `<app>-macos` |
| Gradle's project (settings, the app's `build.gradle.kts` with ArkUI's dependencies and the modules'), the AndroidManifest (permissions, the scheme's links, services, meta-data), the launcher's adaptive and themed icon and the mark (vectors from icon.svg), the splash | app.toml's `[android]`, the modules' `[android]` | `<app>-apk` |
| The SwiftUI and Compose of every `#[view]` | app.toml's `[screens]` and `[views]`, the app's #[view]s | inside both |
| The phones' library: `<id>-ffi` (lib and UniFFI namespace `<id>_ffi`): the twins, the first run, each screen's runtime and the object the phone holds over the app's crate (ui/prebuild.py's `ffi`), as a static library (the iPhone, the Mac) and a shared one (Android) | app.toml's `[ffi]` and `[screens]` | `<app>-android-lib`, `<app>-ios-kernel*` |
| The entry points over the app library: the `@main`, the Application and the activity, the screens' holders, and — only with `[background]` — the background's hookup ("The App Library") | app.toml's `[ffi] host`, `[screens]`, `[apple] scene`, `[android] root`, `[background]` | inside both |
| The generator's main (`ui/prebuild.py views`) | app.toml's `[screens]` and `[views]`, linked with the app's crate | `<app>-ui` |
| The browser's page: `<id>-web` (`main.rs`, `Cargo.toml`: arkui-web's `WebHost` over the app's part, the modules' browser devices), its `index.html` and `favicon.svg` ("The Browser's Page") | app.toml, `[ffi]`'s `app`, `domain` and `demo`, the modules' `[web]` (`ui/prebuild.py web`) | `<app>-web` |
| F-Droid's metadata (`<bundle id>.yml`) | app.toml's `[store]` (`ui/prebuild.py fdroid`) | `apps-fdroid`, `fdroid-repo` |

`nix build .#<app>-native` is the projects as a build lays them out
(`<app>/.native/ios`, `<app>/.native/android`), and **`nix run
.#<app>-prebuild`** writes them into the working tree's `<app>/.native`
(ignored by git) with the phones' libraries linked where the projects
look for them (`.native/ffi/android`, and on a Mac `.native/ffi/ios`), for
Xcode (`xcodegen generate`, then the `.xcodeproj`) and Android Studio.
Signing is never generated: the team is CI's
secret (ios.yaml), and the Android key app.toml names is the one
committed, public, debug key.

### app.toml

```toml
name = "Money"                 # what the person sees
id = "money"                   # us.koon.<id>: the bundle id and the applicationId
version = "0.1"
accent = "green"               # ui/theme.toml's [accent.<name>]
platforms = ["iphone", "ipad", "mac", "android", "desktop", "web"]
scheme = "money"               # money://auth (the sign-in's return), money://<route>
crate = "money-app"            # the app's crate

[screens]                      # each screen's runtime, holders, #[view] and the names it is written under
accounts = { app = "accounts::Accounts", runtime = "AccountsApp", page = "AccountsPage", msg = "AccountsMsg", swift = "AccountsScreen", kotlin = "AccountsHolder", view = "accounts::view::accounts", swift-view = "AccountsView", kotlin-view = "AccountsScreen" }
[views]                        # a screen's further views, and its menu bar's commands
settings = { screen = "shell", view = "shell::view::money_settings", swift-view = "MoneySettingsView", kotlin-view = "MoneySettings" }

[icon]
svg = "icon.svg"               # its full-canvas rectangle the ground, the rest the mark

[apple]
product = "Money"              # Money.app, the scheme
category = "public.app-category.finance"
deployment = "26.0"
[apple.info]                   # Info.plist keys of the app's own (usage strings)
[apple.entitlements]           # the app's own entitlements

[android]
package = "dev.money.android"  # the Kotlin package of its code and its generated screens
min-sdk = 26
target-sdk = 35
keystore = "native/android/debug.keystore"
permissions = ["android.permission.INTERNET"]
features = [{ name = "android.hardware.camera", required = false }]
hosts = ["auth", "join"]       # the scheme's hosts
dependencies = []              # Maven coordinates its own Kotlin needs

[ffi]                          # the phones' library, generated ("The Phones' Library")
host = "MoneyHost"             # the object the phone holds
app = "host::Money"            # its part of its host, in its crate
route = "routes::Route"        # its routes, and what only the phone's shell does
native = "host::ShellAsk"
domain = "money_core::shared"  # the domain its replica's log is of
demo = "money-seed"            # its demo's crate (`seeded_disk`, `open`); none without one
android = ["model"]            # ark-ffi's features each phone's library links
apple = []
[ffi.twins]                    # what else crosses, by crate, as a `use` names it
money_core = ["changes::MoneyChange", "types::{AccountLine, …}"]

[store]                        # what a store lists: F-Droid's .yml is written from it
summary = "A household's budget, shared, every dollar given a job"
categories = ["Money"]
license = "MIT OR Apache-2.0"
website = "https://k2on.github.io/apps/"
source = "https://github.com/k2on/apps"
issues = "https://github.com/k2on/apps/issues"
description = """…"""
```

A module's `module.toml`:

```toml
crate = "ark-media-player"
[ios]
background-modes = ["audio"]
entitlements = { "com.apple.developer.carplay-audio" = true }
[ios.info]
NSMicrophoneUsageDescription = "…"
[android]
permissions = ["android.permission.FOREGROUND_SERVICE"]
dependencies = ["androidx.media3:media3-session:1.5.1"]
services = [{ name = "dev.arkui.modules.PlayerService", foreground = "mediaPlayback", actions = ["androidx.media3.session.MediaSessionService"] }]
meta-data = [{ name = "com.google.android.gms.car.application", resource = "@xml/automotive_app_desc" }]
```

`checks.prebuild` holds every app.toml to what it must say, every module
it lists to one that exists, and its crate to the modules' crates; in a
checkout it also fails when anything is committed where the generated
projects and library go. `checks.money-native` builds the generator and
the projects.

**What is still committed native, and why** (`<app>/native/`): the
shell's host that hosts the generated screens by name (money's
Shell.swift and MoneyApp.kt: the sections' stacks, a register's own
runtime, the account sheet), the app's theme, marks and glyphs, each
going as it moves into the app's crate or into ArkUI; the hand-written
tests over them;
the Maven graph Gradle replays offline (`native/android/deps.json`); and
the debug key. Nothing there is a project file.

## Escape Hatches

- **`Command::Request(MoneyRequest)`** — something only the host can do
  yet, typed: the bank sync (`Refresh`, whose demo differs per shell), a
  message for another of money's runtimes (`Budget { msg }`, `Sheet { msg
  }`). There is no call
  into a shell by name.
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
- nix: `checks.theme`, `checks.prebuild`, `checks.money-native`,
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

`ui/rust/web` draws an `ark_app` `View` in the DOM with Leptos 0.8 — in
the browser alone by default (`csr`), or written as HTML first and taken
over by script (`ssr`, `hydrate`: "The Browser's Modes", below) — the way
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
- **`host`** (feature `host`): an Ark app's page whole, its part of its
  host over a replica in the page, signed in and synced or on its demo
  (`WebHost<W>`, `WebPlace`, `live`), what an app's generated entry point
  runs ("The Browser's Page"); `parts::settings_window` is its Settings.
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
- **`writing`**: long-form writing (`ark_app::writing`, a view's
  `writing(md)`): Markdown's headings, paragraphs with links, emphasis and
  code, lists, highlighted code, callouts and tables, as the HTML that
  says each, in `writing.css`; `arkui::writing` draws the same blocks in
  iced. SwiftUI's and Compose's `Writing` are what is left. The docs site
  (docs/site) is written in it.
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

## Addresses

One route model for every app, on every shell (`ark-router`). An app
declares where it can be once, as a `Route` enum in its domain or app
crate — `harken_app::routes`, `money_app::routes`, the docs'
`docs/site/src/routes.rs` — with `#[derive(Routes)]`, a pattern per
variant (`#[route("/album/:key")]`), and every shell reads that one enum:
the browser's address bar (arkui-web, `Address::of_url`), the desktop's
(arkui's `router`, which says the same URL), the phones' deep links and
back stacks (`route_from_url`, `Route.url()` over UniFFI), a "played from"
in a listening session, a notification's target, a static site's pages.

An **address** (`ark_router::Address`) is a path and a query, as Next.js
has them: the **path** names the content — the page and the ids of what
it shows — and the **query** holds the UI's state over it (an inspector
open, a search, a sheet and what is typed in it). A route reads its own
query keys (a sign-in's `server`, `code`) and passes over every other: a
key no route names is the UI's.

The URL conventions every app keeps (`assert_round_trips` holds each
app's routes to them, route → URL → route):

- **A slash at the end** of every path: `/budget/`, `/menu/`; the root is
  `/`. A link written without it reads the same.
- **Lowercase kebab** for a kind: `/expanding-card/`, `/bank/…`; the
  derive refuses an uppercase or `_` segment in a pattern.
- **One segment, one value**, percent-encoded (`url::encode`): a `/` in a
  key is `%2F` (`/work/johann-sebastian-bach%2Fbwv-988/`), a space `%20`.
- **A row by its id**: 32 lowercase hex digits, a readable slug of its
  name after it, only to read (`/account/<id>/checking/`); the slug is no
  part of which route it is (`Routable::key`). A row is never found by its
  name.
- **A month** is `YYYY-MM` (`month=2026-10`), a day `YYYY-MM-DD`.
- **Query keys** are lowercase kebab (`inspector`, `search`,
  `draft-category`), written in the order set; a key with no value is
  not written, and a flag is `1`.
- **The base** is the site's, taken off whole and put back on: the docs at
  `/apps/docs/` (`/apps/docs/menu/`), money and harken at their server's
  root (`/`). `Address::url_under(base)` writes under it and
  `Address::of_url(url, base)` reads without it; a phone's link
  (`money://budget/`) reads as its path.
- **No old spellings.** A URL that names nothing lands on the app's home
  (`Routable::route_of`); there are no redirects from earlier forms.

money's window says its address itself (`ShellPage.address`, a window's
`Windowed::address`): the place its path, `money_app::Route`'s where money
has one (`/budget/`, `/insights/`, `/account/<id>/<slug>/`, Category
Details as `/category/<id>/<slug>/`) and the shell's own word for Accounts
and Transactions (`/accounts/`, `/transactions/`; `Route::Accounts` is the
phones' All Accounts register), Settings as `/settings/?pane=banks`; the
UI over it its query (`inspector`, `search`, `transaction`, and `month`
when it is not this one). `ShellMsg::Address` reads it back, so a load of
it is the same window. The launch arguments (`?screen=…`) are still read
beside it; folding them onto routes, and the page writing its address
into the bar as it moves (its own history), are what is left.

## The Browser's Modes

How the page is made is chosen per app, by app.toml's `[web] modes`; the
app's crate is the same in each.

| Mode | What the browser is sent | Without script |
|---|---|---|
| `spa` (the default) | index.html and the module (`nix build .#<app>-web`, feature `csr`); the page draws itself | nothing |
| `ssg` | a page of HTML per path the app declares, written natively (`arkui_web::prerender`, feature `ssr`), then the module (`hydrate`), which takes it over | every page and every link between them; no UI state, nothing that changes the data |
| `ssr` | each request's page, written by the app's own server, then the module | everything: a press is a form the server answers |

**Which app uses which.** money and harken are the SPA alone
(`modes = ["spa"]`), served by their own servers beside the sync socket
and sign-in: a household's budget and a person's library are private, so
there is nothing to write ahead for anybody, and a page that draws itself
over what it syncs needs no server to draw it. An app's page is a client
like the phones (arkui-web's `live`): it walks the same sign-in
machine (`ark_auth::onboarding`), trades the code at `/auth/session` so
its session is an `HttpOnly`, `SameSite=Strict` cookie no script reads,
and holds a replica (an ark-peer in wasm, in memory) synced over the
socket, which takes the cookie from a page of the server's own origin.
`?demo` runs the demo household in the page instead, with no server. The docs site is the static
case (`ssg`): public, finite, read more than written. harken's public
pages (a show, an episode, a transcript) are the next. `ssr` is a
capability the layers keep — the same routes, controls and forms, taken by
the server — that no app turns on.

The SPA is what it always was: none of what follows is drawn in it, and
its markup is the same.

**Routes** are addresses (`ark_router::Address`, below): the app's
`WebApp::route` says where it is — its `Route` enum's address and the
UI's query over it — and `WebApp::arrive` brings it back there from one,
so the state a page is written at is the state the module arrives at,
from the address and the data alone. `static_paths` is every page a
static site writes: an address's path, never a query.

**Browser-only calls** (the window, its width, `DomRect`, history, focus,
the keyboard) run only in wasm (`cfg!(target_arch = "wasm32")`).

**Every width without script** (`page::Band`). Natively there is no
window, so a written page is drawn once per band of `[layout]`'s size
classes — compact (the tab bar at the foot), medium (the tab bar at the
top, a split view a column at a time), large (its columns side by side)
and sidebar — each in a `.ark-at-<band>` that theme.css's media queries
(written by ui/theme.py from the same tokens) show alone at its widths.
Each band's controls are the page's, in the order drawn, so a form from
any band is taken against the same draw made again; an element id is
suffixed by its band (`page::unique`). The module's first draw is the same
four bands (`data-ark-width="every"`), so it takes over exactly what was
written; from its next frame it is the window's own width, one band.

**The no-JS contract** (`page`). Written as HTML, every control a draw
makes is recorded in the order it is drawn, by what it says and what it
does (`page::tap`, `page::text`), and wired so that it works with no
script:

- a button is a submit button of the page's one form, `#ark-act`, its
  value `<index>.<check>` (the check a hash of its label); a field is that
  form's `t<index>`. A part that is no button (a table's row, a toggle) is
  pressed through an invisible button over all of it (`.ark-hit`).
- a control's menu (`menu::button`) is the browser's own `popover`; a date
  field is the browser's `<input type="date">` in its place (`.ark-nojs`,
  put away once script runs; `.ark-js` is what is put away until then).
- a form is taken against the same draw made again (`page::take`): every
  field whose text changed sent first, then the press. A press the draw
  cannot make — another page's, a stale one, a label that is not the one
  at that index — is refused (`Refused::Stale`) and nothing runs; a value
  past `page::MAX_TEXT` or not of the form is refused (`Malformed`). So no
  message is taken that the page as it is could not send.
- what needs script — the keyboard's shortcuts and vim layer, a row's
  right-click menu, a hold, a drag, a spring — is not there without it:
  the same commands are buttons, menus and links on the page. With script
  it all behaves as in the SPA, and motion is the shared tokens' as ever
  (ui/docs/motion.md): the written page draws no motion of its own.
- once the module runs it takes the wiring away (the forms' attributes,
  what the writer disabled) before taking the page over, and catches any
  form sent from then on: a press is the click's, as in the SPA.

**The static site** (`ssg`) writes one page per path the app declares,
`WebApp::static_paths` (Next.js's `generateStaticParams`): finite, and
from the data — for a docs site its pages. There is no crawl and no cap;
`arkui_web::prerender` refuses past the sanity bound app.toml's
`[web] static-bound` sets, saying so. A query is never written: each page
is its path at the UI's defaults, and with script the module takes the
address's query once it has taken the page over (Next.js's client-only
`searchParams`). There is no server to run `update`, so each control is
pressed on the app put back where the page opened (`WebApp::rewind`;
`dry_run` counts a change to the data and sends none): one that only
moves the app to another declared path, its query empty, is a link to
that page (`formaction` on the form `#ark-go`, a plain GET); one that
changes the data, or leads to UI state (a query) or to a path not
declared, is drawn **disabled** until script runs. A control's menu is
the browser's own `popover` within the page. `page::dangling` finds every
link on a written page that leads to no written page (arkui-serve's
test holds a fixture site to none), the check a site's build runs over what it
wrote.

**Server-rendered** (`ssr`, crate `arkui-serve`) is a capability no app
turns on: the same page with the app's own server behind its form, one
call — `arkui_serve::site::<A>(auth, config, look, dir)` given to
`ark_peer::Server::site` — beside the sync socket and sign-in and under
the server's layers (`Server::layer`), so one process answers it all.

- **Who** is the ark-auth login in the `ark-session` cookie (HttpOnly,
  same-site `Lax`), set at `/web/session` where a sign-in's code is traded
  for it, and judged by `Auth::authenticate`: the `Who` (account, session,
  roles) the sync socket resolves a token to. Nobody signed in is sent a
  page that links to sign in.
- **Reads**: the app (`arkui_serve::Page::open`) is opened over a replica
  the hub feeds through its in-process dialer, as the bank importer is —
  never a socket — caught up to the hub's head, so its feed and row scopes
  are the socket's, and it asks the domain's queries and views once as
  that `Who` (`Peer::ask`, a query's `Ctx`). The page is written at its
  path and its query, at every band.
- **Writes**: a form is taken against the same draw made again
  (`Served::take`: a stale press `409`, a malformed one refused, the body
  held to `MAX_BODY` and each field to `page::MAX_TEXT`), so the app's own
  `update` runs — a sheet's fields (a payee, an amount, a category, a
  date) taken together, then its press — and what it
  authors is the domain's mutators on the replica, their role checks and
  `Refusal`s run there and again on the hub. Confirmed, `303` to the
  app's new route (UI state in its query, never the server's memory);
  refused, `422`, the page drawn as the form left it, its input kept, and
  the refusal's own words — the native apps' — over it (`.ark-notice`).
- **Forms** carry the `ark-csrf` cookie's token (same-site `Strict`) as
  `csrf`, which must match (`403` otherwise), and a token of their own,
  `once`: sent again — the back button, a second click — a form runs
  nothing and is sent where the first went.

Its tests (ui/rust/serve/tests/pages.rs, with script off, over money's
domain as it is and a fixture page of its groups and bank asks): a group
added is the hub's and a socket client sees it; a form sent twice authors
once; no CSRF token, another's, a stale press and nobody signed in run
nothing; a group's name past its refinement and the bank's Import without
the `bank` role are `422` in the words the socket's client is refused
with, the name as typed; alice's bank ask is on her page and not sam's.
What is left before an app could turn it on: a bank login's waiting page
(the importer's Hosted Link address, refreshing itself until answered),
money's own screens served so, and the replica kept per session rather
than caught up each request.

**Checked**: arkui-serve's tests (tests/static_site.rs) write a fixture
site from its declared paths — a band each, a link to each declared path, the rest
disabled, nothing dangling, the bound refused — and round-trip routes.
`.github/screens/app-web.sh` with `ARK_WEB_MODE=ssg` photographs a static
site by its routes' addresses, with script and with `ARK_WEB_JS=0`
without; with script, a written page fails its shot on any error or any
word of hydration in the console.

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
   sidebar only, Mail's mailboxes), `tab_section` (a group that is a
   place of its own: in the sidebar its heading, short of one a tab whose
   page is an index of its sections, each pushed over it with the way
   back named for the group — so far arkui-web's alone, for the docs),
   `pushes` (`navigationDestination`),
   `split_view`, `toolbar`
   with `bar_item`s by `Placement`, `Tabs::account` (an `account_item`:
   who is signed in, their picture or their initial, every section's
   toolbar's last item at every width, said "Account" and their name:
   `ToolbarAccount` on the phones and the Mac), `searchable`, `inspector`, `hosted`,
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
   `View` with Leptos over the real DOM ("The Browser", below), and its
   `host` runs an app's part of its host in the page ("The Browser's
   Page"): money's page, generated from its app.toml, is money's shell
   and every screen, signed in to the server that serves it and synced
   over its socket as every device is (an ark-peer in wasm; `?demo` is the
   demo household in the page), and nothing of it is money's own code
   but money-app's views; `nix build .#money-web` is the page
   (`checks.money-web`),
   `checks.money-web-sync` holds it to signing in and syncing with
   another device in Playwright's Chromium, and
   `.github/screens/app-web.sh` photographs the demo.
   Left: harken's client, an SSR or pre-rendered page for the docs, the
   vim grammar shared with iced (below), the page's own history.
4. **macOS, iPad, Android tablets and foldables**: money's target is one
   multiplatform target with a macOS destination (`project.yml`,
   `supportedDestinations`), its kernel's Mac slice and `money-macos`
   (flake.nix) built by nix; ArkUI builds for the Mac (its colours
   through `Color.dynamic`, what only the iPhone has behind
   `Platform.swift`'s shims). Never built
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
   regenerated), Share on Android, and media-player and notifications
   everywhere; secure-store's shared scope (its README).
7. **Primitives**: the hand-written layer shrinks to primitives (layout,
   content, token-only surfaces, interaction, the native containers and
   controls, motion), and ArkUI's components become `#[view]`s emitted into
   ui/swift and ui/kotlin. Today's `ScreenParts` are the component layer
   written by hand, the step before that.
8. **The browser's modes**: money and harken are the SPA alone, served
   by their own server (ark-peer's `web::Site`), and money-web is a
   client of it, signed in and synced (arkui-web's `live`): the
   sign-in machine the phones walk, the session an `HttpOnly` cookie the
   socket takes, the replica in memory. Next: a durable replica (ark-peer's
   `Disk` is synchronous and IndexedDB is not), and harken's client.
   The docs site is the static case (`static_paths`, `prerender`,
   `page::dangling`), harken's public pages after it. `ssr`
   (`arkui-serve`) is built and tested and turned on by no app; "The
   Browser's Modes" says what it lacks.


### The Desktop's Own Pages, Moved

money/iced is still the window's own pages (about 9,000 lines with its
tests): it hosts the shell (`sections.rs`) and the Accounts page
(`overview.rs`) from Rust, and draws everything else itself. It goes the
way the phones' screens went: each behaviour another app could use
becomes a shared part, in arkui (iced) and in arkui-web, with its
SwiftUI and Compose beside it where a phone has it, its numbers in
`ui/theme.toml`; then each page is the shared `#[view]` money-app already
has, drawn by `arkui::view`, and the window's own page goes. The desktop
works at every step: a page moves only once the view draws everything
the window's own page did, and the window's tests (`money/iced/src/
tests.rs`, in iced's simulator) are moved onto the view's before its page
is deleted.

**What becomes shared** (each a part with a message of the app's, never
a closure; the app's update decides what it means):

| Behaviour | Today | Shared as |
|---|---|---|
| Keyboard navigation: `j`/`k`, counts, `gg`/`G`, `/`, `Enter`, `Esc` over rows | `main.rs`'s keys over `arkui::vim` | a list's or a table's `keyboard()` modifier (`arkui::vim::Grid` under it), the row under the cursor its `selected`; arkui-web the same over the DOM's keys; `.focusable()`/`onKeyPress` and Compose's `onKeyEvent` on a hardware keyboard |
| Drag to reorder a category or a group, and onto another group | `view.rs` over `arkui::reorder` | the existing `reorder` part on `card_section`, its drop a message (`MoveCategory`, `MoveGroup`), as the phones' Reorder already is |
| A formula field: terms of a filter and a factor, what it gives as typed | `formula.rs` | `formula_field(terms, total, on_change)` over a record money-app works out (`FormulaForm`): rows of a picker and a number field, the figure beside; the parsing and the sum stay money-core's |
| A register's filter as chips | `filter.rs` | `filter_bar(chips)`: chips with a menu each and "Save as Filter…", the conditions money-core's `Filter`, the chips' words money-app's |
| A receipt drawn as its paper, and a photo dropped | `receipt.rs` | the receipt is money-app's `receipts::view` (the phones' already); the drop is `arkui::files`' and the page's input, a `Command` the scanner's module answers |
| Settings' banks and stores | `banks.rs`, `stores.rs` | the shell's Settings panes (`money_settings`), the stores' a pane of its own over money-core's `setup` calls |

**The order the pages move**, each its own green commit: Insights
(already a view everywhere else); the register (`register::view`, with
the filter bar); Transactions; the budget with the reorder and the
keyboard; the inspector's formula; the receipt form; Settings' banks
and stores. Then `sections.rs` hosts only views, the window is
`ark_app::host::Hosting<Money>` over an iced place as the browser's is,
and `main.rs` is a generated entry point (`arkui::run` over the app's
crate, as the phones' library is generated) with money/iced gone.

### What Is Left of money's Own Native and Web Code

- **The phones' shell** (money/native: Shell.swift, MoneyApp.swift,
  MoneyApp.kt): the hosting of the shell's screens by name, the section
  stacks a native `Open` pushes onto, a beside screen for an account's
  register, the account sheet over `ShellPage.household`, Settings and
  Add a Device. Generic in shape, money's only in its names: it becomes
  the app library's (`ArkShell` in ArkApp, its Compose twin in
  ark-app-android), driven by app.toml (which `[screens]` entry is the
  shell, which names it hosts and over which screen, which routes are
  pushed as which beside screen) and generated holders, keeping the
  `-shots` hooks and `Ready` where they are. The account marks
  (`AccountMark`, `KindTile`: a kind's glyph in a chart colour) become
  the record's (a row's glyph and its chart slot, worked out in
  money-app) drawn by ArkUI's `Tile`; money's tab glyphs go into ArkUI's
  glyphs and `ui/theme.toml`. What stays app-committed: the debug key
  app.toml names, and nothing else.
- **The browser** — done: money/web is gone. Its place and sign-in are
  arkui-web's (`host::WebHost<W>`, `WebPlace`, `live`), for any app over
  its domain and its demo's crate; its address is the shell's
  (`ShellPage.address`, read back by `ShellMsg::Address`); what it drew by
  hand (the budget's empty inspector, the household's foot, Settings as a
  modal) is money-app's views (`money_app::window`) and arkui-web's
  `parts::settings_window`; and its entry point and index.html are
  generated from app.toml ("The Browser's Page"). The desktop's window is
  to be hosted over the same `Windowed`.
