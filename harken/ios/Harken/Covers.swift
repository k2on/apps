// Covers, fetched once and shrunk before anything draws them — Android's
// `Covers.kt` on an iPhone, and the cover every screen draws (`CoverView`).
//
// Two caches: the encoded original on the disk, under Caches/covers, named
// by the URL's FNV-1a (a cache: the system may sweep it, and losing it
// costs a fetch); and decoded images in memory (`NSCache`, bounded by cost
// in bytes). The decode is bounded too: ImageIO's thumbnail of the original
// at the size it is drawn, never past `bound` pixels, so a 3000-pixel scan is
// a few hundred pixels before it is ever an image. One fetch and one decode
// per URL however many rows ask at once; a failure is remembered for the
// launch and its reason dropped — a cover is decoration, and the derived
// square under it is already right. `/media` needs no token, which is also
// what lets the lock screen draw the same URL.
import ArkUI
import SwiftUI
import UIKit
import ImageIO

@MainActor
final class Covers: ObservableObject {
    static let shared = Covers()

    /// The most pixels a cover is decoded to across: the full player's, at a phone's scale.
    static let bound: CGFloat = 1024

    private let memory = NSCache<NSString, UIImage>()
    private var missing = Set<String>()
    private var loading = [String: Task<UIImage?, Never>]()
    private let dir: URL

    init() {
        memory.totalCostLimit = 48 * 1024 * 1024
        dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("covers")
    }

    /// A cover already decoded, without waiting.
    func cached(_ url: String, side: CGFloat) -> UIImage? { memory.object(forKey: key(url, side)) }

    private func key(_ url: String, _ side: CGFloat) -> NSString {
        // Decoded at one of a few sizes, so a row and a shelf share theirs.
        "\(Covers.bucket(side))|\(url)" as NSString
    }

    private static func bucket(_ side: CGFloat) -> Int {
        let px = min(side * UITraitCollection.current.displayScale, bound)
        for b in [96, 192, 384, 768] where px <= CGFloat(b) { return b }
        return Int(bound)
    }

    /// The cover at `url`, decoded to about `side` points across, or none.
    func load(_ url: String, side: CGFloat) async -> UIImage? {
        let k = key(url, side)
        if let m = memory.object(forKey: k) { return m }
        if missing.contains(url) { return nil }
        if let t = loading[k as String] { return await t.value }
        let px = CGFloat(Covers.bucket(side))
        let file = dir.appendingPathComponent(String(format: "%08x", Art.hash(url)))
        let dir = self.dir
        let task = Task.detached(priority: .utility) { () -> UIImage? in
            let data: Data
            if let d = try? Data(contentsOf: file) {
                data = d
            } else {
                guard let u = URL(string: url), let got = try? await URLSession.shared.data(from: u),
                      let http = got.1 as? HTTPURLResponse, http.statusCode / 100 == 2 else { return nil }
                let d = got.0
                try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                try? d.write(to: file, options: .atomic)
                data = d
            }
            return Covers.decode(data, px: px)
        }
        loading[k as String] = task
        let image = await task.value
        loading[k as String] = nil
        if let i = image {
            memory.setObject(i, forKey: k, cost: Int(i.size.width * i.size.height * i.scale * i.scale * 4))
        } else {
            missing.insert(url)
        }
        return image
    }

    /// ImageIO's thumbnail of `data`, `px` across at most: decoded small, never whole.
    nonisolated static func decode(_ data: Data, px: CGFloat) -> UIImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: px,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        return UIImage(cgImage: cg)
    }

    /// Start decoding the covers of a list that has just arrived, so the rows that draw them find them.
    func warm(_ urls: [String], side: CGFloat) {
        for u in urls where !u.isEmpty && cached(u, side: side) == nil && !missing.contains(u) {
            Task { _ = await load(u, side: side) }
        }
    }
}

/// A picture, `side` across: the cover at `url` (a whole URL; "" for none),
/// over the square derived from `seed`. Round for a person.
struct CoverView: View {
    let seed: String
    let url: String
    var side: CGFloat
    var round = false
    var corner: CGFloat = Space.corner

    @State private var image: UIImage? = nil

    var body: some View {
        let shape = round ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: corner, style: .continuous))
        ZStack {
            Square(seed: seed)
            if let i = image ?? (url.isEmpty ? nil : Covers.shared.cached(url, side: side)) {
                Image(uiImage: i).resizable().scaledToFill()
            }
        }
        .clipShape(shape)
        .task(id: url) {
            image = nil
            if url.isEmpty { return }
            image = await Covers.shared.load(url, side: side)
        }
    }
}
