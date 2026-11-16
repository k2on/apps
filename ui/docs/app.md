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
  store holds (read, held, released), the time (and the clock itself, for
  a module's listener to read on its own thread), a change sent to its
  peer (an intent on the log, or a call on the live space for a live
  function: `Peer::change`), its device modules and what wakes it when
  one answers later, a file fetched or kept, a sentence said, the server
  dialled, who is signed in where (`Session`: the demo, the day, the
  login, the server as shown and its base URL (what a file it serves is
  fetched from), the link, what is pending, this device as
  the live space names it and how many connections have opened, the
  version, and whether this host can sign out or add a device), and two
  facts of its toolkit: whether it
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
  `link` and `launched` for a launch's arguments or an address; and
  `ShellAsk`s — what only the toolkit's shell can do, the same for every
  app (Settings or Add a Device put up, its own pages put away for a link,
  signed out) — taken by the host after the call. A route is one of the
  shell's places on every host, a phone's too (its section, its inspector,
  a Settings pane: money's account is its section, which a phone's
  Accounts pushes where it is compact), so no route is asked of a shell.

**Its live space** is the app's too (`HostedApp::live`): given what this
device is (`ThisDevice`: its `Form` — a phone, a tablet, a computer, a
browser — and its name as its person reads it), the app says what its
`Hello` tells the space this device is, the transitions relayed as
ephemeral and how often (`LiveSpace`), or none (the default: no live
space, the log alone). Every host opens its peer on a server with it,
through ark-peer's `Config::standing`: the phones' (`ark_ffi::hosted::config`,
the device from the `Launch` the shell reads — UIDevice's or the Mac's
name, Android's device name, the idiom or the smallest width for the
form), the browser's (`arkui_web::live::standing`, "Firefox on Linux"
from the user agent) and the desktop's (`arkui::host::live::standing`,
the machine's host name). The demo opens with none. harken's is its
listening session (a phone or a computer that can play, the scrubber
relayed at `RELAY_MS`), so every generated client stands in it.

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
| iPhone, iPad, Mac, Android | ark-ffi's `PhonePlace` (the session's views over the process's peer, the process's modules, the login, the link) | ark-ffi's `Hosted<P>`, the `HostApp` under `ArkHost`; one UniFFI object per screen (`ark_ffi::hosted_screen!`; one the phone runs for as long as it shows it, `beside`) |
| Browser | arkui-web's `WebPlace` (the page's replica and store; `Seen` to read) | `arkui_web::host::WebHost<W>`, over the app's `Windowed` |
| Desktop | arkui's `DeskPlace` (the replica on this machine's disk and the views it holds, the desktop's devices, the login in the Secret Service) | `arkui::host::DesktopHost<W>`, over the app's `Windowed` |
| Tests | `money/app/src/host/tests.rs`'s place over the demo household | `Hosting` |

On the phones an app names no code of its own: its library is generated
("The Phones' Library", below), and what `Hosted` asks of it beside its
part (`PhoneApp`) is app.toml's — its name and scheme, the domain its
replica's log is of, its demo's crate. Its part says the rest
(`HostedApp`): what it keeps (`KEEPS`), its first run's words
(`welcome`), how a change is encoded.

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
`MoneyMsg`), taken inside the loop (`take`); a `ShellAsk` the toolkit's
shell would carry out is the part's own in a window (`native`: Settings
shown is its state there). A page written ahead reads three more of it:
its title and description (`title`, `description`) and every path its
export writes (`paths`). The desktop's window is hosted the same way
("The Desktop's Window", below).

