#!/usr/bin/env python3
# The theme's generator and its contrast check: ui/theme.toml, the one
# place a colour, a motion or a measure of a window is written down, said in each platform's language.
#
#   theme.py write [ROOT]   # the generated files, under ROOT (the repository's root by default)
#   theme.py contrast       # every text colour on every ground it is drawn on, at WCAG's ratio
#
# `nix run .#update-theme` is the first, from the repository's root;
# `checks.theme` runs it into a directory of its own and compares, and
# `checks.theme-contrast` is the second. Python's standard library only, so
# the checks need nothing but the interpreter. The browser's is CSS
# (ui/rust/web/theme.css): variables, springs as `linear()`, size classes.
import json
import os
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "theme.toml")

# Where each generated file goes, from the repository's root.
KOTLIN = "ui/kotlin/src/dev/arkui/Tokens.kt"
SWIFT_CORE = "ui/swift/Sources/ArkUICore/Tokens.swift"
SWIFT_UI = "ui/swift/Sources/ArkUI/Colors.swift"
RUST = "ui/rust/arkui/src/tokens.rs"
KOTLIN_MOTION = "ui/kotlin/src/dev/arkui/MotionTokens.kt"
SWIFT_MOTION = "ui/swift/Sources/ArkUICore/MotionTokens.swift"
RUST_MOTION = "ui/rust/arkui/src/motion_tokens.rs"
KOTLIN_LAYOUT = "ui/kotlin/src/dev/arkui/LayoutTokens.kt"
SWIFT_LAYOUT = "ui/swift/Sources/ArkUICore/LayoutTokens.swift"
RUST_LAYOUT = "ui/rust/arkui/src/layout_tokens.rs"
CSS = "ui/rust/web/theme.css"

# The layout tokens, in the order they are written, and each one's sentence.
LAYOUT = {
    "compact-width": "Narrower than this (dp, pt or logical px) a window is compact: the sidebar is a tab bar, the split view a stack, the inspector a sheet.",
    "expanded-width": "At least this wide a window is expanded: the split view shows its sidebar, its content and its detail at once.",
    "sidebar-min-width": "At least this wide the sections are a sidebar; narrower but regular, a tab bar at the top (an iPad upright).",
    "sidebar-width": "A split view's sidebar, at rest.",
    "content-width": "A split view's content column beside a detail, at rest.",
    "inspector-width": "An inspector at the trailing edge, at rest.",
    "toolbar-height": "A window's toolbar row.",
    "search-width": "The search field at a toolbar's trailing end.",
    "amount-column-width": "A column of figures in a table or a budget's row.",
    "progress-width": "The bar under a row's name that fills toward its goal, beside columns.",
    "line-height": "A line of text as tall as this many times its size (the desktop's: iced's).",
}

# The motion tokens, in the order they are written, and each one's sentence.
MOTION = {
    "stiffness": "The one stiffness every move rides: a page, the player, a sheet, a row.",
    "move": "The damping of a move nothing threw: arrives and stops.",
    "momentum": "The damping of a move a flick threw: its momentum spending itself.",
    "size": "The damping of how large a thing is and what of it shows: never past its size.",
    "fade-stiffness": "The stiffness of a fade, a hover, and the stand-in when motion is reduced.",
    "rest": "Where a 0..1 progress counts as there.",
    "rest-px": "Where a move in pixels counts as there.",
    "cap-ms": "The most a settle may take, in milliseconds.",
    "flick": "A release at least this fast, a second, was a flick.",
    "deceleration": "How much of its speed a projected release keeps each millisecond.",
    "snappy": "The same, for a release that decides an act: a skip, going back.",
    "preview": "The most a back gesture's preview comes, as a share of what it previews.",
    "give": "How much of the gesture brings the preview 63% of the way.",
    "press": "How small a pressed cover or chip gives.",
    "enter-scale": "The least a thing scales from when it enters.",
    "rubber-band": "How hard a drag past its edge pulls back: Apple's rubber band's constant.",
    "lift": "How large a row held to be dragged grows.",
    "shift": "How far a page or a step slides as it comes or goes, as a share of its width.",
    "paused-scale": "How small a cover is drawn while what it plays is paused.",
    "rise": "How far a thing that comes in where it stays rises as it comes: a nudge.",
    "band-resistance": "How hard a scroll past its end is pulled: Gecko's resistance, 1/this of a pixel at the edge.",
    "band-gain": "How many of Gecko's pixels of pull one trackpad pixel is.",
    "band-step": "The most one move draws a band further, in pixels.",
    "band-stiffness": "The stiffness a band springs back on: Firefox's overscroll spring.",
    "band-damping": "The damping a band springs back with: a touch over critically damped.",
    "band-throw": "The share of a release's speed a band springs back set off at.",
    "band-fling-max": "The fastest a band springs back set off, a second.",
    "band-rest": "Within this of the edge, in pixels, a band has sprung back.",
    "coast-stop": "A coast slower than this, a second, has stopped.",
    "coast-min": "A release slower than this, a second, does not coast.",
    "velocity-ms": "How far back a release's speed is read from its moves, in milliseconds.",
    "velocity-gap-ms": "A pause this long between a drag's moves, in milliseconds, means it had stopped.",
    "lift-ms": "With no word from the system, a trackpad still this long has been let go, in milliseconds.",
    "band-lift-ms": "The same, while it is pulled past an end, in milliseconds.",
    "coast-spent": "A lift seen only by its stillness, slower than this share of the gesture's fastest, has run down by itself: no coast after it.",
}

