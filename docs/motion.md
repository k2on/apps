# Motion and Interaction

How things move, and how they answer a finger or a pointer, in every Ark
app: harken and money, on Android (Compose, `kotlin/arkui`), on the iPhone
(SwiftUI, `swift/ArkUI`) and on the desktop (iced, `rust/arkui`). Read this
before any change that adds, removes or retunes motion, a gesture, a press
or a haptic, and run the checklist at the end on the diff before pushing.

The ideas are drawn from two places and translated, not copied. One is Emil
Kowalski's design-engineering skills,
[github.com/emilkowalski/skills](https://github.com/emilkowalski/skills)
(MIT), which are written for the web — CSS transitions, Motion, React
Native — and whose rules are restated here in each toolkit's terms. The
other is Apple's *Designing Fluid Interfaces* (WWDC 2018), which those
skills also lean on, and which `Springs.kt`, `Theme.swift` and `motion.rs`
already follow. Where a web rule has no native meaning (hover gating,
`will-change`, the 300 ms tap delay) it is left out; where the platforms
add something the web lacks (haptics, the system's own transitions, the
predictive back gesture) it is added.

Everything below names the shared tokens it maps to. A rule that has no
token yet says so, and the token goes into the shared library, not into an
app: two things that move together must move alike, and they only do if
they read the same number.

## The Principles

**Motion has to earn its place.** Each animation answers "why does this
move?" with one of five reasons: *feedback* (the interface heard the
touch), *spatial consistency* (where a thing came from or went), *state
indication* (a change is made legible), *preventing a jarring change*
(content would otherwise teleport), or, only on rare first-time moments,
*delight*. "It looks nice" is not a reason on anything seen more than a few
times a day. Deleting an animation is a legitimate outcome of a motion
change, and often the best one.

**How often it is seen decides how much it moves.** An action repeated a
hundred times a day — a vim motion, Ctrl+K, a tab switch — has no animation
at all, because at that frequency any duration reads as lag. An action seen
tens of times a day gets motion that is barely there. Sheets, dialogs and
the player get the standard treatment. Onboarding and success moments get
the delight budget. This is the single rule agents most often get wrong,
because each added animation is defensible alone and together they make an
app feel slow.

**A move is a spring, not a curve over a time.** A tween has a length:
interrupted halfway it either finishes first or restarts and jumps, and its
speed breaks either way. A spring has only a target and a velocity, so sent
somewhere new mid-flight it turns from where it is drawn at the speed it is
drawn at. A flick carries its speed into the settle; a tap starts it from
rest; both are done in about the same time. All three libraries already
say this in their headers. This is the native form of the web rule "use
transitions, not keyframes, for anything that can be interrupted": on
these platforms the interruptible primitive is the spring.

**Critically damped unless a finger threw it.** A thing moved because it
was tapped, or because the app moved it, arrives and stops — an overshoot
there is the interface showing off, and the eye reads it as a wobble. A
thing let go while still moving has momentum the finger gave it, and a
little overshoot is that momentum spending itself. So the release picks its
spring by its speed, and only a flick bounces.

**Response starts on touch-down.** The moment a finger lands, something
changes under it: a cover gives, a row lights. Waiting for the lift to show
anything is the latency people actually perceive, even at 0 ms.

**The finger and the content move together.** During a drag the thing
tracks the finger one to one from where it was grabbed, continuously, and
can be grabbed again mid-animation and reversed without waiting. Letting go
hands the finger's velocity to the spring, so there is no seam.

**Less motion is gentler motion, not none.** Under reduced motion what
travels, scales or overshoots becomes a short cross-fade in place; what
fades still fades and what changes colour still does, because those carry
meaning without asking the eye to follow anything.

**Draw it moving; don't lay it out moving.** Move with translation, scale,
alpha and clip, which the toolkit can apply at draw time. Never animate a
width, height, padding or offset that forces layout every frame.

**The three apps are one product.** Android, iPhone and desktop share
token values so that the same album opening on each feels like the same
album opening. Where a toolkit cannot express a rule (iced has no group
opacity), it says how it approximates it, in the code's doc comment.

## The Shared Tokens

The numbers everything moves by, said once per toolkit. The Android column
is `kotlin/arkui/src/dev/arkui/Springs.kt`, `Press.kt`, `Haptics.kt` and
`Accessibility.kt` as they stand on the Android motion branch
(`claude/sleepy-faraday-lcbow5-android-motion`), which is on its way into
`claude/sleepy-faraday-lcbow5`; until it lands the branch still has the
older `Springs.MOVE = 0.88` and no `Press`, `Haptics` or reduced motion.

