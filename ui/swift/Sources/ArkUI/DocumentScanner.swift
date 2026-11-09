// A paper document scanned, as Notes scans one: VisionKit's document
// camera on the iPhone and the iPad (the page's edges found and drawn over
// it live, the picture taken when it is steady and squared up); nothing on
// the Mac, which has no camera to hold over a receipt (`isSupported`), where
// a photo is chosen instead. The first page, as JPEG; none when it was
// cancelled or failed.
#if canImport(SwiftUI)
import SwiftUI
#if canImport(VisionKit) && os(iOS)
import UIKit
import VisionKit
#endif

public struct DocumentScanner: View {
    let done: (Data?) -> Void

    public init(done: @escaping (Data?) -> Void) { self.done = done }

    /// Whether this device can scan one.
    public static var isSupported: Bool {
        #if canImport(VisionKit) && os(iOS)
        VNDocumentCameraViewController.isSupported
        #else
        false
        #endif
    }

    public var body: some View {
        #if canImport(VisionKit) && os(iOS)
        Camera(done: done)
        #else
        Color.clear.onAppear { done(nil) }
        #endif
    }
}

#if canImport(VisionKit) && os(iOS)
private struct Camera: UIViewControllerRepresentable {
    let done: (Data?) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(done) }

    func makeUIViewController(context: Context) -> VNDocumentCameraViewController {
        let c = VNDocumentCameraViewController()
        c.delegate = context.coordinator
        return c
    }

    func updateUIViewController(_ controller: VNDocumentCameraViewController, context: Context) {}

    final class Coordinator: NSObject, VNDocumentCameraViewControllerDelegate {
        private let done: (Data?) -> Void
        init(_ done: @escaping (Data?) -> Void) { self.done = done }

        func documentCameraViewController(_ controller: VNDocumentCameraViewController, didFinishWith scan: VNDocumentCameraScan) {
            done(scan.pageCount > 0 ? scan.imageOfPage(at: 0).jpegData(compressionQuality: 0.8) : nil)
        }

        func documentCameraViewControllerDidCancel(_ controller: VNDocumentCameraViewController) { done(nil) }

        func documentCameraViewController(_ controller: VNDocumentCameraViewController, didFailWithError error: Error) { done(nil) }
    }
}
#endif
#endif