# The roles of one side, in the order they are written, and each one's
# sentence for the generated code's documentation.
NEUTRAL = {
    "background": "The page.",
    "surface": "A plane over the page: a sheet, a card, a bar.",
    "container-lowest": "The faintest step a container takes off the page.",
    "container-low": "A container one step off the page.",
    "container": "A container: a field, a grouped list's rows.",
    "container-high": "A container a step further: a chip, a track.",
    "container-highest": "The furthest step: a pressed row, a meter's track.",
    "glass": "The fill of a glass chip, menu and sheet.",
    "outline": "The edge of a control: a field, an unchecked box.",
    "hairline": "A divider, and a cover's edge.",
    "ink": "Text.",
    "quiet": "Secondary text: a subtitle, a count, a caption.",
    "disabled": "What cannot be pressed.",
    "scrim": "What dims the page under a dialog.",
    "inverse-surface": "A snackbar: the other side's surface.",
    "inverse-ink": "Text on the inverse surface.",
}
SIGNAL = {
    "error": "Wrong: an error, an overspent amount.",
    "on-error": "What is written on the error colour.",
    "error-container": "A quiet container for an error.",
    "on-error-container": "What is written on that.",
    "caution": "Worth a look: an amount short of its target.",
    "on-caution": "What is written on the caution colour.",
    "caution-container": "A quiet container for a caution.",
    "on-caution-container": "What is written on that.",
    "positive": "Good: money with a job, a goal met.",
}
ACCENT = {
    "accent": "The accent: a filled button, a slider, a selected tab.",
    "on-accent": "What is written on the accent.",
    "accent-text": "The accent as text: a link, the row playing.",
    "accent-container": "A quiet container in the accent.",
    "on-accent-container": "What is written on that.",
}
HEADER = "Generated by `nix run .#update-theme` from ui/theme.toml: edit that, then run it."


def load():
    with open(SOURCE, "rb") as f:
        t = tomllib.load(f)
    for side in ("light", "dark"):
        for name in NEUTRAL:
            assert name in t["neutral"][side], f"neutral.{side} has no {name}"
        for name in SIGNAL:
            assert name in t["signal"][side], f"signal.{side} has no {name}"
        for a, accent in t["accent"].items():
            for name in ACCENT:
                assert name in accent[side], f"accent.{a}.{side} has no {name}"
    for a, accent in t["accent"].items():
        assert accent["art"] in t["art"], f"accent.{a} names art {accent['art']}, which is not there"
    assert len(t["chart"]["light"]) == len(t["chart"]["dark"]), "chart has a different number of colours on each side"
    for side in ("light", "dark"):
        for c in t["chart"][side]:
            rgb(c)
    return t


def rgb(hex_):
    h = hex_.lstrip("#")
    assert len(h) == 6, f"{hex_} is not #RRGGBB"
    return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))


def camel(name):
    head, *rest = name.split("-")
    return head + "".join(w.capitalize() for w in rest)


def pascal(name):
    return "".join(w.capitalize() for w in name.split("-"))


def screaming(name):
    return name.upper().replace("-", "_")


def side_colors(t, side):
    """Every shared role of one side: the neutrals then the signals."""
    return {**t["neutral"][side], **t["signal"][side]}


def roles():
    return {**NEUTRAL, **SIGNAL}


