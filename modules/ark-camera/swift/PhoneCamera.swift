// ark-camera on the iPhone and the iPad (modules/ark-camera/README.md): the
// `Camera` protocol UniFFI writes. A QR code is read by VisionKit's data
// scanner (ArkUI's `QrScanner`); a photo is the system's camera; a watched
// photo is AVFoundation's back camera full screen, each analysis frame's
// luminance shown to the eyes in Rust (`Eyes.look`), what they answer drawn
// over the preview — their outline gliding on `Springs.move`, their words —
// and the photo taken when they say, or at the shutter, as the sensor took
// it (unturned: the frames' `turns` stand both upright). A photo picked is
// PHPicker's, one image, as a JPEG. The Mac has no camera to hold over a
// page: every operation there but a pick (an open panel of images) fails,
// said.
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
#if os(iOS)
import ArkUI
import AVFoundation
import PhotosUI
import SwiftUI
import UIKit
#elseif os(macOS)
import AppKit
import UniformTypeIdentifiers
#endif

/// The phone's camera, presented over whatever is in front.
public final class PhoneCamera: Camera, @unchecked Sendable {
    public init() {}

    /// Whether there is a camera to hold over a page.
    @MainActor public static var canWatch: Bool {
        #if os(iOS)
        UIImagePickerController.isSourceTypeAvailable(.camera)
        #else
        false
        #endif
    }

    public func scanQr(reply: ScanReply) {
        #if os(iOS)
        DispatchQueue.main.async {
            guard QrScanner.available else { return reply.failed(why: "This device can't scan a code") }
            var sheet: UIViewController?
            let scanner = QrScanner { text in
                sheet?.dismiss(animated: true)
                reply.scanned(code: text)
            }
            let host = UIHostingController(rootView: scanner.ignoresSafeArea())
            host.presentationController?.delegate = Dismissed { reply.scanned(code: nil) }.kept(by: host)
            sheet = host
            PhoneCamera.top()?.present(host, animated: true)
        }
        #else
        reply.failed(why: "There's no camera to scan with here")
        #endif
    }

    public func takePhoto(reply: PhotoReply) {
        #if os(iOS)
        DispatchQueue.main.async {
            guard UIImagePickerController.isSourceTypeAvailable(.camera) else { return reply.failed(why: "This device has no camera") }
            let picker = UIImagePickerController()
            picker.sourceType = .camera
            let taken = Taken(reply)
            picker.delegate = taken
            objc_setAssociatedObject(picker, &PhoneCamera.keep, taken, .OBJC_ASSOCIATION_RETAIN)
            PhoneCamera.top()?.present(picker, animated: true)
        }
        #else
        reply.failed(why: "There's no camera here")
        #endif
    }

    public func watch(eyes: Eyes, reply: PhotoReply) {
        #if os(iOS)
        DispatchQueue.main.async {
            let c = WatchController(eyes: eyes, reply: reply)
            c.modalPresentationStyle = .fullScreen
            PhoneCamera.top()?.present(c, animated: true)
        }
        #else
        reply.failed(why: "There's no camera to hold over a page here")
        #endif
    }

    public func pickPhoto(reply: PhotoReply) {
        #if os(iOS)
        DispatchQueue.main.async {
            var config = PHPickerConfiguration()
            config.filter = .images
            config.selectionLimit = 1
            let picker = PHPickerViewController(configuration: config)
            let picked = Picked(reply)
            picker.delegate = picked
            picker.presentationController?.delegate = Dismissed { reply.taken(jpeg: nil) }.kept(by: picker)
            objc_setAssociatedObject(picker, &PhoneCamera.picked, picked, .OBJC_ASSOCIATION_RETAIN)
            PhoneCamera.top()?.present(picker, animated: true)
        }
        #elseif os(macOS)
        DispatchQueue.main.async {
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [.image]
            panel.allowsMultipleSelection = false
            panel.message = "A photo of the receipt"
            guard panel.runModal() == .OK, let url = panel.url else { return reply.taken(jpeg: nil) }
            guard let image = NSImage(contentsOf: url), let tiff = image.tiffRepresentation,
                  let jpeg = NSBitmapImageRep(data: tiff)?.representation(using: .jpeg, properties: [.compressionFactor: 0.9])
            else { return reply.failed(why: "That file isn't a photo") }
            reply.taken(jpeg: jpeg)
        }
        #else
        reply.failed(why: "There are no photos to choose from here")
        #endif
    }

