// The shared pieces the docs show, on a real simulator: what a Debug build
// opens at instead of the app when it is launched with `-showcase <shot>`.
// Only what is any app's — the derived square, the spacing — on its own,
// with placeholder content; what ArkUI for SwiftUI is to become
// (docs/plan-arkui-swift.md).
//
// The shots are the docs' (`rust/docs/src/components.rs`); the screens
// workflow (.github/workflows/screens.yaml) captures each from the
// simulator a few seconds after launch, in the theme it set. A Release
// build has none of this.
#if DEBUG
import SwiftUI

enum Showcase {
    /// The shot this launch was asked for, if any.
    static var shot: String? {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "-showcase"), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    /// Places, which mean nothing to any app in particular.
    static let places = ["Harbour", "Orchard", "Meadow", "Summit", "Delta", "Lagoon", "Canyon", "Glacier", "Prairie", "Fjord", "Dune", "Grove"]

    /// One shot, by the id the docs give it.
    struct Screen: View {
        let shot: String

        var body: some View {
            switch shot {
            case "art":
                ScrollView {
                    VStack(alignment: .leading, spacing: Space.m) {
                        Text("Squares").font(.title2.bold())
                        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: Space.m), count: 3), spacing: Space.m) {
                            ForEach(Showcase.places, id: \.self) { p in
                                Square(seed: p).aspectRatio(1, contentMode: .fit).clipShape(RoundedRectangle(cornerRadius: 8))
                            }
                        }
                    }
                    .padding(Space.page)
                }
            default:
                Text("No showcase for \(shot)")
            }
        }
    }
}
#endif
