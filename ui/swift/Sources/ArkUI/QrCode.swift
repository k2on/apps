// A QR code, drawn, and one read off the camera: what "Add a Device" shows,
// and what "Scan a Code" looks for. Which modules are dark is the sign-in's,
// made once in Rust (`ark_auth::onboarding::qr_code`, the bridge's `qrCode`):
// here it is squares on a light ground with the four-module quiet zone a
// scanner needs, always dark on light whatever the scheme. The scanner is
// VisionKit's, for QR codes alone, and hands back the text it read.
import SwiftUI
#if canImport(VisionKit) && os(iOS)
import VisionKit
#endif

/// The code `size` modules a side, `dark` row by row from the top left.
public struct QrCode: View {
    let size: Int
    let dark: [Bool]

    public init(size: Int, dark: [Bool]) {
        self.size = size
        self.dark = dark
    }

    /// The light modules around the code.
    public static let quiet = 4

    public var body: some View {
        Canvas { c, area in
            let across = CGFloat(size + 2 * QrCode.quiet)
            let m = min(area.width, area.height) / across
            c.fill(Path(CGRect(origin: .zero, size: area)), with: .color(.white))
            var p = Path()
            for (i, d) in dark.enumerated() where d {
                let x = CGFloat(i % size + QrCode.quiet), y = CGFloat(i / size + QrCode.quiet)
                p.addRect(CGRect(x: x * m, y: y * m, width: m, height: m))
            }
            c.fill(p, with: .color(.black))
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityLabel("QR code")
    }
}

#if canImport(VisionKit) && os(iOS)
/// The camera, looking for a QR code: the first one read is handed to `found`, once.
public struct QrScanner: UIViewControllerRepresentable {
    let found: (String) -> Void

    public init(found: @escaping (String) -> Void) { self.found = found }

    /// Whether this phone can scan at all (VisionKit's data scanner, and a camera).
    @MainActor public static var available: Bool { DataScannerViewController.isSupported && DataScannerViewController.isAvailable }

    public func makeUIViewController(context: Context) -> DataScannerViewController {
        let v = DataScannerViewController(recognizedDataTypes: [.barcode(symbologies: [.qr])], qualityLevel: .balanced,
                                          isHighlightingEnabled: true)
        v.delegate = context.coordinator
        try? v.startScanning()
        return v
    }

    public func updateUIViewController(_ v: DataScannerViewController, context: Context) {}

    public func makeCoordinator() -> Coordinator { Coordinator(found) }

    public final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        private let found: (String) -> Void
        private var done = false
        init(_ found: @escaping (String) -> Void) { self.found = found }

        public func dataScanner(_ s: DataScannerViewController, didAdd items: [RecognizedItem], allItems: [RecognizedItem]) {
            guard !done else { return }
            for case let .barcode(b) in items {
                if let text = b.payloadStringValue {
                    done = true
                    s.stopScanning()
                    found(text)
                    return
                }
            }
        }
    }
}
#endif

/// "Add a Device": the join link another device scans or opens — as a QR
/// code and to share — made by `make` (the server is asked, as this login),
/// waiting while it is made; or why it could not be.
public struct AddDeviceView: View {
    let make: @Sendable () async -> DeviceLinkShown
    @State private var shown: DeviceLinkShown? = nil

    public init(make: @escaping @Sendable () async -> DeviceLinkShown) { self.make = make }

    public var body: some View {
        VStack(spacing: Space.l) {
            if let s = shown, let e = s.error {
                Text(e).font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            } else if let s = shown, let l = s.link {
                QrCode(size: s.size, dark: s.dark).frame(maxWidth: 260)
                Text("Scan this with the other device's camera, or open the link there. It works once, for ten minutes.")
                    .font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center)
                ShareLink(item: l) { Label("Share Link", systemImage: "square.and.arrow.up") }
            } else {
                ProgressView()
            }
        }
        .padding(Space.xl)
        .navigationTitle("Add a Device")
        .task { shown = await make() }
    }
}
