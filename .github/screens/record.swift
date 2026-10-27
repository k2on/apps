// A recording of a booted simulator's screen, every frame it composes,
// at the time it composed it: frames are taken straight from the device's
// framebuffer as Simulator.app shows them (SimulatorKit's display port and
// its frame callback), scaled down and written to an H.264 file by
// AVFoundation, each with its own time. `simctl io recordVideo` scales and
// encodes the whole screen itself and, on a runner with no hardware video
// scaler, keeps some ten frames a second of the sixty the app draws.
// CoreSimulator and SimulatorKit are private, so they are reached through
// the Objective-C runtime.
//
//   xcrun swift record.swift <udid> <seconds> <width> <out.mov>
import AVFoundation
import Foundation
import IOSurface
import QuartzCore
import VideoToolbox

let args = CommandLine.arguments
guard args.count == 5, let seconds = Double(args[2]), let width = Int(args[3]) else {
    FileHandle.standardError.write("usage: record.swift <udid> <seconds> <width> <out.mov>\n".data(using: .utf8)!)
    exit(2)
}
let (udid, out) = (args[1], URL(fileURLWithPath: args[4]))

func fail(_ why: String) -> Never {
    FileHandle.standardError.write("record: \(why)\n".data(using: .utf8)!)
    exit(1)
}

// The frameworks, from the active Xcode.
let xcodeSelect = Process()
xcodeSelect.executableURL = URL(fileURLWithPath: "/usr/bin/xcode-select")
xcodeSelect.arguments = ["-p"]
let pipe = Pipe()
xcodeSelect.standardOutput = pipe
try? xcodeSelect.run()
xcodeSelect.waitUntilExit()
let dev = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    .trimmingCharacters(in: .whitespacesAndNewlines)
for path in [
    "/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator",
    "\(dev)/Library/PrivateFrameworks/CoreSimulator.framework/CoreSimulator",
    "\(dev)/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit",
    "\(dev)/../SharedFrameworks/SimulatorKit.framework/SimulatorKit",
] {
    _ = dlopen(path, RTLD_NOW)
}

func object(_ target: NSObject, _ selector: String, _ argument: Any? = nil, _ second: Any? = nil) -> NSObject? {
    let sel = NSSelectorFromString(selector)
    guard target.responds(to: sel) else { return nil }
    return target.perform(sel, with: argument, with: second)?.takeUnretainedValue() as? NSObject
}

// The device, and the framebuffer display ports of its IO.
guard let contextClass = NSClassFromString("SimServiceContext") as? NSObject.Type,
      let context = object(contextClass, "sharedServiceContextForDeveloperDir:error:", dev),
      let deviceSet = object(context, "defaultDeviceSetWithError:"),
      let devices = deviceSet.value(forKey: "devices") as? [NSObject],
      let device = devices.first(where: { ($0.value(forKey: "UDID") as? NSUUID)?.uuidString == udid })
else { fail("no device \(udid)") }
guard let io = object(device, "io") else { fail("no IO for \(udid)") }
_ = object(io, "updateIOPorts")
let displays: [NSObject] = (io.value(forKey: "deviceIOPorts") as? [NSObject] ?? []).compactMap { port in
    guard let id = object(port, "portIdentifier"), "\(id)" == "com.apple.framebuffer.display",
          let descriptor = object(port, "descriptor"),
          descriptor.responds(to: NSSelectorFromString("framebufferSurface"))
    else { return nil }
    return descriptor
}
guard !displays.isEmpty else { fail("no framebuffer display") }

func surface(of display: NSObject) -> IOSurface? {
    guard let surface = display.perform(NSSelectorFromString("framebufferSurface"))?.takeUnretainedValue() else { return nil }
    return unsafeBitCast(surface, to: IOSurface.self)
}

@objc protocol FramebufferDescriptor {
    @objc(registerScreenCallbacksWithUUID:callbackQueue:frameCallback:surfacesChangedCallback:propertiesChangedCallback:)
    func registerScreenCallbacks(
        uuid: UUID,
        callbackQueue: DispatchQueue,
        frameCallback: @convention(block) @escaping () -> Void,
        surfacesChangedCallback: @convention(block) @escaping () -> Void,
        propertiesChangedCallback: @convention(block) @escaping () -> Void
    )
    @objc(unregisterScreenCallbacksWithUUID:)
    func unregisterScreenCallbacks(uuid: UUID)
}

