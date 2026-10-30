# arkui for Android

What every Ark app on Android shares, with nothing of any app in it: the
Kotlin twin of `rust/arkui` and of ArkUI for SwiftUI (`swift/ArkUI`).
Package `dev.arkui`, on Compose and Material 3 alone — no ArkDB, no
kernel. Its sources are `src/dev/arkui/`, and each app compiles them as
part of itself (`kotlin.srcDir` in its `app/build.gradle.kts`, and
`shared = [ "kotlin/arkui" ]` in the flake's `androidApp`):

- `harken/android`, which this was drawn out of;
- `money/android`;
- `showcase/android`, the docs' captures of the components.

There is no Gradle library project yet: a source directory several apps
compile is one fewer moving part than a module each depends on, while
every app's Maven graph is the same.

## What is here

**Harken's look, as it was** (`docs/plan-android.md`, "ark-ui"): the
springs (`Springs`), the page motion and its back plumbing (`Motion`),
the expanding card the player is (`ExpandingCard`), the derived squares
and covers (`Art`), the reorderable list (`Reorder`), the tabs, the
components (`Heading`, `PageBar`, `ListRow`, `SheetRow`, `CoverCard`,
`Empty`, `Fetching`, `MoreButton`, …), the volume, the spacings (`Space`)
and `ArkTheme`, the onboarding's pages without their state
(`ServerScreen`, `SignInScreen`, `PermissionScreen`, `OpeningScreen` —
each now taking an app's own sentence, placeholder and extra actions) and
the splash's keeping (`holdSplashUntil`).

**Finding a server** (`server/`): what an address could mean (`Address`),
whether a `/healthz` is an Ark server running this app's domain
(`Health`), and asking each URL in order (`Probe`). Plain Kotlin over
`HttpURLConnection`, held by JVM tests (harken's `ServerTests`).

**Material You** (`MaterialYou.kt`, added for money): `MaterialYou`, the
wallpaper's scheme on Android 12 and later or the app's own; the
expressive shapes (`ExpressiveShapes`) and an emphasised type scale
(`expressiveTypography`); `Pill` and `Tone`, a value in a tinted capsule
(positive, neutral, caution, negative, from the scheme's containers);
`Meter`, a bar that springs toward a goal; `Stat`, a figure under its
label; `SectionHeader`; and `Stepper`, a title stepped back and forth.

**Money as a locale writes it** (`Amounts.kt`, `AmountField.kt`): minor
units formatted and parsed in the phone's locale and currency
(`Amounts`, `LocalAmounts`), drawn in tabular figures (`AmountText`), and
typed on the number pad with the symbol before it (`AmountField`).

**Charts** (`Charts.kt`): `Donut`, a ring of parts with a figure in its
middle, and `Bars`, the same parts against the largest — in the scheme's
colours (`chartPalette`), drawn in on a spring, each read by TalkBack as
the sentence the caller writes.

**Swipe to act** (`Swipe.kt`): `SwipeToAct`, a row that does one thing
when swiped toward the end, Gmail's way — the action's colour and glyph
from under it, a tick where letting go acts, the row home after — with
the same action on the row for TalkBack.

## Verified

The pieces harken uses are its tests' and screenshots' (`harken/android`).
The Material You pieces are money's tests' (`AmountsTests`) and its
screenshots'; none has been seen on a phone.
