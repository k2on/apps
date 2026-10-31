// Rows put in another order as a list's Edit mode hands them over — the
// offsets lifted and the offset they are let go in front of, both counted
// before anything was lifted — so an app's arithmetic over a drop (what a
// row now stands in front of, which section it is now under) is Foundation
// alone, and tested on Linux. SwiftUI's own `move(fromOffsets:toOffset:)`,
// said again without SwiftUI.
import Foundation

public enum Order {
    /// `items` with those at `offsets` taken out and put, in their order, in
    /// front of what was at `destination` — after the last when it is past
    /// the end.
    public static func moved<T>(_ items: [T], from offsets: IndexSet, to destination: Int) -> [T] {
        let lifted = offsets.filter { items.indices.contains($0) }.map { items[$0] }
        var rest = [T]()
        var at = 0
        for (i, x) in items.enumerated() where !offsets.contains(i) {
            if i < destination { at += 1 }
            rest.append(x)
        }
        rest.insert(contentsOf: lifted, at: at)
        return rest
    }
}