**An app with no data** (a static site: the docs) is the same part of its
host over no domain: its views and changes are `ark_app::host::Nothing`,
its runtimes none (its state is the part's own), and it says no first run
(`KEEPS` and `welcome` have none by default). Its page holds no replica,
and its session says only the day, where the site is rooted (`base`) and
what its export prepared for it (`prepared`).

### The Phones' Library

Generated, never committed: `ui/prebuild.py ffi <app>` writes `lib.rs`
and `Cargo.toml` of `<id>-ffi` (lib and UniFFI namespace `<id>_ffi`;
Kotlin's `uniffi.money_ffi`, Swift's types in the app's module) from
app.toml's `[ffi]` and `[screens]`, which nix/ark-app.nix builds
(`ffiSrc`, `crates`) and the iPhone's cargo build adds at `<app>/.ffi`.
It is, in order:

- the twins of what crosses (`ark::twins!`): `[ffi.twins]` by crate, and
  without being listed each screen's page and message (beside its `app`
  type), the app's route (`[ffi] route`), and the first run's records
  and who is signed in as the shell shows it (`ark_app::shell::Household`);
- the first run's object (`ark_ffi::welcome_app!`);
- `Phone`, its `PhoneApp`: `[ffi] app` (the part), `domain` (a function
  giving the domain), `demo` (the demo's crate, whose `seeded_disk(today,
  now_ms)` its `open(disk)` opens), the name and the scheme;
- one object per `[screens]` entry (`ark_ffi::hosted_screen!`, or
  `beside_screen!` for one that says `beside`: a screen the phone runs
  for as long as it shows it, made with its first message; money has
  none, its register being its host's);
- the object the phone holds (`ark_ffi::phone_host!`, `[ffi] host`):
  `new(launch, store)`, `host()`, `welcome()`, each screen by its name
  (its blank page while no session is open), each beside screen by its
  name (`<name>(first:)`), `today()`; and `routeFromUrl` and the route's
  `url()`. The session's `generation()` and what only the phone's shell
  does (`natives()`: ark-ffi's own `ShellAsk`, every app's, taken) are the
  app host's (`ArkHost`), so the app library's shell reads them with
  nothing of the app's.

Its Rust is tested without UniFFI by the app's own tests over the same
pieces (money/app/tests/phone.rs: a demo launch running every screen, the
demo remembered and left, a server that is not there, a launch's
arguments, the replica opened, the demo seeded, a household of two
devices synced through `ark_ffi::hall::Hall`); `checks.ffi` has UniFFI
write its Kotlin and Swift.

### The Browser's Page

Generated, never committed, the phones' library's twin: `ui/prebuild.py
web <app>` writes `<id>-web` — `main.rs` and `Cargo.toml` — and, for an
app drawn in the browser alone, the page that loads it, `index.html`, with
the app's icon beside it (`favicon.svg`), from app.toml: its name, scheme,
accent and version, its part of its host (`[web] app`, else `[ffi]`'s),
its domain and demo where it has data (`[ffi]`), its look (`[web] css`,
`head`, `files`), how its pages are made (`[web] modes`) and what its
export works out first (`[web] prepare`). nix/ark-app.nix builds it
(`webSrc`, `webCrate`), flake.nix's `<app>-web` binds it (`wasmPage`), and
a cargo build of the page adds it to the workspace at `<app>/.web`
(ignored by git). Its `main.rs` is one `arkui_web::WebHosted`, with the
browser's device of each module its crate depends on whose module.toml
says `[web]` (`device`, `port`: ark-scanner's file input, ark-clipboard's
clipboard; `optional` where the device made is an `Option`, none on a page
without one: ark-media-player's `<audio>` and Media Session), and `arkui_web::host::start` for wasm32; natively, for an app
whose pages are written ahead, `arkui_web::export::main` (Export, below).

arkui-web's `host` is the rest, the same for every app: `WebPlace` (the
page's replica and the views it holds; `Seen` to read; none at all for an
app with no data), `WebHost<W>` driving `Hosting` over it and drawing what
the part says (`Windowed`), and `live`, the sign-in and the socket: the
server is whoever served the page, the first run is every app's
(`ark_app::welcome`), and `?demo` runs the app's demo in the page with no
server, seeded to `?today=`. Every page is that `WebHost`, whatever made
it: the SPA, a page the export wrote and the module takes over, a page a
server writes (arkui-serve). The address arrives as a link
(`HostedApp::link`): a launch when its query names a screen
(`?screen=budget&inspector=1`), the address itself otherwise
(`/account/<id>/checking/?inspector=1`), and the browser's address follows
the app's (`Windowed::address`) after every message: a new history entry
for a move the person made, the entry replaced for a refinement (what is
typed in a field, the page arriving where its address says); back and
forward bring the app where the address says. (`data-ark-address`, the part's `address`) beside its ready signal.

### Export

As Expo's `expo export --platform web`: an app is written once, and its
web output is a command, the same for every app, built from its definition
(app.toml and its crate) and nothing else.

```sh
nix run .#money-export-web -- dist   # the SPA: index.html, favicon.svg, pkg/
nix run .#docs-export-web -- dist    # every page written ahead, and pkg/
```

What it writes follows app.toml's `[web] modes`: `spa` is the page that
loads the module, which draws the app in the browser alone; `ssg` is every
path the app declares (`Windowed::paths`) written as HTML that is whole
without script, each with its title and description (`Windowed::title`,
`description`), a canonical link, `sitemap.xml` and `robots.txt`, and the
module that takes it over. The page's crate is its own export: built for
wasm32 it is the module (`arkui_web::host::start`), and natively, for an
`ssg` app, the writer (`arkui_web::export::main`: `<id>-web <dir> --base
/apps/docs/ --site https://… --version …`). Before writing it runs the
app's `[web] prepare` (a function of its crate, native only: the docs draw
their pictures with iced) and hands what it returned to every page as
`Session::prepared`. An app's own CSS, `<head>` and files are `[web]
css`, `head` and `files` (Expo's `public/`), copied beside the generated
`main.rs`; nothing of it is hand-written, and no app names a toolkit:

```toml
[web]
modes = ["ssg"]
app = "app::Docs"            # its part of its host (else [ffi]'s `app`)
css = ["public/docs.css"]
head = "public/head.html"    # `{base}`, `{version}` filled in
files = ["public/docs.js"]
prepare = "export::prepare"  # fn(&Path) -> io::Result<String>
path = "docs"                # published at /apps/docs/, one channel
```

An app asks which platform it runs on only where the platform itself is
the fact (Expo's `Platform.OS`): `ark_app::platform()` is `Ios`,
`Android`, `Mac`, `Desktop` or `Web` (a page written natively included).
Everything else that differs is the toolkit's, or a device module's.

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
over it (harken-app's `MediaSound`).

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
`CameraPage`). A device made once for Android's process, not per activity
(`kotlin-scope = "process"`: ark-media-player's, so an activity made
again by a rotation or a theme never makes a second player), is
registered by the generated Application (`ArkApplication.devices`,
`context` the Application); an app writes nothing native for its modules. An answer
that comes later (the camera's) wakes the app's host, which drains its
runtimes. `checks.prebuild` holds that no app.toml lists modules, that
every module's crate declares itself, and that ark-ffi has a feature for
each device a module's `[phone]` registers. The desktop's devices are
arkui's features, named by a module's `[desktop]` (`feature`), which an
app's generated window turns on (`arkui::host`'s `devices`: ark-clipboard's
`Queued`, ark-scanner's `Chosen`, ark-media-player's `Desktop` — rodio and
MPRIS — under `media-player`; the secure store always).

The iPhone typecheck (`checks.money-ios-typecheck`, ui/swift/typecheck.sh)
compiles the app's modules' Swift and what of the app library Linux can
read on Linux against the generated bindings, so every name they use is
checked.

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

**With no browser** (a phone with none installed: GrapheneOS with
Vanadium removed and no other; a desktop whose opener has nothing to open
a link with) the shell says so to the machine (`Event::NoBrowser`: Android
when Custom Tabs find no activity, `ArkApp.noBrowser`; the desktop when
`client::try_open_browser` fails), and the sign-in page says "No browser
is installed to sign in with. Install one, or sign in with a code from a
device that's already signed in." and offers that code: pasted (Add a
Device's shared link, or its bare code) and redeemed at `/auth/redeem`,
or scanned with the camera. A server older than device codes answers with
its own sentence (`client::OLDER`). Never a WebView in the app instead:
the provider's session is the browser's, and a page drawn inside the app
is the app's, its password typed where the app could read it. The iPhone,
the iPad and the Mac always have `ASWebAuthenticationSession`.

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
  (arkui-shots) asks `DesktopHost::is_idle()` and `version()` (arkui's
  `host`, over `Hosting::idle`/`version`, signed in and the server's data
  here).

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
playback = true            # sound on with the app away: the media player's audio mode and foreground service
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

**Push is off for now** (below, Servers Already Deployed): the push table
and `session.push` are in no schema, the hub wakes nobody, and "Notify Me"
schedules the periodic sync alone. Each piece is behind an off-by-default
`push` feature — money-core's and harken-domain's (the table, and the
function registered: `ark::auth_tables!(App, push)`), ark-peer's (the
hub's wakes, `Server::push`, `Apns::sender`), ark-ffi's (the endpoint
written as the session's push row) and ark-app's (the device registered
for push) — and comes back with them on, as docs/releasing.md says.

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

## Servers Already Deployed

The servers the owner runs are built from an older revision: the one
k2on/os pins (`koon-apps` in its flake.lock, k2on/apps 77da9807 — the
same money server, store path for store path, as main before the app
restructure merged, 7a4f5634). The clients built now — the phones, the
browser page, the desktop — work against that server unchanged, and
nothing a client sends or expects is only a newer server's:

- **The schema is the deployed one.** A client's schema hash is what the
  server's `/healthz` names (`domain <hash> <version> current`): the
  phone finds it the same domain, not newer, and every state hash an
  entry carries is one the client works out too (an empty table is in
  the state hash). The push table is the one table added since, so it is
  off (Background And Notifications). `tests/deployed.json` in money-core
  and harken-domain is that server's catalog (`ark::domain::Catalog`),
  and `tests/deployed.rs` holds the schema to it and every intent a
  client can send (a mutator, a live call) to one the server registers
  with the same input; with `push` on, that the table and `session.push`
  are all that is added.
- **The sync protocol** (`ark::peer`, the frames, the value format) has
  not changed since that revision.
- **Sign-in.** `/auth/login`, `/auth/callback`, `/auth/exchange` (`{"code":
  …}`), `/auth/picture`, the login's JSON and `ARK_DEV_USER`'s straight
  redirect are as they were. What is newer, and how a client gets by
  without it:
  - the browser's session cookie (`/auth/session`, `/auth/logout`): a
    404 or 405 there and the page trades the code at `/auth/exchange`, as
    before; the login then holds its token, which the page remembers and
    the socket says in the cookie's place (`ark_auth::web::session`);
  - device codes (`/auth/device-code`, `/auth/redeem`, Add a Device and a
    join link): refused with a sentence that says the server is older
    (`ark_auth::client::OLDER`), and the device signs in instead;
  - the display name (`ARK_DEV_NAME`, a dev server's): the login's name
    is its id, and every client already says the id when the name is
    empty or the id itself (`login_name`, money's `who`);
  - a provider's other trusted clients and its groups rule: the server's
    own configuration, nothing a client sends.
- **`/healthz`** says `ok` and the `domain` line as before; harken's newer
  `busy scanning` line is read only by the screenshot scripts
  (.github/screens/app-server.sh), against servers built from the commit,
  and an older server that never says it reads as not busy.
- **No other request is new.** harken's `/media` routes, `/sync` and the
  pictures are the same.

`.github/compat-check.sh [REV]` proves it end to end: it builds money's
and harken's servers from that revision (Cachix has them), starts them
with dev auth (money's on its own seed's household), and runs this
build's clients against them — money's phone host
(money/app/tests/deployed.rs: the first run, the address typed, the dev
sign-in, the household synced, a transaction confirmed by the server and
seen on a second phone) and harken's headless client (`harken-peer`: a
playlist made, confirmed, and seen by a second peer). It is not in CI: it
builds the old servers and money's phone host. When the deployed
servers move, the revision in the script, k2on/os's pin and
`tests/deployed.json` move together (docs/releasing.md).

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
| The process's app host, held for the app's life | `ArkApp<Library>` (an `ObservableObject`: `step`, `showsApp`, `introShown`, `notice`, each published only when it changed; `moves`, told on `moved`), attached to `ArkProcess` while the app runs | `ArkApplication` (makes it at the first launch: `host(launch)`), `ArkApp` (`StateFlow`s: `step`, `showsApp`, `moves`, `notices`) |
| The root | `ArkRoot(app) { WelcomeView() } content: { … }`: the first run until the app shows, the code scanner, the toast | `ArkActivity`: the splash held until the first screen has what it draws, `Content(app)` |
| Browser sign-in | `ASWebAuthenticationSession`, `preferredBrowserSession: .shared` | Custom Tabs |
| Links and the app's phase | `.onOpenURL`, the scene's phase | the launch and new intents (`singleTask`), `ON_START` after `ON_STOP` |
| The secure store | the app's `PhoneSecureStore()`, handed to its library | the same, `PhoneSecureStore(this)` |
| The generic screen holder | `ArkScreen<Runtime: ArkRuntime>` (`page`, `send`, `reload`, `attach`, the after-draw `drawn()` handshake) | `ArkScreen<P, M>` (the same over function references) |
| Background, only with `[background]` | `BackgroundHooks` and `ArkAppDelegate` (the task registered before launch finishes, APNs' token, a remote notification) | `BackgroundHooks` (`ArkNotices` told the process's notices; a notice's tap) |
| Ready signal | one probe: `ArkHost.idle()` at a `version()` that stood still | the same, `Ready.probe("app")` |
| The shell (`ArkShell`) | `ArkShell<Library, Shell>`: the shell screen's holder for the app's life, an `ArkSlot` per other screen made again with each session (`ArkHost.generation()`), the names hosted (`hostedScreens`, set once), `ArkShellView` (the sheets over it, Settings and Add a Device, Safari's view, `arkAppearance`), `ArkSettings`, the account face and sheet (`ArkShellPlace`), `ArkHost.natives()` carried out | `ArkShell<H, P, M>` and its `ArkSlot`s, `ArkShellRoot` (`ArkAppTheme` in the app's accent, the first run, then the shell: navigation-compose with Settings pushed on `Transitions`, the tab frame `LocalTabFrame`, the sheets, Add a Device, the account sheet, a Custom Tab, the snackbar, `natives()`) |
| Money in the locale | `ArkNumbers` (`Cents.write` by ark_format) | `LibraryAmounts`, `LocalAmounts` in `ArkAppTheme` |
| Shots mode's warm-up | `ShotsWarm`: an off-screen alert with a text field, the data detectors and the language identifier, the bundle's images decoded, before the walk is timed | none needed (no system work of the kind landed in Android's shots) |

**What is generated per app is tiny** (ui/prebuild.py, from app.toml):
Swift's `Generated/ArkApp.swift` — a `@main` holding the app's library
(`[ffi] host`: `ArkApp(MoneyHost(launch: .current(scan:), store:
PhoneSecureStore()))`), its accent, the scene it draws (with `[shell]`
the generated `ArkScenes`, else `[apple] scene`), and a typealias over
`ArkScreen` for each of its screens (`[screens]`) — and, with `[shell]`,
`Generated/ArkShell.swift` (`ArkScreens`: the shell over the library, a
slot per screen, which hosted name is which screen's view; `ArkScenes`:
the first run, then `ArkShellView`, the commands, `ArkSettings`) —
and, only for an app that declares `[background]`,
`Generated/ArkBackground.swift` (`backgroundHooks()`, the modules' parts
the delegate reaches) with the delegate adaptor in its `@main`; Kotlin's
`generated/ArkApp.kt` (`class MoneyApplication : ArkApplication()`, its
library and, with `[background]`, its `BackgroundHooks`) and
`generated/ArkActivity.kt` (`class MainActivity : ArkActivity()` drawing
`ArkAppRoot` with `[shell]`, else `[android] root`, and the holders) and
`generated/ArkShell.kt` (`ArkScreens`, `ArkAppRoot` over `ArkShellRoot`);
the manifest names the Application.
The app's own part of its host is its app crate's (`money_app::host`,
"The App's Part of Its Host"), run under `ArkHost` by ark-ffi's
`Hosted`; its library is generated from app.toml ("The Phones'
Library"). Nothing native is left an app's own: its shell is
the library's (`ArkShell`), its theme arkui's in its accent, its marks a
record's glyph and chart slot drawn by ArkUI's `MarkTile`.

**Background stays off by default.** Nothing of `BackgroundHooks` is
written, and no delegate adaptor, for an app without `[background]`;
`checks.prebuild`'s self-test holds it, and its fixture (money's
app.toml with every key) is compiled: its Kotlin by
`packages.ark-app-android` (`:fixture`), its Swift by
`packages.aarch64-darwin.ark-app-fixture-ios` (mac-compile.yaml). Neither
money nor harken declares any.

**harken** is being moved onto the library: "harken's Port", below, is
the plan and where it stands.

## harken's Port

harken is ported the way money was: its committed shape becomes
`app.toml`, `icon.svg`, `core/`, `app/`, `server/`, `demo/`, its README
and `.gitignore` (and the debug key app.toml names), every screen a
`#[view]` in harken-app, its host logic a `HostedApp`, the phones on the
shared shell, the browser and the desktop generated, and harken/ios,
harken/android, harken/iced, harken/ffi and harken/player gone. Each step
is pushed to main on its own and keeps harken building and working: the
old clients stay until the generated ones replace them, and are deleted
in the same commit that replaces them.