    /// The longest side a photo picked is handed over at: what the server
    /// reads a receipt from, and no more.
    static let longest: CGFloat = 3000

    #if os(iOS)
    nonisolated(unsafe) static var keep = 0
    nonisolated(unsafe) static var picked = 0

    /// What is in front, to present over.
    @MainActor static func top() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        var top = scenes.first?.keyWindow?.rootViewController
        while let next = top?.presentedViewController { top = next }
        return top
    }
    #endif
}

#if os(iOS)
/// A sheet swiped away: the scan backed out of.
private final class Dismissed: NSObject, UIAdaptivePresentationControllerDelegate {
    let done: () -> Void
    init(_ done: @escaping () -> Void) { self.done = done }
    func kept(by c: UIViewController) -> Dismissed {
        objc_setAssociatedObject(c, &PhoneCamera.keep, self, .OBJC_ASSOCIATION_RETAIN)
        return self
    }
    func presentationControllerDidDismiss(_ p: UIPresentationController) { done() }
}

/// The system camera's photo, as a JPEG.
private final class Taken: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
    let reply: PhotoReply
    init(_ reply: PhotoReply) { self.reply = reply }
    func imagePickerController(_ p: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
        p.dismiss(animated: true)
        reply.taken(jpeg: (info[.originalImage] as? UIImage)?.jpegData(compressionQuality: 0.9))
    }
    func imagePickerControllerDidCancel(_ p: UIImagePickerController) {
        p.dismiss(animated: true)
        reply.taken(jpeg: nil)
    }
}

/// PHPicker's photo, upright, at most `PhoneCamera.longest` on its long
/// side, as a JPEG; none when the person backed out.
private final class Picked: NSObject, PHPickerViewControllerDelegate {
    let reply: PhotoReply
    init(_ reply: PhotoReply) { self.reply = reply }
    func picker(_ p: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        p.dismiss(animated: true)
        guard let provider = results.first?.itemProvider, provider.canLoadObject(ofClass: UIImage.self) else {
            return reply.taken(jpeg: nil)
        }
        let reply = self.reply
        provider.loadObject(ofClass: UIImage.self) { object, error in
            guard let image = object as? UIImage else {
                return reply.failed(why: error?.localizedDescription ?? "That photo couldn't be read")
            }
            let side = max(image.size.width, image.size.height)
            let scale = min(1, PhoneCamera.longest / max(side, 1))
            let size = CGSize(width: image.size.width * scale, height: image.size.height * scale)
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            // Drawn, so it is upright whatever its orientation said.
            let upright = UIGraphicsImageRenderer(size: size, format: format).image { _ in image.draw(in: CGRect(origin: .zero, size: size)) }
            reply.taken(jpeg: upright.jpegData(compressionQuality: 0.9))
        }
    }
}

/// What the overlay draws: the eyes' last answer.
@MainActor
private final class CameraLook: ObservableObject {
    @Published var outline: [CGPoint] = []
    @Published var held = false
    @Published var words = ""
    @Published var taking = false
    var shutter: () -> Void = {}
    var cancel: () -> Void = {}
}

/// The camera full screen, its frames shown to the eyes.
private final class WatchController: UIViewController, AVCaptureVideoDataOutputSampleBufferDelegate, AVCapturePhotoCaptureDelegate, @unchecked Sendable {
    private let eyes: Eyes
    private let reply: PhotoReply
    private let session = AVCaptureSession()
    private let frames = AVCaptureVideoDataOutput()
    private let photos = AVCapturePhotoOutput()
    private let queue = DispatchQueue(label: "ark-camera.frames")
    private let watched = CameraLook()
    private var preview: AVCaptureVideoPreviewLayer?
    /// The reply given: nothing more is said, whatever comes after.
    private var done = false
    private var taking = false