# Contrast, as WCAG 2 has it: the relative luminance of each, and their ratio.
def luminance(hex_):
    def linear(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (linear(c) for c in rgb(hex_))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(a, b):
    hi, lo = sorted((luminance(a), luminance(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def over(top, bottom, alpha):
    """`top` laid over `bottom` at `alpha`, as the player's wash is."""
    return "#" + "".join(f"{round(alpha * x + (1 - alpha) * y):02X}" for x, y in zip(rgb(top), rgb(bottom)))


def wash(t, side):
    return over(t["wash"]["cover"], t["neutral"][side]["background"], t["wash"][side])


def contrast(t):
    """Every pair the rule names, as (side, accent, fg role, bg role, fg, bg, ratio, floor)."""
    rule = t["contrast"]
    pairs = []
    for side in ("light", "dark"):
        shared = side_colors(t, side)
        for a, accent in t["accent"].items():
            colors = {**shared, **accent[side], "wash": wash(t, side)}
            seen = set()

            def add(fg, bg, floor):
                key = (fg, bg)
                # The shared pairs are the same under every accent: read once, under the first.
                if key in seen or (a != next(iter(t["accent"])) and fg not in ACCENT and bg not in ACCENT and bg != "wash"):
                    return
                seen.add(key)
                shared_pair = fg not in ACCENT and bg not in ACCENT and bg != "wash"
                pairs.append((side, "shared" if shared_pair else a, fg, bg, colors[fg], colors[bg], ratio(colors[fg], colors[bg]), floor))

            for fg in rule["text"]:
                for bg in rule["grounds"]:
                    add(fg, bg, 4.5)
            for fg in rule["wash"]:
                add(fg, "wash", 4.5)
            for fg, bg in rule["on"]:
                add(fg, bg, 4.5)
            for fg, bg in rule["large"]:
                add(fg, bg, 3.0)
        # The chart's colours are every app's: read once, as shared pairs.
        for i, c in enumerate(t["chart"][side]):
            for bg in rule["chart"]:
                pairs.append((side, "shared", f"chart-{i}", bg, c, shared[bg], ratio(c, shared[bg]), 3.0))
    return pairs


def check_contrast():
    t = load()
    failed = 0
    for side, a, fg, bg, cf, cb, r, floor in contrast(t):
        bad = r < floor
        failed += bad
        mark = "FAIL" if bad else "ok  "
        print(f"{mark} {side:5} {a:6} {fg:21} {cf} on {bg:18} {cb}  {r:5.2f}:1 (needs {floor}:1)")
    if failed:
        print(f"{failed} pairs are under their ratio: change ui/theme.toml, then nix run .#update-theme", file=sys.stderr)
        sys.exit(1)
    print("every pair is legible")


# Kotlin: ui/kotlin, the Material scheme each app is drawn in.
def kotlin_color(hex_):
    return f"Color(0xFF{hex_.lstrip('#').upper()})"


def kotlin(t):
    shared = roles()
    out = [f"// {HEADER}", "//"]
    out += [
        "// The theme every app on Android is drawn in: the neutrals and signals all",
        "// share (`Tokens`), each app's accent (`Accent`), and the Material scheme",
        "// made of the two (`arkuiScheme`), which is the only scheme an app's",
        "// screens are drawn in. The roles are ui/README.md's.",
        "package dev.arkui",
        "",
        "import androidx.compose.material3.ColorScheme",
        "import androidx.compose.material3.darkColorScheme",
        "import androidx.compose.material3.lightColorScheme",
        "import androidx.compose.runtime.Immutable",
        "import androidx.compose.ui.graphics.Color",
        "",
        "/** One side of what every app shares: the neutrals and the signals. */",
        "@Immutable",
        "public class Neutrals internal constructor(",
    ]
    for name, doc in shared.items():
        out.append(f"    /** {doc} */")
        out.append(f"    public val {camel(name)}: Color,")
    out.append(")")
    out.append("")
    out.append("/** The shared colours, both sides. */")
    out.append("public object Tokens {")
    for side in ("light", "dark"):
        colors = side_colors(t, side)
        out.append(f"    public val {side.capitalize()}: Neutrals =")
        out.append("        Neutrals(")
        for name in shared:
            out.append(f"            {camel(name)} = {kotlin_color(colors[name])},")
        out.append("        )")
        out.append("")
    out.append("    /** The side asked for. */")
    out.append("    public fun of(dark: Boolean): Neutrals = if (dark) Dark else Light")
    out.append("")
    out.append("    /** How much of the cover shows through behind the full player. */")
    out.append(f"    public fun washOpacity(dark: Boolean): Float = if (dark) {t['wash']['dark']}f else {t['wash']['light']}f")
    out.append("}")
    out.append("")
    out.append("/** A chart's colours, in the order its parts take them: the largest first. */")
    out.append("public object ChartColors {")
    for side in ("light", "dark"):
        out.append(f"    public val {side.capitalize()}: List<Color> =")
        out.append("        listOf(")
        for c in t["chart"][side]:
            out.append(f"            {kotlin_color(c)},")
        out.append("        )")
        out.append("")
    out.append("    /** The side asked for. */")
    out.append("    public fun of(dark: Boolean): List<Color> = if (dark) Dark else Light")
    out.append("}")
    out.append("")
    out.append("/** One side of an app's accent. */")
    out.append("@Immutable")
    out.append("public class AccentColors internal constructor(")
    for name, doc in ACCENT.items():
        out.append(f"    /** {doc} */")
        out.append(f"    public val {camel(name)}: Color,")
    out.append(")")
    out.append("")
    out.append("/** The derived squares' gradients, as each one's two stops. */")
    out.append("internal object ArtSets {")
    for art, sets in t["art"].items():
        for side in ("light", "dark"):
            out.append(f"    val {pascal(art)}{side.capitalize()}: List<Pair<Color, Color>> =")
            out.append("        listOf(")
            for a, b in sets[side]:
                out.append(f"            {kotlin_color(a)} to {kotlin_color(b)},")
            out.append("        )")
    out.append("}")
    out.append("")
    out.append("/** An app's accent: the one colour that is the app's own, and the squares it draws. */")
    out.append("public enum class Accent {")
    names = list(t["accent"])
    for i, a in enumerate(names):
        out.append(f"    {pascal(a)}{',' if i < len(names) - 1 else ';'}")
    out.append("")
    out.append("    /** The accent for the side asked. */")
    out.append("    public fun colors(dark: Boolean): AccentColors =")
    out.append("        when (this) {")
    for a, accent in t["accent"].items():
        out.append(f"            {pascal(a)} -> if (dark) {pascal(a)}Dark else {pascal(a)}Light")
    out.append("        }")
    out.append("")
    out.append("    /** The derived squares' gradients for the side asked. */")
    out.append("    public fun art(dark: Boolean): List<Pair<Color, Color>> =")
    out.append("        when (this) {")
    for a, accent in t["accent"].items():
        art = pascal(accent["art"])
        out.append(f"            {pascal(a)} -> if (dark) ArtSets.{art}Dark else ArtSets.{art}Light")
    out.append("        }")
    out.append("}")
    out.append("")
    for a, accent in t["accent"].items():
        for side in ("light", "dark"):
            out.append(f"private val {pascal(a)}{side.capitalize()} =")
            out.append("    AccentColors(")
            for name in ACCENT:
                out.append(f"        {camel(name)} = {kotlin_color(accent[side][name])},")
            out.append("    )")
            out.append("")
    out.append("/**")
    out.append(" * The Material scheme an app is drawn in: the shared neutrals and signals, with `accent` in")
    out.append(" * the primary slots, a neutral grey in the secondary (so a tonal button on an accent card")
    out.append(" * still shows) and the caution in the tertiary. Nothing is tinted by elevation (`surfaceTint`")
    out.append(" * is the surface), and `surfaceBright` is the glass.")
    out.append(" */")
    out.append("public fun arkuiScheme(accent: Accent, dark: Boolean): ColorScheme {")
    out.append("    val n = Tokens.of(dark)")
    out.append("    val a = accent.colors(dark)")
    out.append("    val inverse = accent.colors(!dark).accent")
    out.append("    return if (dark) {")
    out.append("        darkColorScheme(")
    out += [f"            {k} = {v}," for k, v in scheme_args(dark=True)]
    out.append("        )")
    out.append("    } else {")
    out.append("        lightColorScheme(")
    out += [f"            {k} = {v}," for k, v in scheme_args(dark=False)]
    out.append("        )")
    out.append("    }")
    out.append("}")
    return "\n".join(out) + "\n"


def scheme_args(dark):
    """The arguments of Material's scheme, slot by slot."""
    args = [
        ("primary", "a.accent"),
        ("onPrimary", "a.onAccent"),
        ("primaryContainer", "a.accentContainer"),
        ("onPrimaryContainer", "a.onAccentContainer"),
        ("inversePrimary", "inverse"),
        ("secondary", "n.quiet"),
        ("onSecondary", "n.background"),
        ("secondaryContainer", "n.containerHighest"),
        ("onSecondaryContainer", "n.ink"),
        ("tertiary", "n.caution"),
        ("onTertiary", "n.onCaution"),
        ("tertiaryContainer", "n.cautionContainer"),
        ("onTertiaryContainer", "n.onCautionContainer"),
        ("background", "n.background"),
        ("onBackground", "n.ink"),
        ("surface", "n.surface"),
        ("onSurface", "n.ink"),
        ("surfaceVariant", "n.containerHigh"),
        ("onSurfaceVariant", "n.quiet"),
        ("surfaceTint", "n.surface"),
        ("inverseSurface", "n.inverseSurface"),
        ("inverseOnSurface", "n.inverseInk"),
        ("error", "n.error"),
        ("onError", "n.onError"),
        ("errorContainer", "n.errorContainer"),
        ("onErrorContainer", "n.onErrorContainer"),
        ("outline", "n.outline"),
        ("outlineVariant", "n.hairline"),
        ("scrim", "n.scrim"),
        ("surfaceBright", "n.glass"),
        ("surfaceDim", "n.background" if dark else "n.containerHighest"),
        ("surfaceContainer", "n.container"),
        ("surfaceContainerHigh", "n.containerHigh"),
        ("surfaceContainerHighest", "n.containerHighest"),
        ("surfaceContainerLow", "n.containerLow"),
        ("surfaceContainerLowest", "n.containerLowest"),
    ]
    return args


# Swift: ui/swift, its Foundation half and its SwiftUI half.
def swift_hex(hex_):
    return "0x" + hex_.lstrip("#").upper()


def swift_core(t):
    shared = roles()
    out = [f"// {HEADER}", "//"]
    out += [
        "// The theme's numbers, Foundation alone, so Linux builds them: the colours",
        "// every app shares, each app's accent and the derived squares, as",
        "// `0xRRGGBB` for each side. ArkUI's `Color` tokens are made of these.",
        "",
        "/// A colour for each side, as `0xRRGGBB`.",
        "public struct Sides: Equatable, Sendable {",
        "    public let light: UInt32",
        "    public let dark: UInt32",
        "",
        "    public init(light: UInt32, dark: UInt32) {",
        "        self.light = light",
        "        self.dark = dark",
        "    }",
        "",
        "    /// The colour for one side.",
        "    public func of(dark: Bool) -> UInt32 { dark ? self.dark : light }",
        "}",
        "",
        "/// The colours every app shares: the neutrals and the signals.",
        "public enum Tokens {",
    ]
    light, dark = side_colors(t, "light"), side_colors(t, "dark")
    for name, doc in shared.items():
        out.append(f"    /// {doc}")
        out.append(f"    public static let {camel(name)} = Sides(light: {swift_hex(light[name])}, dark: {swift_hex(dark[name])})")
    out.append("    /// How much of the cover shows through behind the full player.")
    out.append(f"    public static func washOpacity(dark: Bool) -> Double {{ dark ? {t['wash']['dark']} : {t['wash']['light']} }}")
    out.append("}")
    out.append("")
    out.append("/// A chart's colours, in the order its parts take them: the largest first.")
    out.append("public enum ChartTokens {")
    out.append("    public static let series: [Sides] = [")
    for lc, dc in zip(t["chart"]["light"], t["chart"]["dark"]):
        out.append(f"        Sides(light: {swift_hex(lc)}, dark: {swift_hex(dc)}),")
    out.append("    ]")
    out.append("}")
    out.append("")
    out.append("/// An app's accent: the one colour that is the app's own, and the squares it draws.")
    out.append("public struct AccentSet: Equatable, Sendable {")
    for name, doc in ACCENT.items():
        out.append(f"    /// {doc}")
        out.append(f"    public let {camel(name)}: Sides")
    out.append("    /// The derived squares it draws.")
    out.append("    public let squares: Squares")
    out.append("")
    for a, accent in t["accent"].items():
        out.append(f"    public static let {camel(a)} = AccentSet(")
        for name in ACCENT:
            out.append(f"        {camel(name)}: Sides(light: {swift_hex(accent['light'][name])}, dark: {swift_hex(accent['dark'][name])}),")
        out.append(f"        squares: .{camel(accent['art'])}")
        out.append("    )")
    out.append("}")
    out.append("")
    out.append("extension Squares {")
    for art, sets in t["art"].items():
        lights = ", ".join(swift_hex(a) for a, _ in sets["light"])
        darks = ", ".join(swift_hex(a) for a, _ in sets["dark"])
        out.append(f"    public static let {camel(art)} = Squares(light: [{lights}], dark: [{darks}])")
    out.append("}")
    return "\n".join(out) + "\n"


def swift_ui(t):
    shared = roles()
    out = [f"// {HEADER}", "//"]
    out += [
        "// The theme as SwiftUI colours: each shared role (`.ink`, `.quiet`, `.page`…)",
        "// and the app's accent (`.accent`, `.accentText`…), each following light",
        "// and dark by itself. They are `ShapeStyle`s where the style is a `Color`,",
        "// so `.foregroundStyle(.quiet)` and `Color.quiet` both read. An app names",
        "// its accent once, before its first view: `Accent.use(.gold)`.",
        "import SwiftUI",
        "",
        "extension Color {",
        "    /// A token's two sides as one colour, following the trait collection.",
        "    public init(_ sides: Sides) {",
        "        self.init(light: sides.light, dark: sides.dark)",
        "    }",
        "}",
        "",
        "/// The accent the app is drawn in. Read as each colour is drawn, so naming it",
        "/// before the first view is enough.",
        "public enum Accent {",
        "    public private(set) static var current: AccentSet = .blue",
        "",
        "    /// Names the app's accent: once, at launch.",
        "    public static func use(_ set: AccentSet) { current = set }",
        "",
        "    static func color(_ pick: @escaping (AccentSet) -> Sides) -> Color {",
        "        Color.dynamic { pick(current).of(dark: $0) }",
        "    }",
        "}",
        "",
        "/// The shared colours, made once.",
        "private enum Made {",
    ]
    for name in shared:
        out.append(f"    static let {camel(name)} = Color(Tokens.{camel(name)})")
    out.append("}")
    out.append("")
    out.append("extension ShapeStyle where Self == Color {")
    for name, doc in shared.items():
        swift_name = swift_role(name)
        out.append(f"    /// {doc}")
        out.append(f"    public static var {swift_name}: Color {{ Made.{camel(name)} }}")
    for name, doc in ACCENT.items():
        out.append(f"    /// {doc}")
        out.append(f"    public static var {camel(name)}: Color {{ Accent.color {{ $0.{camel(name)} }} }}")
    out.append("}")
    out.append("")
    out.append("extension Color {")
    out.append("    /// A chart's colours, in the order its parts take them: the largest first.")
    out.append("    public static let chart: [Color] = ChartTokens.series.map { Color($0) }")
    out.append("}")
    return "\n".join(out) + "\n"


def swift_role(name):
    """A role's Swift name: the page is `.page` and the glass `.glassFill`, since SwiftUI has a `.background` style and a `.glass` button."""
    return {"background": "page", "glass": "glassFill"}.get(name, camel(name))


# Rust: ui/rust/arkui, the desktop's palettes.
def rust_rgb(hex_):
    r, g, b = rgb(hex_)
    return f"rgb(0x{r:02X}, 0x{g:02X}, 0x{b:02X})"


def rust(t):
    out = [f"//! {HEADER}", "//!"]
    out += [
        "//! The theme's palettes for the desktop: one [`Palette`] per accent, each",
        "//! the shared page, ink and signals with that accent, and its squares. An",
        "//! app installs its own ([`crate::theme::install`]); [`BLUE`] is what is",
        "//! drawn when none is.",
        "use crate::theme::Palette;",
        "use iced::Color;",
        "",
        "const fn rgb(r: u8, g: u8, b: u8) -> Color {",
        "    Color::from_rgb8(r, g, b)",
        "}",
        "",
    ]
    for side in ("light", "dark"):
        colors = side_colors(t, side)
        out.append(f"/// The shared colours of the {side} side, by role: for what is drawn on one")
        out.append("/// side whatever the theme, as a receipt's paper is.")
        out.append(f"pub mod {side} {{")
        out.append("    use super::rgb;")
        out.append("    use iced::Color;")
        out.append("")
        for name, doc in roles().items():
            out.append(f"    /// {doc}")
            out.append(f"    pub const {screaming(name)}: Color = {rust_rgb(colors[name])};")
        out.append("}")
        out.append("")
    for side in ("light", "dark"):
        out.append(f"/// A chart's colours on the {side} side, in the order its parts take them: the largest first.")
        out.append(f"pub const CHART_{side.upper()}: [Color; {len(t['chart'][side])}] = [")
        for c in t["chart"][side]:
            out.append(f"    {rust_rgb(c)},")
        out.append("];")
        out.append("")
    for art, sets in t["art"].items():
        for side in ("light", "dark"):
            out.append(f"const {screaming(art)}_{side.upper()}_ART: [[Color; 2]; {len(sets[side])}] = [")
            for a, b in sets[side]:
                out.append(f"    [{rust_rgb(a)}, {rust_rgb(b)}],")
            out.append("];")
            out.append("")
    for a, accent in t["accent"].items():
        out.append(f"/// The {a} accent over the shared neutrals.")
        out.append(f"pub const {screaming(a)}: Palette = Palette {{")
        for side in ("light", "dark"):
            c = {**side_colors(t, side), **accent[side]}
            out.append(f"    {side}: iced::theme::Palette {{")
            for field, role in (("background", "background"), ("text", "ink"), ("primary", "accent"), ("success", "positive"), ("warning", "caution"), ("danger", "error")):
                out.append(f"        {field}: {rust_rgb(c[role])},")
            out.append("    },")
        art = screaming(accent["art"])
        out.append(f"    light_art: &{art}_LIGHT_ART,")
        out.append(f"    dark_art: &{art}_DARK_ART,")
        out.append("};")
        out.append("")
    return "\n".join(out).rstrip() + "\n"


def response(stiffness):
    """A spring's period at unit mass, Apple's `response`: 2π/√k."""
    import math
    return 2 * math.pi / math.sqrt(stiffness)


def motion_values(t):
    m = t["motion"]
    missing = [k for k in MOTION if k not in m]
    if missing:
        sys.exit(f"theme.toml [motion] lacks {', '.join(missing)}")
    return m


def kotlin_motion(t):
    m = motion_values(t)
    out = [f"// {HEADER}", "package dev.arkui", "", "/** How everything moves (ui/theme.toml `[motion]`, ui/docs/motion.md). */", "public object MotionTokens {"]
    for k, doc in MOTION.items():
        v = m[k]
        out.append(f"    /** {doc} */")
        if isinstance(v, int) and not isinstance(v, bool):
            out.append(f"    public const val {screaming(k)}: Int = {v}")
        else:
            out.append(f"    public const val {screaming(k)}: Float = {float(v)!r}f")
    out.append("}")
    return "\n".join(out) + "\n"


def swift_motion(t):
    m = motion_values(t)
    out = [f"// {HEADER}", "import Foundation", "", "/// How everything moves (ui/theme.toml `[motion]`, ui/docs/motion.md).", "public enum MotionTokens {"]
    for k, doc in MOTION.items():
        v = m[k]
        out.append(f"    /// {doc}")
        if isinstance(v, int) and not isinstance(v, bool):
            out.append(f"    public static let {camel(k)}: Int = {v}")
        else:
            out.append(f"    public static let {camel(k)}: Double = {float(v)!r}")
    out.append("    /// A move's response, Apple's period of the spring: 2π/√stiffness, in seconds.")
    out.append(f"    public static let response: Double = {round(response(m['stiffness']), 6)!r}")
    out.append("    /// A fade's response.")
    out.append(f"    public static let fadeResponse: Double = {round(response(m['fade-stiffness']), 6)!r}")
    out.append("    /// A band's response as it springs back.")
    out.append(f"    public static let bandResponse: Double = {round(response(m['band-stiffness']), 6)!r}")
    out.append("}")
    return "\n".join(out) + "\n"


def rust_motion(t):
    m = motion_values(t)
    out = [f"//! {HEADER}", "//!", "//! How everything moves (ui/theme.toml `[motion]`, ui/docs/motion.md).", ""]
    for k, doc in MOTION.items():
        v = m[k]
        out.append(f"/// {doc}")
        if isinstance(v, int) and not isinstance(v, bool):
            out.append(f"pub const {screaming(k)}: u64 = {v};")
        else:
            out.append(f"pub const {screaming(k)}: f32 = {float(v)!r};")
    out.append("/// A move's response, Apple's period of the spring: 2π/√stiffness, in seconds.")
    out.append(f"pub const RESPONSE: f32 = {round(response(m['stiffness']), 6)!r};")
    out.append("/// A fade's response.")
    out.append(f"pub const FADE_RESPONSE: f32 = {round(response(m['fade-stiffness']), 6)!r};")
    out.append("/// A band's response as it springs back.")
    out.append(f"pub const BAND_RESPONSE: f32 = {round(response(m['band-stiffness']), 6)!r};")
    return "\n".join(out) + "\n"


def layout_values(t):
    m = t["layout"]
    missing = [k for k in LAYOUT if k not in m]
    if missing:
        sys.exit(f"theme.toml [layout] lacks {', '.join(missing)}")
    return m


def kotlin_layout(t):
    m = layout_values(t)
    out = [f"// {HEADER}", "package dev.arkui", "", "/** How a window is laid out (ui/theme.toml `[layout]`, ui/docs/design.md). */", "public object LayoutTokens {"]
    for k, doc in LAYOUT.items():
        out.append(f"    /** {doc} */")
        out.append(f"    public const val {screaming(k)}: Float = {float(m[k])!r}f")
    out.append("}")
    return "\n".join(out) + "\n"


def swift_layout(t):
    m = layout_values(t)
    out = [f"// {HEADER}", "import Foundation", "", "/// How a window is laid out (ui/theme.toml `[layout]`, ui/docs/design.md).", "public enum LayoutTokens {"]
    for k, doc in LAYOUT.items():
        out.append(f"    /// {doc}")
        out.append(f"    public static let {camel(k)}: Double = {float(m[k])!r}")
    out.append("}")
    return "\n".join(out) + "\n"


def rust_layout(t):
    m = layout_values(t)
    out = [f"//! {HEADER}", "//!", "//! How a window is laid out (ui/theme.toml `[layout]`, ui/docs/design.md).", ""]
    for k, doc in LAYOUT.items():
        out.append(f"/// {doc}")
        out.append(f"pub const {screaming(k)}: f32 = {float(m[k])!r};")
    return "\n".join(out) + "\n"


def spring_curve(stiffness, damping, rest):
    """A spring from rest to rest as CSS has it: how long it takes to come
    within `rest` of where it goes, in ms, and `linear()` sampled along it.
    The same spring as every toolkit's (unit mass, critically damped or
    under): what the browser approximates, since CSS has no spring."""
    import math
    w = math.sqrt(stiffness)
    z = damping

    def x(t):
        if z >= 1.0:
            return 1 - (1 + w * t) * math.exp(-w * t)
        wd = w * math.sqrt(1 - z * z)
        return 1 - math.exp(-z * w * t) * (math.cos(wd * t) + z * w / wd * math.sin(wd * t))

    t, step, end = 0.0, 0.001, 0.0
    while t < 5.0:
        if abs(1 - x(t)) > rest:
            end = t
        t += step
    end += step
    n = 24
    points = ", ".join(f"{x(end * i / n):.4f}" for i in range(n)) + ", 1"
    return round(end * 1000), f"linear({points})"


def css_px(v):
    return f"{float(v):g}px"


def css(t):
    m = motion_values(t)
    lay = layout_values(t)
    out = [f"/* {HEADER} */", "", "/* The theme as CSS variables for the browser (arkui-web): every role by",
           "   name, light by default, dark by the system's choice unless the page says",
           "   (`data-theme`), an app's accent by `data-accent`; the window's measures,",
           "   the springs as `linear()` curves sampled from the same stiffness and",
           "   damping every toolkit rides, and the size classes' breakpoints. */", ""]

    def block(sel, side, accent=None):
        lines = [f"{sel} {{"]
        if accent is None:
            colors = side_colors(t, side)
            for name in roles():
                lines.append(f"  --ark-{name}: {colors[name]};")
            for i, c in enumerate(t["chart"][side]):
                lines.append(f"  --ark-chart-{i}: {c};")
            lines.append(f"  --ark-wash: {wash(t, side)};")
            lines.append(f"  color-scheme: {side};")
        else:
            for name in ACCENT:
                lines.append(f"  --ark-{name}: {accent[side][name]};")
        lines.append("}")
        return lines

    out += [":root {"]
    for k in LAYOUT:
        v = lay[k]
        out.append(f"  --ark-{k}: {v if k == 'line-height' else css_px(v)};")
    move_ms, move_curve = spring_curve(m["stiffness"], m["move"], m["rest"])
    size_ms, size_curve = spring_curve(m["stiffness"], m["size"], m["rest"])
    fade_ms, fade_curve = spring_curve(m["fade-stiffness"], m["move"], m["rest"])
    out += [f"  --ark-move-ms: {move_ms}ms;", f"  --ark-move: {move_curve};",
            f"  --ark-size-ms: {size_ms}ms;", f"  --ark-size: {size_curve};",
            f"  --ark-fade-ms: {fade_ms}ms;", f"  --ark-fade: {fade_curve};",
            f"  --ark-enter-scale: {m['enter-scale']};", f"  --ark-press: {m['press']};",
            f"  --ark-rise: {css_px(m['rise'])};", f"  --ark-shift: {m['shift']};", "}", ""]
    out += block(":root", "light")
    for a, accent in t["accent"].items():
        out += block(f':root[data-accent="{a}"]', "light", accent)
    out.append("")
    out.append("@media (prefers-color-scheme: dark) {")
    out += ["  " + l for l in block(':root:not([data-theme="light"])', "dark")]
    for a, accent in t["accent"].items():
        out += ["  " + l for l in block(f':root:not([data-theme="light"])[data-accent="{a}"]', "dark", accent)]
    out.append("}")
    out.append("")
    out += block(':root[data-theme="dark"]', "dark")
    for a, accent in t["accent"].items():
        out += block(f':root[data-theme="dark"][data-accent="{a}"]', "dark", accent)
    out.append("")
    out += ["/* Reduced motion: what travels fades in place (ui/docs/motion.md). The",
            "   page says so (`data-motion`), as `motion::reduced()` does, never the",
            "   system directly: arkui-web asks it only when the override is off. */",
            ':root[data-motion="reduced"] {',
            "  --ark-move-ms: var(--ark-fade-ms);", "  --ark-move: var(--ark-fade);",
            "  --ark-size-ms: var(--ark-fade-ms);", "  --ark-size: var(--ark-fade);",
            "  --ark-enter-scale: 1;", "  --ark-press: 1;", "  --ark-rise: 0px;", "  --ark-shift: 0;", "}", ""]
    below = lambda k: f"{float(lay[k]) - 0.02:g}px"
    c, s, e = css_px(lay["compact-width"]), css_px(lay["sidebar-min-width"]), css_px(lay["expanded-width"])
    out += ["/* The size classes (ui/docs/design.md), decided by the window's width as",
            "   every toolkit decides them: a sidebar from sidebar-min-width, a tab bar at",
            "   the top short of it, at the foot when compact. A part shows what is its",
            "   at a width by these classes alone. */",
            ".ark-window { container: ark-window / inline-size; }",
            f"@container ark-window (min-width: {s}) {{", "  .ark-unless-sidebar { display: none !important; }", "}",
            f"@container ark-window (max-width: {below('sidebar-min-width')}) {{", "  .ark-if-sidebar { display: none !important; }", "}",
            f"@container ark-window (max-width: {below('compact-width')}) {{", "  .ark-unless-compact { display: none !important; }", "}",
            f"@container ark-window (min-width: {c}) {{", "  .ark-if-compact { display: none !important; }", "}",
            f"@container ark-window (min-width: {c}) and (max-width: {below('sidebar-min-width')}) {{", "  .ark-unless-topbar { display: none !important; }", "}",
            f"@container ark-window (max-width: {below('compact-width')}), (min-width: {s}) {{", "  .ark-if-topbar { display: none !important; }", "}",
            f"@container ark-window (min-width: {e}) {{", "  .ark-unless-expanded { display: none !important; }", "}",
            f"@container ark-window (max-width: {below('expanded-width')}) {{", "  .ark-if-expanded { display: none !important; }", "}"]
    return "\n".join(out) + "\n"


def colorset(accent):
    def color(hex_):
        r, g, b = rgb(hex_)
        return {"color-space": "srgb", "components": {"alpha": "1.000", "blue": f"0x{b:02X}", "green": f"0x{g:02X}", "red": f"0x{r:02X}"}}

    doc = {
        "colors": [
            {"color": color(accent["light"]["accent"]), "idiom": "universal"},
            {"appearances": [{"appearance": "luminosity", "value": "dark"}], "color": color(accent["dark"]["accent"]), "idiom": "universal"},
        ],
        "info": {"author": "xcode", "version": 1},
    }
    return json.dumps(doc, indent=2) + "\n"


def files(t):
    out = {KOTLIN: kotlin(t), SWIFT_CORE: swift_core(t), SWIFT_UI: swift_ui(t), RUST: rust(t)}
    out |= {KOTLIN_MOTION: kotlin_motion(t), SWIFT_MOTION: swift_motion(t), RUST_MOTION: rust_motion(t)}
    out |= {KOTLIN_LAYOUT: kotlin_layout(t), SWIFT_LAYOUT: swift_layout(t), RUST_LAYOUT: rust_layout(t)}
    out[CSS] = css(t)
    for accent in t["accent"].values():
        for path in accent["colorsets"]:
            out[os.path.join(path, "Contents.json")] = colorset(accent)
    return out


def write(root):
    for path, text in files(load()).items():
        full = os.path.join(root, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as f:
            f.write(text)
        print(f"wrote {path}")


def main():
    match sys.argv[1:]:
        case ["write"]:
            write(os.path.dirname(HERE))
        case ["write", root]:
            write(root)
        case ["contrast"]:
            check_contrast()
        case ["files"]:
            print("\n".join(files(load())))
        case _:
            sys.exit(__doc__ or "theme.py write [ROOT] | contrast | files")


if __name__ == "__main__":
    main()