### Where Each Directory Goes

| Today | Becomes |
|---|---|
| `harken/core` (harken-domain) | `harken/core`, the same crate and name: the domain, pure (layers.toml's core) |
| `harken/player` (harken-player) | split: the reconciler's decisions, its typed memory and the presses (`press.rs`, `reconcile.rs`'s rules, `memory.rs`'s values, `Listening`) into `harken_domain::player`, pure over the session and what a sound said; what drives a peer (`Player`, `config.rs`, the rows it keeps) into the adapters that need it — the server's Home Assistant speakers (`harken/server`) and the app's host through its place (below) |
| `harken/app` (routes, first-run words) | harken-app: every screen's state, messages, update, page record and `#[view]`; `host::Harken` (its `HostedApp` and `Windowed`); `words` (lengths, counts, "played from") |
| `harken/ffi` (hand-written bridge, `HarkenSession`, `Player`, the `Hall`) | generated (`[ffi]`): `harken-ffi` from app.toml like `money-ffi`; the Hall moves to the server's tests (`harken-server`'s `hall`), which already hold the same hub |
| `harken/ios`, `harken/android` | gone: the app library's `ArkShell` from `[shell]`, the generated views, the modules' Swift and Kotlin |
| `harken/iced` | gone: the generated window (`arkui::host::run`), its keys and its walk |
| `harken/demo` | stays: harken-seed, the catalogue, the covers, `shots.json` (rewritten as an in-app walk by ids), `server.sh` |
| `harken/server` | stays, unchanged on the wire (Servers Already Deployed) |
| `harken/iced/icon.svg` | `harken/icon.svg` |