| Concept | Compose (`dev.arkui`) | SwiftUI (`ArkUI`, `ArkUICore`) | iced (`arkui::motion` and friends) |
|---|---|---|---|
| Stiffness / response | `Springs.STIFFNESS` 700 (unit mass) | `Springs.stiffness` 700, `Springs.response` = 2π/√700 ≈ 0.2375 s | `motion_tokens::RESPONSE` (the same 2π/√700); `motion::SMOOTH` rides it, and `dialog::SPRING`, `sheet::SPRING`, `reorder::SLIDING`, `swipe::BACK` and `compact::SPRING` are `SMOOTH` |
| A move, nothing threw it | `Springs.MOVE` 1.0, `Springs.move()` | `Springs.move` (damping 1) | `motion::SMOOTH` (damping `MOVE`) |
| A move a flick threw | `Springs.MOMENTUM` 0.8, chosen by `Springs.settle(velocity, density, reduced)` | `Springs.momentum` 0.8, chosen by `Springs.released(velocity:from:to:)` | `motion::THROWN` (damping `MOMENTUM`), chosen by `motion::released(speed)` (`reorder::settling`) |
| What counts as a flick | `Springs.FLICK` 500 dp/s, `Springs.isFlick` | `Springs.thrown` 500 pt/s | `motion_tokens::FLICK` 500 px/s (`motion::released`); `reorder` hands on speed unless held still for 60 ms |
| A size, clip or dim | `Springs.SIZE` 1.0, `Springs.size()` | `Springs.size` | `motion::SIZE` (damping `SIZE`) |
| Something going, quickly | `Springs.fade()`, `FADE_STIFFNESS` 2500 (≈150 ms) | `Springs.fade` (≈150 ms) | `motion::FADE` (`FADE_RESPONSE`, ≈150 ms) |
| How far a thing that comes in where it stays rises | `MotionTokens.RISE` 8 dp, `PinnedBar.RISE` | `MotionTokens.rise` 8 pt (`InlineHeader`) | `motion_tokens::RISE` |
| The longest a settle may take | `Springs.CAP_MS` (450 ms), `Capped` | — (SwiftUI springs are not capped) | `motion::CAP` (`CAP_MS`) on `reorder`'s settle; closed form otherwise, `LAST_FRAME` 50 ms past rest |
| Where a flick lands | `Springs.project`, `Springs.nearest`, `DECELERATION` 0.998, `SNAPPY` 0.99 | `Momentum.project`, `Momentum.target`, `Momentum.normal`/`snappy` | — |
| Velocity as the spring wants it | `Animatable.animateTo(…, initialVelocity)` in px/s | `Momentum.relative(_:from:to:)` (share of the gap per second) | `Slide::moving_at(speed)` in px/s |
| A drag past its edge | `Springs.preview(r)`, `PREVIEW` 0.1, `GIVE` 0.12; Apple's rubber band `Springs.band(x, limit)` (`RUBBER_BAND` 0.55), the player card past its ends | `Momentum.band(x, limit:)`, Apple's rubber band (`MotionTokens.rubberBand` 0.55) | `motion::rubberband(x, d)` (`RUBBER_BAND` 0.55), a list past its ends (`scroll::elastic`); `swipe::band(x)`, `STRETCH` 180 px |
| No coasting after a fling | `Springs.noDecay()` | — | — |
| Reduced motion | `reducedMotion()`, `LocalReducedMotion`, `ReducedMotion.ALWAYS_ANIMATE`, `unlessReduced(spec)`, `keepMotion()` | `\.reducesMotion`, `Motion.alwaysAnimate`, `.motion(_:value:)`, `.motionTransition(_:)`, `.motionScale(_:)`, `Animation.reduced(_:)`, `Springs.calm` | `motion::reduced()`, `ALWAYS_ANIMATE`, `ARK_REDUCE_MOTION`, `Spring::or_instant`, `Shown::new` (becomes `FADE`) |
| Press | `Modifier.pressable(press = Press.Give \| Press.Highlight)`, `Press.SCALE` 0.97, `Press.HIGHLIGHT` 0.1 | `.buttonStyle(.press)`, `.buttonStyle(.pressRow)` | the widget's pressed colour; no scale |
| Haptics | `SensoryFeedback(Feedback.Success \| Selection, trigger)`, `View.perform` | `.sensoryFeedback(.success \| .selection, trigger:)` | none |
| Drawn moved | `graphicsLayer { }`, `drawWithContent` | `scaleEffect`, `offset`, `opacity` | `motion::transformed(content, by, scale, veil, radius)` |

