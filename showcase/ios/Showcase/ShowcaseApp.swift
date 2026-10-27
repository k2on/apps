// ArkUI's components on their own, on a real simulator: one shot by name
// (`-showcase art`), with placeholder content and ArkUI's neutral colours.
//
// The shots are the docs' (`docs/site/src/components.rs`, those it says iOS
// has); the screens workflow (.github/workflows/screens.yaml) captures each a
// few seconds after launch, in the theme it set.
import ArkUI
import SwiftUI

@main
struct ShowcaseApp: App {
    var body: some Scene {
        WindowGroup { Screen(shot: Self.shot) }
    }

    /// The shot this launch was asked for: `art` when none was.
    static var shot: String {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "-showcase"), i + 1 < args.count else { return "art" }
        return args[i + 1]
    }
}

/// Places, which mean nothing to any app in particular.
private let places = ["Harbour", "Orchard", "Meadow", "Summit", "Delta", "Lagoon", "Canyon", "Glacier", "Prairie", "Fjord", "Dune", "Grove"]

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
                        ForEach(places, id: \.self) { p in
                            Square(seed: p)
                                .aspectRatio(1, contentMode: .fit)
                                .clipShape(RoundedRectangle(cornerRadius: Space.corner))
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