### Screens and Behaviours

Each of harken's screens today (SwiftUI, Compose, iced), the runtime and
view it becomes, and the shared parts it is drawn with; **bold** is a
shared part that is not in ArkUI yet and is added there, on every toolkit,
before harken uses it (ui/docs/primitives.md lists the components).

| Screen today | Runtime (`[screens]`) | Drawn with |
|---|---|---|
| The shell: Home, Library and Search tabs (`Shell.swift`, `HarkenApp.kt`, iced's sidebar and its sections) | `shell::HarkenShell` | `adaptive_tabs` and `section_tab` (Library's kinds as children: Playlists, Albums, Artists, Songs, Podcasts, Sermons), `searchable`, `Tabs::account`, `hosted`; the player's **now-playing accessory** below |
| Home: last played, recently added, playlists, albums (`HomeView`, `HomeScreen.kt`) | `home::Home` | **media row** on a card (last played, with a round play), **artwork shelf** of **artwork tiles** (a `scroll_across`), `card_section` headers with an action (See All) |
| Library: the chips, playlists, albums grid, artists, songs, podcasts and sermons (`LibraryView`, `LibraryScreen.kt`) | `library::Library` | `chip_row`/`chip`, `list_row` (New Playlist, a playlist), **artwork grid** of **artwork tiles** (round for a person), **media row** (a song), `empty_state` |
| Search: songs, albums, artists, recent searches (`SearchView`, `SearchScreen.kt`) | `search::Search` | `searchable`, **artwork shelf**, **media row**, `list_row` for a recent search, `empty_state` |
| Album, artist, work, recording, show and playlist pages (`Pages.swift`, `Pages.kt`, iced's `view.rs`) | `page::Page` (one runtime, its route the page shown; a phone's stack holds one page record per pushed route) | **media header** (artwork beside a title stack, **play/shuffle pair** under it), **media row** (tracks, episodes with a `meter` of how far), a work's heading (a `card_section` header), **figure with label** (a record's small print), the person's banner (`HeroBanner`, already ArkUI's) |
| The now-playing bar and the full player (`PlayerViews.swift`, `PlayerScreens.kt`, iced's `player.rs`) | `player::Player` | **now-playing accessory** (the bar on glass over the tab bar that grows into the full player: SwiftUI's `tabViewBottomAccessory` and zoom, Compose's `ExpandingCard`, iced's bar at the window's foot, the browser's), **artwork** at the player's size, **title stack**, **scrubber** (a drag, its position counted from a **clock** record so it moves with no message), **transport controls**, **progress hairline** (`meter` on a clock), the volume (`VolumeSlider`, made a part) |
| Queue, transcript, devices (`QueueSheet`, `TranscriptSheet`, `DevicesSheet`; `Sheets.kt`, `PlayerSheet.kt`) | `player::Player`'s sheets | `form_sheet`, **media row** (queue rows, reorder through `card_section`'s existing reorder), `list_row` with a glyph (a device, a group), the transcript's lines (a `vstack` of `text`, the line said now in the accent, moved by the clock) |
| Add to playlist, new playlist, follow a show (`Sheets.swift`, `Sheets.kt`) | `sheets::Sheets` (raised by any screen, as money's) | `form_sheet`, `name_field` (the domain's sentence under it), `list_row` with a check, the podcast directory's results as **media rows** |
| Onboarding (`OnboardingViews.swift`, `onboarding/`) | none: the shared first run (`ark_app::welcome`, `harken_app::welcome::copy`) | the generated `WelcomeView` / `WelcomeScreen` |
| Settings, diagnostics (`SettingsView.swift`, `SettingsScreen.kt`) | the shell's `harken_settings` view (`[views] settings`) | `settings_panes`, `list_row`, **figure with label** (diagnostics' numbers), the version tapped seven times a message of the shell's |
| CarPlay / Android Auto browse tree (`Car.kt`) | none drawn: the host answers the media player's `Browse` | `set_children` from the held views (root: Playlists, Albums, Artists, Podcasts, Sermons), `PlayItem` a `start` |

**Behaviours** and where each lives:

- *Tapping a cover opens its page, zooming out of it*: a route (`Route`),
  a pushed page on the section's stack; the zoom is the toolkit's push
  from the tapped **artwork tile** (SwiftUI's `matchedTransitionSource`
  and `.zoom`, Compose's `Transitions` from the tile's bounds), a part's
  own business, never the app's.
- *A song's menu* (play next, add to queue, add to playlist, go to album or
  artist): `ListRow::menu`, already ArkUI's, on the **media row**.