    /// About how wide a frame is shown to the eyes.
    private static let look = 640
    /// Quarter turns clockwise that stand the back camera's frames upright, held as a phone is.
    private static let turns: UInt8 = 1

    init(eyes: Eyes, reply: PhotoReply) {
        self.eyes = eyes
        self.reply = reply
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { nil }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        watched.shutter = { [weak self] in self?.take() }
        watched.cancel = { [weak self] in self?.finish(nil) }
        let overlay = UIHostingController(rootView: WatchOverlay(watched: watched))
        overlay.view.backgroundColor = .clear
        addChild(overlay)
        overlay.view.frame = view.bounds
        overlay.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        AVCaptureDevice.requestAccess(for: .video) { ok in
            DispatchQueue.main.async {
                guard ok else { return self.fail("The camera isn't allowed: Settings can allow it") }
                self.start(overlay: overlay)
            }
        }
    }

    private func start(overlay: UIViewController) {
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
              let input = try? AVCaptureDeviceInput(device: device), session.canAddInput(input) else {
            return fail("The camera couldn't be opened")
        }
        session.beginConfiguration()
        session.sessionPreset = .photo
        session.addInput(input)
        frames.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange]
        frames.alwaysDiscardsLateVideoFrames = true
        frames.setSampleBufferDelegate(self, queue: queue)
        if session.canAddOutput(frames) { session.addOutput(frames) }
        if session.canAddOutput(photos) { session.addOutput(photos) }
        // As the sensor sees it: the eyes' frames and the photo alike.
        frames.connection(with: .video)?.videoRotationAngle = 0
        photos.connection(with: .video)?.videoRotationAngle = 0
        session.commitConfiguration()
        let layer = AVCaptureVideoPreviewLayer(session: session)
        layer.videoGravity = .resizeAspect
        layer.frame = view.bounds
        view.layer.addSublayer(layer)
        preview = layer
        view.addSubview(overlay.view)
        overlay.didMove(toParent: self)
        queue.async { self.session.startRunning() }
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        preview?.frame = view.bounds
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sample: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard !taking, let px = CMSampleBufferGetImageBuffer(sample) else { return }
        CVPixelBufferLockBaseAddress(px, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(px, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddressOfPlane(px, 0) else { return }
        let w = CVPixelBufferGetWidthOfPlane(px, 0)
        let h = CVPixelBufferGetHeightOfPlane(px, 0)
        let row = CVPixelBufferGetBytesPerRowOfPlane(px, 0)
        let step = max(1, w / WatchController.look)
        let (sw, sh) = (w / step, h / step)
        let src = base.assumingMemoryBound(to: UInt8.self)
        var luma = Data(count: sw * sh)
        luma.withUnsafeMutableBytes { (out: UnsafeMutableRawBufferPointer) in
            for y in 0..<sh { for x in 0..<sw { out[y * sw + x] = src[y * step * row + x * step] } }
        }
        let at = Int64(CACurrentMediaTime() * 1000)
        let look = eyes.look(frame: Frame(width: UInt32(sw), height: UInt32(sh), luma: luma, rgb: Data(), turns: WatchController.turns, atMs: at))
        let corners = stride(from: 0, to: look.outline.count - 1, by: 2).map { CGPoint(x: CGFloat(look.outline[$0]), y: CGFloat(look.outline[$0 + 1])) }
        DispatchQueue.main.async {
            self.watched.outline = corners
            self.watched.held = look.held
            self.watched.words = look.words
            if look.take { self.take() }
        }
    }

    private func take() {
        guard !taking else { return }
        taking = true
        watched.taking = true
        let settings = AVCapturePhotoSettings(format: [AVVideoCodecKey: AVVideoCodecType.jpeg])
        photos.capturePhoto(with: settings, delegate: self)
    }

    func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto, error: Error?) {
        let jpeg = photo.fileDataRepresentation()
        DispatchQueue.main.async {
            if let jpeg { self.finish(jpeg) } else { self.fail("The camera didn't take the picture") }
        }
    }

