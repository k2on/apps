// What a watch's buttons do, beside what it holds (`Part::ActionBar`,
// ui/docs/pebble.md): Pebble's `ActionBarLayer`, its up, select and down
// buttons' glyphs at the trailing edge. A phone has no such buttons, so here
// it is a column of glyph buttons there, top, middle and bottom, each
// pressed as a give (`Pressable`) and said by its label. Its twins are
// Compose's ActionBar.kt, iced's and arkui-web's `Part::ActionBar` and
// ark-pebble's action bar.
import SwiftUI

/// Which of a watch's buttons an `ActionItem` is.
public enum ActionButton {
    case up, select, down
}

/// One button of an action bar: its glyph, what it is said as, and what
/// pressing it does.
public struct ActionItem {
    let glyph: String
    let label: String
    let button: ActionButton
    let onTap: () -> Void

    public init(_ glyph: String, label: String, button: ActionButton, onTap: @escaping () -> Void) {
        self.glyph = glyph
        self.label = label
        self.button = button
        self.onTap = onTap
    }
}

/// `content`, and a column of its buttons at the trailing edge.
public struct ActionBar<Content: View>: View {
    let items: [ActionItem]
    let content: Content

    public init(items: [ActionItem], @ViewBuilder content: () -> Content) {
        self.items = items
        self.content = content()
    }

    public var body: some View {
        HStack(alignment: .center, spacing: Space.s) {
            VStack(alignment: .leading, spacing: 0) { content }
                .frame(maxWidth: .infinity, alignment: .leading)
            VStack(spacing: 0) {
                Spacer(minLength: 0)
                slot(.up)
                Spacer(minLength: 0)
                slot(.select)
                Spacer(minLength: 0)
                slot(.down)
                Spacer(minLength: 0)
            }
        }
    }

    @ViewBuilder private func slot(_ button: ActionButton) -> some View {
        if let item = items.first(where: { $0.button == button }) {
            Pressable(item.label, press: .give, onTap: item.onTap) {
                Image(systemName: ArkGlyphs.sf(item.glyph))
                    .arkType(.title)
                    .foregroundStyle(Color.ink)
                    .padding(Space.s)
            }
        }
    }
}
