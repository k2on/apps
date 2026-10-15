# ArkUI for SwiftUI: the plan

`kotlin/ark-ui` is what an Ark app on Android looks like and how it moves,
with nothing of any app in it (`kotlin/ark-ui/README.md`,
`docs/plan-android.md`, "ark-ui"). This is the same library for iOS, in
SwiftUI, under `swift/ArkUI`, so that harken on an iPhone — and the next Ark
app on either phone — moves by the same numbers and draws the same squares.
**A plan; no Swift is written yet.** Nothing here has been built or run.

## Where it lives, and why not in the ArkDB package

`swift/Package.swift` is the runtime and the client, built on Linux by
`checks.swift` with nixpkgs' Swift 5.8, which has no SwiftUI. So ArkUI is a
package of its own, `swift/ArkUI/Package.swift`, with two targets:

- **`ArkUICore`** — no SwiftUI, no UIKit: the spring numbers and their
  conversion, the preview curve, the cap, `Art.hash` and `Art.pick`,
  `keepsSplash`. Built and tested on Linux, so `checks.swift` (or a
  `checks.arkui-swift` beside it) pins the same values the Kotlin tests pin:
  `ArtTests`' five hashes, `SpringsTests`' saturation and cap,
  `SplashTests`' four cases. An executable test runner, as `ArkDBTests` is,
  because the devshell has no XCTest.
- **`ArkUI`** — the views, `import SwiftUI`, iOS 17 and later: the first
  release with `Spring` as a value, `CustomAnimation` (which the cap needs)
  and a drag's `velocity`; where iOS 18 has something better
  (`onGeometryChange`, `onScrollGeometryChange`), it is used there and a
  `GeometryReader` stands in below it. Built by `harken/ios`'s Xcode project
  (`project.yml`), which depends on the package by path, as the Android app
  depends on `kotlin/ark-ui`.

`ArkDB` and `ArkDBClient` do not depend on either, and neither depends on
them: exactly the Kotlin split, where `ark-ui` has no ArkDB in it and
`ark-android`'s flow drives `ark-ui`'s onboarding screens. The iOS
equivalent of `ark-android` (the onboarding's machine, `ASWebAuthenticationSession`
for the sign-in, the session) is a separate piece and not this plan.

## The modules, one for one

| Kotlin (`dev.arkdb.ui`) | SwiftUI (`ArkUI`) |
|---|---|
| `Springs.kt` | `Springs.swift` (`ArkUICore`) |
| `Motion.kt` | `PageMotion.swift`, `PageStack.swift` |
| `ExpandingCard.kt` | `ExpandingCard.swift` |
| `Art.kt` | `Art.swift` (hash in `ArkUICore`), `Square.swift`, `Cover.swift` |
| `Components.kt` | `Components.swift` |
| `Onboarding.kt` | `Onboarding.swift` |
| `Splash.kt` | `Splash.swift` |
| `Theme.kt` | `Theme.swift` |
| `Glyphs.kt` | `Glyphs.swift` |
| `Recompositions.kt` | a test hook, `BodyCount.swift` (below) |

## Each API and its counterpart

### Springs

Compose's `spring(dampingRatio, stiffness)` with unit mass is the same
second-order system as SwiftUI's springs, so the numbers convert exactly:
natural frequency ω₀ = √stiffness = √700 = 26.458 rad/s.

| Kotlin | SwiftUI |
|---|---|
| `Springs.move()` — `spring(0.88, 700, 0.0005)` | `.spring(response: 0.2375, dampingFraction: 0.88)`; equivalently `.interpolatingSpring(stiffness: 700, damping: 46.56)` |
| `Springs.size()` — `spring(1.0, 700, 0.0005)` | `.spring(response: 0.2375, dampingFraction: 1.0)`; `.interpolatingSpring(stiffness: 700, damping: 52.92)` |
| `Springs.settle()` — the move spring from the release's speed, capped at 300 ms | `.interpolatingSpring(stiffness: 700, damping: 46.56, initialVelocity: v / remaining)` inside a `CustomAnimation` that returns the target at 300 ms (below) |
| `Springs.noDecay()` | nothing: a SwiftUI settle is always the spring the code asks for; there is no decay to turn off |
| `Springs.preview(r)` = `0.1 · (1 − e^(−r / 0.12))` | the same function, in `ArkUICore` |
| `Capped(spec, ms)` | `struct Capped: CustomAnimation` wrapping an `Animation`, whose `animate(value:time:context:)` returns `nil` (done) at `ms` and the wrapped value before it |