    private func fail(_ why: String) {
        guard !done else { return }
        done = true
        session.stopRunning()
        dismiss(animated: true)
        reply.failed(why: why)
    }

    private func finish(_ jpeg: Data?) {
        guard !done else { return }
        done = true
        queue.async { self.session.stopRunning() }
        dismiss(animated: true)
        reply.taken(jpeg: jpeg)
    }
}

/// The eyes' outline over the preview, the words above, the shutter and
/// Cancel below.
private struct WatchOverlay: View {
    @ObservedObject var watched: CameraLook

    var body: some View {
        ZStack {
            GeometryReader { g in
                // The preview is the frame's 3:4, fitted to the screen's width.
                let w = g.size.width
                let h = w * 4 / 3
                let top = (g.size.height - h) / 2
                Outline(corners: watched.outline.map { CGPoint(x: $0.x * w, y: top + $0.y * h) })
                    .fill(Color.accentColor.opacity(watched.held ? 0.38 : 0.26))
                    .overlay(Outline(corners: watched.outline.map { CGPoint(x: $0.x * w, y: top + $0.y * h) })
                        .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 3, lineJoin: .round)))
                    .opacity(watched.outline.isEmpty ? 0 : 1)
                    .motion(Springs.move, value: watched.outline)
            }
            VStack {
                Text(watched.taking ? "Keeping it…" : watched.words)
                    .padding(.horizontal, Space.l)
                    .padding(.vertical, Space.s)
                    .background(.black.opacity(0.55), in: Capsule())
                    .foregroundStyle(.white)
                    .padding(.top, Space.xl)
                Spacer()
                HStack {
                    Button("Cancel") { watched.cancel() }
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity)
                    Button { watched.shutter() } label: {
                        Circle().fill(watched.taking ? Color.gray : Color.white)
                            .padding(8)
                            .overlay(Circle().stroke(.white, lineWidth: 4))
                            .frame(width: 76, height: 76)
                    }
                    .buttonStyle(.press)
                    .disabled(watched.taking)
                    .accessibilityLabel("Take the Picture")
                    Color.clear.frame(maxWidth: .infinity, maxHeight: 1)
                }
                .padding(.bottom, Space.xl)
            }
        }
    }
}

/// Four corners, closed; none drawn while there are fewer.
private struct Outline: Shape {
    var corners: [CGPoint]

    var animatableData: AnimatableVector {
        get { AnimatableVector(corners.flatMap { [Double($0.x), Double($0.y)] }) }
        set { corners = stride(from: 0, to: newValue.values.count - 1, by: 2).map { CGPoint(x: newValue.values[$0], y: newValue.values[$0 + 1]) } }
    }

    func path(in rect: CGRect) -> Path {
        var p = Path()
        guard corners.count == 4 else { return p }
        p.addLines(corners)
        p.closeSubpath()
        return p
    }
}

/// The corners as SwiftUI animates them: a vector of their coordinates.
private struct AnimatableVector: VectorArithmetic {
    var values: [Double]
    init(_ values: [Double]) { self.values = values }
    static var zero: AnimatableVector { AnimatableVector([]) }
    private static func zip(_ a: AnimatableVector, _ b: AnimatableVector, _ f: (Double, Double) -> Double) -> AnimatableVector {
        let n = max(a.values.count, b.values.count)
        return AnimatableVector((0..<n).map { f($0 < a.values.count ? a.values[$0] : 0, $0 < b.values.count ? b.values[$0] : 0) })
    }
    static func + (a: AnimatableVector, b: AnimatableVector) -> AnimatableVector { zip(a, b, +) }
    static func - (a: AnimatableVector, b: AnimatableVector) -> AnimatableVector { zip(a, b, -) }
    mutating func scale(by rhs: Double) { values = values.map { $0 * rhs } }
    var magnitudeSquared: Double { values.reduce(0) { $0 + $1 * $1 } }
}
#endif