- *The bar's sideways swipe skips*: the accessory's own gesture (Compose's
  `Momentum`, `Springs.released`; SwiftUI's drag), sending the app's
  `Next`/`Previous`; reduced motion a fade.
- *The position moves with no message*: the scrubber, the hairline and the
  transcript read a **clock** (`at_ms`, `since_ms`, `playing`, `length_ms`)
  and count it as they draw (SwiftUI's `TimelineView`, Compose's
  `withFrameMillis`, iced's frame subscription while playing, the
  browser's `requestAnimationFrame`); a drag sends `Seek { at_ms }` once,
  on release, and `Scrub` as it goes (the session's ephemeral scrub).
- *The listening session* (devices, the queue, who is the output, the
  clock): read through the place as a view of its own (`HarkenView::Listening`:
  the account's live part, a view the peer answers from its volatile
  tables as `harken_listening` does), and every press a live call sent as
  a change (`HarkenChange::Press`). **A view over a peer's live part**
  (`ark::views` and ark-peer's `Peer::view` over volatile tables) is the
  one kernel piece this needs.
- *This device's own player*: the host is the reconciler's driver. It
  holds the reconciler's memory (state, not rows a screen reads), asks the
  media player module (`ark_media_player`: `load`, `play`, `seek`,
  `set_now_playing`, `set_controls`), and takes its events (`MediaEvent`)
  through the module's listener as a message of its own (a `Waker`-woken
  `Inbox`, as the camera's answer is); `Remote` commands are presses,
  `Browse` is answered from the views. The plays it counts are changes.
  On every platform the same code: AVPlayer, Media3, rodio and MPRIS, the
  page's `<audio>` and Media Session.
- *Background playback*: off unless app.toml says so. harken's says
  `[background] playback = true`, a new key that gates ark-media-player's
  `audio` background mode and its foreground service (an app that depends
  on the player without it plays only while it shows). While it plays the
  process's runtime is held online (`ArkProcess.work()`), as now.
- *Android Auto and the car*: `ArkMediaService`'s library over the browse
  tree; harken's subclass (`Car.kt`: search, the remote's volume, covers
  through a `content://` provider) becomes the module's own: **search**
  (`MediaEvent::Search { query }`, answered by `set_children`),
  **remote volume** (`set_volume_remote(level, max)`, `Remote { Volume }`)
  and **artwork through a provider** (`ArkArtworkProvider`), each generic.
  CarPlay's templates stay not built (the module's README).
- *Follow a podcast from the directory*: the search goes from the device
  to the directory (Apple's), never through the server; a **fetch** port
  (`ark_module::fetch`: a GET's body, URLSession, HttpURLConnection, ureq,
  the browser's `fetch`) carries it; the feed's address is then
  `subscribe`, as now.
- *Recent searches*, kept on the device only: a durable local row of the
  domain (ark's local instance, as the plays the reconciler counts are).
- *Covers*: `/media/<key>/cover?size=` as today; ArkUI's cover loaders
  (`CoverLoader`, `Covers`) behind the **artwork** part, the derived
  square of its seed while none has come, a screenshot waiting on it
  (`Ready`).
- *Diagnostics*: the shell's (`Session`'s facts: version, pending, the
  peer's numbers through the place).

### Shared Parts to Add (ArkUI, every toolkit)

Laid out once, in `ark_app::view`, of the primitives where nothing native
is needed, and as native parts where a gesture, a decoder or a timeline
is; their motion from `ui/theme.toml`'s `[motion]` through `Springs`/
`Press`/`arkui::motion`, presses `Press.Give` (artwork, tiles) and
`Press.Highlight` (rows), haptics `SensoryFeedback` on a change:

| Part | API | Kind |
|---|---|---|
| a pressable primitive | `pressable(content, tap, Press::Give \| Highlight)` | primitive (a `Button` with `.press`/`.pressRow`, `Modifier.pressable`, an iced button, a `<button>`) |
| artwork | `artwork(url, seed, label).size(ArtSize::Row \| Tile \| Header \| Player).round()` | native leaf (the cover loaders, the derived square, `Ready`) |
| artwork tile | `artwork_tile(art, title, caption, tap)` | primitives |
| artwork shelf, artwork grid | `artwork_shelf(tiles)`, `artwork_grid(tiles)` | a sideways scroll; the grid a lazy native container |
| media row | `media_row(art, title).under(..).trailing(..).playing(..).menu(..).on_tap(..)` | primitives in a row's press |
| media header | `media_header(art, title_stack, play, shuffle)` | primitives |
| play/shuffle pair | `play_shuffle(play, shuffle)` | two `wide_button`s |
| transport controls | `transport(playing, can_back, can_forward, shuffle, repeat, msgs…)` | primitives of glyph buttons |
| clock | `Clock { at_ms, since_ms, playing, length_ms }` | a record the timeline parts count |
| scrubber | `scrubber(clock, scrub, seek)` | native (a drag with a `Selection` haptic at its ends) |
| progress hairline | `hairline(clock)` | native leaf over the clock |
| figure with label | `figure_label(label, figure)` | primitives |
| now-playing accessory | `Tabs::accessory(bar, full, open, close)` | the shell's, native (the bar on glass, the expanding card) |
| volume | `volume(level, change)` | native (`VolumeSlider`) |

### Generated Targets and the Old Clients

- app.toml: `[screens]` above, `[shell]` (screen `shell`, settings
  `settings`, over `sheets`, the hosted names), `[ffi]` (`host =
  "HarkenHost"`, `app = "host::Harken"`, `route = "routes::Route"`,
  `domain = "harken_domain::shared"`, `demo = "harken-seed"`), `[web]
  modes = ["spa"]` (harken-server serves it as money-server does), its
  `[background] playback = true`, its Android `keystore` (harken's
  committed debug key, kept), its accent and icon.
- flake.nix: `arkApps` gains harken; its hand-written `harkenFfi`,
  `harkenIos`, `harken` androidApp and `harken-iced` give way to `arkOf.harken`'s.
- The old clients go in the commit that adds their generated twin: the
  phones' native trees with the generated phones; harken/iced with the
  generated window; harken/ffi with the generated library (its `Hall`
  first moved to harken-server's tests).

Done, in one commit (step 4 below). Beside the plan: `[shell.hosted]`
names a further view of a runtime as well as a screen's own
(`"player.bar" = "player_bar"`, a `[views]` entry over `player`:
ui/prebuild.py's `shell_hosted`), the demo's crate is linked without its
defaults by every generated target (`bare`: harken-seed's command line
fetches covers, which no phone or page runs), harken-seed says what
every app's demo says (`seeded_disk(today, now_ms)`, `open`), the peers'
configuration that harken-player kept moved to the server
(`harken_server::config`), and the F-Droid repository's key and README,
which were under harken/android, are nix/fdroid's.

### Screenshots

harken is back in app-screens (it was left out until ported)
with `demo/shots.json` an in-app walk by ids (`tab:home`, `tab:library`,
`library.row:…`, `player.open`, `player.devices`), on the iPhone, the iPad
in both orientations, Android's phone, tablet and Fold, the desktop and the
browser, in both themes, as money's — only once its walk passes on main.

### Servers

The clients stay compatible with the deployed harken server (77da9807):
no request is added (the directory goes to Apple from the device), the
schema is `tests/deployed.json`'s, `push` stays off. If a client request
changes, `.github/compat-check.sh`'s harken part grows with it.
The generated clients' first run, sign-in and sync are new to that
server, so the check walks harken's phone host against it as money's
(harken-app's tests/deployed.rs: the first run, the dev sign-in, the
server's Favorites synced, a playlist confirmed and seen on a second
device), beside harken-peer's walk; both pass against 77da9807.

### Where It Stands

1. The plan: this section.
2. Shared parts: pressable primitives, artwork (with its tile, shelf and
   grid), media row and header, play/shuffle, transport controls, the
   timeline (scrubber and hairline) and figure with label are built on
   every toolkit (ui/docs/primitives.md), and so are the now-playing
   accessory (`Tabs::accessory`: the bar over the sections that grows into
   the whole window) and the volume (`volume`).
3. harken-app's screens and host: the domain is in core/ (with
   `HarkenChange`); Home, Library, a record's page (album, artist,
   playlist, show), Search and the shell (Home, Library and Search, a
   page pushed over each) are `#[view]`s over their runtimes, and
   `host::Harken` is their `HostedApp` (a route a page pushed over the
   section it came from, an index the library's way), tested over the
   demo library. A screen that is `searchable` on its own (Search, as
   the shell hosts it) is drawn by every toolkit, its field over it.
   The live view is built: an `Opening` names its space
   (`Opening::live`), ark-peer's `Peer::view_in` and `Watched::open_in`
   open a view over the live space, `Store` holds it and reads it afresh
   when a sign-in makes the live machine anew, and with no live machine
   (alone, the demo) it is no rows and no refusal; the domain's
   `listening.session`, `listening.devices` and `listening.scrubbing` are
   `HarkenView::Listening`'s parts, read into `host::Listening` by
   `Listening::read` — on the device, no request added to the server.
   The reconciler is split: its rules, its memory as plain values
   (`Mind`, and the play it counted, `Loaded`, which its driver keeps),
   the presses and the `Sound`/`Say`/`View` ports are
   `harken_domain::player`, pure; harken-player keeps only adapters
   (`Player` over a peer, keeping `Loaded` on a durable local instance,
   `MediaSound`, `config`), and the server's speakers count in memory.
   The player is a runtime (`player::Player`: what plays, from the live
   view while this device stands in the session and from its own sound
   while alone; the full player a `#[view]` of the shared parts) and
   the host drives this device's sound (`host::listen`): the domain's
   reconciler over `sound::MediaSound` (moved here from harken-player),
   the media player module through its port, its answers and every
   press `HarkenChange`s the place sends (the live transitions among
   them, `HarkenChange::of` reading one by name), a play a press of the
   session while joined and of the sound while alone, and the system's
   controls pressed the same way. Tested over a hub (joined) and the
   module's fake (alone). The player is in the shell: `HarkenShell`'s
   accessory hosts `player.bar` (the player's `now_playing` view) and
   `player` (the full player, with the volume: the group's while joined,
   this device's own alone), up while something plays (the host tells the
   shell), grown and put back by the shell, its swipe a skip; the
   reconciler and the sound read one clock, the sound's. A car browses
   through the module (`host::car`: the library's kinds, their records,
   a record's tracks, each held before it is read and answered at the
   next step; a track's id `t|<route>|<n>|<id>` plays its list from
   there; an add queues). `[background] playback` is a key of app.toml
   (ui/prebuild.py gates the media player's `audio` mode, its
   foreground service and `dev.arkui.playback`, which `ArkMediaService`
   reads); harken's app.toml says it once it is written (step 4). The
   sheets are a runtime (`sheets::Sheets`, raised by the host: a
   track's playlists, each pressed to put it on or take it off;
   following a show by its feed's address, from Library's podcasts and
   sermons), and Settings is the shell's (`harken_settings`: General,
   the server and its link and the version; Diagnostics, what waits to
   sync, this device, the connections, the host telling the shell its
   session's facts). Next: the podcast directory's search (a fetch
   port, not built: following is by address until then) and recent
   searches as a durable local row (which needs a place's local
   instance, not built).
4. Generated targets, the old clients deleted: done. harken/app.toml says
   it all (its screens, `[shell]` with the player's bar hosted as a
   further view, `[views]`, `[background] playback`, `[web]`, `[apple]`,
   `[android]` with the committed key, `[store]`, `[channels]`, `[ffi]`),
   and `harken-ffi`, the phones' projects (`harken-native`), the page
   (`harken-web`, `nix run .#harken-export-web`) and the window
   (`harken-desktop`) are generated from it as money's; `host::Harken` is
   `Windowed` (`harken_app::window`: every hosted name harken's views, the
   address the section, the library's kind or the page pushed, Settings
   the window's place, the menu's View and Go). The shell says who is
   signed in (its account face opens Settings), a launch's section, pane
   and look, and `PageClosed` (harken opens no page in the system's
   browser). harken/ios, harken/android, harken/iced, harken/ffi and
   harken/player are gone; the Hall is `harken-server`'s tests/hall.rs.
   The media player is a phone device as the camera is (its module.toml's
   `[phone]`, ark-ffi's `media-player`, `registerMediaPlayer`; `{accent}`
   in a `[phone]` device's Swift is app.toml's), registered by the
   generated entry points — on Android by the Application, once for the
   process (`kotlin-scope = "process"`) — and the host takes it at its
   first step after the open if it came later.
   The browser's `<audio>` with the Media Session API and the desktop's
   rodio with MPRIS are the module's `[web]` (an optional device: a page
   with no `<audio>` has none) and `[desktop]` (arkui's `media-player`),
   so the page and the window play through the same port, and the
   system's media keys and controls press the same presses.
   Every generated host stands in the listening session
   (`HostedApp::live`, above: harken's says a phone or a computer that can
   play, and relays the scrubber), checked against the deployed server by
   tests/deployed.rs: both devices stand in its session and each sees the
   other's playback. The queue, the transcript and the groups' picker
   the iced window drew are not views yet.
5. Screenshots: harken/demo/shots.json is an in-app walk by ids (the
   first run — harken's now opens on Welcome to Harken, as money's on
   its page — Home, Search, Library, Albums, an album, the player grown
   from the now-playing bar by `shell.open-player` and paused, Settings
   from who is signed in), the same list on the phones and the larger
   screens, the desktop's the same walk, and the Mac's and the browser's
   screens by their launch arguments (`-screen albums` and the library's
   other kinds are a launch's); app-screens' `which` no longer leaves
   harken out. Its first run on main (app-screens 43770824445): Android's
   phone, tablet and Fold walked welcome to the album page and then
   stopped at the player, whose elements a walk did not reach (a screen
   hosted inside the grown accessory was taken as under it: fixed in
   `ark_app::shots`, a hosted name carrying the modal it is in); the
   desktop has no player shot (its media player device is not wired,
   so nothing plays there); the iPhone's job hung in the Mac's install
   of money's build and was cancelled with nothing taken. With that
   fixed (app-screens 43786482522, a76729a6) Android's phone, tablet and
   Fold and the desktop take every shot, light and dark, the player
   included, and publish them. Not yet: the iPhone's, the iPad's and
   the Mac's — the iOS job (43787509355) sat in "The apps, built by
   nix" for over an hour, as 43771365729 did before it was cancelled
   at its limit (that one's log ends in the install of money's
   simulator build; the harken archive had logged "Cannot allocate
   memory" from CoreSimulator on the same Mac), so what holds the Mac's
   build is the next thing to find; the browser's are taken by hand
   (app-web.sh is not in CI). The covers on the phones and the desktop
   are the derived squares: the server seeded by server.sh answers no
   cover the clients draw (the browser's demo, playing from the
   catalogue's addresses, has them). The open issue it waited on is
   found and
   fixed: a hub opened on `harken_seed::seeded_disk` (a log standing at
   its start on a snapshot of rows) took a device at cursor 0 for one
   that had it all, and the device took the fill's first part at seq 0
   for one it had; both now compare the state (`Store::empty_state`), so
   a seeded hub fills a device on its first hello
   (harken-app's tests/seeded_hub.rs).

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
money_app::{…})`, from app.toml's `[ffi.twins]`, harken's the same way)
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
(`arkui::view` by arkui's host,
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
└── debug.keystore  the one public Android debug key app.toml's `keystore` names
```

**`iced/` is being removed** (`bridge/`, `web/` and money's `native/` are
gone; revelation's `native/` is its own Kotlin, not yet an Ark app's).
The end state is `app.toml`, `icon.svg`, `core/`, `app/`, `server/`,
`demo/` and the debug key, nothing else committed: the layout is this
convention, held by review, and each of the other directories only
shrinks until it is gone.
The phones' library is generated from app.toml ("The Phones' Library");
money's part of its host is money-app's; every money screen is Rust, and
the phones' `Model.swift`, `Bridge.swift`, `Book.kt` and `Start.kt` and
then its shell (`Shell.swift`, `MoneyApp.swift`, `MoneyApp.kt`), theme,
glyphs and marks are gone into the app library, arkui and the host ("The
App Library"). The
browser's page is generated from app.toml ("The Browser's Page"), and
`iced/` becomes a generated entry point the same way. (`views/`
is gone: the phones' generator's main is written from app.toml's
`[screens]` and `[views]`.)

If you are an agent, or anyone, about to add to one of these:

- **A screen, or what a screen shows:** write it in `app/` (state,
  message, `update`, a `#[view]` from ArkUI's parts), never as SwiftUI or
  Compose of the app's, and not as an export of the phones' library for
  a native screen to read. A part another app could use goes in ArkUI, in
  every toolkit.
- **Something of the shell** (a section, a tab's glyph, a toolbar, an
  inspector, what is hosted where, Settings): the shell's `#[view]` in
  `app/` and app.toml's `[shell]`; what every app's shell does goes in
  ArkApp and ark-app-android (`ArkShell`), a value in `ui/theme.toml`, a
  glyph in ArkUI's glyph sets, a mark in the record (its glyph and chart
  slot, drawn by `MarkTile`).
- **A device feature** (camera, keychain, player, sharing…): a port in
  `ark-module`, implemented in a module under `modules/ark-*`, asked for
  with a `Command`; never a native call of the app's.
- **Domain rules or data:** `core/`, pure.
- **Something the phones must call that has no other home** (rare): a
  generic export of ark-ffi's (`phone_host!`) every app's library writes,
  or a key of app.toml's `[ffi]` it reads; never an app's own Rust for
  its library, which has none.
- **Never** commit a `native/`, or
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

[shell]                        # the phones' shell, the app library's ArkShell, from these names
screen = "shell"               # the [screens] entry whose view is the sections, tabs and glyphs, toolbars, search, inspectors
settings = "settings"          # the [views] entry that is Settings
commands = "commands"          # the [views] entry of the menu bar's commands
over = "sheets"                # the screen drawn over every section (its sheets and dialogs)
account = "household"          # the hosted name drawn as who is signed in (face and account sheet)
section = "section"            # the shell page's fields: the section showing (the ready signal's route),
appearance = "appearance"      #   Light or Dark, who is signed in (ark_app::shell::Household),
household = "household"        #   a page shown in the system's browser while set,
browse = "settings.plaid"      #   and the message that says it closed (with its `url`)
closed = "PageClosed"
[shell.hosted]                 # each name the shell's view hosts and the screen whose view it is
budget = "budget"
"accounts.register" = "register"

[icon]
svg = "icon.svg"               # its full-canvas rectangle the ground, the rest the mark

[apple]
product = "Money"              # Money.app, the scheme
# scene = "…"                  # the scene its @main draws, for an app with no [shell]
category = "public.app-category.finance"
deployment = "26.0"
[apple.info]                   # Info.plist keys of the app's own (usage strings)
[apple.entitlements]           # the app's own entitlements

[android]
package = "dev.money.android"  # the Kotlin package of its generated screens and entry points
# root = "…"                   # the composable its activity draws, for an app with no [shell]
min-sdk = 26
target-sdk = 35
keystore = "debug.keystore"    # the one public debug key every build signs with: committed, the only file not generated
permissions = ["android.permission.INTERNET"]
features = [{ name = "android.hardware.camera", required = false }]
hosts = ["auth", "join"]       # the scheme's hosts
dependencies = []              # Maven coordinates beyond ArkUI's template (its graph is ui/kotlin/android/deps.json)

[ffi]                          # the phones' library, generated ("The Phones' Library")
host = "MoneyHost"             # the object the phone holds
app = "host::Money"            # its part of its host, in its crate
route = "routes::Route"        # its routes (what only the phone's shell does is ark-ffi's `ShellAsk`, every app's)
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

**What is still committed native, and why**: for money, only the debug
key (`money/debug.keystore`): Android signs every build with it so one
APK installs over the last on any machine, and a key cannot be generated
the same twice. Its shell is the app library's (`ArkShell`, drawn from
`[shell]`), its theme arkui's in its accent, its tab glyphs the shell's
view's names in ArkUI's glyph sets, its account marks the records' glyph
and chart slot drawn by ArkUI's `MarkTile`, and its Maven graph ArkUI's
(`ui/kotlin/android/deps.json`, which every app on the app library
replays). Revelation, not on the app library, keeps its own Kotlin and
resources in `revelation/native/android`.

## Escape Hatches

- **`Command::Request(MoneyRequest)`** — something only the host can do
  yet, typed: the bank sync (`Refresh`, whose demo differs per shell), a
  message for another of money's runtimes (`Budget { msg }`, `Sheet { msg
  }`). There is no call
  into a shell by name.
- **What the shell provides a part** — the screens it hosts by name
  (`\.hostedScreens`, `LocalHostedScreens`) and, on Android, the frame a
  tab's page sits in (`LocalTabFrame`), set once by the app library's
  shell. Nothing is an app's own: a row's mark is its glyph and chart slot
  (`MarkTile`), a glyph is ArkUI's by its Lucide name.
- **A hand-written screen beside a generated one** — none of money's, on
  any platform: the desktop's window is money-app's views too.

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
  in cents, the row's menu in order), arkui with `host`, `clipboard` and
  `scanner` (`arkui-host`), the generated
  money-desktop built (`checks.money-desktop`), money-ffi (`--no-default-features`: the session's day).
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
- **`host`**: the page is an Ark app's whole, its part of its host
  (`Windowed`) over a replica in the page, signed in and synced, on its
  demo, or with no data of its own (`WebHost<W>`, `WebPlace`, `live`): the
  one thing arkui-web drives, whatever made the page. `host::start` is
  what an app's generated entry point runs ("The Browser's Page"):
  **`run`** draws it in the browser alone, **`hydrate`** takes over a page
  written ahead; a message is **`send(m)`**: the app's part takes it and
  the page is drawn again, Leptos keeping the elements that are the same
  (a focused field keeps its keys and its caret), and the browser's
  address and title follow the app's. `parts::settings_window` is its
  Settings.
- **`specimens`**: arkui-web's own parts, live, over a little state of
  its own, what goes over a window drawn open in its frame: what a page
  about ArkUI shows of a part (`ark_app::shell::specimen`, the docs').
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
| `ssg` | a page of HTML per path the app declares, written natively by its export (`arkui_web::export`, `prerender`, feature `ssr`), then the module (`hydrate`), which takes it over | every page and every link between them; no UI state, nothing that changes the data |
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
`Windowed::address` says where it is — its `Route` enum's address and the
UI's query over it — and `HostedApp::link` brings it back there from one
(`WebHost::arrive`), so the state a page is written at is the state the
module arrives at, from the address and the data alone. `Windowed::paths`
is every page a static site writes: an address's path, never a query.

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
`Windowed::paths` (Next.js's `generateStaticParams`): finite, and from the
data — for a docs site its pages. There is no crawl and no cap. A query is never written: each page
is its path at the UI's defaults, and with script the module takes the
address's query once it has taken the page over (Next.js's client-only
`searchParams`). There is no server to run `update`, so each control is
pressed on the app put back where the page opened (`WebHost::rewind`;
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
   The docs site is the static case (`Windowed::paths`, the export,
   `page::dangling`), harken's public pages after it. `ssr`
   (`arkui-serve`) is built and tested and turned on by no app; "The
   Browser's Modes" says what it lacks.


### The Desktop's Window

money/iced is gone: the desktop draws money's shared views as every other
platform does. Its entry point is generated from app.toml (ui/prebuild.py's
`desktop`, the page's twin: `<app>/.desktop`'s main.rs and Cargo.toml,
never committed, built by nix/ark-app.nix's `desktopSrc` as
`<id>-desktop`; `nix run .#money-desktop`), and is `arkui::host::run::<W>()`
over a `Desktop` of app.toml's name, scheme, accent, version, mark, domain
and demo. arkui's `host` (feature `host`) is the browser's host over iced:

- **The place** (`DeskPlace`): the app's replica of its domain in its data
  directory (`$XDG_DATA_HOME/<scheme>/<server>`), read on a thread at
  launch while the window says so; the views it holds; the desktop's
  devices (`devices`); who is signed in where. `-demo [day]` is the demo's
  household, as the Mac's `-demo` is.
- **Signing in** (`live`): `--server` (a dev server on this machine by
  default) and `--user`; the machine every shell walks, the system's
  browser back on a loopback port, the login kept in the Secret Service;
  the session shown once the server's data is here, as the page's.
- **The window**: `Windowed::window` drawn by `arkui::view::shell`, each
  name it hosts its `hosted` view, `over` above it, `settings` in its
  place under a bar with Done (a Mac's Settings window is a window of its
  own; here it is the window's place, as before).
- **The keys** (`keys`): the existing layer, not a new one. The app's
  commands (`Windowed::commands`) by their shortcuts and in the command box
  (`arkui::command`, `:` or Ctrl-K, a group per menu); `arkui::shell::
  Keyboard`'s vim layer over the rows a view draws (`arkui::view::cursor`,
  arkui-web's `row_drawn` twin): `j`/`k`, counts, `gg`/`G`, `Enter`, `/`
  and `n`/`N`, `h`/`l` and `Tab` between panes, the sidebar's cursor the
  section shown, `?` the keys panel (`arkui::which`).
- **Its screenshots** (`walk`): arkui-shots' `run_walk` hands each step
  that names an element by its id to the window, which takes it against
  its views (`ark_app::shots`), as the phones' walks are; keys and waits
  stay the driver's. money's `desktop.shots` are such a walk.

**What money/iced had that the shared views do not yet**, each to come
back as a shared part with a message of the app's (never a closure), on
every toolkit where it applies, its numbers in `ui/theme.toml`:

| Behaviour | Shared as |
|---|---|
| Drag to reorder a category or a group, and onto another group | the existing `arkui::reorder` on `card_section`, its drop a message (`MoveCategory`, `MoveGroup`), as the phones' Reorder already is |
| A formula field: terms of a filter and a factor, what it gives as typed | `formula_field(terms, total, on_change)` over a record money-app works out; the parsing and the sum money-core's |
| A register's filter as chips, "Save as Filter…" | `filter_bar(chips)`, the conditions money-core's `Filter`, the words money-app's |
| The category dropdown under a cell (YNAB's) | the search picker already drawn under its field (`arkui::search_picker`) |
| Visual mode, the budget's bindings (`cc`, `cg`, `rc`, `rg`, `J`/`K`, `{`/`}`) | done: a menu item's keys (`MenuItem::keys`, `.keys("cc")`, `.keys_from_last("J")`) run it on the cursor's row or a visual selection (`v`, `vig`), on the desktop (`arkui::host::keys`, `view::cursor`) and in the browser (arkui-web's `keys`); money binds `cc`, `rg`, `{`, `}` on a group's heading and `K`/`J` on a category. Left: `cg` and `rc` (New Group is a command, a category has no Rename) |
| A receipt's paper on the desktop, a photo dropped with no scan asked | `receipts::view` is the phones' already; a drop with nothing waiting a `Windowed` message |
| Settings' stores pane | a pane of money's `money_settings` over money-core's `setup` calls |
| The appearance a launch or Settings asks (`Windowed::appearance`) | done: the window's theme over the system's (`DesktopHost::theme`) |
| `Esc` to put a sheet away; `Shift-F10` and the menu key on the cursor's row | done: the sheet or question on top recorded as it is drawn (`cursor::modal_drawn`), and `context_menu::on_row`'s `Keyed` |
| The notice's and confirm's buttons | done: ArkUI's (`style::quiet`, `action`, `destructive`), not iced's own |

### What Is Left of money's Own Native and Web Code

- **The phones' shell** — done: money/native is gone. Its shell is the
  app library's (`ArkShell` in ArkApp, its Compose twin in
  ark-app-android), driven by app.toml's `[shell]` (which screen is the
  shell, its Settings and commands, what goes over every section, which
  names it hosts as which screen's view, the shell page's fields it reads)
  and generated holders (`ArkScreens`), keeping the `-shots` hooks and
  `Ready` where they were. No route is pushed by a phone: a route is one
  of the shell's places on every host (an account its section, which
  Accounts pushes where it is compact; every account's Transactions; a
  bank Connected Banks in Settings), so the register is the host's screen
  like any other and no screen runs beside the host. The account marks are
  the records' (a kind's glyph and its chart slot, `accounts::kind_glyph`
  and `kind_slot`) drawn by ArkUI's `MarkTile` (the iPhone's filled SF
  Symbols, Android's mark glyph set, iced's and the browser's glyphs); the
  tab glyphs are the shell's view's names in ArkUI's glyph sets (`layers`
  and `link` moved into Android's); the theme is arkui's in app.toml's
  accent (`ArkAppTheme`, `arkWindow`); the Maven graph is ArkUI's. What
  stays app-committed: the debug key app.toml names, and nothing else.
- **The browser** — done: money/web is gone. Its place and sign-in are
  arkui-web's (`host::WebHost<W>`, `WebPlace`, `live`), for any app over
  its domain and its demo's crate; its address is the shell's
  (`ShellPage.address`, read back by `ShellMsg::Address`); what it drew by
  hand (the budget's empty inspector, the household's foot, Settings as a
  modal) is money-app's views (`money_app::window`) and arkui-web's
  `parts::settings_window`; and its entry point and index.html are
  generated from app.toml ("The Browser's Page").
- **The desktop** — done: money/iced is gone ("The Desktop's Window").