`response` is 2π / ω₀ = 0.2375 s; `damping` for `interpolatingSpring` is
2ζω₀ (46.56 for 0.88, 52.92 for 1). SwiftUI's `initialVelocity` is in units
of the whole distance per second, not points, so a release at `v` points a
second with `d` points left is `v / d`. Compose's rest threshold (0.0005 of
the way) has no SwiftUI parameter: a `Spring`'s `settlingDuration` decides
when it is done, by an epsilon of its own. On a 2,340-pixel page the
difference is in the last pixel and under the cap; the cap is what bounds
it, as it does on Android.

### Pages: `PageMotion`, `Moving`, `Hero`, `PageBack`, `Pull`

The Kotlin pages are not `SharedTransitionLayout`: a lookahead layout cost
the fling 43% per row, so a page is laid out once at its full size and drawn
scaled, offset and clipped onto the tapped art. The same reasoning holds in
SwiftUI, and the same shape is available:

- **Origins.** `Hero`/`Spot` record a frame in the window. SwiftUI reads it
  with `onGeometryChange(for: CGRect.self) { $0.frame(in: .global) }` (iOS
  18; a `GeometryReader` in a background before it), stored in the
  `PageMotion` model, never in view state, so a moving card re-renders
  nothing.
- **The grow.** `matchedGeometryEffect` matches two views in one hierarchy
  under one namespace, and cannot reach across a `NavigationStack` push.
  iOS 18's `.navigationTransition(.zoom(sourceID:in:))` with
  `.matchedTransitionSource` is the platform's own "grow out of the tapped
  card" and comes with an interactive dismiss — but its curve, its preview
  and its corner are Apple's, and it would make the two phones move
  differently. So ArkUI has its own stack: **`PageStack`**, a `ZStack` of
  the pages in a path the app owns, each drawn by `Moving` with
  `scaleEffect(anchor: .topLeading)`, `offset` and a `clipShape` of a
  rounded rectangle at the interpolated rect — `PageFrame.growing`'s
  arithmetic, ported line for line — and the page under it dimmed by
  `PAGE_DIM`. The zoom transition stays the fallback for an app that wants
  the platform's motion more than the shared one.
- **Tabs cut.** As on Android, a tab switch is no animation at all
  (`withTransaction(Transaction(animation: nil))`), and a tab keeps its own
  stack.