/// Writes each new frame of the screen, scaled to `width`, at its time.
final class Recorder {
    let queue = DispatchQueue(label: "record", qos: .userInteractive)
    private var writer: AVAssetWriter?
    private var input: AVAssetWriterInput?
    private var adaptor: AVAssetWriterInputPixelBufferAdaptor?
    private var transfer: VTPixelTransferSession?
    private var started = false
    private var lastSeed: UInt32 = 0
    private var lastSurface: IOSurfaceID = 0
    private(set) var frames = 0
    private(set) var dropped = 0
    private var first: CFTimeInterval = 0
    private var last: CFTimeInterval = 0

    /// The screen: the largest display surface, the device's own (not a
    /// presentation surface of 7680x4320, which a newer Xcode adds).
    private func screen() -> IOSurface? {
        displays.compactMap(surface(of:))
            .filter { IOSurfaceGetWidth($0) <= 4096 }
            .max { IOSurfaceGetWidth($0) * IOSurfaceGetHeight($0) < IOSurfaceGetWidth($1) * IOSurfaceGetHeight($1) }
    }

    private func open(for source: IOSurface) {
        let (w, h) = (IOSurfaceGetWidth(source), IOSurfaceGetHeight(source))
        let height = (width * h / w) & ~1
        try? FileManager.default.removeItem(at: out)
        guard let writer = try? AVAssetWriter(outputURL: out, fileType: .mov) else { fail("cannot write \(out.path)") }
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: 12_000_000,
                AVVideoExpectedSourceFrameRateKey: 60,
                AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
            ],
        ])
        input.expectsMediaDataInRealTime = true
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
            kCVPixelBufferIOSurfacePropertiesKey as String: [:],
        ])
        writer.add(input)
        guard writer.startWriting() else { fail("cannot start writing: \(writer.error.map { "\($0)" } ?? "?")") }
        var transfer: VTPixelTransferSession?
        VTPixelTransferSessionCreate(allocator: nil, pixelTransferSessionOut: &transfer)
        (self.writer, self.input, self.adaptor, self.transfer) = (writer, input, adaptor, transfer)
        print("recording \(w)x\(h) as \(width)x\(height)")
    }

    /// The screen as it is now, if it has changed since the last frame.
    func take() {
        let now = CACurrentMediaTime()
        guard let source = screen() else { return }
        let (seed, id) = (IOSurfaceGetSeed(source), IOSurfaceGetID(source))
        if started, seed == lastSeed, id == lastSurface { return }
        (lastSeed, lastSurface) = (seed, id)
        if writer == nil { open(for: source) }
        guard let writer, let input, let adaptor, let transfer, let pool = adaptor.pixelBufferPool else { return }
        guard input.isReadyForMoreMediaData else { dropped += 1; return }
        var wrapped: Unmanaged<CVPixelBuffer>?
        guard CVPixelBufferCreateWithIOSurface(nil, source, nil, &wrapped) == kCVReturnSuccess,
              let from = wrapped?.takeRetainedValue() else { return }
        var made: CVPixelBuffer?
        guard CVPixelBufferPoolCreatePixelBuffer(nil, pool, &made) == kCVReturnSuccess, let to = made,
              VTPixelTransferSessionTransferImage(transfer, from: from, to: to) == noErr else { return }
        let time = CMTime(seconds: now, preferredTimescale: 600_000)
        if !started {
            writer.startSession(atSourceTime: time)
            (started, first) = (true, now)
        }
        if adaptor.append(to, withPresentationTime: time) { frames += 1; last = now } else { dropped += 1 }
    }

    func finish(_ done: @escaping () -> Void) {
        guard let writer, let input else { fail("no frame came") }
        input.markAsFinished()
        writer.finishWriting {
            if writer.status != .completed { fail("writing failed: \(writer.error.map { "\($0)" } ?? "?")") }
            print("recorded \(self.frames) frames over \(String(format: "%.2f", self.last - self.first))s (\(self.dropped) dropped)")
            done()
        }
    }
}

let recorder = Recorder()
let id = UUID()
for display in displays {
    let frame: @convention(block) () -> Void = { recorder.take() }
    let changed: @convention(block) () -> Void = { recorder.take() }
    let properties: @convention(block) () -> Void = {}
    (display as AnyObject).registerScreenCallbacks(
        uuid: id, callbackQueue: recorder.queue,
        frameCallback: frame, surfacesChangedCallback: changed, propertiesChangedCallback: properties)
}
// The screen as it is when the recording starts, still or not.
recorder.queue.async { recorder.take() }
recorder.queue.asyncAfter(deadline: .now() + seconds) {
    for display in displays { (display as AnyObject).unregisterScreenCallbacks(uuid: id) }
    recorder.take()
    recorder.finish { exit(0) }
}
dispatchMain()
