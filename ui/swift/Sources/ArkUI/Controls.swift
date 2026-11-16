// The native controls the primitives cannot be composed into
// (ui/docs/primitives.md, "The Boundary"), as `ark_app::view` names them:
// something pressed as one, with its menu (`Pressable`: a button's press and
// a context menu are the platform's), and where a sound is and how loud,
// counted from a clock as it is drawn or dragged by a finger
// (`ClockScrubber`, `ClockHairline`, `VolumePart`). Their twins are
// Compose's Controls.kt, iced's `arkui::view` and arkui-web's parts.
import SwiftUI

/// How a pressed thing answers (ui/docs/motion.md, Press Feedback): a cover,
/// a tile or a glyph gives (`.press`), a row the page's width lights
/// (`.pressRow`).
public enum PressKind {
    case give
    case highlight
}

/// What is laid out in `content`, pressed as one: its press on touch-down,
/// said as `label`, its menu on a long press.
public struct Pressable<Content: View>: View {
    let label: String
    let press: PressKind
    let menu: [MenuAction]
    let onTap: () -> Void
    let content: Content

    public init(_ label: String, press: PressKind, menu: [MenuAction] = [], onTap: @escaping () -> Void, @ViewBuilder content: () -> Content) {
        self.label = label
        self.press = press
        self.menu = menu
        self.onTap = onTap
        self.content = content()
    }

    public var body: some View {
        let button = Button(action: onTap) { content.contentShape(Rectangle()) }
            .accessibilityLabel(label)
        Group {
            switch press {
            case .give: button.buttonStyle(.press)
            case .highlight: button.buttonStyle(.pressRow)
            }
        }
        .contextMenu {
            if !menu.isEmpty { MenuActions(menu: menu) }
        }
    }
}

/// Milliseconds as `m:ss`, or `h:mm:ss` from an hour: ark_format's `clock`.
func clockWords(_ ms: Int64) -> String {
    let secs = max(ms, 0) / 1000
    let h = secs / 3600
    return h == 0
        ? String(format: "%d:%02d", secs / 60, secs % 60)
        : String(format: "%d:%02d:%02d", h, secs / 60 % 60, secs % 60)
}

/// Where a clock says a sound is at `now`: `at` read at `since` (the
/// device's epoch milliseconds), counting on while it plays.
func clockAt(_ at: Int64, since: Int64, playing: Bool, length: Int64, now: Date) -> Int64 {
    let n = Int64(now.timeIntervalSince1970 * 1000)
    return min(max(playing ? at + max(n - since, 0) : at, 0), max(length, 1))
}

/// Where a sound is, which a finger drags: ArkUI's `Scrubber` over its
/// clock, redrawn as it plays (`TimelineView`) and still while it does not;
/// let go, `onSeek` is sent once with where. Meeting either end under the
/// finger is a selection's haptic.
public struct ClockScrubber: View {
    let at: Int64
    let since: Int64
    let playing: Bool
    let length: Int64
    let onSeek: (Int64) -> Void
    @State private var end = 0

    public init(_ at: Int64, since: Int64, playing: Bool, length: Int64, onSeek: @escaping (Int64) -> Void) {
        self.at = at
        self.since = since
        self.playing = playing
        self.length = length
        self.onSeek = onSeek
    }

    public var body: some View {
        let whole = Double(max(length, 1))
        TimelineView(.periodic(from: .now, by: playing && !Shots.on ? 0.25 : 3600)) { tick in
            let now = clockAt(at, since: since, playing: playing, length: length, now: tick.date)
            Scrubber(
                value: Double(now) / whole,
                left: { clockWords(Int64($0 * whole)) },
                right: { _ in clockWords(length) },
                onValue: { v in end = v <= 0 ? -1 : v >= 1 ? 1 : 0 },
                onDone: { v in
                    end = 0
                    onSeek(Int64(v * whole))
                })
        }
        .sensoryFeedback(.selection, trigger: end) { _, new in new != 0 }
    }
}

/// Words lit in the accent while the clock is at or past `from` and
/// before `until`, quiet otherwise: a transcript's line said now, redrawn
/// as it plays on the same clock as `ClockHairline`. A colour, never
/// motion.
public struct ClockWords: View {
    let text: String
    let at: Int64
    let since: Int64
    let playing: Bool
    let from: Int64
    let until: Int64

    public init(_ text: String, at: Int64, since: Int64, playing: Bool, from: Int64, until: Int64) {
        self.text = text
        self.at = at
        self.since = since
        self.playing = playing
        self.from = from
        self.until = until
    }

    public var body: some View {
        TimelineView(.periodic(from: .now, by: playing && !Shots.on ? 0.25 : 3600)) { tick in
            let n = Int64(tick.date.timeIntervalSince1970 * 1000)
            let now = playing ? at + max(n - since, 0) : at
            Text(text)
                .arkType(.body)
                .foregroundStyle(now >= from && now < until ? Color.accentText : Color.quiet)
        }
    }
}

/// How far along a sound is, a hairline in the accent, counting on as it
/// plays.
public struct ClockHairline: View {
    let at: Int64
    let since: Int64
    let playing: Bool
    let length: Int64

    public init(_ at: Int64, since: Int64, playing: Bool, length: Int64) {
        self.at = at
        self.since = since
        self.playing = playing
        self.length = length
    }

    public var body: some View {
        TimelineView(.periodic(from: .now, by: playing && !Shots.on ? 0.25 : 3600)) { tick in
            let now = clockAt(at, since: since, playing: playing, length: length, now: tick.date)
            // Drawn, not laid out: the fill scaled from the leading edge.
            ZStack(alignment: .leading) {
                Rectangle().fill(Color.quiet.opacity(0.25))
                Rectangle().fill(Color.accent)
                    .scaleEffect(x: CGFloat(now) / CGFloat(max(length, 1)), y: 1, anchor: .leading)
            }
            .frame(height: 2)
            .accessibilityElement()
            .accessibilityLabel("Position")
            .accessibilityValue(clockWords(now))
        }
    }
}

/// How loud, as `volume` lays it out: `VolumeSlider` over a level 0 to
/// 100, its loudspeaker sending `onMute`; the level follows the finger and
/// `onChange` is sent once, with where it was let go, as the scrubber's
/// seek is. A level from elsewhere is sprung to (`VolumeSlider`).
public struct VolumePart: View {
    let level: Int64
    let muted: Bool
    let about: String
    let onChange: (Int64) -> Void
    let onMute: () -> Void
    @State private var held: Int?

    public init(_ level: Int64, muted: Bool, about: String, onChange: @escaping (Int64) -> Void, onMute: @escaping () -> Void) {
        self.level = level
        self.muted = muted
        self.about = about
        self.onChange = onChange
        self.onMute = onMute
    }

    public var body: some View {
        VolumeSlider(
            level: Int(min(max(level, 0), 100)), muted: muted, about: about,
            onLevel: { held = $0 },
            onDone: {
                if let h = held { onChange(Int64(h)) }
                held = nil
            },
            onMute: onMute)
    }
}
