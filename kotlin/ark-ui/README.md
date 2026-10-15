# ark-ui

What an Ark app on Android looks like and how it moves, with nothing of any
app in it: package `dev.arkdb.ui`, an Android library on Compose alone — no
ArkDB, no network. The Kotlin twin of `rust/arkui`, the desktop's. harken is
its first caller; `ark-android` draws its onboarding with it. A SwiftUI twin
is planned (`docs/plan-arkui-swift.md`).

It is a project of the app's build, as `ark-android` is:

```kotlin
// settings.gradle.kts
include(":ark-ui")
project(":ark-ui").projectDir = file("../../kotlin/ark-ui")

// app/build.gradle.kts
implementation(project(":ark-ui"))
```

Its tests are JVM unit tests (`:ark-ui:testDebugUnitTest`), which
`harken-apk` runs before it builds the APK, and its sources are held to
ktfmt's kotlinlang style by `checks.kotlin`. Everything another module may
call says `public` (`explicitApi()`).

## What is in it

| file | what it is |
|---|---|
| `Springs.kt` | the numbers everything moves by, said once: `Springs.STIFFNESS` 700, `MOVE` 0.88 (where a thing is: a whisper of overshoot), `SIZE` 1 (how large it is and what of it shows), `REST` 0.0005 of the way, `CAP_MS` 300; `move()`, `size()`, `settle()` (from the release's speed, capped), `noDecay()`; the back gesture's preview curve, `preview(r) = PREVIEW · (1 − e^(−r / GIVE))` with 0.1 and 0.12; `Capped`, any finite spec cut at a time |
| `Motion.kt` | pages: `PageMotion` (told which routes are tabs), `Moving` around every NavHost destination, a page grown out of what was tapped (`Hero`, `rememberHero`, `Origin`, `Tapped`, `Spot`, `PageFrame`, `LocalPageFrame`), a page with no origin rising; the back gesture that previews and then commits (`PageBack` before the NavHost, `Relayed` around it); a page pulled down from its top going back (`Pull`) |
| `ExpandingCard.kt` | a bar that expands under the finger into the whole screen: `ExpandingCardState` (an anchored drag whose offset is one number `p`), `rememberExpandingCard`, `ExpandingCard` with slots for the collapsed face, the expanded face and the piece that travels between them, `ExpandingCardSpec` for its measurements; the settle, the axis split (a sideways drag on the bar is the bar's), the back gesture mapped onto `p` |
| `Art.kt` | the derived square (`Square`, `Art.hash` — FNV-1a over UTF-16, `arkui::art::hash` byte for byte — and `Art.pick`), and `Cover`, a picture over it from the app's `CoverLoader` (`LocalCoverLoader`) |
| `Components.kt` | `Glyph`, `Heading`, `TabTitle`, `PageBar`, `Empty`, `Shelf`, `CoverCard` (a card that opens its page out of its cover), `ListRow` (a picture, two lines, a chevron or what the caller puts at its end), `SheetRow`, `Fetching`, and a row's `MoreButton` with the one shared painter (`LocalMoreGlyph`) and the `RowOrigin` its row says it grows from |
| `Onboarding.kt` | the onboarding's screens with no state of their own: `ServerScreen`, `SignInScreen` (`SignInPhase`), `PermissionScreen`, `OpeningScreen`, and the `OnboardingPage` they are made of |
| `Splash.kt` | `holdSplashUntil(ceilingMs) { ready }`, the system splash kept until the app says, and `keepsSplash` |
| `Theme.kt` | `Space` (the spacings and the corner), `ArkTheme` / `LocalArkTheme` (the squares' gradients; neutral by default, `rust/arkui`'s), `quiet` |
| `Glyphs.kt` | `ArkGlyphs`: the three drawings these pieces draw — `More`, `Back`, `Chevron` — as `rust/arkui`'s Lucide paths, and `glyph` to draw an app's own the same way |
| `Recompositions.kt` | a counter a test installs to see what was composed again, and nothing at all otherwise |

The flow that decides which onboarding screen shows — the server's health,
the sign-in's Custom Tab, the permissions, what is remembered — is
`ark-android`'s, which drives these screens. What a screen is about is the
app's.

## What an app supplies

- **Colours**: a `MaterialTheme`. Every piece here asks
  `MaterialTheme.colorScheme` and `typography`, and writes no colour down.
- **The squares' gradients**, if it has its own: `LocalArkTheme provides
  ArkTheme(art)`, one per side. Without, the neutral greys.
- **Icons**: its own `ImageVector`s, handed to `Empty`, `SheetRow` and the
  rest; `LocalMoreGlyph provides rememberVectorPainter(ArkGlyphs.More)` once
  in its theme, so every row's menu shares one painter.
- **An image loader**: an implementation of `CoverLoader` — where a thing's
  art is, one already decoded at once, the rest loaded — provided as
  `LocalCoverLoader`. With none, only the squares are drawn (a screenshot's
  case). harken's is `Covers`, `HttpURLConnection` and `BitmapFactory` with a
  disk and a memory cache.
- **Which routes are tabs** (`PageMotion(tabs)`), and for an expanding card,
  what its two faces and its travelling piece are.

## Pages that grow out of what was tapped

```kotlin
val motion = remember { PageMotion(tabs = setOf("home", "search", "library")) }
CompositionLocalProvider(LocalPageMotion provides motion) {
    PageBack(motion, blocked = { card.p > 0f })      // before anything that takes back first
    Relayed(motion) {
        NavHost(nav, "home",
            enterTransition = { motion.push(initialState, targetState); EnterTransition.None },
            exitTransition = { motion.push(initialState, targetState); ExitTransition.None },
            popEnterTransition = { motion.pop(initialState, targetState); EnterTransition.None },
            popExitTransition = { motion.pop(initialState, targetState); ExitTransition.None },
        ) {
            composable("home") { e -> Moving(motion, e) { Home() } }
            composable("album/{name}") { e -> Moving(motion, e) { Album(e) } }
        }
    }
}

// On the card a page opens from:
CoverCard(seed = a.name, art = a.art, title = a.name, under = a.by, side = Space.shelf,
    onClick = { nav.navigate("album/${a.name}") }, opens = true)
// …and on the page, where its header's art stands:
Box(LocalPageFrame.current?.art?.modifier ?: Modifier) { Cover(a.name, a.art, Space.header) }
```

The page is laid out once at its full size and drawn scaled, offset and
clipped so its header's art sits on the tapped cover, then opens out; back
runs it the other way. The edge swipe and a pull from the top of the page
preview it toward a limit, and finish or spring back.

## An expanding card

```kotlin
val card = rememberExpandingCard()
ExpandingCard(
    card,
    stand = 80.dp,                                     // what the bar stands on: a tab bar
    onOpen = { scope.launch { card.open() } },
    collapsed = { m -> Bar(modifier = m) },            // leaves the travelling piece's place empty
    expanded = { slot, m -> Full(slot = slot, modifier = m) },
    traveler = { m -> Cover(seed, art, 360.dp, m, corner = 0.dp) },
    onSwipeLeft = player::next,
    onSwipeRight = player::previous,
    openLabel = "Open the player",
)
```

`card.p` goes from 0 at the bar to 1 covering the screen, under the finger
or along the same path for a tap; it is read in layout and drawing only, so
the card moving composes nothing. `ExpandingCardState.held(p, density)` is a
card that stays at `p`, for a screenshot.
