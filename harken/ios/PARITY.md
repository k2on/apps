# harken on iOS: parity with the Android app

The Android app (`harken/android/app/src/main/kotlin/dev/harken/android/`,
11,800 hand-written lines) is the spec. Every screen, sheet, control,
gesture, animation and background behaviour it has is listed here with the
iOS counterpart, the API it uses, and the file it is in — or, for what is
not built, why. **Nothing under SwiftUI, AVFoundation, MediaPlayer or
AuthenticationServices has been compiled** (no Mac, no Xcode in the
container this was written in); what *was* compiled and run is said in the
last section. Every API marked † is iOS 26 only; ‡ is an API whose exact
shape is the most likely to need a fix at the first build (README.md,
"First build").

## Screens and sheets

| Android | iOS | API | File | Built |
|---|---|---|---|---|
| Three tabs, Home / Search / Library, one bar (`HarkenApp.kt` `Tab`, `TabBar`) | The system tab bar: Home, Library, and Search in its own place at the bar's end | `TabView(selection:)`, `Tab(_:systemImage:value:)`, `Tab(value:role: .search)` | Shell.swift | yes |
| Tab bar slides away under the full player | The tab bar shrinks to a capsule as a list scrolls down, and the full player covers it | `.tabBarMinimizeBehavior(.onScrollDown)` † | Shell.swift | yes |
| Each tab its own stack, kept while another tab is shown (`Tabs.kt` `TabStack`) | Each tab its own `NavigationStack(path:)`, paths kept in the shell's state | `NavigationStack`, `navigationDestination(for:)` | Shell.swift | yes |
| Tab reselected: back to its root, then to its list's top (`Reselect`, `ScrollsToTop`) | The system does both for a tab's `NavigationStack` | (system) | — | yes, by the system |
| Back at a tab's root goes to Home, with the predictive-back preview (`PageBack` `toFirst`) | Not built: iOS has no back gesture at a tab's root; the tab bar is how one goes Home | — | — | no, no iOS idiom |
| Home (`HomeScreen.kt`): Last played card, Recently added shelf + All songs, Your playlists shelf + New, Albums shelf + See all, offline banner, "getting your library" after a beat, empty state | The same sections; the last-played card on glass, its play button a tinted interactive glass circle | `glassEffect(_:in:)` †, `Glass.tint(_:)` †, `Glass.interactive()` †, `ContentUnavailableView` | HomeView.swift | yes |
| Last played: the session's track, else the log's newest play; pressing it resumes the session (and opens the player) or plays the list it came from | Same rule (`LastPlayed.of`, held by `NowPlayingTests`) | — | Playing.swift, Model.swift | yes |
| Search (`SearchScreen.kt`): field, as-you-type after 150 ms, songs (50) / albums / artists, recent searches (8, this phone's), clear, offline banner, nothing found | The system search field in the Search tab, the same three groups and recent searches | `.searchable(text:prompt:)`, `.onSubmit(of: .search)`, `ContentUnavailableView.search(text:)` | SearchView.swift | yes |
| Library (`LibraryScreen.kt`): Playlists / Albums / Artists / Songs chips, New playlist, album grid (adaptive 150 dp), artists list, songs list | A segmented control under the title (glass in iOS 26), the same four lists, a `+` in the bar | `Picker(...).pickerStyle(.segmented)`, `LazyVGrid(.adaptive(minimum: 150))`, `safeAreaInset(edge: .top)` | LibraryView.swift | yes |
| Album page: header (cover, kind, title, artist link, facts), Play / Shuffle, sections by part, track numbers, performer under the title | **Ahead of Android, at the desktop's** (`view_album`): the cover beside the title in bold, who played most of it (`albums`' `performer`) in the gold and whose it is under it, each opening their page; Play `.glassProminent`, Shuffle `.glass` in one `GlassEffectContainer`; the tracks in runs of one work, each under its composer in small capitals and its title with the run's length, movements unnumbered; at the foot the year, how much, and the label | `.buttonStyle(.glassProminent)` †, `.buttonStyle(.glass)` †, `GlassEffectContainer(spacing:)` † | Pages.swift, Hero.swift, Catalogue.swift | yes |
| Artist page: header, their albums shelf, their songs | **Ahead of Android, at the desktop's**: one page a person has, laid out as a catalogue's composer page — their banner across the page up to the top of the screen, under the status bar and the navigation bar (its back button glass over the picture, the bars light over it until it has scrolled up past the status bar), or their portrait beside their name; "Composer" when the `works` view has any of theirs, else "Artist", and when they lived or else how much of them there is; their most recorded work set apart with a Play when it leads the next; "Well-Known Works" (six, the most recorded then the most tracks) and "See All N Works" by catalogue; their albums; their songs only when they wrote nothing here | `.ignoresSafeArea(.container, edges: .top)`, `.toolbarBackgroundVisibility(.hidden, for: .navigationBar)` ‡, `.toolbarColorScheme(.dark, for: .navigationBar)`, `.onScrollGeometryChange(for:of:action:)` | Pages.swift, Hero.swift, Catalogue.swift | yes |
| — (Android has no work page) | A work's page, as the desktop's `view_work`: its composer in small capitals, the way back to them; its title in bold; catalogue, form, period; its fullest performance set apart ("Most Complete", or "The Recording" for the only one) with a Play; every performance — cover, who played it or "Performer not named", "1992 · 3 tracks · 40 min" — opening its page | the `work` and `recordings` views | Pages.swift | yes |
| — (Android has no performance page) | A performance's page, as the desktop's `view_recording`: laid out as its album's, then the records it is on and everybody on it, the composer first, each opening their page | the `recording`, `credits`, `work` and `recordings` views | Pages.swift | yes |
| — | A Play pressed on a page of works or performances reads the performance's tracks (`recording`) — for a work, its first `recordings` row's — and plays them through the one play path, from `#recording/…` | `Bridge.read`, `Bridge.fullest` | Model.swift, Bridge.swift | yes |
| — | The Library's Artists list has the composers first (`composers`), each once; a song's menu goes to its work's composer | — | LibraryView.swift, Components.swift | yes |
| Playlist page: header, its songs, "gone" and "nothing on it yet" | Same, and a swipe on a row takes it off | `.swipeActions` | Pages.swift | yes |
| Podcasts and Sermons (`LibraryScreen.kt`'s `ShowsGrid`): chips only where there is a show of the kind, a grid of shows (adaptive 150 dp) | Two more segments, on the same terms | `LazyVGrid(.adaptive(minimum: 150))` | LibraryView.swift | yes |
| Show page (`ShowPage`): header with who makes it, what it is about, its episodes newest first, "gone" and "no episodes yet"; an episode's menu has Go to show, not Go to artist | Same | `List`, `Menu` | Pages.swift, Components.swift | yes |
| Show page, followed: how many episodes are not on the server yet, and Fetch 20 older / Fetch all while the feed has more than the library keeps (`library.fetch_older`) | Same, two bordered buttons | `Button`, `.buttonStyle(.bordered)` | Pages.swift | yes |
| A row whose file is not on the server yet says queued, how far its download is, or failed, in place of its length (`downloads`) | Same | — | Components.swift | yes |
| An episode begun resumes a few seconds before where its listener stopped (`my_progress`); a show's page says under each episode how far everybody has got, "you 40% · Ada played" (`show_progress`) | Same | — | Model.swift, Pages.swift | yes |
| An episode's menu: Mark as played / unplayed (`library.mark_played`), and its chapters, each playing it from there (`chapters`) | Same; the chapters a submenu | `Menu` | Components.swift | yes |
| The full player names the chapter that plays | Same, over the scrubber | `TimelineView` | PlayerViews.swift | yes |
| Settings (`SettingsScreen.kt`): account, server and whether connected, Sign out (+ the sentence), About, version tapped 7 times for Diagnostics (remembered) | Same, as a `Form`, the account's face its picture; "4 more taps" said on glass | `Form`, `LabeledContent` | SettingsView.swift | yes |
| Settings' Podcasts and sermons: each followed feed with how much of it is here and why its last read failed, Unfollow; a feed followed by its URL as a podcast or as sermons (`library.subscribe`, `library.unsubscribe`) | Same; Unfollow a swipe or a long press, the kind a segmented control | `Picker(.segmented)`, `TextField`, `.swipeActions` | SettingsView.swift | yes |
| Diagnostics: the peer's numbers, the listening session's numbers, every change with its standing | Same (the desktop's `D`), read again whenever the peer reports a change | `.task(id:)` | SettingsView.swift | yes |
| A song's menu (`TrackMenu`, a bottom sheet): Play next, Add to queue, Add to playlist, Go to album, Go to artist, Remove from this playlist | The row's `…` is a `Menu`, and the row's long press a context menu, with the same six | `Menu`, `.contextMenu` (glass menus in iOS 26) | Components.swift | yes |
| Add to playlist sheet: New playlist, every playlist ticked where the song is, tap toggles | A sheet with the same list | `.sheet(item:)`, `.presentationDetents` | Sheets.swift | yes |
| New playlist dialog: name, `create_playlist`'s own checks said as typed, "Make it" / "Make and add" | A small sheet with the field and the domain's sentence in the footer | `Form`, `TextField`, `.onChange(of:)` | Sheets.swift | yes |
| Queue sheet: now playing, up next (100), tap to play from there | Same, and swipe to remove and drag to reorder — each the session's `remove` / `move` | `List`, `.onDelete`, `.onMove`, `EditButton` | PlayerViews.swift | yes (more than Android: its sheet has no remove/reorder; Media3's controller does) |
| Device picker (`DevicesSheet`): rows by name, ticked where the sound is, the group first ("Kitchen + 1"), slider and mute per row, join `+` / leave `−`, cursor on the output | Same rows (`DevicePicker`, held by `PressTests`) | `List`, `Slider` | Sheets.swift, Session.swift | yes |
| Snackbar for a refusal or rejection, said once | An alert, said once | `.alert(_:isPresented:)` | Shell.swift | yes |
| Account: sign out in Settings | And the account menu at the top right of every tab, as the browser has it: the person's picture (their initials until it arrives), who, the link, Settings, Sign out | `ToolbarItem(placement: .topBarTrailing)`, `Menu`, `sharedBackgroundVisibility(.hidden)` † | Components.swift `AccountMenu` | yes |

## The player

| Android | iOS | API | File | Built |
|---|---|---|---|---|
| Mini player above the tabs: cover, title, artist or "On Laptop" in the accent, play/pause, next, progress hairline | The tab view's bottom accessory, on glass: the same; compact (title and play) while the tab bar is minimized | `.tabViewBottomAccessory` †, `\.tabViewBottomAccessoryPlacement` † ‡ | PlayerViews.swift `MiniPlayer` | yes |
| Nothing playing: no bar | The accessory says "Not Playing" (as Apple Music's does): the accessory is always there | — | PlayerViews.swift | differs, on purpose: hiding it needs `tabViewBottomAccessory(isEnabled:)`, iOS 26.1, above the deployment target ‡ |
| Sideways drag on the bar: left next, right previous, past 25 % of its width or 600 dp/s, springing back | Same thresholds (`Springs.wide`, `Springs.flick`), springing back on `Springs.move` | `DragGesture`, `withAnimation` | PlayerViews.swift | yes |
| The bar and the full player are one card (`ExpandingCard`): drag up, tap to open, cover travels between the faces, dim behind, back gesture previews it down | The full player zooms out of the accessory and back into it, interactively dragged down to close; the cover is the player's own | `.matchedTransitionSource(id:in:)`, `.fullScreenCover`, `.navigationTransition(.zoom(sourceID:in:))` ‡ | Shell.swift, PlayerViews.swift | yes, the system's zoom in place of the hand-built card: the drag up from the bar is not built (a tap opens it, as on iOS) |
| Full player: close, album name, more (the song's menu), cover, title, artist in the accent, scrubber with time gone and left, shuffle / previous / play-pause / next / repeat, volume, device chip + queue chip | Same; the transport in one `GlassEffectContainer`, play/pause a prominent glass circle, the chips glass buttons that morph as the device chip comes and goes; the cover shrinks a little while paused | `GlassEffectContainer` †, `.glassEffectID(_:in:)` †, `.buttonStyle(.glass)` †, `.buttonBorderShape(.circle)` | PlayerViews.swift | yes |
| Shuffle and repeat only while alone (the session has neither) | Same (`PlayerState.alone`) | — | Playing.swift | yes |
| Scrubber follows the finger, another device's scrub while one drags, else the clock; says `scrub` while dragged, seeks on release | Same | `Slider(value:in:onEditingChanged:)`, `TimelineView(.periodic)` | PlayerViews.swift | yes |
| Volume slider: mute + slider; a level from elsewhere sprung to | Same, on `Springs.move` | `Slider`, `withAnimation` | PlayerViews.swift | yes |
| Position ticks twice a second only while counting, read only by the hairline and scrubber | `TimelineView(.periodic(by: 0.5))` around the two of them; the position is counted from the state's clock as drawn, so nothing else recomposes | `TimelineView` | PlayerViews.swift, Playing.swift | yes |

## The session, the sound, the background

| Android | iOS | API | File | Built |
|---|---|---|---|---|
| The library's views spliced, never re-read (`Library.kt`): songs, playlists, albums first, then artists, details, last played; pages held while on the stack | Same: one queue, `LiveView<T>.take()` spliced; a page held by its `@StateObject` | the bridge's `LiveView<T>` | Bridge.swift, Shell.swift `Holding` | yes |
| Writes: create (and put the song on it), add, remove; each kept with its entry for the diagnostics | Same | the generated stubs | Bridge.swift | yes |
| Playlist reorder | Not built: the domain has no function that reorders a playlist (only `create_playlist`, `add_to_playlist`, `add_all_to_playlist`, `remove_from_playlist`), and Android has none | — | — | no, no domain function |
| The listening session through harken's one reconciler (`Listen.kt`, S3b): stepped on every change, one view (`Heard`), every press one way in | Same: the FFI's `Player` stepped on every drain, `Heard.of(view)` | `Player`, `PhoneSound` | Bridge.swift, Session.swift | yes, and run against a `Hall` (`SoundTests`) |
| Queue: play next, add, remove, move, jump, skip | Each a `Press` (`.next`, `.append`, `.remove`, `.move`, `.jump`, `.skip`), or alone done to the player itself | — | Model.swift, Session.swift `Queue` | yes |
| Transfer, volume, mute, groups | `.transfer`, `.loud`, `.join`, `.unjoin`; a dragged slider through the 50 ms throttle (`Drag`) | — | Model.swift, Session.swift | yes |
| ExoPlayer as the reconciler's `PhoneSound` (`ExoSound`): said state kept, commands posted to the main thread | `SoundCore` (the same shape) over an `AVPlayer` engine, one track at a time, playing through its queue on its own | `AVPlayer`, `AVPlayerItem.didPlayToEndTimeNotification` | Sound.swift, AVSound.swift | yes |
| Alone: the screens drive the player through a controller; shuffle the rest, repeat off/all/one | The screens drive `SoundCore` directly; the same shuffle and repeat | — | Sound.swift | yes |
| Audio focus: paused by a call or the headphones out is said as a stall; a transient loss resumes | An interruption or the old route gone is a stall; an interruption that ends with `.shouldResume` resumes | `AVAudioSession.interruptionNotification`, `.routeChangeNotification` (`.oldDeviceUnavailable`) | AVSound.swift | yes |
| Plays with the screen off and the app away (foreground service, wake lock) | Background audio | `UIBackgroundModes: [audio]`, `AVAudioSession` `.playback` | project.yml, AVSound.swift | yes |
| The phone's volume is its media stream, reported to the session; another device's volume set on it | The phone's session volume is its player's own gain (no app may set the system volume: `outputVolume` is read-only), which another device may move | `AVPlayer.volume`, `.isMuted` | Sound.swift | yes, as the player's gain |
| Media session: notification and lock screen, a remote for the session wherever the sound is, "Playing on Laptop" | Now Playing info and the remote commands, a remote for the session (each command a press), "Playing on Laptop" in the artist line | `MPNowPlayingInfoCenter`, `MPRemoteCommandCenter` (play, pause, toggle, next, previous, stop, change position) | NowPlaying.swift | yes; iOS shows the card for the app whose audio last played, so while the sound has only been elsewhere it may not appear |
| Volume keys move the remote device's volume (`DeviceInfo.PLAYBACK_TYPE_REMOTE`) | Not built: iOS gives an app no way to take the hardware volume buttons for a remote device | — | — | no, no public API |
| Headset buttons | The remote commands are what a headset presses | `MPRemoteCommandCenter` | NowPlaying.swift | yes |
| Covers: disk cache by FNV-1a, memory cache bounded by bytes, decoded small, one fetch per URL, failures remembered, warm a list | Same: Caches/covers by `Art.hash`, `NSCache` by cost, ImageIO thumbnails at a few sizes | `CGImageSourceCreateThumbnailAtIndex`, `URLSession` | Covers.swift | yes, a search's covers warmed as its answer arrives |
| The derived square: the name's gradient and a note, picked by FNV-1a over UTF-16 | Same, the desktop's gradients (`RowsTests` pins the hash) | `Canvas` | Theme.swift | yes |
| Covers for a car (`CoverProvider`, `content://`) | Not built: there is no car (below) | — | — | no |
| Android Auto (`Car.kt`): browse tree, search, voice, resumption | Not built. CarPlay audio apps need the `com.apple.developer.carplay-audio` entitlement, granted by Apple on request; a follow-up: a `CPTemplateApplicationSceneDelegate` with a `CPListTemplate` tree over the same views. CarPlay's own now-playing screen already shows what `NowPlaying.swift` says | — | — | no, needs Apple's entitlement |
| The session outlives the activity (`Kept.kt`, `Sessions.kt`, the media service holding it) | One process, one `Model` for its life: the session lives as long as the app does, and background audio keeps the app alive while it plays | — | HarkenApp.swift | yes, by the platform |
| The system splash held until Home has what it draws (`Splash.kt`, 1.5 s ceiling) | `UILaunchScreen`, and Home draws nothing for the milliseconds before its lists are read | `UILaunchScreen` | project.yml, HomeView.swift | partly: iOS takes the launch screen down when the first frame is drawn, and gives no hold |
| Scroll places kept per destination across process death (`Destination.kt`) | Kept while the app runs (the stacks keep their views); not restored after the system ends the app | — | — | partly: state restoration is not built |

## Onboarding

| Android | iOS | API | File | Built |
|---|---|---|---|---|
| Server step: one field, `host`, `host:port` or a URL; https first, http too for local addresses only; `/healthz` probed; sentences for every failure | The same machine (`Address`, `Health`, `Probe`, held by `OnboardingTests`), the field on glass | `URLSession`, `URLError` codes | Onboarding.swift, Model.swift, OnboardingViews.swift | yes |
| Sign in: a button opens a Custom Tab on `/auth/login?redirect=harken://auth`; the code comes back to the activity (`singleTask`) | **No button**, as the browser now has it: a server with nobody signed in goes to sign in by itself, in the system's browser sheet, back on `harken://auth`; once a launch — a sign-in that comes back empty or refused is said on the page and not tried again until the app comes back to the front, so it can never loop | `@Environment(\.webAuthenticationSession)`, `authenticate(using:callbackURLScheme:)` (SwiftUI's `ASWebAuthenticationSession`) ‡, `.onOpenURL`, `CFBundleURLTypes` | Onboarding.swift, HarkenApp.swift | yes |
| Exchange the code (`exchange`), remember the login whole | Same (`Logins`, `Remembered` over `UserDefaults`) | the bridge's `exchange` | Model.swift, Onboarding.swift | yes |
| Opening: "Getting your library from …" after 600 ms, until synced | Same | — | OnboardingViews.swift | yes |
| Denied by the server: back to sign in, the server kept | Same, and the sign-in goes by itself again — once a launch | — | Onboarding.swift | yes |
| Notifications permission step | Not built: iOS needs no permission for the lock screen's now-playing card | — | — | no, nothing to ask |
| "Use another server" | Same | — | OnboardingViews.swift | yes |

## Motion

Android's numbers (`ui/Springs.kt`) are a spring of stiffness 700 at unit
mass: damping ratio 0.88 for where a thing is (`MOVE`, a whisper of
overshoot), 1 for how large it is and what of it shows (`SIZE`), and a
critically damped 2500 for a fade. SwiftUI's
`Animation.spring(response:dampingFraction:)` takes the same spring as its
undamped period `2π/√(k/m)`:

| Android | SwiftUI (Theme.swift `Springs`) | Where |
|---|---|---|
| `Springs.move()`: stiffness 700, damping 0.88 | `.spring(response: 0.2375, dampingFraction: 0.88)` | the mini player's swipe springing back, a volume level from elsewhere, the cover's size on play/pause, the glass chips morphing, the "more taps" note |
| `Springs.size()`: 700, damping 1 | `.spring(response: 0.2375, dampingFraction: 1)` | (kept for a size or clip; the system's zoom does the pages' and the player's) |
| `Springs.fade()`: 2500, damping 1, ~150 ms | `.spring(response: 0.1257, dampingFraction: 1)` | the onboarding's steps, "Getting your library…" appearing |
| `Springs.settle()`: the move spring capped at 300 ms | not expressible: SwiftUI's springs have no cap; the system's interactive dismissals settle themselves | — |
| Page grows out of the tapped cover, the page under it dims 25 %, back shrinks it into the cover (`Motion.kt`) | `navigationTransition(.zoom(sourceID:in:))` with `matchedTransitionSource` on every cover that opens a page (shelves, album grid, playlists and artists lists); the system dims and scales | Shell.swift, Components.swift, LibraryView.swift |
| Back gesture previews the page shrinking, free then harder, never past 10 % (`Springs.preview`) | The zoom transition's own interactive dismiss: the edge swipe and a drag down | (system) |
| A page pulled down from its top goes back (`Pull`) | The zoom transition's drag-down dismiss | (system) |
| A page with nothing to grow from rises 24 dp and fades | A route opened from no cover (a menu's Go to album) zooms from nowhere, which the system draws as its default push | (system) |
| Tabs switch at once | Tabs switch at once | (system) |
| The full player: one card, `p` from 0 to 1, collapsed face out by 20 %, expanded face in from 60 %, the cover travelling between, dim to 40 % | The zoom from the accessory into the full-screen player and back, interactive | Shell.swift |
| A fling through a list: no frame dropped (`FlingTests`) | `List`/`LazyVStack`, covers decoded small off the main thread and cached by size | Covers.swift |

The durations differ where the system draws the move: the zoom
transitions' springs are UIKit's, not settable from SwiftUI.

## Liquid Glass and iOS 26 APIs used

As this was written against them (the iOS 26 SDK's SwiftUI), and where:

| API | Signature as used | Where |
|---|---|---|
| `View.toolbarBackgroundVisibility(_:for:)` (iOS 18) ‡ | `.toolbarBackgroundVisibility(.hidden, for: .navigationBar)` over a banner | Pages.swift |
| `View.toolbarColorScheme(_:for:)` (iOS 16) | `func toolbarColorScheme(_ colorScheme: ColorScheme?, for bars: ToolbarPlacement...) -> some View`, `.dark` while a banner is under the bars, else `nil` | Pages.swift |
| `View.onScrollGeometryChange(for:of:action:)` (iOS 18) | `func onScrollGeometryChange<T: Equatable>(for type: T.Type, of transform: @escaping (ScrollGeometry) -> T, action: @escaping (T, T) -> Void) -> some View`, whether a banner is still under the status bar | Pages.swift |
| `View.glassEffect(_:in:)` † | `func glassEffect(_ glass: Glass = .regular, in shape: some Shape = DefaultGlassEffectShape()) -> some View` | Components.swift (`Offline`), HomeView.swift (last played), LibraryView.swift (New playlist tile), SettingsView.swift (avatar, the taps note), OnboardingViews.swift (mark, field) |
| `Glass.regular`, `.tint(_:)`, `.interactive()` † | `static var regular: Glass`, `func tint(_ color: Color?) -> Glass`, `func interactive(_ isEnabled: Bool = true) -> Glass` | HomeView.swift, SettingsView.swift, OnboardingViews.swift |
| `GlassEffectContainer` † | `init(spacing: CGFloat? = nil, @ViewBuilder content: () -> Content)` | Components.swift (Play/Shuffle), PlayerViews.swift (transport, chips) |
| `View.glassEffectID(_:in:)` † | `func glassEffectID(_ id: (some Hashable & Sendable)?, in namespace: Namespace.ID) -> some View` | PlayerViews.swift (device and queue chips) |
| `ButtonStyle.glass`, `.glassProminent` † | `.buttonStyle(.glass)`, `.buttonStyle(.glassProminent)` | Components.swift, PlayerViews.swift, OnboardingViews.swift |
| `View.tabBarMinimizeBehavior(_:)` † | `func tabBarMinimizeBehavior(_ behavior: TabBarMinimizeBehavior) -> some View`, `.onScrollDown` | Shell.swift |
| `View.tabViewBottomAccessory(content:)` † | `func tabViewBottomAccessory<Content: View>(@ViewBuilder content: () -> Content) -> some View` | Shell.swift |
| `EnvironmentValues.tabViewBottomAccessoryPlacement` † ‡ | `var tabViewBottomAccessoryPlacement: TabViewBottomAccessoryPlacement?`, `.inline` / `.expanded` | PlayerViews.swift |
| `Tab(value:role:content:)` (iOS 18), `TabRole.search` | `Tab(value: Pane.search, role: .search) { … }` | Shell.swift |
| `Tab(_:systemImage:value:content:)` (iOS 18) | `Tab("Home", systemImage: "house.fill", value: Pane.home) { … }` | Shell.swift |
| `matchedTransitionSource(id:in:)` (iOS 18) | `func matchedTransitionSource(id: some Hashable, in namespace: Namespace.ID) -> some View` | Components.swift, LibraryView.swift, Shell.swift |
| `navigationTransition(.zoom(sourceID:in:))` (iOS 18) ‡ on a `fullScreenCover` | `func navigationTransition(_ style: some NavigationTransition) -> some View` | Shell.swift |
| Toolbars, menus, sheets, the search field, segmented controls | take Liquid Glass by default in iOS 26: no API | everywhere |

Plain, well-known SwiftUI was preferred wherever a newer API's shape was
uncertain: the accessory is always present (no `isEnabled:`), the sign-in
uses `authenticate(using:callbackURLScheme:)` rather than the iOS 17.4
`Callback` form, the full player is a `fullScreenCover` rather than a custom
presentation, and no `ToolbarSpacer`, `scrollEdgeEffectStyle` or
`backgroundExtensionEffect` is used.

## What was checked, and what was not

Checked here, on Linux: every file that imports nothing of Apple's but
Foundation and Combine — Rows, Session, Playing, Onboarding, Sound, Bridge,
Model and the generated types — type-checks against UniFFI's Swift for
harken's library (`tools/typecheck.sh`, `checks.harken-ios-typecheck`, Swift
5.8); and HarkenTests (32 tests) compiled with them and run against the
library built for the host, the player's two over a `Hall` — the real
reconciler driving `SoundCore` — each new test falsified once. Every file
parses (`swiftc -parse`).

The catalogue's pages (Catalogue.swift, Hero.swift, the work, performance
and person pages, the bridge's held listings, CatalogueTests) were written
in a container with no Swift toolchain at all: neither type-checked nor
parsed there. Catalogue.swift, Rows.swift, Bridge.swift and Model.swift
are in `tools/typecheck.sh`'s set and are checked by the next run of it.

Not checked: the screens (SwiftUI), the engine (AVFoundation), the lock
screen (MediaPlayer), the sign-in sheet (AuthenticationServices), the
covers (UIKit, ImageIO), the project (`xcodegen`), the xcframework, a
build in Xcode, a simulator, a device, a real server, App Store
Connect.
