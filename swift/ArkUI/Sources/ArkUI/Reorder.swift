// A stack whose rows are dragged into a new order by a handle, as the Music
// app's queue has it: the row lifts — a little larger, a shadow under it —
// and follows the finger, and the rows it passes slide out of its way on
// Springs.move, so the gap it would land in is always open under it. Let
// go, it settles into that gap, and the app is told from where to where.
//
// The row being dragged is drawn over the stack, where the finger has it,
// and its place in the stack is left empty: the stack is laid out in the
// order the row has been dragged into, so the rows it passes move with the
// stack's own animation, and the one dragged never jumps as its place does.
// Where it would land is worked out from the rows' heights, measured as they
// are laid out, so rows need not all be one height.
//
// A handle, not the whole row: the row keeps its tap and its context menu,
// and a drag anywhere else still scrolls what the stack is in.
//
// The app owns the order. Until the items it gives come back in the order
// dragged into — the session's answer comes back from the room — that order
// is kept, so the row does not jump back on the way.
import SwiftUI

/// A stack of `items` whose rows are dragged into another order by the
/// handle each is given. `key` names each item, uniquely; `onMove(from, to)`
/// is told when a row is let go somewhere new, both counted from the top.
public struct Reorderable<Item, Key: Hashable, Row: View>: View {
    let items: [Item]
    let key: (Item) -> Key
    let spacing: CGFloat
    let onMove: (Int, Int) -> Void
    let row: (Item, ReorderHandle) -> Row

    public init(
        _ items: [Item],
        key: @escaping (Item) -> Key,
        spacing: CGFloat = 0,
        onMove: @escaping (Int, Int) -> Void,
        @ViewBuilder row: @escaping (Item, ReorderHandle) -> Row
    ) {
        self.items = items
        self.key = key
        self.spacing = spacing
        self.onMove = onMove
        self.row = row
    }

    /// The order drawn while a row is held or settling, by key.
    @State private var drawn: [Key]?
    /// The row held, and where it started.
    @State private var held: Key?
    @State private var from = 0
    @State private var startTop: CGFloat = 0
    /// How far the finger has moved since the row lifted.
    @State private var travel: CGFloat = 0
    @State private var lifted = false
    @State private var settling = false
    /// Each row's height, as laid out.
    @State private var heights: [Key: CGFloat] = [:]
    /// Rings once as a row lifts.
    @State private var lifts = 0

    /// How long an order dragged into is drawn ahead of the app's saying so.
    static var pending: Duration { .seconds(2) }

    public var body: some View {
        let order = ordered()
        ZStack(alignment: .top) {
            VStack(spacing: spacing) {
                ForEach(order, id: \.0) { pair in
                    row(pair.1, handle(pair.0))
                        .opacity(pair.0 == held ? 0 : 1)
                        .background(GeometryReader { g in
                            Color.clear.preference(key: Heights<Key>.self, value: [pair.0: g.size.height])
                        })
                }
            }
            if let held, let item = order.first(where: { $0.0 == held })?.1 {
                row(item, ReorderHandle(changed: { _ in }, ended: {}))
                    .background(.background)
                    .scaleEffect(lifted ? 1.02 : 1)
                    .shadow(color: .black.opacity(lifted ? 0.18 : 0), radius: 12, y: 4)
                    .offset(y: settling ? top(of: held, in: order.map(\.0)) : liftedTop(order.map(\.0)))
                    .allowsHitTesting(false)
                    .zIndex(1)
            }
        }
        .onPreferenceChange(Heights<Key>.self) { heights.merge($0) { $1 } }
        .onChange(of: items.map(key)) { _, keys in
            if held == nil, drawn == keys { drawn = nil }
        }
        .sensoryFeedback(.selection, trigger: lifts)
    }

    /// The items in the order to draw them, each with its key.
    private func ordered() -> [(Key, Item)] {
        let pairs = items.map { (key($0), $0) }
        guard let drawn else { return pairs }
        let byKey = Dictionary(pairs, uniquingKeysWith: { a, _ in a })
        // One the app took out meanwhile is gone; one it added comes after.
        return drawn.compactMap { k in byKey[k].map { (k, $0) } } + pairs.filter { !drawn.contains($0.0) }
    }

    private func height(_ k: Key) -> CGFloat { heights[k] ?? 0 }

    /// Where row `k` starts down the stack, laid out in `order`.
    private func top(of k: Key, in order: [Key]) -> CGFloat {
        guard let at = order.firstIndex(of: k) else { return 0 }
        return order[..<at].reduce(0) { $0 + height($1) } + spacing * CGFloat(at)
    }

    /// Where the lifted row is drawn: with the finger, never off either end.
    private func liftedTop(_ order: [Key]) -> CGFloat {
        guard let held else { return 0 }
        let total = order.reduce(0) { $0 + height($1) } + spacing * CGFloat(max(order.count - 1, 0))
        return min(max(startTop + travel, 0), max(total - height(held), 0))
    }

    private func handle(_ k: Key) -> ReorderHandle {
        ReorderHandle(changed: { dy in drag(k, dy) }, ended: { drop() })
    }

    private func drag(_ k: Key, _ dy: CGFloat) {
        if held == nil {
            let order = ordered().map(\.0)
            guard let at = order.firstIndex(of: k) else { return }
            drawn = order
            held = k
            from = at
            startTop = top(of: k, in: order)
            settling = false
            lifts += 1
            withAnimation(Springs.size) { lifted = true }
        }
        guard held == k, var order = drawn else { return }
        travel = dy
        // The lifted row lands where its middle is among the others.
        let middle = liftedTop(order) + height(k) / 2
        order.removeAll { $0 == k }
        var y: CGFloat = 0
        var to = 0
        for other in order {
            if middle > y + height(other) / 2 { to += 1 }
            y += height(other) + spacing
        }
        order.insert(k, at: to)
        if order != drawn {
            withAnimation(Springs.move) { drawn = order }
        }
    }

    private func drop() {
        guard let held, let order = drawn, let to = order.firstIndex(of: held) else { return }
        let from = from
        withAnimation(Springs.move) {
            settling = true
            lifted = false
        } completion: {
            self.held = nil
            settling = false
            travel = 0
        }
        if to != from { onMove(from, to) }
        // Let go of the drawing once the app's order has caught up, or after a while if it never does.
        Task { @MainActor in
            try? await Task.sleep(for: Self.pending)
            if self.held == nil { drawn = nil }
        }
    }
}

/// The handle a reorderable row is dragged by: the three lines at its
/// trailing edge.
public struct ReorderHandle: View {
    let changed: (CGFloat) -> Void
    let ended: () -> Void

    public var body: some View {
        Image(systemName: "line.3.horizontal")
            .foregroundStyle(.secondary)
            .frame(width: 44, height: 44)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .global)
                    .onChanged { changed($0.translation.height) }
                    .onEnded { _ in ended() }
            )
            .accessibilityLabel("Reorder")
    }
}

private struct Heights<Key: Hashable>: PreferenceKey {
    static var defaultValue: [Key: CGFloat] { [:] }
    static func reduce(value: inout [Key: CGFloat], nextValue: () -> [Key: CGFloat]) {
        value.merge(nextValue()) { $1 }
    }
}