- **The back gesture.** iOS has no predictive back; its equivalent is the
  interactive pop, the edge swipe that drags a pushed page off to the
  right. ArkUI's `PageStack` has its own: a `DragGesture` from the leading
  20 points (a `UIScreenEdgePanGestureRecognizer` through a
  `UIGestureRecognizerRepresentable` on iOS 18, which also wins over a
  list's scroll), its progress `r` = translation / width mapped through
  `Springs.preview`, the page shedding that share of its size, moving aside
  and rounding to 28 points, the page under it showing dimmed. Let go past
  half the width or faster than the system's flick, and the page shrinks
  the rest of the way into its card on the size spring; short of it, it
  springs back. This is Android's back preview on iOS, not iOS's slide: the
  two phones then go back the same way, which is the point of the library.
- **The pull.** `Pull` listens to a list's post-scroll overflow. SwiftUI's
  `ScrollView` does not hand that over; the iOS 18
  `onScrollGeometryChange` reports a negative content offset at the top,
  and a simultaneous `DragGesture` on the page decides "at the top, going
  down, at least twice as vertical as horizontal" exactly as `Pull` does,
  with the same `REACH` 0.6, `COMMIT` 0.2 and `FLICK` 1.2.

### The expanding card

| Kotlin | SwiftUI |
|---|---|
| `ExpandingCardState` — an `AnchoredDraggableState` whose offset is `p` | an `@Observable` model holding `p` (0…1) and `travel`, moved by a `DragGesture` |
| positional threshold half the way, velocity threshold 400 dp/s | `onEnded`: the end the finger was moving toward if `abs(value.velocity.height) > 400`, else the nearer end |
| `Springs.settle()` from the release's speed | `withAnimation(Capped(.interpolatingSpring(stiffness: 700, damping: 46.56, initialVelocity: v / remaining), 0.3)) { p = target }` |
| the axis split: `draggable(Horizontal)` on the bar, `anchoredDraggable(Vertical)` on the card | one `DragGesture(minimumDistance: 10)` on the card that locks to the axis of its first movement; horizontal on the bar at rest is the skip, anything else is `p` |
| `PredictiveBackHandler` mapped onto `p` | none on iOS: a downward drag anywhere on the expanded card is its dismiss, as Apple Music's is; the back preview curve applies to that drag's overshoot past the top |
| `p` read in layout and drawing only | `p` read inside `visualEffect` / `.offset` / `.scaleEffect` closures and a `Layout` whose `placeSubviews` reads it, so the faces' `body` is not re-evaluated as it moves |
| `collapsed`, `expanded(slot)`, `traveler` slots | three `@ViewBuilder` closures; the slot is a `Color.clear` whose frame the expanded face reports with `onGeometryChange` |
| `ExpandingCardSpec` | the same struct, in points |

### Art, covers and components

- `Art.hash`: FNV-1a over `text.utf16`, `UInt32` with `&*`, the same five
  pins (`""`, `"a"`, `"Water Music"`, `"Für Elise"`, `"🎵"`). `Art.pick`
  reads the hash as unsigned, as the Kotlin does.
- `Square`: a `Canvas` drawing the gradient and the note in a 100-unit box,
  the tint from an `ArkTheme` in the environment (`@Environment(\.arkTheme)`),
  neutral by default with `rust/arkui`'s three pairs.
- `Cover` and `CoverLoader`: a protocol with `url(of:)`, `cached(_:)` and an
  `async load(_:side:)`; the app injects one through the environment. Not
  `AsyncImage`, which neither shares a decode between rows nor decodes to
  the drawn size.
- `Shelf` → `ScrollView(.horizontal)` over a `LazyHStack` with the page's
  inset; `ListRow`, `SheetRow`, `Heading`, `TabTitle`, `PageBar`, `Empty`,
  `Fetching` → plain views over `Space`; `CoverCard` → a `Button` with a
  `Hero` on its cover.
- `MoreButton` → a `Button` with the three-dot glyph in a 44-point target
  (Apple's minimum, where Material's is 48), labelled "More for …", that
  reads its row's frame on the tap. `RowOrigin` is the same protocol.
- Glyphs: the Lucide paths as `Path` from SVG data (a small parser in
  `ArkUICore`), stroked 2 units with butt caps and miter joins — the same
  shapes, not SF Symbols, for the reason the desktop's are Lucide: one menu
  button is one shape on every client.
- `Theme`: `Space` as `CGFloat` constants, `ArkTheme` in the environment,
  colours from the app's asset catalogue or its own `Color`s — there is no
  Material scheme to read, so ArkUI declares the dozen roles it draws with
  (background, surface-container-high, on-surface, on-surface-variant,
  primary, outline-variant, scrim) and the app fills them.

### Onboarding and the splash

- `ServerScreen`, `SignInScreen` (with `SignInPhase`), `PermissionScreen`,
  `OpeningScreen` and `OnboardingPage`: the same stateless views, a mark (an
  `Image`), a title, a sentence and the buttons, the same sentences.
- `holdSplashUntil`: iOS's launch screen cannot be held. ArkUI's `Splash`
  draws a copy of it (the mark on the launch colour) as the app's first
  view and takes it away when `ready` holds or the 1.5 s ceiling passes,
  through the same `keepsSplash` in `ArkUICore`.

### Recompositions

SwiftUI has no composition count to install a sink into. `BodyCount` is a
test hook: a view modifier that, in DEBUG builds and only when a test has
set a sink, increments a counter in `body`. The scroll test it serves —
nothing re-evaluated per position tick — is then the same assertion.

## What will differ, and why

- **The back gesture is Android's on both.** iOS users expect the interactive
  pop's slide; ArkUI gives them the shrink into the card with the same
  rubber band. That is a decision for the owner before this is built, and
  the zoom transition is the native alternative.
- **Touch targets** are 44 points, not 48 dp.
- **Springs are exact; the rest threshold is not.** SwiftUI decides when a
  spring is done; the 300 ms cap makes the two agree where it matters.
- **Velocity units** are distance-relative in SwiftUI; the conversion is in
  one function, not at every call site.
- **No predictive back for the expanding card**; its dismiss is a drag.
- **Measurement.** The Kotlin numbers are HotSpot and layoutlib's; the
  Swift ones will be Instruments' on a device. Neither phone has been timed.

## Tests

`ArkUICore`'s runner on Linux: the art pins, the preview's saturation, the
cap, the splash's four cases — the Kotlin `ArtTests`, `SpringsTests` and
`SplashTests` value for value. The views: snapshot tests in `harken/ios`
(rendered on macOS by `ImageRenderer`, light and dark), and the motion
tests driven by a `UIHostingController` with a stepped clock, the iOS
counterpart of `MotionTests` — written when there is a macOS runner to
build them on, which this repository does not have today.