Two things follow from the table. The Swift spring and the Compose spring
are the same spring — SwiftUI's `response` is the period `2π/√(k/m)` of
Compose's stiffness — so a value tuned on one is the value on the other.
And iced's springs are the same spring too: `motion::SMOOTH`, `THROWN`,
`SIZE` and `FADE` are built on `motion_tokens` (`RESPONSE` is the period
of the phones' 700 stiffness), and every module's spring is one of them,
so a new iced motion uses those rather than adding another number.

### Translating the Web's Curves

The skills give strong cubic-béziers for each job. Here each job has a
spring instead, because the spring is interruptible and the curve is not:

| The web says | Why | Here |
|---|---|---|
| Enter or exit: strong ease-out, `cubic-bezier(0.23, 1, 0.32, 1)` | Fast at first, so the user sees it answer at once | A critically damped spring from rest (`Springs.move`, `SMOOTH`) has the same shape: most of the way in the first third, then a long soft landing |
| Moving on screen: strong ease-in-out, `cubic-bezier(0.77, 0, 0.175, 1)` | Gathers, travels, settles | The same spring; a spring started with velocity (a release) is the ease-in-out's back half |
| Hover or colour: `ease` | Gentle, no sense of travel | `Springs.fade` / `FADE`, or no animation |
| Constant motion, progress: `linear` | Progress should not ease | Not a spring: the scrubber and a hold's fill follow the value directly |
| `ease-in` on UI | Starts slow, delays the moment being watched | Never. No toolkit here has a token for it, and none should get one |
| Under 300 ms for UI | A 180 ms dropdown feels faster than a 400 ms one | `Springs.move` is all but there in ≈280 ms, `Springs.fade` in ≈150 ms; `CAP_MS` bounds a settle at 450 ms whatever the release |
| Press 100–160 ms | Felt, not watched | Down at once (`snapTo`), up on `Springs.size` |

Where a tween is genuinely needed — a platform API that takes nothing else —
the tween goes into the shared library as a named token beside the springs,
with the reason in its doc comment, and is an ease-out. There is none
today: SwiftUI's `Springs.calm`, reduced motion's stand-in, is `Springs.fade`,
the same critically damped spring as Compose's `Springs.fade()`.

## What Moves, and What Doesn't

### The Frequency Rule

| How often | Here | Decision |
|---|---|---|
| Keyboard, or 100+ times a day | every vim motion (`j`/`k`, `gg`, `^d`), the cursor, `/` search, Ctrl+K's command box, which-key, a menu or picker opened from the keyboard, switching tabs, stepping the month again and again | **No animation.** It is there on the frame the key goes down. A scroll to keep the cursor in sight (`arkui::scroll::reveal`) is a snap, not a glide |
| Tens of times a day | press feedback, a row lit, a row sliding out of a drag's way, a track's cover breathing as it plays and pauses, a glass chip appearing | Barely there: under ~150 ms perceived, small distances, `Springs.fade`/`size`, never an overshoot |
| Occasional | a page opening from its cover, a sheet, a dialog, the player card, a glass button growing into its menu, a toast | Standard: `Springs.move`/`SMOOTH`, the path symmetric in and out |
| Rare or first time | onboarding's pages, a welcome, the first sync landing, a bank connected | The delight budget: a longer beat, a stagger, a little more travel — still never blocking a control |

Keyboard-initiated motion is disqualified, not judged. The web skills'
example is Raycast, which has no open or close animation at all; this
repo's equivalent is the command box, the which-key panel and the menus,
which the desktop apps open from the keyboard all day.

### Decision Table

| Element | Enter | Exit | Spring | Notes |
|---|---|---|---|---|
| Press on a cover, card, tile | gives to 0.97 at once | back on `Springs.size` | `Press.Give`, `.press` | rows never shrink (below) |
| Press on a row | highlight at once | fades on `Springs.fade` | `Press.Highlight`, `.pressRow` | a full-width row moving reads as the list jumping |
| Page from its cover (hero, zoom) | grows out of what was tapped | shrinks back into it | `Springs.move` / system zoom | the origin is the tapped thing, always |
| Page with nothing to grow from | rises a little and fades | fades | `Springs.move`; Compose `Transitions.pushIn`/`popOut` (a `SHIFT` slide) | restored pages, deep links |
| Tab switch | at once | at once | none (Compose `EnterTransition.None`, `Transitions.none`) | tabs are peers; a slide or zoom implies depth that isn't there |
| Sheet | from its edge | to the same edge | system sheet; iced `sheet::SPRING` | use the platform's sheet wherever it exists |
| Dialog, command box opened by click | fades and grows from 0.95 | the same, reversed | iced `dialog::SPRING` | modals stay centred; opened from the keyboard, no motion |
| Menu, popover | grows out of its trigger | shrinks back into it | Compose `GlassMorph`, SwiftUI `Menu` | never from the centre, never from scale 0 |
| Toast | from its edge | to the same edge, a little faster | `Springs.move` | iced shows it at once; see Materials |
| A list's rows on load | none | — | — | content the user visits daily is already there |
| A row added or removed | the others close the gap as it fades | together, not after | Compose `reflowing()` (or `animateItem` on `Springs.slide()`), SwiftUI `List` | the leaving row stops taking space at once |
| Content swapped in place | cross-fade | cross-fade | `Springs.fade`, Compose `Transitions.crossFade()` | add an 8 pt shift only if direction matters |
| Stepper (month) | slides in from the side it was stepped toward | out the other side | `Springs.move`, Compose `Transitions.stepped` | direction-aware; fast repeated stepping should not queue |
| A number changing on a user's act | rolls | — | `.contentTransition(.numericText())`, tabular figures | not on load, not on refresh |
| Charts | draw in once on first show | — | `Springs.size` | never redraw from zero on refresh |
| Something appearing (a chip, a badge) | fades and grows from 0.9–0.97 | the reverse | `Springs.size` | never from scale 0 |

**Nothing appears from nothing.** A scale entrance starts at 0.9 or more,
with an opacity from 0, in every toolkit: Compose's `scaleIn(initialScale
= 0.95f)`, never the default; SwiftUI's `.scale(scale: 0.95)` combined with
`.opacity`, never bare `.scale`, whose default is 0; iced's `dialog::FROM`
(0.95) is the model.

**Origin-aware.** A menu or popover grows out of the control that opened
it: Compose sets `TransformOrigin` at the trigger's edge (as `GlassMorph`
anchors the menu at the button), SwiftUI uses `.scaleEffect(anchor:)` or
the system's `Menu`, iced places the panel at `Menu::open`'s origin. Modals
are exempt: they belong to the window, not to a button, and stay centred.

**Exits mirror entries.** What came in from the bottom leaves through the
bottom; what grew out of a cover shrinks back into it. Exits are a little
quicker than entries — the user has already decided — and never staggered.

**Asymmetric where someone is deciding.** A press-and-hold that confirms
something destructive fills slowly and linearly while held, and snaps back
on `Springs.fade` the moment it is let go: slow where the person decides,
fast where the system answers.

## Choreography

When two or more things move on one event, decide their relationship before
any timing.

**Cast the elements.** One *lead* — the thing the user must see: the page,
the player, the item acted on — moves first and furthest. *Support* follows
with smaller, shorter motion. *Decoration* (a badge, a divider, a wash)
only fades, or does nothing. Everything else is *static*. If two elements
both want to lead, that is a hierarchy problem motion cannot fix.

**One entrance per container.** If the panel slides in, its content is
already in it. Never animate a sheet's arrival and also stagger its rows:
the user waits through both. The player card is the example — the card
moves, and its cover, title and transport are carried, not separately
animated.

**Together by default.** Parts of one object move on one spring and in one
family: the expanding card's size, corner, shadow and veil all derive from
its single progress; an iced dialog's dimming is worked out from the same
`Shown` as the dialog (`dialog::dimmed`, leading by `DIM_LEAD`), so it
turns back with it. Two springs for one object read as two objects.

**Carry what you cannot steer.** Where a child's motion is the system's (a
SwiftUI zoom, a `matchedGeometryEffect`, a glass merge in a
`GlassEffectContainer`), move the parent and let the child come along,
rather than fighting the system for its path.

**Stagger only where order means something, and rarely.** Product UI the
user opens many times a day never staggers. Where a stagger earns its place
(an onboarding page, the first sync's shelves), it is 30–80 ms between
items, the whole group done in under 300 ms, only the first handful
staggered and the rest arriving with the last; the delays shrink as the
group goes on, and later items travel less than the lead. Uniform delays
read as a metronome. A stagger never holds back a control the user is
already reaching for: everything is tappable the moment it is visible.

**Sequence only when overlap would look broken.** Two dense views in a
container that changes width and height at once go out, then in. Otherwise
overlap: start the second just before the first ends, so a queue becomes
one gesture.

**Direction follows the model.** Forward moves content left and back moves
it right; the month stepper's next month comes from the right. Pass the
direction to the exit too (Compose's `AnimatedContent` reads it from
`targetState` versus `initialState`; SwiftUI's transition must be chosen
before the state changes), or the leaving view goes the old way.

**Survive interruption.** Re-trigger mid-flight, tap twice fast, go back
while a page is still opening: each must turn from where it is, never
stack a second animation on the first or replay from the start. Springs do
this for free (`Animatable` continues from its value and velocity; SwiftUI
springs retarget additively; `Slide::toward` keeps place and speed), and
code that `snapTo`s back to a start value before animating throws it away.

**Reduced motion collapses the choreography.** Under reduced motion the
group appears together on one fade; delays and travel go.

## Gestures

The finger is on the element, so interruption and velocity are the
baseline, not polish.

**Track one to one, from the grab.** The content moves exactly as far as
the finger, from where it was grabbed — never re-centred on the finger.
Update on every move event, not only at the end: a SwiftUI `DragGesture`
needs an `onChanged` that drives the offset, not only an `onEnded` that
decides; Compose's `AnchoredDraggableState`/`draggable` drive the offset as
the finger goes; iced's reorder and swipe feed every event. A drag that
only decides on release, with nothing following the finger before, feels
dead.

**Commit to an axis after a little travel.** About 10 pt of hysteresis
before a drag claims a direction (`DragGesture(minimumDistance: 12)`,
Compose's touch slop, `reorder::THRESHOLD` 4 px for a mouse); then lock the
axis so a sideways swipe in a vertical list doesn't steal the scroll. Only
the first pointer drives the drag; a second finger is ignored until the
first lifts.

**Start from where it is drawn.** Grabbing a thing mid-settle continues
from its presentation value. In Compose that is the `Animatable`'s current
value (`stop()`, then drag from `value`); in SwiftUI it is what a
`withAnimation` retarget already does; in iced it is `Slide::at(now)`,
never `target()`.

**Hand off the velocity.** On release, the spring starts at the finger's
speed: Compose's `animateTo(target, Springs.settle(v, density, reduced),
initialVelocity = v)` and `AnchoredDraggable` with `Springs.settle()` and
`Springs.noDecay()` (so the release is always a spring, never a decay that
coasts on whatever speed was left); SwiftUI's `Springs.released(velocity:
from:to:)`, which converts to `Momentum.relative`; iced's
`Slide::new(…).moving_at(speed)`. This is the single detail that most
separates "fluid" from "fine".

**Decide by where it was going.** Project the release forward by its
velocity and choose the rest nearest the projection: `Springs.nearest(at,
velocity, rests)` and `Momentum.target(points, at:, velocity:)`, both
Apple's `(v / 1000) · d / (1 − d)` with `d` 0.998, or `SNAPPY`/`snappy`
0.99 for a release that decides an act (a skip, going back) rather than a
place. A short fast flick commits; a long slow drag let go near the middle
goes to whichever end is nearer. Never a distance threshold alone. Decide
reverse-versus-commit by the velocity's sign as well: a drag going back up
when let go does not close.

**Pick the release's spring by its speed.** At a flick's speed
(`Springs.isFlick`, `Springs.thrown`) the settle is on `MOMENTUM` 0.8 and
may overshoot by the throw; slower, it is critically damped. Under reduced
motion it is always critically damped.

**Resist at boundaries, never stop dead.** Past a natural edge the content
follows less and less, then springs back. `Springs.preview(r)` is
`limit · (1 − e^(−r / give))`, the curve Android's own back preview uses;
`swipe::band(x)` is the same shape for a trackpad's two-finger pull; Apple's
rubber band is `(x · d · c) / (d + c · |x|)` with `c` 0.55
(`Springs.band`, `Momentum.band`, `motion::rubberband`, all on the
`rubber-band` token), and a new gesture uses one of these rather than a
fourth. A hard wall reads as frozen; resistance reads as "responsive, but
there's nothing more here". The expanding card (the player) bands past
both its ends on Android, as the iPhone's full player bands past its top;
on the desktop every page and list is `arkui::scroll::elastic`, which bands
a trackpad's pull past either end of a scrollable — never a mouse wheel's
lines, never under reduced motion — and springs back on `SMOOTH` from the
speed it was drawn going, never past the edge, without touching the scroll
offset the keys, the scroll edge and the small header read.

**Cap the settle.** However slow and far the release, the settle is done
within `CAP_MS`: Compose's `Capped` cuts a spring's tail that the tuning did
not see. The cap is long enough that a critically damped settle lands by
itself first, so it never cuts a visible step.

**Fire a threshold's side effect once, on the crossing.** A detent caught,
a back gesture armed, a swipe passing the point where letting go acts: the
haptic and any state change fire when the value crosses, not per frame and
not when the animation ends.

**The system's gestures first.** The predictive back gesture (Compose's
`PredictiveBackHandler`, as `Motion.kt` drives it), the iPhone's edge swipe
and sheet drag, a `fullScreenCover`'s or zoom's interactive dismissal: use
them where they exist. Rebuilding a platform transition by hand loses its
interactivity and makes the app feel unlike every other on the device.

## Press Feedback

**On touch-down, at once.** The pressed state appears the instant the
finger lands (Compose's `PressNode` snaps to pressed on
`PressInteraction.Press`; SwiftUI's `configuration.isPressed`). It
releases on a spring when the finger lifts or slides off, and sliding off
cancels the tap, as a native button does.

**Covers, cards and tiles give; rows highlight.** A cover gives to 0.97 of
its size (`Press.Give`, `.press`): `scale` takes its picture and words with
it, which is what makes it read as physical. A row the width of the page
never shrinks; it lights (`Press.Highlight` at 10% of the content colour,
`.pressRow`'s `containerHighest`) and the light fades as it is let go.

**No ripples in these apps.** The apps draw their own look, and a ripple
on Android only, beside a give on the iPhone, is two products. Every
tappable thing in the apps' own UI is `Modifier.pressable`, not bare
`clickable` (whose default indication is Material's ripple); Material's own
components keep theirs, and a deliberate ripple says why in a comment.

**Under reduced motion, nothing shrinks.** What would give is dimmed or
highlighted instead (`PressNode` draws the highlight over a cover;
`PressStyle` keeps its 0.8 opacity and drops the scale).

**Targets are 48 dp / 44 pt** however small the glyph: grow the hit area,
not the drawing.

**On the desktop there is no give.** A pointer is precise and a scaled
button under a mouse reads as a glitch; a row or button takes its pressed
and hovered colours from the theme, at once. Hover is the pointer's own
affordance and is never relied on for anything touch also needs.

### Haptics

The phone's answer in the hand, kept to three rules from Apple's
audio-haptic guidance:

- **Caused by the change itself.** Fire on the state that changed — the
  transaction saved, the show followed, the month stepped — not on a tap
  that may not change anything. Compose's `SensoryFeedback(feedback,
  trigger)` and SwiftUI's `.sensoryFeedback(_, trigger:)` both fire when
  the trigger's value changes, never for its first value.
- **On the same frame as what the eye sees.** Both are performed as the
  change is composed or rendered; a haptic that lags its visual reads as a
  glitch.
- **Kept for what matters.** `Success` (`.success`) when something is
  kept; `Selection` (`.selection`) when a choice steps — a month, a tick, a
  digit, a detent. One per action, never per frame or on scroll, never on
  an entrance the user did not cause, never the only feedback (many people
  have haptics off). `HapticFeedbackType.LongPress` is for a long press;
  a threshold crossing is a `Selection`.

iced has no haptics; the desktop's visual answer stands alone, which it
must anyway.

## Materials and Scroll Edges

**Chrome floats; content scrolls under it.** Bars and the player's
transport are a layer over the content, not a strip that takes room from
it. On the iPhone that is Liquid Glass
(`GlassEffectContainer`, `.buttonStyle(.glass)`); on Android it is
`Glass.kt`'s solid capsule in the brightest surface with a soft shadow,
which borrows only how glass moves (pieces glide on `Springs.move`, bud out
of a neighbour, merge through a neck, and a `GlassMorph` grows into the
menu it opens); on the desktop it is the theme's raised surface.

**A soft edge, not a hairline.** Where content scrolls under something
floating, it goes soft under it rather than meeting a 1 px line:
`arkui::edge::faded` lays the ground over the band and thins it over
`edge::FADE` (28 px), and at rest — scrolled not at all — there is nothing,
so an unscrolled page looks exactly as it would without it. SwiftUI's
`Surfaces.swift` does the same with the system's scroll edge, and gives a
hard edge with its line under Reduce Transparency.

**The header goes small into the band.** Once a page's own header has
scrolled up out of sight, `arkui::compact` brings a small one — the same
art at 28 px, the name on one line — into the band beside the face,
rising 4 px and unveiling on `SMOOTH`, the art and the name as one; it
goes back the same way when the header returns, turning mid-way. Pressed,
it scrolls the page to its top. A scroll the keys made, or reduced motion,
puts it there or takes it away at once.

**The account is on the title's line.** A tab's root has the person's
face at the trailing end of its large title's own line, centred on it, and
what the tab offers (a +) just before it, as Apple's iOS 26 apps have it:
no glass and no button chrome, only the round picture or the initials on
the accent. On the iPhone the line is the page's own first row
(`LargeTitleRow`, Account.swift), which scrolls with the page; once it has
gone under the bar the inline title rises into the bar's middle on
`Springs.move` (`MotionTokens.rise`, transform and opacity only, a fade
under Reduce Motion), and goes back the same way (`largeTitleBar`). On
Android it is `TabTitle`'s `account`, or `LargeTitleLine` in a large top
app bar, and goes up with the large title. Tapped, either raises the
account sheet (`AccountSheet`), the system's own sheet motion.

**Materials follow the person's settings.** Reduce Transparency makes glass
the solid container; Increase Contrast gives it a defined edge and stronger
hairlines (`Surfaces.swift`). Android has no clean equivalent, so its glass
is already solid.

**Never stack translucent on translucent**, and never animate a blur's
radius: a large blur is expensive on every toolkit, and an animated one
redraws it every frame. A blurred wash (the player's cover at 80) is drawn
once and left still; to make a material arrive, fade its opacity.

**iced fades with a veil.** iced draws each widget's colours as they are
and has no opacity for a group, so `motion::transformed` lays the theme's
ground over what it holds, thinning as it shows — which over a page dimmed
to its ground looks like a fade. It is in a layer of its own, because
within one iced draws text after every quad. Where the background under a
thing is not one colour (the toast over a page of rows), a veil would show
as a box, so the toast is there at once and then gone; that is a decision,
recorded in `toast.rs`, not an omission.

## Reduced Motion

Reduced is gentler, not none. What goes is travel — a slide, a scale, an
overshoot, a parallax — that a moving eye has to follow. What stays is a
short cross-fade in place, and colour and opacity changes that explain a
state. A screen transition becomes a fade; the expanding card fades out and
in at its other end; a press dims instead of giving; a release settles
without its momentum bounce. The drag itself still follows the finger one
to one: that is the finger's motion, not the app's.

**Ask the library, never the system.** Every animation reads the shared
switch and takes its gentle path through the shared helpers:

- Compose: `reducedMotion()` (or `LocalReducedMotion` in a test or a
  screenshot); `unlessReduced(spec)` for what only travels; `Springs.fade()`
  for the cross-fade; `Springs.settle(…, reduced)` for a release. The
  activity calls `keepMotion()` before `setContent`, so Android's "Remove
  animations" (a duration scale of 0) is the app's to interpret rather than
  every animation jumping to its end.
- SwiftUI: `@Environment(\.reducesMotion)`, never
  `\.accessibilityReduceMotion` directly; `.motion(_:value:)` and
  `withAnimation(Springs.move.reduced(still))` for animations,
  `.motionTransition(_:)` for transitions (`.opacity` when reduced),
  `.motionScale(_:)` for a scale that should vanish.
- iced: `motion::reduced()`, asked once at startup; `Shown::new` becomes a
  `FADE` by itself; `Spring::or_instant` for what only travels; a widget
  that slides or grows reads `reduced()` and veils in place instead
  (`sheet.rs`, `dialog.rs`).

**The override is on, on purpose.** `ReducedMotion.ALWAYS_ANIMATE`
(Compose), `Motion.alwaysAnimate` (SwiftUI) and `motion::ALWAYS_ANIMATE`
(iced) are all `true`: the devices these apps are made on ask for less
motion, and the owner wants to see the motion while it is being worked on.
While true, the system's setting is not consulted at all (iced's
`ARK_REDUCE_MOTION=1` still forces the gentle path, and `0` forces full
motion). Do not flip any of them as part of an unrelated change, and flip
all three together when the owner says the motion is done. Because the
gentle path is never seen on these devices, every change that adds motion
must also exercise its reduced path: a test with `LocalReducedMotion
provides true` (as `ReducedMotionTests` does for the card and the
settle), `ARK_REDUCE_MOTION=1` on the desktop, or a SwiftUI preview with
the view's `still` forced.

Reduce Transparency and Increase Contrast are separate settings, followed
as the system has them (`Surfaces.swift`); they are not covered by the
motion override.

## Performance

The bar is a frame every 8 ms: the phones and many displays run at 120 Hz.
A fast development machine hides most of what follows, so judge feel on a
release build on the slowest device to hand, and say plainly what was not
seen on one.

**Compose.**

- Read animated values in the draw phase. `Modifier.graphicsLayer { scaleX
  = s; alpha = a }`, `Modifier.offset { IntOffset(…) }` and
  `drawWithContent` with lambdas read the value at draw time and skip
  composition and layout; `Modifier.scale(s)` or `alpha(a)` with a value
  read in composition recompose every frame. `Press` and `Motion.kt` draw
  the move ("drawn, not laid out") — follow them.
- Never animate a size, padding or offset that is laid out. A collapsing
  height relays out everything below it every frame; translate and clip
  inside a fixed frame instead. `animateContentSize` is tolerated on a
  small, leaf container whose height genuinely changes, never on a list.
- No state written per frame from a gesture or a scroll that recomposes a
  list. `Recompositions.hit` counts recompositions of hot rows in tests;
  keep it flat while something moves.
- Lazy rows never carry an entrance (`AnimatedVisibility` on enter): rows
  are recycled, and the entrance replays every time one scrolls back. List
  reflow is `animateItem(fadeInSpec = null, fadeOutSpec = null,
  placementSpec = …)`, as `Reorder.kt` does.
- Never animate `elevation` or a shadow's size: it repaints the shadow every
  frame. Fade a pre-shadowed layer.

**SwiftUI.**

- Animate `scaleEffect`, `offset`, `opacity` and `rotationEffect`, which
  are render-time; animating a `frame`, `padding` or a `GeometryReader`
  -driven layout relays out its subtree every frame.
- Keep per-frame `@State` writes from a gesture to the one view that moves;
  a write that invalidates the whole page body is a recomposition storm in
  SwiftUI's terms.
- No animated blur or material thickness; no `matchedGeometryEffect` across
  large hierarchies where the system's zoom transition
  (`navigationTransition(.zoom)`, which harken already uses) does the job.
- ProMotion needs `CADisableMinimumFrameDurationOnPhone` set to true in
  the app's Info.plist for an app's own animation to run past 60 Hz.

**iced.**

- Nothing ticks at rest. A `Slide` is a closed form — where it set off,
  how fast, toward where, when — so its value is computed when drawn, and
  no state is stepped. The app runs a frame clock only while something
  says it is `moving(now)` (which holds `LAST_FRAME` past rest, so the
  frame at rest is the last one drawn), and none at all otherwise.
- The frame clock is the display's: `motion::Clock::Frames`
  (`iced::window::frames()`) ticks with the compositor, where a 16 ms
  `iced::time::every` is a wall-clock timer that drifts against a 60 Hz
  display and caps a 120 Hz one. `Clock::Every` is for a nudge now and
  then (a scrubber, a dwell), never for motion.
- `motion::transformed` lays its content out where it would be, unmoved,
  and only draws it moved, so nothing around it relays out; and it maps the
  pointer back through the move, so what it holds answers where it is drawn.
- A veil is one quad in its own layer; never fake a fade by recolouring
  every widget per frame.

## Vocabulary

The words this repo uses for motion, so a request, a review and a commit
mean the same thing. Terms are the skills' and Apple's; the names after
them are where each lives here.

- **Spring** — motion with a target and a velocity rather than a duration.
  `Springs`, `motion::Spring`.
- **Response** — how quickly a spring gets there, its undamped period in
  seconds; not a duration. `Springs.response`, `Spring::response`.
- **Damping ratio** — 1 arrives without passing (*critically damped*);
  below 1 it overshoots. `MOVE`, `MOMENTUM`, `Spring::damping`.
- **Perceived duration** — when a spring looks done, though it keeps
  micro-settling; what the duration budgets mean for a spring.
- **Momentum** — velocity a finger gave a thing, spent after release; the
  only licence to overshoot.
- **Velocity handoff** — the spring starts at the finger's speed, so release
  has no seam. `initialVelocity`, `Springs.released`, `Slide::moving_at`.
- **Projection** — where a release would come to rest if it kept
  decelerating; what a snap is chosen from. `Springs.project`,
  `Momentum.project`.
- **Settle** — the spring after a release, capped. `Springs.settle`,
  `CAP_MS`.
- **Rubber-banding** — resistance past a boundary, then snap-back.
  `Springs.band`, `Momentum.band`, `motion::rubberband`,
  `scroll::elastic`; `Springs.preview`, `swipe::band`.
- **Preview** — a gesture's visible progress before it commits, bounded,
  as Android's predictive back. `Springs.PREVIEW`, `GIVE`.
- **Interruptible** — can be grabbed or retargeted mid-flight from its
  *presentation value* (where it is drawn), not its target.
- **One-to-one tracking** — content moves exactly with the finger from the
  grab point.
- **Hysteresis** — the small travel before a drag commits to an axis.
- **Give** — a cover or card shrinking a little under the finger.
  `Press.Give`, `.press`.
- **Highlight** — a row lit under the finger. `Press.Highlight`,
  `.pressRow`.
- **Hero / zoom** — a page growing out of the thing tapped and shrinking
  back into it; a *shared element transition*. `Hero`, `Spot`,
  `navigationTransition(.zoom)`.
- **Origin-aware** — a menu or popover growing out of its trigger.
  `GlassMorph`, `Menu::open`.
- **Morph** — one shape becoming another (a glass button becoming its
  menu).
- **Cross-fade** — one state fading out as another fades in, in place;
  reduced motion's universal stand-in. `Springs.fade`, `FADE`,
  `Springs.calm`.
- **Veil** — iced's fade: the ground laid over a thing, thinning.
  `motion::transformed`'s `veil`.
- **Shown** — iced's 0..1 of something coming and going on a spring.
  `motion::Shown`.
- **Slide** — iced's value on a spring. `motion::Slide`.
- **Frame clock** — the subscription that redraws while something moves,
  and stops at rest.
- **Lead / support / decoration / static** — the roles in a choreography.
- **Stagger** — items entering one after another with a small delay;
  **carry** — children moving because their parent does.
- **Direction-aware** — forward and back move opposite ways.
- **Scroll edge** — content going soft under floating chrome.
  `edge::faded`.
- **Gentle path** — what an animation does under reduced motion.
- **Frequency of use** — the more often it is seen, the less it moves.
- **Jank / dropped frame** — a frame missed; on these displays, an 8 ms
  budget.

## Review Checklist

Run it on the diff before pushing any change to UI or motion. Each item is
a question with a way to answer it from the diff; anything that cannot be
answered from code goes in the commit message as "not seen on a device".

```sh
base=origin/claude/sleepy-faraday-lcbow5
git diff "$base"... -U0 -- '*.kt' '*.swift' '*.rs' > /tmp/motion.diff
```

1. **Should it move at all?** For each new animation, name its purpose and
   its frequency tier. Anything opened or stepped by the keyboard, a tab
   switch, or a list the user visits daily, has none.
2. **Tokens, not literals.**
   `grep -nE 'tween\(|keyframes|Easing|DampingRatio|Stiffness[A-Z]|\.easeIn|\.easeOut|\.linear\b|\.smooth\b|\.snappy\b|\.bouncy\b|withAnimation *\{|Spring *\{ *damping' /tmp/motion.diff`
   — each hit is either a shared token (`Springs.*`, `motion::SMOOTH`/
   `FADE`) or a new token added to the shared library with its reason.
   Compose's bare `fadeIn()`, `slideIn…{}`, `Crossfade(…)` and
   `animate…AsState(x)` take Material's defaults and count as literals.
3. **Critically damped unless thrown.** No damping below 1 on anything a
   tap or the app moved; `MOMENTUM` only through `settle`/`released`.
4. **The gentle path.** Every new motion goes through `reducedMotion()` /
   `unlessReduced` / `Springs.fade`, `.motion`/`.motionTransition`/
   `.motionScale`/`.reduced(still)`, or `reduced()`/`or_instant`/`Shown`.
   `grep -n 'accessibilityReduceMotion\|areAnimatorsEnabled\|enable-animations' /tmp/motion.diff`
   outside the libraries is a finding. `ALWAYS_ANIMATE`/`alwaysAnimate`
   unchanged. A test or preview exercises the reduced path.
5. **Nothing from nothing.** `grep -nE '\.scale\b[^(E]|scaleIn\(|initialScale' /tmp/motion.diff`
   — a SwiftUI `.scale` transition takes `scale: 0.9…0.97`; a Compose
   `scaleIn` sets `initialScale` ≥ 0.9.
6. **Origin and path.** Menus grow from their trigger; modals stay centred;
   the exit retraces the entry; a stepper is direction-aware.
7. **Press.** `grep -nE '\.clickable\(|onTapGesture|buttonStyle\(\.plain\)' /tmp/motion.diff`
   — each tappable thing in an app's own UI is `pressable` (Give or
   Highlight) or `.press`/`.pressRow`, or says why not. Feedback is on
   touch-down. No ripple added.
8. **Gestures.** A drag tracks one to one in `onChanged`/as it goes, not
   only on release; starts from the presentation value; hands its velocity
   to the spring; chooses its rest by projection (`nearest`/`target`);
   rubber-bands past its edges with an existing curve; locks an axis.
9. **Haptics.** On the change, not the tap; `Success` or `Selection`; one
   per action; never in a per-frame path; never the only feedback.
10. **Choreography.** One entrance per container; no stagger on frequent
    surfaces; parts of one object on one spring; exits together and
    quicker; interruption tested (re-trigger mid-flight, go back mid-open).
11. **Performance.** `grep -nE 'animateContentSize|animate.*(Dp|Size)AsState|\.frame\(.*(width|height):.*anim|padding\(.*anim|elevation' /tmp/motion.diff`
    — no layout property animated per frame; Compose values read in
    `graphicsLayer {}`/`offset {}`/draw lambdas; no lazy-row entrance; no
    animated blur; an iced frame clock that stops at rest.
12. **Parity.** The same interaction on the other two toolkits uses the
    same token values, or the diff says why it differs.
13. **Said, not assumed.** The commit says what was verified by tests or
    screenshots and what has not been felt on a device — flick it,
    interrupt it, reverse it, run it slow.

## One Source for Every Value

Every number in this document is written once, in `arkui/theme.toml`'s
`[motion]`, and `nix run .#update-theme` writes it into each toolkit:
`dev.arkui.MotionTokens` (Kotlin), `MotionTokens` in ArkUICore (Swift) and
`arkui::motion_tokens` (Rust). Kotlin's springs take the stiffness; Swift
and Rust take the response worked out from it, 2π/√stiffness, so a spring
is the same spring everywhere. `checks.theme` fails when a generated file
is not what the TOML makes. The libraries' `Springs` and `arkui::motion`
are built on these tokens, and the apps use the libraries, never a number
of their own: a value an app needs that is not there is added to the TOML,
and a motion one app has that another could use moves into arkui.
